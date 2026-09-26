//! The `Vault` façade — B2's one typed core API and the only surface the two dumb
//! adapters call (ADR-0012). It owns the connection and the injected embedder, and
//! exposes only what the shipped commands need; add an operation when a command
//! needs it, never speculatively.
//!
//! A vault is one portable folder: the index lives under `<root>/.b2/` (ADR-0002).
//! [`open`](Vault::open) defaults to the deterministic [`FakeEmbedder`];
//! [`open_with_embedder`](Vault::open_with_embedder) wires the real model
//! (ADR-0005). Under the fake, `search`'s BM25 half is still real but the vector
//! half is not semantic — callers must not overstate it.

use crate::add;
use crate::chunk::ChunkConfig;
use crate::db;
use crate::dirs;
use crate::discover;
use crate::embed::{Embedder, FakeEmbedder};
use crate::error::{Error, Result};
use crate::graph::{self, Direction};
use crate::import;
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

// Report types the façade returns — re-exported so adapters name them through the
// one typed contract.
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

/// Headroom [`Vault::search_chunks`] keeps over `limit`. A small constant, because
/// façade headroom is multiplied: each unit buys [`search::pool_size`] more
/// candidates from each signal, and candidate width changes answers (GH #142).
const TORN_READ_HEADROOM: usize = 2;

/// [`Vault::search`]'s hit pool: **3×**, and load-bearing. Its results are
/// note-level, so chunks sharing a note collapse onto that note's best one; without
/// the headroom an ordinary query under-fills `limit` distinct notes. The same
/// headroom absorbs a chunk whose note row vanished mid-query (the
/// concurrent-reindex window C1 allows). Dedup scales with `limit`, so this does too.
fn note_hit_pool(limit: usize) -> usize {
    limit.saturating_mul(3)
}

/// [`Vault::search_chunks`]'s hit pool: `limit` + [`TORN_READ_HEADROOM`]. No dedup
/// here — this is the un-deduped passage view — so the only hit it drops is one
/// whose `chunk_detail` lookup missed on a torn read, a bounded window a constant
/// covers (GH #137).
///
/// Deliberately *not* `search`'s 3× (GH #142): RRF scores `Σ 1/(k + rank + 1)`, so a
/// wider pool returns different answers, not merely more of them — 10 of 10 probes
/// changed their top-4 passages across exactly that step (`make stability`). Wider
/// may well be better; the labelled corpus is too small to price it (GH #141), so
/// width stays at the conservative setting until an eval can (ADR-0013).
fn chunk_hit_pool(limit: usize) -> usize {
    limit.saturating_add(TORN_READ_HEADROOM)
}

/// Candidates **each retrieval signal** pulls for a `limit`-sized
/// [`Vault::search`]: the façade asks retrieval for [`note_hit_pool`] hits, and each
/// signal widens that again before the two lists are fused (ADR-0008).
///
/// Public because a *measurement* needs it. A corpus with no more chunks than this
/// truncates neither candidate list, so its scores are invariant under candidate
/// width — a pool change reads as "no change" there while moving real-vault results.
/// The eval harness prints that blindness rather than let a reader trust a number
/// that could not have moved (GH #141). ([`search::RRF_K`] re-weights the *same*
/// lists, so it reorders even a tiny corpus.)
pub fn note_candidate_pool(limit: usize) -> usize {
    search::pool_size(note_hit_pool(limit))
}

/// [`note_candidate_pool`] for the passage view, over [`chunk_hit_pool`]. Always
/// the narrower of the two, which makes it the threshold a *blindness* claim must
/// clear: under it, no number a run prints can move with candidate width (GH #141).
pub fn chunk_candidate_pool(limit: usize) -> usize {
    search::pool_size(chunk_hit_pool(limit))
}

/// An open vault: the Markdown at `root`, projected into the disposable index at
/// `root/.b2/b2.sqlite` (ADR-0002).
pub struct Vault {
    root: PathBuf,
    conn: Connection,
    /// Injected through the seam (ADR-0005): the adapters wire the real model,
    /// `open` defaults to `FakeEmbedder`.
    embedder: Box<dyn Embedder>,
    /// The vault's one chunking policy, held here rather than re-defaulted per
    /// call, so every path that chunks cuts identically and `incremental ≡ full
    /// rebuild` holds by construction. Across a `set_chunk_config` change that
    /// guarantee is doc-enforced instead: the change must pair with
    /// `project(force)`. The retrieval eval is the only client that overrides it.
    chunk_config: ChunkConfig,
}

impl Vault {
    /// Open the vault rooted at `vault_root` with the deterministic [`FakeEmbedder`]
    /// — the default for tests/dev. Creating `<root>/.b2/` if absent; idempotent.
    pub fn open(vault_root: &Path) -> Result<Self> {
        Self::open_with_embedder(vault_root, Box::new(FakeEmbedder::new(EMBED_DIM)))
    }

    /// Open the vault with a caller-supplied embedder — the seam the adapters use to
    /// inject the real candle model while tests keep the fake.
    ///
    /// `open` **never mutates the embedding space** (ADR-0007): shaping the vector
    /// tables and any re-embed happen only on `reindex`, so changing the configured
    /// model can never silently wipe vectors on the next command. `search` fails
    /// fast on a mismatch instead.
    pub fn open_with_embedder(vault_root: &Path, embedder: Box<dyn Embedder>) -> Result<Self> {
        // Every façade op opens a `tracing` span (target `b2::vault`), so each op
        // reports its own duration and the per-query `b2::sqlite` events carry which
        // op they ran under. Inert until an adapter installs a subscriber, so the
        // core still reads no clock of its own.
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

    /// The vault's **projection context** — the `(conn, root, cfg)` bundle every
    /// write-side op threads, built once here rather than per call site (GH #134).
    /// Handing an op this rather than [`embed_ctx`](Self::embed_ctx) is what *makes*
    /// it model-free: it carries no embedder, so `rm`/`create_note`/`write` cannot
    /// embed even by mistake.
    fn ctx(&self) -> ingest::ProjectionCtx<'_> {
        ingest::ProjectionCtx::new(&self.conn, &self.root, &self.chunk_config)
    }

    /// [`ctx`](Self::ctx) plus the injected embedder — for the ops that re-embed what
    /// they touch (`reindex`, `add`, `link`, `mv`).
    fn embed_ctx(&self) -> ingest::EmbedCtx<'_> {
        ingest::EmbedCtx::new(self.ctx(), self.embedder.as_ref())
    }

    /// Override the vault's chunking policy. Every subsequent op that chunks cuts
    /// with it, so the index stays self-consistent. The client is the out-of-CI
    /// retrieval eval (ADR-0013), which sweeps chunker levers in one process; the
    /// shipped adapters never call it. It does not re-chunk by itself — pair it with
    /// `project(force)`.
    pub fn set_chunk_config(&mut self, cfg: ChunkConfig) {
        self.chunk_config = cfg;
    }

    /// Rebuild the FTS index over the **same stored chunk text** with a different
    /// tokenizer — the eval harness's lexical-half lever (ADR-0013), not an adapter
    /// surface, so the GH #157 stemmer A/B can flip it without re-chunking or
    /// re-embedding. The choice is recorded nowhere durable: the index is disposable,
    /// and a `reindex` into a fresh `.b2/` restores the shipped default.
    pub fn rebuild_fts(&self, tokenizer: db::FtsTokenizer) -> Result<()> {
        db::rebuild_fts(&self.conn, tokenizer)
    }

    /// Re-project every `.md` note under the vault root into the index (Flow ①):
    /// notes, chunks (+embeddings), and the typed graph. Writes nothing to the vault.
    /// **Incremental** — a note whose body is unchanged reuses its vectors rather
    /// than re-embedding (see [`reindex_with_progress`](Self::reindex_with_progress)
    /// to force a full re-embed or observe progress).
    pub fn reindex(&self) -> Result<ReindexReport> {
        self.reindex_with_progress(false, &mut |_| ControlFlow::Continue(()))
    }

    /// [`reindex`](Self::reindex) with the three knobs its adapters need: `force`
    /// re-chunks every note even if unchanged; `on_progress` fires after each embed
    /// batch so a slow reindex shows a live line instead of looking frozen; and
    /// returning [`ControlFlow::Break`] from it **cooperatively cancels** the embed
    /// phase at that batch boundary while projection still completes, leaving a
    /// consistent, resumable index ([`ReindexReport::cancelled`]). The desktop maps
    /// its cancel flag to `Break`; the CLI always returns `Continue`.
    ///
    /// **`force` re-chunks; whether it re-*embeds* is content's to decide.** Vectors
    /// are keyed by chunk text (ADR-0006), so forcing a rebuild over unchanged notes
    /// finds every vector already stored and reports `embedded: 0` truthfully. Where
    /// `force` is actually reached for — a chunker-policy change — the chunk text
    /// moves, the hashes miss, and the model runs on exactly what changed. It no
    /// longer repairs a *damaged* stored vector; the index is disposable, so deleting
    /// `.b2/` is the answer there.
    pub fn reindex_with_progress(
        &self,
        force: bool,
        on_progress: &mut dyn FnMut(ingest::ReindexProgress) -> ControlFlow<()>,
    ) -> Result<ReindexReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "reindex", force).entered();
        let ingested = ingest::ingest_vault_with_progress(self.embed_ctx(), force, on_progress)?;
        Ok(ReindexReport {
            indexed: ingested.notes.len(),
            embedded: ingested.notes.iter().filter(|i| i.embedded).count(),
            cancelled: ingested.cancelled,
            skipped: ingested.skipped,
            notes_pruned: ingested.notes_pruned,
            resources_indexed: ingested.resources_indexed,
            resources_pruned: ingested.resources_pruned,
        })
    }

    /// The **projection pass** alone: re-project every `.md` note into
    /// `notes`/`chunks`(+FTS)/`edges` with **no model and no vector work**, and no
    /// write to the vault. After it returns the file tree lists, notes open, keyword
    /// search answers and the graph resolves; only vectors — and thus `similar` and
    /// semantic ranking — wait for [`embed`](Self::embed). `force` re-chunks every
    /// note, so `project(force)` + `embed` is a full rebuild, costing model calls
    /// only where chunk text genuinely moved (ADR-0006).
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

    /// The **embed pass** alone: fill a vector for every chunk that lacks one. The
    /// pending set is derived from the index itself, so this needs no prior
    /// [`project`](Self::project) call in the same process and heals any interruption
    /// — a cancelled embed, a crash between passes — by embedding exactly what is
    /// still missing. Progress and cooperative cancel behave as in
    /// [`reindex_with_progress`](Self::reindex_with_progress), and progress is
    /// determinate from the first batch.
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

    /// Preview a reindex (`reindex --dry-run`): what [`reindex`](Self::reindex)
    /// **would** index and (re)embed, with no write of any kind — not to the
    /// Markdown, the index, or the vectors. `force` previews a re-chunk of every
    /// note. A pure read, so it needs no model.
    pub fn plan_reindex(&self, force: bool) -> Result<ReindexPlan> {
        let _op = tracing::debug_span!(target: "b2::vault", "plan_reindex", force).entered();
        let planned = ingest::plan_reindex(&self.conn, &self.root, force)?;
        Ok(ReindexPlan {
            would_index: planned.len(),
            would_embed: planned.iter().filter(|p| p.would_embed).count(),
        })
    }

    /// Active neighbors of the note referenced by `note_ref` (a path, with or
    /// without the `.md`), each resolved to the other note's path + title. Errors
    /// with [`Error::NoteNotFound`] for an unknown ref — distinct from a found note
    /// with no neighbors, which is an empty list.
    pub fn neighbors(&self, note_ref: &str) -> Result<Vec<NeighborView>> {
        let _op = tracing::debug_span!(target: "b2::vault", "neighbors", note = note_ref).entered();
        let path = self.resolve_ref(note_ref)?;
        self.neighbors_of(&path)
    }

    /// The active neighbors of an already-resolved path. Shared by
    /// [`neighbors`](Self::neighbors) and [`explain`](Self::explain), so the two
    /// present the same edge shape.
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

    /// The resource links of an already-resolved path — every outbound edge at a
    /// non-`.md` file, resolved with its inventory class for display (GH #22).
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

    /// The unresolved (dangling) outbound links of an already-resolved path, shared
    /// by [`explain`](Self::explain) and [`unresolved_links`](Self::unresolved_links).
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

    /// The unresolved (dangling) outbound links of `note_ref`: links it authored
    /// that resolve to no note and no resource — a `[[Hermes]]` naming a *folder*, or
    /// a typo (GH #12). A pure graph read; [`Error::NoteNotFound`] for an unknown
    /// ref, an empty list for a note whose every link resolves.
    pub fn unresolved_links(&self, note_ref: &str) -> Result<Vec<UnresolvedLink>> {
        let _op = tracing::debug_span!(target: "b2::vault", "unresolved_links", note = note_ref)
            .entered();
        let path = self.resolve_ref(note_ref)?;
        self.unresolved_of(&path)
    }

    /// Explain a note's connections (`b2 explain`): the note resolved to its
    /// identity + title, every active typed edge and its "why", its outbound
    /// **resource** links (GH #22), and any **unresolved** outbound links — surfaced,
    /// not dropped (GH #12). A pure graph read; [`Error::NoteNotFound`] for an
    /// unknown ref, empty vectors for a note with no edges.
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

    /// Read a note for display: resolve `note_ref` to its file and return the note's
    /// **raw Markdown body from disk** (the source of truth, not the index
    /// projection) plus the frontmatter metadata worth showing a reader. A pure,
    /// model-free read, and ref resolution lives here so the adapter never touches
    /// the filesystem itself. [`Error::NoteNotFound`] for an unknown ref.
    pub fn read(&self, note_ref: &str) -> Result<NoteView> {
        let _op = tracing::debug_span!(target: "b2::vault", "read", note = note_ref).entered();
        let path = self.resolve_ref(note_ref)?;
        let raw = fs::read_to_string(self.root.join(&path))?;
        let revision = revision_of(&raw);
        let parsed = note::parse(&raw);
        let fields = parsed.fields();
        // The display title is the filename (the frontmatter `title:` is inert),
        // derived from the path here so even a not-yet-reindexed note shows one.
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

    /// Save a note's **body** — the editing surface's body write op
    /// ([`write_frontmatter`](Self::write_frontmatter) is its frontmatter sibling).
    /// Markdown-first and **model-free**: validate that the file on disk still hashes
    /// to `base_revision` (else [`Error::WriteConflict`] and nothing is written),
    /// splice `body` in verbatim after the untouched frontmatter, write, and
    /// re-project the note. A changed body's stale vectors are cleared and join the
    /// pending set for any later [`embed`](Self::embed), so saving works with no
    /// model provisioned.
    ///
    /// Returns the **new revision**, hashing the *final* on-disk bytes — which the
    /// editor chains its next save on: sequential saves never self-conflict, and only
    /// an external write trips the guard.
    pub fn write(&self, note_ref: &str, body: &str, base_revision: &str) -> Result<WriteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "write", note = note_ref).entered();
        // The byte-honest splice: frontmatter bytes untouched.
        self.save_guarded(note_ref, base_revision, |parsed| {
            parsed.replace_body(body);
            Ok(())
        })
    }

    /// Save a note's **frontmatter** — [`write`](Self::write)'s sibling (GH #79),
    /// with the same shape: validate the content-hash `base_revision`, splice
    /// `frontmatter` in verbatim between the fences (every body byte preserved by
    /// construction), write, re-project **model-free**. An unchanged body keeps its
    /// chunks and vectors, so a frontmatter save never re-embeds.
    ///
    /// **One** refusal, before any byte reaches disk: a top-level `---` line
    /// ([`Error::Frontmatter`]) would close the block early and shift the rest into
    /// the body, and the body is not this op's to change. Everything else in the
    /// block is the human's: malformed YAML saves fine — it round-trips verbatim,
    /// projects best-effort, and surfaces through
    /// [`NoteView::frontmatter_readable`], exactly as the same edit made in an
    /// external editor would.
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

    /// The shared shape of the two editor saves: resolve, check that the file on disk
    /// still hashes to `base_revision` (else [`Error::WriteConflict`], nothing written),
    /// apply `splice` (which may refuse, also before any byte reaches disk), write, and
    /// re-project **model-free** through the ordinary path.
    ///
    /// The returned revision is read back from disk rather than assumed: the token the
    /// editor chains its next save on must describe the file, not our intent.
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

    /// Every indexed note as a lightweight [`NoteSummary`], ordered by `path` — what
    /// the file tree is built from. A pure model-free read: the tree shows exactly
    /// the notes the index knows, and every one is [`read`](Self::read)-resolvable,
    /// so a click always opens. A never-reindexed vault lists nothing, no error.
    pub fn list_notes(&self) -> Result<Vec<NoteSummary>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_notes").entered();
        Ok(db::all_notes(&self.conn)?
            .into_iter()
            .map(|(path, title)| NoteSummary { path, title })
            .collect())
    }

    /// Every inventoried resource as a lightweight [`ResourceSummary`], ordered by
    /// `path` — the file tree's resource half, a sibling of
    /// [`list_notes`](Self::list_notes) rather than a widened union; the adapters
    /// compose the tree.
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

    /// Every folder in the vault (vault-relative, sorted, empty ones included) — the
    /// file tree's structure half. Read **live off the filesystem, never the index**:
    /// folders are user-authored structure with no derived data, so the walk itself
    /// is the projection and the tree stays one-to-one with disk. Dot-folders are
    /// skipped, as in the ingest walk. Works on a never-reindexed vault.
    pub fn list_dirs(&self) -> Result<Vec<String>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_dirs").entered();
        dirs::list_dirs(&self.root)
    }

    /// Read a resource's **bytes** — [`read`](Self::read)'s non-note sibling, for the
    /// viewers an adapter shows in place of the fallback card (an image, today).
    ///
    /// Inventory-checked *before* the filesystem is touched, the same posture the
    /// desktop's *Open in system default* takes: the path must name a row the walk put
    /// in `resources`, so this can never be talked into "read any file this process can
    /// reach" by a link a note authored. Errors with [`Error::ResourceNotFound`]
    /// otherwise — including for a note, which is not resource inventory.
    ///
    /// Reads whole, because every caller wants the whole file; a resource too large for
    /// a viewer is the *adapter's* judgement (it knows the size from
    /// [`explain_resource`](Self::explain_resource) before asking), not a rule the
    /// engine imposes.
    pub fn read_resource_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let _op = tracing::debug_span!(target: "b2::vault", "read_resource_bytes", path).entered();
        self.require_resource(path)?;
        Ok(fs::read(self.root.join(path))?)
    }

    /// The fallback card's data for one resource: inventory metadata plus the
    /// backlinks panel, straight off the materialized graph. `path` is vault-relative
    /// (the adapters dispatch here via [`crate::resource::doc_kind`]); errors with
    /// [`Error::ResourceNotFound`] when it is not inventoried.
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

    /// Move/rename a resource — the note move minus the identity step: rewrite every
    /// inbound link's authored text (each syntax keeping its own relative-vs-root
    /// convention), move the file, update the inventory, re-project the touched
    /// notes. Errors with [`Error::ResourceNotFound`]; destination errors mirror
    /// [`move_note`](Self::move_note).
    pub fn move_resource(&self, path: &str, to: &str) -> Result<ResourceMoveReport> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "mv_resource", from = path, to).entered();
        self.require_resource(path)?;
        mv::move_resource(self.embed_ctx(), path, to)
    }

    /// Hybrid search (BM25 ⊕ vector → RRF) resolved to notes, best first, capped at
    /// `limit` *notes*: chunk hits dedup to the highest-scoring chunk per note, so
    /// one note never appears twice (ADR-0008).
    ///
    /// **Keyword-first fallback:** when the vector space does not exist yet — a
    /// projected-but-unembedded vault — this runs BM25-only rather than returning
    /// nothing, so a vault is searchable the moment [`project`](Self::project)
    /// finishes. A never-indexed vault yields no hits and no error; callers read
    /// [`embed_status`](Self::embed_status) to present keyword-only results honestly.
    ///
    /// A `limit` of 0 short-circuits ahead of [`retrieve`](Self::retrieve), so it
    /// costs no query embedding and no [`Error::ModelMismatch`] either: that guard
    /// exists to stop *wrong results*, and there are none to be wrong about.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let _op = tracing::debug_span!(target: "b2::vault", "search", query, limit).entered();
        if limit == 0 {
            return Ok(Vec::new());
        }
        let hits = self.retrieve(query, note_hit_pool(limit))?.hits;
        self.note_results(hits, query, limit)
    }

    /// [`search`](Self::search)'s dense half alone — vector KNN resolved to notes,
    /// deduped, best first. **The eval harness's ablation instrument** (ADR-0013,
    /// GH #158), not an adapter surface: scoring it beside bm25-only and hybrid is
    /// what gives fusion a measured single-signal baseline. Same model-mismatch
    /// fail-fast as `search`, but a projected-but-unembedded vault returns no hits —
    /// where `search` honestly *falls back* to keywords, an ablation that quietly did
    /// the same would be measuring the wrong signal.
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

    /// [`search`](Self::search) with the **evidence behind it** (ADR-0015): the same
    /// served results, plus the two absolute signals RRF discards and the verdict
    /// they produce.
    ///
    /// Flow ② could not answer *zero* as first shipped — the dense half always has k
    /// nearest, *nearest* is a fact about the vault rather than evidence about the
    /// query, and RRF keeps only ranks — so a nonsense query served `limit`
    /// confident-looking results. This read is what a surface consults to say **"no
    /// matches"** honestly. It judges nothing itself: the results come back whole and
    /// in order, and what a surface *vouches for* is the surface's decision to make.
    ///
    /// `limit` caps the rows and nothing else — the evidence is a fact about the
    /// query and the vault, not about how many results were asked for. `vouched` is
    /// `None` when the active embedder has no calibrated bar (a fake space has no
    /// geometry to hold a cosine bar over, and an uncalibrated model's bar would be
    /// another model's constant); a caller that gets `None` never guesses one.
    ///
    /// [`search_evidence_excluding`](Self::search_evidence_excluding) is this same
    /// read minus a caller-named set of notes — the follow-up-search form.
    pub fn search_evidence(&self, query: &str, limit: usize) -> Result<SearchEvidenceView> {
        self.search_evidence_excluding(query, limit, &[])
    }

    /// [`search_evidence`](Self::search_evidence) minus a **caller-named** set of notes:
    /// hits on `exclude` paths are dropped before note resolution, so the served rows
    /// backfill from the same ranking with the next fresh notes. This is the
    /// follow-up-search form an agent loop passes its already-inspected paths to — a
    /// re-query that hands back the same head reads as progress and is none.
    ///
    /// The subtraction is the caller's, never the verdict's: `vouched`, the terms and
    /// `best_cos` still read the whole vault, because the evidence is a fact about the
    /// query and the vault (ADR-0015), not about what the caller has already read. The
    /// retrieval pool is also unchanged — width is a ranking choice priced by the eval,
    /// not plumbing (GH #141/#142) — so exclusion spends the same headroom note-dedup
    /// does, and a heavily-excluded query may serve fewer than `limit` rows: the
    /// ranking's head is spent, and the honest next move is a refined query, not a
    /// deeper page.
    ///
    /// Paths match exactly as a result served them — a note's identity is its
    /// vault-relative path (L1) — and an unknown path excludes nothing.
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
        // `limit.max(1)` where `search` would short-circuit: a zero limit caps the
        // *rows*, and the question this read answers is about the evidence. Going
        // through `retrieve(0)` would skip the dense scan and hand back
        // `best_cos: None` — which reads as "no embedding space" — so a verdict taken
        // from it would be the lexical half wearing both halves' name.
        let mut retrieval = self.retrieve(query, note_hit_pool(limit.max(1)))?;
        // The subtraction drops hits only. `best_cos` is a scalar the retrieval
        // already carries, so the vault's nearest chunk still reports even when the
        // caller has already read the note it belongs to.
        retrieval.hits.retain(|h| !exclude.contains(&h.note_path));
        // The lexical half is read here rather than inside retrieval: it costs a
        // `count(*)` per distinct query term, and only a caller that wants a verdict
        // should pay for it.
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

    /// [`resolve_note_hits`](Self::resolve_note_hits) without the provenance — the
    /// shared tail of [`search`](Self::search) and
    /// [`search_vector_only`](Self::search_vector_only).
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

    /// The note-resolution tail of every note-level search: dedup chunk hits to their
    /// best-scoring note, resolve path + title + query-windowed snippet, stop at
    /// `limit`.
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
            // The chunk or its note can only be missing on a torn read; drop the hit
            // rather than name a note that is gone — the pool has headroom to backfill
            // it (GH #137).
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

    /// [`search`](Self::search) at **chunk** granularity: the top `limit` ranked
    /// chunks resolved to their note + heading breadcrumb + full text, with **no note
    /// dedup** — one note may appear several times when several of its passages rank.
    /// Same retrieval, same fallback, same fail-fast (see [`ChunkSearchResult`] for
    /// who consumes this). Retrieves a narrower pool than [`search`](Self::search) —
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
            // The lookup misses only on a torn read; drop such a hit rather than emit a
            // half-resolved one — a rank slot with an empty path would read as a real
            // result. The pool backfills it (GH #137).
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

    /// The shared retrieval core of [`search`](Self::search),
    /// [`search_chunks`](Self::search_chunks) and
    /// [`search_evidence_excluding`](Self::search_evidence_excluding): hybrid when the embedding space
    /// exists (failing fast on a model mismatch), BM25-only on a
    /// projected-but-unembedded vault.
    fn retrieve(&self, query: &str, pool: usize) -> Result<search::Retrieval> {
        if db::embedding_space_exists(&self.conn)? {
            self.ensure_query_space_matches()?;
            search::hybrid_search(&self.conn, self.embedder.as_ref(), query, pool)
        } else {
            search::keyword_only_search(&self.conn, query, pool)
        }
    }

    /// The model-identity guard every query-embedding read shares: the stored vectors
    /// must have been produced by the active embedder, or the query vector is
    /// incomparable with them and results would be silently wrong (ADR-0007). The fix
    /// is a `reindex`.
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

    /// The vault's embedding coverage as an [`EmbedStatus`] — the honest "N/M
    /// embedded" read (#26). A **pure model-free count**, so an adapter can flag
    /// results "keyword-only for now" rather than silently under-rank while a vault
    /// embeds behind the first tree paint.
    pub fn embed_status(&self) -> Result<EmbedStatus> {
        let _op = tracing::debug_span!(target: "b2::vault", "embed_status").entered();
        let (embedded, total) = db::embed_progress(&self.conn)?;
        Ok(EmbedStatus { embedded, total })
    }

    /// The notes most semantically similar to `note_ref` that are **not already
    /// connected** to it — [`discover::candidates`] surfaced directly: vector KNN over
    /// the stored embeddings, minus the anchor's 1-hop neighbours, ranked
    /// nearest-best-passage first. **The ranked list is what is served** (ADR-0014):
    /// no statistic gates it, `limit` is a cap that under-fills only for want of
    /// scorable notes, and an empty result means the candidate set is genuinely empty,
    /// never a verdict that nothing relates. Each row carries path + title, the
    /// passage that made it similar, and — on a real-embedded space with a large
    /// enough population — the `z` the strength band derives from. A **pure read over
    /// stored vectors, no model call**. [`Error::NoteNotFound`] for an unknown ref.
    pub fn similar(&self, note_ref: &str, limit: usize) -> Result<Vec<SimilarView>> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "similar", note = note_ref, limit).entered();
        // Never claim a statistic over a fake-embedded space (`grades`). Grading changes
        // what the rows carry, never which rows exist (ADR-0014).
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

    /// **Explain** one note against an anchor's *Similar & unlinked* list (GH #236):
    /// where it stands (ranked, past the list, linked, not embedded, not shortlisted),
    /// its z and whole-note rank, the field's z population, every passage pair graded on
    /// the card's yardstick, and the neighbors both notes share. `limit` is the list
    /// length the surface showed, so the rank and z are the card's. Any note can be
    /// explained, not only a served one. A pure read over stored vectors, no model call.
    /// [`Error::NoteNotFound`] for an unknown ref on either side;
    /// [`Error::ResourceUnsupported`] for a resource anchor, as [`similar`](Self::similar).
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
            // A chunk row that vanished mid-read (a concurrent reindex, C1) is skipped
            // rather than shown half-empty.
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

    /// Whether this vault's discovery is graded: never over a fake-embedded space, whose
    /// hash vectors have no semantic geometry for a z to describe. Judged by the RECORDED
    /// identity (the space being searched), not the injected embedder.
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

    /// Commit a typed connection `src --type--> dst` (`b2 link`, flow ③): append a
    /// typed-link string to the **source note's frontmatter `b2_relations:`** — never
    /// the body (ADR-0010) — and re-project it as an `origin='frontmatter'` active
    /// edge. Both ends resolve by path. `edge_type` must be a **core** verb; a
    /// non-core one errors with [`Error::InvalidRelation`] rather than silently store
    /// a typo. **Idempotent:** an existing `(src, dst, type)` edge writes nothing
    /// (`created: false`).
    ///
    /// Re-projection re-reads the source note, so the adapters open the vault with the
    /// same embedder the index was built with, as for `add`/`mv`.
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
        // The link path drops the `.md` Obsidian omits (matches how `[[links]]` are written).
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

        // The spec targets the dst's path. A note's title is its filename, so a bare
        // `[[path]]` already reads as the title — B2 writes no alias.
        let link = format!("[[{dst_path}]]");
        let spec = match explanation {
            Some(e) => format!("{edge_type} {link} — {e}"),
            None => format!("{edge_type} {link}"),
        };

        // 1. Markdown first: append to frontmatter b2_relations: (never the body, §0).
        let abs = self.root.join(&src_path);
        let mut parsed = note::parse(&fs::read_to_string(&abs)?);
        parsed.add_relation(&spec)?;
        fs::write(&abs, parsed.as_str())?;

        // 2. Re-project the source note so the edge re-materializes from the Markdown
        //    as origin='frontmatter' — a projection of the line just written.
        ingest::ingest_file(self.embed_ctx(), &src_path)?;

        Ok(LinkReport {
            src_path,
            dst_path,
            relation: edge_type.to_string(),
            created: true,
        })
    }

    /// Move/rename the whole folder `from` to `to` (both vault-relative; a trailing
    /// `/` is tolerated). One `fs::rename` moves the directory — unindexed files
    /// inside travel too — after every inbound link at the moved set (including
    /// vault-root wikilinks *between* co-moved notes) is rewritten; the index then
    /// re-projects and the graph never breaks (each moved note's rows re-key to its
    /// new path, `ON UPDATE CASCADE`). Errors with [`Error::DirNotFound`],
    /// [`Error::MoveDestination`] (including a destination inside the moved folder),
    /// or [`Error::MoveTargetExists`] rather than merge into an existing entry.
    ///
    /// Rewriting an inbound file changes its body, so this **re-embeds** those files:
    /// the adapters open with the real model for a dir move.
    pub fn move_dir(&self, from: &str, to: &str) -> Result<DirMoveReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mv_dir", from, to).entered();
        mv::move_dir(self.embed_ctx(), from, to)
    }

    /// Move/rename the note `note_ref` to `to` (a vault-relative path, `.md`
    /// optional), rewriting every inbound `[[oldpath|alias]]` link to the new path and
    /// re-projecting. The graph never breaks — the note's rows re-key to the new path
    /// (`ON UPDATE CASCADE`), so neighbors and backlinks show the same set before and
    /// after and not one chunk is re-embedded (ADR-0003, ADR-0006); the human-readable
    /// link text is repaired alongside. Errors with [`Error::NoteNotFound`],
    /// [`Error::MoveDestination`] or [`Error::MoveTargetExists`].
    ///
    /// Rewriting an inbound file changes its body, so this **re-embeds** those files:
    /// the adapters open with the real model for `mv`.
    pub fn move_note(&self, note_ref: &str, to: &str) -> Result<MoveReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mv", from = note_ref, to).entered();
        let old_rel = self.resolve_ref(note_ref)?;
        mv::move_note(self.embed_ctx(), &old_rel, to)
    }

    /// Delete the note `note_ref`: the file leaves the disk, its projection rows leave
    /// the index, and every inbound link at it **dangles** — never rewritten,
    /// surfacing as unresolved (GH #12) — exactly the state an external `rm` plus a
    /// full reindex produces. **Model-free**: no body changes, so the inbound
    /// re-projection touches no vectors. [`Error::NoteNotFound`] for an unknown ref.
    pub fn delete_note(&self, note_ref: &str) -> Result<DeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm", note = note_ref).entered();
        let rel = self.resolve_ref(note_ref)?;
        rm::delete_note(self.ctx(), &rel)
    }

    /// [`delete_note`](Self::delete_note)'s resource sibling — same posture (file
    /// off disk, inventory row off the index, inbound links dangle, model-free).
    /// Errors with [`Error::ResourceNotFound`] for a path not in the inventory.
    pub fn delete_resource(&self, path: &str) -> Result<ResourceDeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm_resource", path).entered();
        rm::delete_resource(self.ctx(), path)
    }

    /// Delete the whole folder `dir` (vault-relative) and everything inside it —
    /// one `fs::remove_dir_all`, so unindexed files inside go too — then every
    /// contained note's/resource's rows; surviving linkers outside the folder
    /// dangle, as for [`delete_note`](Self::delete_note). Model-free. Errors with
    /// [`Error::DirNotFound`] for a missing (or invalid) folder.
    pub fn delete_dir(&self, dir: &str) -> Result<DirDeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm_dir", dir).entered();
        rm::delete_dir(self.ctx(), dir)
    }

    /// Create a new note (`b2 add`): write `path` with a minimal valid frontmatter (an
    /// optional `title`, today's `created`) and `content` as its body, then project it
    /// — the note is immediately searchable and in the graph. Nothing is added beyond
    /// what the template wrote, so it stays fully reconstructible from Markdown.
    /// Errors with [`Error::AddDestination`] or [`Error::AddTargetExists`] rather than
    /// clobber an existing file.
    ///
    /// Projection **embeds** the new note, so the CLI opens with the real model.
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

    /// Create a new, empty note **model-free** — the desktop's New-note action, the
    /// create sibling of [`write`](Self::write). Same minimal frontmatter as
    /// [`add_note`](Self::add_note) (no title — a note's display title is its
    /// filename), projected with **no embedder touched**, so creation works with no
    /// model provisioned and a fake-opened vault can never write foreign vectors into
    /// a real embedding space (ADR-0007). The chunks join the pending set any later
    /// [`embed`](Self::embed) heals; an empty body has nothing to embed anyway. Same
    /// refusals as `add_note`.
    pub fn create_note(&self, path: &str) -> Result<AddReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "create", path).entered();
        let created = self.today()?;
        add::create_note(self.ctx(), path, None, None, &created)
    }

    /// Import a file the human already has into the vault folder `dir` (`""` for the
    /// root) under `file_name`, from its **bytes** — the desktop's drag-onto-the-tree
    /// gesture, where the OS hands the webview content rather than a path. A `.md`
    /// lands as a note (projected); anything else as a resource (one inventory row),
    /// exactly as the vault walk would have classified it. The bytes are written
    /// **verbatim** — B2 authors nothing here, unlike [`add_note`](Self::add_note),
    /// which mints a document. **Model-free**, like [`create_note`](Self::create_note).
    ///
    /// Errors with [`Error::ImportDestination`] for an invalid name or folder/name
    /// pair, and [`Error::ImportTargetExists`] rather than clobber an existing file:
    /// the destination path *is* the arriving note's identity (ADR-0003), so refusing
    /// an occupied one is the whole of the collision story. If projection fails the
    /// placed file is removed again — an import either lands and indexes, or leaves
    /// nothing behind.
    pub fn import_file(&self, dir: &str, file_name: &str, bytes: &[u8]) -> Result<ImportReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "import", dir, file_name).entered();
        import::import_bytes(self.ctx(), dir, file_name, bytes)
    }

    /// [`import_file`](Self::import_file) from a **path** instead of bytes — the same
    /// op for the same gesture's keyboard half (an OS file picker, which yields
    /// paths), keeping the file's own name. Same refusals, plus
    /// [`Error::ImportDestination`] for a source that is a folder or has no file name.
    pub fn import_path(&self, dir: &str, source: &Path) -> Result<ImportReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "import_path", dir).entered();
        import::import_path(self.ctx(), dir, source)
    }

    /// Create the folder `dir` (missing parents included, an occupied target refused)
    /// — the desktop's New-folder action, the structure sibling of
    /// [`create_note`](Self::create_note). A folder is user-authored structure, so
    /// this writes the filesystem and touches **nothing else**: no index rows exist
    /// for a folder (see [`list_dirs`](Self::list_dirs)), and it is real to Finder,
    /// the CLI and a sync the moment this returns. Errors with
    /// [`Error::DirDestination`] or [`Error::DirTargetExists`].
    pub fn create_dir(&self, dir: &str) -> Result<DirCreateReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mkdir", dir).entered();
        dirs::create_dir(&self.root, dir)
    }

    /// Today's date (`YYYY-MM-DD`) from **SQLite** — the same clock that stamps
    /// `indexed_at`, so `b2-core` needs no wall-clock crate and the façade stays the
    /// determinism boundary. The vault convention for a note's `created:` field.
    fn today(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT strftime('%Y-%m-%d','now')", [], |r| r.get(0))?)
    }

    /// The inventory row for `path`, or [`Error::ResourceNotFound`]: the one existence
    /// check every resource op makes before touching the filesystem, so a path a note
    /// authored can never talk B2 into reading or moving a file outside the inventory.
    fn require_resource(&self, path: &str) -> Result<db::ResourceDetail> {
        db::resource_detail(&self.conn, path)?
            .ok_or_else(|| Error::ResourceNotFound(path.to_string()))
    }

    /// Discovery's anchor guard. A resource anchor is honest, not silent: resources
    /// become discoverable once they have chunks + centroids. Until then an inventoried
    /// resource errs "not yet" ([`Error::ResourceUnsupported`]) — never an empty result —
    /// and an unknown path falls through to the caller's not-found.
    fn refuse_resource_anchor(&self, anchor_ref: &str) -> Result<()> {
        if crate::resource::doc_kind(anchor_ref) == crate::resource::DocKind::Resource
            && db::resource_detail(&self.conn, anchor_ref)?.is_some()
        {
            return Err(Error::ResourceUnsupported(anchor_ref.to_string()));
        }
        Ok(())
    }

    /// Resolve a note reference to the indexed note's vault-relative path — which *is*
    /// its identity (ADR-0003), so this is one canonicalization rather than the
    /// two-step "is it an id? is it a path?" it replaced. The ref may be written with
    /// or without the `.md`, as links are. The [`Error::NoteNotFound`] carries the
    /// caller's original `note_ref`, so the error reads as they typed it.
    fn resolve_ref(&self, note_ref: &str) -> Result<String> {
        db::resolve_link_target(&self.conn, note_ref)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))
    }
}

/// A file's save-guard revision: blake3 of its raw bytes. One fn, so `read`
/// (capture) and `write` (validate + return) can never drift.
fn revision_of(raw: &str) -> String {
    blake3::hash(raw.as_bytes()).to_hex().to_string()
}
