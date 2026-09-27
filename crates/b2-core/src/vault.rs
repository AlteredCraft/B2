//! The `Vault` façade: B2's one typed core API and the only surface the adapters call
//! (ADR-0012). It owns the connection and the injected embedder. Add an operation when a
//! command needs it, never speculatively.
//!
//! The index lives under `<root>/.b2/` (ADR-0002). [`open`](Vault::open) uses the
//! deterministic [`FakeEmbedder`], whose vector half is not semantic;
//! [`open_with_embedder`](Vault::open_with_embedder) wires the real model (ADR-0005).

use crate::add;
use crate::chunk::ChunkConfig;
use crate::db;
use crate::dirs;
use crate::discover;
use crate::embed::{Embedder, FakeEmbedder};
use crate::error::{Error, Result};
use crate::graph::{self, Direction};
use crate::import;
use crate::link;
use crate::mv;
use crate::rm;
use crate::snippet::{query_snippet, snippet};
use crate::{ingest, note, relation, search};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

mod chat;
mod views;

pub use views::*;

// Report types the façade returns, re-exported for the adapters.
pub use crate::add::AddReport;
pub use crate::dirs::DirCreateReport;
pub use crate::import::ImportReport;
pub use crate::ingest::{ReindexProgress, SkippedNote};
pub use crate::mv::DirMoveReport;
pub use crate::mv::{MoveReport, ResourceMoveReport};
pub use crate::rm::{DeleteReport, DirDeleteReport, ResourceDeleteReport};

/// Embedding dimension of the *fake* embedder ([`Vault::open`]). The real model
/// brings its own (768); a model or dim swap re-embeds on `reindex` (ADR-0007).
const EMBED_DIM: usize = 64;

/// Headroom [`Vault::search_chunks`] keeps over `limit`. Small, because each unit buys
/// [`search::pool_size`] more candidates per signal, and width changes answers (GH #142).
const TORN_READ_HEADROOM: usize = 2;

/// [`Vault::search`]'s hit pool: 3× `limit`. Chunks of one note collapse to its best,
/// so without headroom a query under-fills `limit` notes. Also absorbs a note row
/// vanishing mid-query (C1).
fn note_hit_pool(limit: usize) -> usize {
    limit.saturating_mul(3)
}

/// [`Vault::search_chunks`]'s hit pool: `limit` + [`TORN_READ_HEADROOM`], covering only
/// torn reads (GH #137). Not 3×: under RRF a wider pool changes answers, and the eval
/// cannot yet price that (GH #141, #142, ADR-0013).
fn chunk_hit_pool(limit: usize) -> usize {
    limit.saturating_add(TORN_READ_HEADROOM)
}

/// Candidates each retrieval signal pulls for a `limit`-sized [`Vault::search`]
/// (ADR-0008). Public for the eval: a corpus smaller than this is blind to pool-width
/// changes, and the harness says so (GH #141).
pub fn note_candidate_pool(limit: usize) -> usize {
    search::pool_size(note_hit_pool(limit))
}

/// [`note_candidate_pool`] for the passage view. The narrower of the two, so the
/// threshold for the blindness claim (GH #141).
pub fn chunk_candidate_pool(limit: usize) -> usize {
    search::pool_size(chunk_hit_pool(limit))
}

/// An open vault: the Markdown at `root`, projected into the disposable index at
/// `root/.b2/b2.sqlite` (ADR-0002).
pub struct Vault {
    root: PathBuf,
    conn: Connection,
    /// Injected through the seam (ADR-0005).
    embedder: Box<dyn Embedder>,
    /// The one chunking policy, held here so every path cuts identically and
    /// incremental ≡ full rebuild.
    chunk_config: ChunkConfig,
}

impl Vault {
    /// Open the vault at `vault_root` with the deterministic [`FakeEmbedder`], creating
    /// `<root>/.b2/` if absent.
    pub fn open(vault_root: &Path) -> Result<Self> {
        Self::open_with_embedder(vault_root, Box::new(FakeEmbedder::new(EMBED_DIM)))
    }

    /// Open the vault with a caller-supplied embedder.
    ///
    /// Never mutates the embedding space (ADR-0007): only `reindex` does, so a model
    /// change can't silently wipe vectors. `search` fails fast on a mismatch instead.
    pub fn open_with_embedder(vault_root: &Path, embedder: Box<dyn Embedder>) -> Result<Self> {
        // Every façade op opens a `b2::vault` span, so `b2::sqlite` events carry their
        // op. Inert until an adapter installs a subscriber, so the core reads no clock.
        let _op = tracing::debug_span!(target: "b2::vault", "open").entered();
        // `Connection::open` creates the DB file but not its parent.
        fs::create_dir_all(vault_root.join(".b2"))?;
        let conn = db::open(&vault_root.join(".b2").join("b2.sqlite"))?;
        Ok(Self {
            root: vault_root.to_path_buf(),
            conn,
            embedder,
            chunk_config: ChunkConfig::default(),
        })
    }

    /// The projection context every write-side op threads (GH #134). It carries no
    /// embedder, so an op given this cannot embed.
    fn ctx(&self) -> ingest::ProjectionCtx<'_> {
        ingest::ProjectionCtx::new(&self.conn, &self.root, &self.chunk_config)
    }

    /// [`ctx`](Self::ctx) plus the embedder, for ops that re-embed what they touch.
    fn embed_ctx(&self) -> ingest::EmbedCtx<'_> {
        ingest::EmbedCtx::new(self.ctx(), self.embedder.as_ref())
    }

    /// Override the chunking policy, for the retrieval eval (ADR-0013). Does not
    /// re-chunk: pair it with `project(force)`.
    pub fn set_chunk_config(&mut self, cfg: ChunkConfig) {
        self.chunk_config = cfg;
    }

    /// Rebuild the FTS index with a different tokenizer: the eval's lexical lever
    /// (ADR-0013, GH #157). Not recorded; a fresh `.b2/` restores the default.
    pub fn rebuild_fts(&self, tokenizer: db::FtsTokenizer) -> Result<()> {
        db::rebuild_fts(&self.conn, tokenizer)
    }

    /// Re-project every note into the index and embed it (Flow ①). Incremental; writes
    /// nothing to the vault.
    pub fn reindex(&self) -> Result<ReindexReport> {
        self.reindex_with_progress(false, &mut |_| ControlFlow::Continue(()))
    }

    /// [`reindex`](Self::reindex) with knobs: `force` re-chunks every note; `on_progress`
    /// fires after each embed batch, and returning [`ControlFlow::Break`] cancels the
    /// embed phase there, leaving a consistent, resumable index.
    ///
    /// `force` re-embeds only chunk text that changed (ADR-0006).
    pub fn reindex_with_progress(
        &self,
        force: bool,
        on_progress: &mut dyn FnMut(ingest::ReindexProgress) -> ControlFlow<()>,
    ) -> Result<ReindexReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "reindex", force).entered();
        // Projection completes before embedding starts, so a cancelled run is still
        // consistent: keyword search and the graph are complete, a prefix is embedded.
        let projected = ingest::project_vault(self.ctx(), force)?;
        let embed = ingest::embed_vault(&self.conn, self.embedder.as_ref(), on_progress)?;
        // A note counts as embedded this run iff the embed pass fully filled it.
        let filled: BTreeSet<&str> = embed.embedded.iter().map(String::as_str).collect();
        Ok(ReindexReport {
            indexed: projected.notes.len(),
            embedded: projected
                .notes
                .iter()
                .filter(|p| filled.contains(p.as_str()))
                .count(),
            cancelled: embed.cancelled,
            skipped: projected.skipped,
            notes_pruned: projected.notes_pruned,
            resources_indexed: projected.resources_indexed,
            resources_pruned: projected.resources_pruned,
        })
    }

    /// The projection pass alone: model-free, no vault write. Everything but vectors
    /// works afterwards; those wait for [`embed`](Self::embed). `force` re-chunks every
    /// note.
    pub fn project(&self, force: bool) -> Result<ProjectReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "project", force).entered();
        let outcome = ingest::project_vault(self.ctx(), force)?;
        Ok(ProjectReport {
            indexed: outcome.notes.len(),
            skipped: outcome.skipped,
            notes_pruned: outcome.notes_pruned,
            resources_indexed: outcome.resources_indexed,
            resources_pruned: outcome.resources_pruned,
        })
    }

    /// The embed pass alone: fill a vector for every chunk that lacks one. The pending
    /// set comes from the index, so any interruption heals here. Progress and cancel as
    /// in [`reindex_with_progress`](Self::reindex_with_progress).
    pub fn embed(
        &self,
        on_progress: &mut dyn FnMut(ingest::ReindexProgress) -> ControlFlow<()>,
    ) -> Result<EmbedReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "embed").entered();
        let outcome = ingest::embed_vault(&self.conn, self.embedder.as_ref(), on_progress)?;
        Ok(EmbedReport {
            embedded: outcome.embedded.len(),
            cancelled: outcome.cancelled,
        })
    }

    /// Preview a reindex (`reindex --dry-run`) with no write of any kind. Model-free.
    pub fn plan_reindex(&self, force: bool) -> Result<ReindexPlan> {
        let _op = tracing::debug_span!(target: "b2::vault", "plan_reindex", force).entered();
        let plan = ingest::plan_reindex(&self.conn, &self.root, force)?;
        Ok(ReindexPlan {
            would_index: plan.notes,
            would_embed: plan.would_embed,
        })
    }

    /// Active neighbors of `note_ref` (a path, `.md` optional).
    /// [`Error::NoteNotFound`] for an unknown ref.
    pub fn neighbors(&self, note_ref: &str) -> Result<Vec<NeighborView>> {
        let _op = tracing::debug_span!(target: "b2::vault", "neighbors", note = note_ref).entered();
        let path = self.resolve_ref(note_ref)?;
        self.neighbors_of(&path)
    }

    /// The active neighbors of an already-resolved path.
    fn neighbors_of(&self, note_path: &str) -> Result<Vec<NeighborView>> {
        let mut out = Vec::new();
        for n in graph::neighbors(&self.conn, note_path)? {
            let (title, created) = db::note_header(&self.conn, &n.other)?;
            out.push(NeighborView {
                path: n.other,
                title,
                relation: n.edge_type,
                direction: match n.direction {
                    Direction::Outbound => "outbound",
                    Direction::Inbound => "inbound",
                }
                .to_string(),
                label: n.label,
                explanation: n.explanation,
                origin: n.origin,
                created,
            });
        }
        Ok(out)
    }

    /// The outbound resource links of an already-resolved path (GH #22).
    fn resource_links_of(&self, note_path: &str) -> Result<Vec<ResourceLinkView>> {
        Ok(db::outbound_resource_edges(&self.conn, note_path)?
            .into_iter()
            .map(|e| ResourceLinkView {
                path: e.path,
                class: e.class,
                relation: e.r#type,
                origin: e.origin,
                caption: e.caption,
                embed: e.embed,
                explanation: e.explanation,
            })
            .collect())
    }

    /// The dangling outbound links of an already-resolved path.
    fn unresolved_of(&self, note_path: &str) -> Result<Vec<UnresolvedLink>> {
        Ok(graph::unresolved_outbound(&self.conn, note_path)?
            .into_iter()
            .map(|u| UnresolvedLink {
                target: u.target,
                relation: u.edge_type,
                origin: u.origin,
                explanation: u.explanation,
            })
            .collect())
    }

    /// The dangling outbound links of `note_ref`: links resolving to no note or
    /// resource, such as a folder name or a typo (GH #12).
    pub fn unresolved_links(&self, note_ref: &str) -> Result<Vec<UnresolvedLink>> {
        let _op = tracing::debug_span!(target: "b2::vault", "unresolved_links", note = note_ref)
            .entered();
        let path = self.resolve_ref(note_ref)?;
        self.unresolved_of(&path)
    }

    /// Explain a note's connections (`b2 explain`): typed edges with their "why",
    /// resource links (GH #22) and unresolved links (GH #12).
    pub fn explain(&self, note_ref: &str) -> Result<ExplainView> {
        let _op = tracing::debug_span!(target: "b2::vault", "explain", note = note_ref).entered();
        let path = self.resolve_ref(note_ref)?;
        let title = db::note_title(&self.conn, &path)?;
        let connections = self.neighbors_of(&path)?;
        let resources = self.resource_links_of(&path)?;
        let unresolved = self.unresolved_of(&path)?;
        Ok(ExplainView {
            path,
            title,
            connections,
            resources,
            unresolved,
        })
    }

    /// Read a note for display: the raw Markdown from disk (not the index) plus its
    /// frontmatter metadata. [`Error::NoteNotFound`] for an unknown ref.
    pub fn read(&self, note_ref: &str) -> Result<NoteView> {
        let _op = tracing::debug_span!(target: "b2::vault", "read", note = note_ref).entered();
        let path = self.resolve_ref(note_ref)?;
        let raw = fs::read_to_string(self.root.join(&path))?;
        let revision = revision_of(&raw);
        let parsed = note::parse(&raw);
        let fields = parsed.fields();
        // The title is the filename (frontmatter `title:` is inert), so even an
        // unindexed note has one.
        let title = Some(note::display_title(&path));
        Ok(NoteView {
            path,
            title,
            r#type: fields.r#type.clone(),
            created: fields.created.clone(),
            updated: fields.updated.clone(),
            tags: fields.tags.clone(),
            body: parsed.body().to_string(),
            frontmatter: parsed.frontmatter().map(str::to_string),
            frontmatter_readable: parsed.frontmatter_readable(),
            revision,
        })
    }

    /// Save a note's body, model-free. Refuses with [`Error::WriteConflict`] unless the
    /// file still hashes to `base_revision`; frontmatter bytes are untouched.
    ///
    /// Returns the new revision of the on-disk bytes, which the editor chains its next
    /// save on, so only an external write trips the guard.
    pub fn write(&self, note_ref: &str, body: &str, base_revision: &str) -> Result<WriteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "write", note = note_ref).entered();
        self.save_guarded(note_ref, base_revision, |parsed| {
            parsed.replace_body(body);
            Ok(())
        })
    }

    /// Save a note's frontmatter verbatim: [`write`](Self::write)'s sibling (GH #79).
    /// Body bytes are untouched, so it never re-embeds.
    ///
    /// The one refusal is a `---` line ([`Error::Frontmatter`]), which would end the
    /// block early. Malformed YAML saves fine and surfaces through
    /// [`NoteView::frontmatter_readable`].
    pub fn write_frontmatter(
        &self,
        note_ref: &str,
        frontmatter: &str,
        base_revision: &str,
    ) -> Result<WriteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "write_frontmatter", note = note_ref)
            .entered();
        self.save_guarded(note_ref, base_revision, |parsed| {
            if frontmatter
                .lines()
                .any(|l| l.trim_end_matches('\r') == "---")
            {
                return Err(Error::Frontmatter(
                    "a `---` line inside frontmatter would end the block early".into(),
                ));
            }
            parsed.replace_frontmatter(frontmatter);
            Ok(())
        })
    }

    /// The shared editor save: check `base_revision`, apply `splice` (which may refuse),
    /// write, re-project model-free. The returned revision is read back from disk, so it
    /// describes the file rather than our intent.
    fn save_guarded(
        &self,
        note_ref: &str,
        base_revision: &str,
        splice: impl FnOnce(&mut note::ParsedNote) -> Result<()>,
    ) -> Result<WriteReport> {
        let path = self.resolve_ref(note_ref)?;
        let abs = self.root.join(&path);
        let raw = fs::read_to_string(&abs)?;
        if revision_of(&raw) != base_revision {
            return Err(Error::WriteConflict(path));
        }
        let mut parsed = note::parse(&raw);
        splice(&mut parsed)?;
        fs::write(&abs, parsed.as_str())?;
        ingest::project_file(self.ctx(), &path)?;
        let final_raw = fs::read_to_string(&abs)?;
        Ok(WriteReport {
            path,
            revision: revision_of(&final_raw),
        })
    }

    /// Every indexed note as a [`NoteSummary`], path-ordered, for the file tree.
    pub fn list_notes(&self) -> Result<Vec<NoteSummary>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_notes").entered();
        Ok(db::all_notes(&self.conn)?
            .into_iter()
            .map(|(path, title)| NoteSummary { path, title })
            .collect())
    }

    /// Every inventoried resource as a [`ResourceSummary`], path-ordered.
    pub fn list_resources(&self) -> Result<Vec<ResourceSummary>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_resources").entered();
        Ok(db::list_resources(&self.conn)?
            .into_iter()
            .map(|r| ResourceSummary {
                path: r.path,
                class: r.class,
                size: r.size,
                mtime: r.mtime,
            })
            .collect())
    }

    /// Every folder in the vault (sorted, empty ones included, dot-folders skipped).
    /// Read live off the filesystem, never the index, so the tree matches disk.
    pub fn list_dirs(&self) -> Result<Vec<String>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_dirs").entered();
        dirs::list_dirs(&self.root)
    }

    /// Read a resource's bytes, for an adapter's viewer.
    ///
    /// Inventory-checked before touching the filesystem, so a note's link can never
    /// read an arbitrary file; [`Error::ResourceNotFound`] otherwise. Size limits are
    /// the adapter's call.
    pub fn read_resource_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let _op = tracing::debug_span!(target: "b2::vault", "read_resource_bytes", path).entered();
        self.require_resource(path)?;
        Ok(fs::read(self.root.join(path))?)
    }

    /// The absolute path of an inventoried resource, for handing to the OS.
    /// Inventory-checked like [`read_resource_bytes`](Self::read_resource_bytes).
    pub fn resource_path(&self, path: &str) -> Result<PathBuf> {
        let _op = tracing::debug_span!(target: "b2::vault", "resource_path", path).entered();
        self.require_resource(path)?;
        Ok(self.root.join(path))
    }

    /// The fallback card's data for one resource: inventory metadata plus backlinks.
    /// [`Error::ResourceNotFound`] when not inventoried.
    pub fn explain_resource(&self, path: &str) -> Result<ResourceExplainView> {
        let _op = tracing::debug_span!(target: "b2::vault", "explain_resource", path).entered();
        let detail = self.require_resource(path)?;
        let backlinks = db::inbound_resource_edges(&self.conn, path)?
            .into_iter()
            .map(|b| ResourceBacklink {
                path: b.note_path,
                title: b.note_title,
                r#type: b.r#type,
                caption: b.caption,
                embed: b.embed,
            })
            .collect();
        Ok(ResourceExplainView {
            path: path.to_string(),
            class: detail.class,
            size: detail.size,
            mtime: detail.mtime,
            content_hash: detail.content_hash,
            backlinks,
        })
    }

    /// Move or rename a resource, rewriting inbound links. Errors mirror
    /// [`move_note`](Self::move_note).
    pub fn move_resource(&self, path: &str, to: &str) -> Result<ResourceMoveReport> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "mv_resource", from = path, to).entered();
        self.require_resource(path)?;
        mv::move_resource(self.embed_ctx(), path, to)
    }

    /// Hybrid search (BM25 ⊕ vector → RRF) resolved to at most `limit` distinct notes,
    /// best first (ADR-0008).
    ///
    /// With no vector space yet it runs BM25-only; callers read
    /// [`embed_status`](Self::embed_status) to say so. A `limit` of 0 returns early,
    /// without [`Error::ModelMismatch`]: there are no results to be wrong.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let _op = tracing::debug_span!(target: "b2::vault", "search", query, limit).entered();
        if limit == 0 {
            return Ok(Vec::new());
        }
        let hits = self.retrieve(query, note_hit_pool(limit))?.hits;
        self.note_results(hits, query, limit)
    }

    /// [`search`](Self::search)'s dense half alone: the eval's ablation arm (ADR-0013,
    /// GH #158). An unembedded vault returns no hits rather than falling back, or the
    /// ablation would measure the wrong signal.
    pub fn search_vector_only(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "search_vector_only", query, limit).entered();
        if limit == 0 {
            return Ok(Vec::new());
        }
        if !db::embedding_space_exists(&self.conn)? {
            return Ok(Vec::new());
        }
        self.ensure_query_space_matches()?;
        let hits = search::vector_only_search(
            &self.conn,
            self.embedder.as_ref(),
            query,
            note_hit_pool(limit),
        )?;
        self.note_results(hits, query, limit)
    }

    /// [`search`](Self::search) plus the evidence RRF discards and its verdict
    /// (ADR-0015), so a surface can say "no matches" honestly. Results come back whole;
    /// what to vouch for is the surface's call.
    ///
    /// `limit` caps rows only. `vouched` is `None` when the embedder has no calibrated
    /// bar; a caller never guesses one.
    pub fn search_evidence(&self, query: &str, limit: usize) -> Result<SearchEvidenceView> {
        self.search_evidence_excluding(query, limit, &[])
    }

    /// [`search_evidence`](Self::search_evidence) minus the `exclude` paths, for an agent's
    /// follow-up search. Rows backfill from the same ranking.
    ///
    /// The verdict, terms and `best_cos` still read the whole vault (ADR-0015). The pool
    /// is unchanged (GH #141/#142), so a heavily-excluded query may serve fewer than
    /// `limit` rows. Paths match exactly (L1).
    pub fn search_evidence_excluding(
        &self,
        query: &str,
        limit: usize,
        exclude: &[String],
    ) -> Result<SearchEvidenceView> {
        let _op = tracing::debug_span!(
            target: "b2::vault",
            "search_evidence",
            query,
            limit,
            excluded = exclude.len()
        )
        .entered();
        // `limit.max(1)`: `retrieve(0)` would skip the dense scan and report
        // `best_cos: None`, which reads as "no embedding space".
        let mut retrieval = self.retrieve(query, note_hit_pool(limit.max(1)))?;
        // Drops hits only; `best_cos` still reports the vault's nearest chunk.
        retrieval.hits.retain(|h| !exclude.contains(&h.note_path));
        // Read here, not in retrieval: a `count(*)` per term only a verdict needs.
        let evidence = search::QueryEvidence {
            lexical: search::lexical_evidence(&self.conn, query)?,
            best_cos: retrieval.best_cos,
        };
        let bar = search::EvidenceBar::for_model(self.embedder.model_id());
        Ok(SearchEvidenceView {
            results: self
                .resolve_note_hits(retrieval.hits, query, limit)?
                .into_iter()
                .map(|(result, p)| EvidencedResult {
                    result,
                    bm25_rank: p.bm25_rank,
                    vector_rank: p.vector_rank,
                    cos: p.distance.map(search::cosine_of_distance),
                })
                .collect(),
            vouched: bar.map(|b| evidence.vouched(b)),
            chunk_total: evidence.lexical.chunk_total,
            terms: evidence
                .lexical
                .terms
                .iter()
                .map(|t| QueryTermView {
                    term: t.term.clone(),
                    df: t.df,
                    idf: evidence.lexical.idf(t.df),
                })
                .collect(),
            best_cos: evidence.best_cos,
        })
    }

    /// [`resolve_note_hits`](Self::resolve_note_hits) without the provenance.
    fn note_results(
        &self,
        hits: Vec<search::Hit>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        Ok(self
            .resolve_note_hits(hits, query, limit)?
            .into_iter()
            .map(|(result, _)| result)
            .collect())
    }

    /// Dedup chunk hits to their best note, resolve each with a snippet, stop at `limit`.
    fn resolve_note_hits(
        &self,
        hits: Vec<search::Hit>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(SearchResult, search::HitProvenance)>> {
        let mut out: Vec<(SearchResult, search::HitProvenance)> = Vec::new();
        for hit in hits {
            if out.len() == limit {
                break;
            }
            if out.iter().any(|(r, _)| r.path == hit.note_path) {
                continue; // note already represented by a higher-scoring chunk
            }
            // Missing only on a torn read; the pool's headroom backfills (GH #137).
            let Some(found) = db::chunk_hit(&self.conn, hit.chunk_id)? else {
                continue;
            };
            out.push((
                SearchResult {
                    snippet: query_snippet(&found.text, query),
                    path: found.note_path,
                    title: found.title,
                    score: hit.score,
                },
                hit.provenance,
            ));
        }
        Ok(out)
    }

    /// [`search`](Self::search) at chunk granularity, with no note dedup. Narrower pool;
    /// see [`chunk_hit_pool`].
    pub fn search_chunks(&self, query: &str, limit: usize) -> Result<Vec<ChunkSearchResult>> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "search_chunks", query, limit).entered();
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for hit in self.retrieve(query, chunk_hit_pool(limit))?.hits {
            if out.len() == limit {
                break;
            }
            // Missing only on a torn read; the pool backfills (GH #137).
            let Some(found) = db::chunk_hit(&self.conn, hit.chunk_id)? else {
                continue;
            };
            out.push(ChunkSearchResult {
                path: found.note_path,
                heading_path: found.heading_path,
                score: hit.score,
                text: found.text,
            });
        }
        Ok(out)
    }

    /// Shared retrieval: hybrid when the embedding space exists (failing fast on a model
    /// mismatch), else BM25-only.
    fn retrieve(&self, query: &str, pool: usize) -> Result<search::Retrieval> {
        if db::embedding_space_exists(&self.conn)? {
            self.ensure_query_space_matches()?;
            search::hybrid_search(&self.conn, self.embedder.as_ref(), query, pool)
        } else {
            search::keyword_only_search(&self.conn, query, pool)
        }
    }

    /// Refuse a query embedding when the stored vectors came from another embedder: the
    /// results would be silently wrong (ADR-0007).
    fn ensure_query_space_matches(&self) -> Result<()> {
        if let Some((indexed_model, indexed_dim)) = db::recorded_embedder(&self.conn)? {
            if indexed_model != self.embedder.model_id() || indexed_dim != self.embedder.dim() {
                return Err(Error::ModelMismatch {
                    indexed: format!("{indexed_model} (dim {indexed_dim})"),
                    active: format!("{} (dim {})", self.embedder.model_id(), self.embedder.dim()),
                });
            }
        }
        Ok(())
    }

    /// The vault's embedding coverage (#26), so an adapter can flag results as
    /// keyword-only while a vault embeds. Model-free.
    pub fn embed_status(&self) -> Result<EmbedStatus> {
        let _op = tracing::debug_span!(target: "b2::vault", "embed_status").entered();
        let (embedded, total) = db::embed_progress(&self.conn)?;
        Ok(EmbedStatus { embedded, total })
    }

    /// The notes most similar to `note_ref` that are not already linked to it, with the
    /// passage that made each similar. No model call.
    ///
    /// The ranked list is served as is (ADR-0014): no statistic gates it, and empty means
    /// no candidates, not that nothing relates. `z` is set only when it means something.
    pub fn similar(&self, note_ref: &str, limit: usize) -> Result<Vec<SimilarView>> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "similar", note = note_ref, limit).entered();
        // Grading changes what rows carry, never which rows exist (ADR-0014).
        let grade = self.grades()?;
        self.refuse_resource_anchor(note_ref)?;
        let anchor = self.resolve_ref(note_ref)?;
        let mut out = Vec::new();
        for c in discover::candidates(&self.conn, &anchor, limit, grade)? {
            let title = db::note_title(&self.conn, &c.note_path)?;
            let evidence = db::chunk_text(&self.conn, c.evidence_chunk_id)?
                .map(|t| snippet(&t))
                .unwrap_or_default();
            out.push(SimilarView {
                path: c.note_path,
                title,
                score: c.score,
                evidence,
                z: c.z,
            });
        }
        Ok(out)
    }

    /// Explain one note against an anchor's Similar & unlinked list (GH #236): where it
    /// stands, its z and rank, graded passage pairs, and shared neighbors. `limit` is the
    /// list length shown, so rank and z match the card. Any note works. No model call.
    pub fn explain_similar(
        &self,
        anchor_ref: &str,
        candidate_ref: &str,
        limit: usize,
    ) -> Result<SimilarExplainView> {
        let _op = tracing::debug_span!(
            target: "b2::vault",
            "explain_similar",
            anchor = anchor_ref,
            candidate = candidate_ref,
            limit
        )
        .entered();
        self.refuse_resource_anchor(anchor_ref)?;
        let anchor = self.resolve_ref(anchor_ref)?;
        let candidate = self.resolve_ref(candidate_ref)?;
        let ex = discover::explain(&self.conn, &anchor, &candidate, limit, self.grades()?)?;

        let standing = match ex.standing {
            discover::Standing::SameNote => SimilarStanding::SameNote,
            discover::Standing::AnchorUnembedded => SimilarStanding::AnchorUnembedded,
            discover::Standing::Linked => SimilarStanding::Linked,
            discover::Standing::Unembedded => SimilarStanding::Unembedded,
            discover::Standing::NotShortlisted { shortlist } => {
                SimilarStanding::NotShortlisted { shortlist }
            }
            discover::Standing::Ranked { rank, of } => SimilarStanding::Ranked {
                rank,
                of,
                served: rank <= limit,
            },
        };
        let mut pairs = Vec::with_capacity(ex.pairs.len());
        for g in ex.pairs {
            let anchor_side = self.passage(g.pair.anchor_chunk_id)?;
            let candidate_side = self.passage(g.pair.candidate_chunk_id)?;
            // A chunk that vanished mid-read (C1) is skipped, not shown half-empty.
            let (Some(a), Some(c)) = (anchor_side, candidate_side) else {
                continue;
            };
            pairs.push(PassagePairView {
                identical: a.text == c.text,
                anchor: a,
                candidate: c,
                score: g.pair.score,
                z: g.z,
            });
        }
        let shared_neighbors = self
            .shared_neighbors(&anchor, &candidate)?
            .into_iter()
            .map(|path| self.note_summary(path))
            .collect::<Result<Vec<_>>>()?;
        Ok(SimilarExplainView {
            anchor: self.note_summary(anchor)?,
            candidate: self.note_summary(candidate)?,
            limit,
            standing,
            z: ex.z,
            centroid_rank: ex.centroid_rank,
            population: ex.population,
            pairs,
            shared_neighbors,
        })
    }

    /// Whether discovery is graded: never over a fake-embedded space. Judged by the
    /// recorded embedder (the space searched), not the injected one.
    fn grades(&self) -> Result<bool> {
        Ok(!matches!(
            db::recorded_embedder(&self.conn)?,
            Some((model, _)) if model == crate::embed::FAKE_MODEL_ID
        ))
    }

    /// One stored chunk as a [`PassageView`], `None` if the row is gone.
    fn passage(&self, chunk_id: i64) -> Result<Option<PassageView>> {
        Ok(db::chunk_detail(&self.conn, chunk_id)?
            .map(|(heading_path, text)| PassageView { heading_path, text }))
    }

    /// A resolved note path with its title, for a view that names a note.
    fn note_summary(&self, path: String) -> Result<NoteSummary> {
        let title = db::note_title(&self.conn, &path)?;
        Ok(NoteSummary { path, title })
    }

    /// Every note `note` is directly linked with, in either direction, by path.
    fn neighbor_paths(&self, note: &str) -> Result<BTreeSet<String>> {
        Ok(graph::neighbors(&self.conn, note)?
            .into_iter()
            .map(|n| n.other)
            .collect())
    }

    /// The notes both `a` and `b` are linked with, in either direction, by path.
    fn shared_neighbors(&self, a: &str, b: &str) -> Result<BTreeSet<String>> {
        Ok(self
            .neighbor_paths(a)?
            .intersection(&self.neighbor_paths(b)?)
            .cloned()
            .collect())
    }

    /// Commit a typed connection `src --type--> dst` (`b2 link`, flow ③) to the source's
    /// frontmatter `b2_relations:`, never the body (ADR-0010). `edge_type` must be a core
    /// verb ([`Error::InvalidRelation`]). Idempotent: an existing edge writes nothing.
    ///
    /// Re-projection embeds, so adapters open with the index's embedder.
    pub fn link(
        &self,
        src_ref: &str,
        dst_ref: &str,
        edge_type: &str,
        explanation: Option<&str>,
    ) -> Result<LinkReport> {
        let _op = tracing::debug_span!(
            target: "b2::vault", "link",
            src = src_ref, dst = dst_ref, edge_type
        )
        .entered();
        if !relation::is_core(edge_type) {
            return Err(Error::InvalidRelation(edge_type.to_string()));
        }
        let src_path = self.resolve_ref(src_ref)?;
        let dst_full = self.resolve_ref(dst_ref)?;
        // Drop the `.md`, as `[[links]]` are written.
        let dst_path = dst_full
            .strip_suffix(".md")
            .unwrap_or(&dst_full)
            .to_string();

        // Idempotent: don't append a duplicate frontmatter line for an existing edge.
        if db::edge_exists(&self.conn, &src_path, &dst_full, edge_type)? {
            return Ok(LinkReport {
                src_path,
                dst_path,
                relation: edge_type.to_string(),
                created: false,
            });
        }

        // The title is the filename, so B2 writes no alias.
        let spec = link::render_relation(edge_type, &dst_path, explanation);

        // 1. Markdown first: append to frontmatter b2_relations: (never the body, §0).
        let abs = self.root.join(&src_path);
        let mut parsed = note::parse(&fs::read_to_string(&abs)?);
        parsed.add_relation(&spec)?;
        fs::write(&abs, parsed.as_str())?;

        // 2. Re-project, so the edge materializes from the line just written.
        ingest::ingest_file(self.embed_ctx(), &src_path)?;

        Ok(LinkReport {
            src_path,
            dst_path,
            relation: edge_type.to_string(),
            created: true,
        })
    }

    /// Move or rename the folder `from` to `to` (trailing `/` tolerated), rewriting every
    /// inbound link first, including links between co-moved notes. Refuses to merge into
    /// an existing entry ([`Error::MoveTargetExists`]).
    ///
    /// Rewritten files re-embed, so adapters open with the real model.
    pub fn move_dir(&self, from: &str, to: &str) -> Result<DirMoveReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mv_dir", from, to).entered();
        mv::move_dir(self.embed_ctx(), from, to)
    }

    /// Move or rename the note `note_ref` to `to` (`.md` optional), rewriting every
    /// inbound link. The note's rows re-key, so the graph never breaks and its chunks
    /// are not re-embedded (ADR-0003, ADR-0006).
    ///
    /// Rewritten inbound files re-embed, so adapters open with the real model.
    pub fn move_note(&self, note_ref: &str, to: &str) -> Result<MoveReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mv", from = note_ref, to).entered();
        let old_rel = self.resolve_ref(note_ref)?;
        mv::move_note(self.embed_ctx(), &old_rel, to)
    }

    /// Delete the note `note_ref` and its rows. Inbound links dangle, never rewritten
    /// (GH #12), as after an external `rm` and a reindex. Model-free.
    pub fn delete_note(&self, note_ref: &str) -> Result<DeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm", note = note_ref).entered();
        let rel = self.resolve_ref(note_ref)?;
        rm::delete_note(self.ctx(), &rel)
    }

    /// [`delete_note`](Self::delete_note) for a resource.
    pub fn delete_resource(&self, path: &str) -> Result<ResourceDeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm_resource", path).entered();
        self.require_resource(path)?;
        rm::delete_resource(self.ctx(), path)
    }

    /// Delete the folder `dir` and everything in it, unindexed files included. Links
    /// from outside dangle, as for [`delete_note`](Self::delete_note). Model-free.
    pub fn delete_dir(&self, dir: &str) -> Result<DirDeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm_dir", dir).entered();
        rm::delete_dir(self.ctx(), dir)
    }

    /// Create a new note (`b2 add`) with minimal frontmatter and `content` as its body,
    /// then project it. Never clobbers ([`Error::AddTargetExists`]).
    ///
    /// Projection embeds, so the CLI opens with the real model.
    pub fn add_note(
        &self,
        path: &str,
        title: Option<&str>,
        content: Option<&str>,
    ) -> Result<AddReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "add", path).entered();
        let created = self.today()?;
        add::add_note(self.embed_ctx(), path, title, content, &created)
    }

    /// Create a new, empty note model-free (the desktop's New note). No embedder, so a
    /// fake-opened vault can't write foreign vectors into a real space (ADR-0007).
    pub fn create_note(&self, path: &str) -> Result<AddReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "create", path).entered();
        let created = self.today()?;
        add::create_note(self.ctx(), path, None, None, &created)
    }

    /// Import a file's bytes verbatim into folder `dir` (`""` for the root) as
    /// `file_name`: the desktop's drag onto the tree. Classified as the walk would.
    /// Model-free.
    ///
    /// Never clobbers ([`Error::ImportTargetExists`]): the path is the identity
    /// (ADR-0003). If projection fails the file is removed again.
    pub fn import_file(&self, dir: &str, file_name: &str, bytes: &[u8]) -> Result<ImportReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "import", dir, file_name).entered();
        import::import_bytes(self.ctx(), dir, file_name, bytes)
    }

    /// [`import_file`](Self::import_file) from a path (an OS file picker), keeping the
    /// file's name.
    pub fn import_path(&self, dir: &str, source: &Path) -> Result<ImportReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "import_path", dir).entered();
        import::import_path(self.ctx(), dir, source)
    }

    /// Create the folder `dir`, parents included. Touches only the filesystem: folders
    /// have no index rows (see [`list_dirs`](Self::list_dirs)).
    pub fn create_dir(&self, dir: &str) -> Result<DirCreateReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mkdir", dir).entered();
        dirs::create_dir(&self.root, dir)
    }

    /// Today's date (`YYYY-MM-DD`) from SQLite, so `b2-core` reads no wall clock itself.
    fn today(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT strftime('%Y-%m-%d','now')", [], |r| r.get(0))?)
    }

    /// The inventory row for `path`, or [`Error::ResourceNotFound`]. Every resource op
    /// checks this first, so a note's link can't reach a file outside the inventory.
    fn require_resource(&self, path: &str) -> Result<db::ResourceDetail> {
        db::resource_detail(&self.conn, path)?
            .ok_or_else(|| Error::ResourceNotFound(path.to_string()))
    }

    /// Discovery's anchor guard: an inventoried resource errs
    /// [`Error::ResourceUnsupported`] rather than return an empty result.
    fn refuse_resource_anchor(&self, anchor_ref: &str) -> Result<()> {
        if crate::resource::doc_kind(anchor_ref) == crate::resource::DocKind::Resource
            && db::resource_detail(&self.conn, anchor_ref)?.is_some()
        {
            return Err(Error::ResourceUnsupported(anchor_ref.to_string()));
        }
        Ok(())
    }

    /// Resolve a note reference (`.md` optional) to its vault-relative path, its
    /// identity (ADR-0003). The error carries `note_ref` as typed.
    fn resolve_ref(&self, note_ref: &str) -> Result<String> {
        db::resolve_link_target(&self.conn, note_ref)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))
    }
}

/// A file's save-guard revision: blake3 of its raw bytes.
fn revision_of(raw: &str) -> String {
    blake3::hash(raw.as_bytes()).to_hex().to_string()
}
