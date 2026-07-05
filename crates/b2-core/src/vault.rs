//! The `Vault` façade — B2's one typed core API (vision-and-scope, "the
//! testability stack" point 1). Everything before this exists only as modules the
//! integration tests call directly; this is the single entry point the `b2` CLI
//! (and future adapters) are the sole clients of. It owns the open connection, the
//! embedder, and the id generator, and exposes *only what the shipped commands need*
//! — `open` / `reindex` / `neighbors` / `explain` / `search` / `similar` / `link` /
//! `add` / `mv`. Add operations when a command needs them; do not pre-build a
//! sprawling surface.
//!
//! A vault is one portable folder: the index lives under `<root>/.b2/` (there is no
//! durable state outside the Markdown — data-model.md §4), so pointing B2 at a folder
//! of Markdown is the whole setup.
//! The embedder is injected ([`open_with_embedder`](Vault::open_with_embedder)):
//! the `b2` CLI wires the candle-backed `LocalEmbedder` (real semantics), while
//! [`open`](Vault::open) defaults to the deterministic [`FakeEmbedder`] so the core
//! test suite stays fast and model-free (testability points 4–5). `search`'s BM25
//! (keyword) half is always real; the vector half is only semantic under a real
//! embedder — callers must not overstate the fake.

use crate::add::{self, AddReport};
use crate::db;
use crate::discover;
use crate::embed::{Embedder, FakeEmbedder};
use crate::error::{Error, Result};
use crate::graph::{self, Direction};
use crate::id::UlidGen;
use crate::mv::{self, MoveReport};
use crate::{ingest, note, relation, search};
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// The embedding dimension the *fake* embedder runs at when [`Vault::open`] is used
/// without an injected model (tests/dev). The real model brings its own `dim` (768)
/// through [`Vault::open_with_embedder`]; a model/dim swap re-embeds on `reindex`.
const EMBED_DIM: usize = 64;

/// Longest snippet (in chars) shown for a search hit, so a result stays one line.
const SNIPPET_CHARS: usize = 160;

/// An open vault: the Markdown at `root`, projected into the disposable index at
/// `root/.b2/b2.sqlite` (a pure projection — no durable state outside the Markdown).
pub struct Vault {
    root: PathBuf,
    conn: Connection,
    // Injected through the seam: the CLI wires the real candle model; `open`
    // defaults to `FakeEmbedder` so the core tests stay deterministic and model-free
    // (the "build for tomorrow's model" seam, vision-and-scope).
    embedder: Box<dyn Embedder>,
    idgen: UlidGen,
}

/// What `reindex` did: how many notes were projected, how many were actually
/// (re)embedded (the rest reused their vectors — incremental), and how many needed
/// a `b2id` stamped (B2's one always-allowed write to the vault, data-model.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReindexReport {
    pub indexed: usize,
    pub embedded: usize,
    pub stamped: usize,
}

/// What a reindex **would** do — the `reindex --dry-run` preview. The `would_*`
/// keys (vs [`ReindexReport`]'s past-tense `indexed`/`embedded`/`stamped`) are the
/// honesty signal: this is a projection, computed read-only with **no** writes —
/// notably no `b2id` stamped to the vault (B2's one write, data-model.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReindexPlan {
    /// Notes a real reindex would project into the index (every `.md` file).
    pub would_index: usize,
    /// …of which this many would be (re)embedded (the rest reuse their vectors).
    pub would_embed: usize,
    /// Notes currently missing a `b2id` that a real reindex would stamp.
    pub would_stamp: usize,
}

/// One neighbor of a note, resolved for display: the note at the other end of an
/// active edge, with its path + title (so the CLI stays a dumb printer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeighborView {
    /// The `b2id` at the other end of the edge.
    pub b2id: String,
    /// The other note's vault-relative path.
    pub path: String,
    /// The other note's title, if it has one.
    pub title: Option<String>,
    /// The stored relation verb (outbound direction of the edge).
    pub relation: String,
    /// `"outbound"` (this note → other) or `"inbound"` (other → this note).
    pub direction: String,
    /// Display label: the verb outbound, its inverse inbound (data-model.md §2).
    pub label: String,
    pub explanation: Option<String>,
    /// Edge origin: `"inline"` (a human body link) or `"frontmatter"` (a relation
    /// committed via `b2 link`, or a human/importer authored) — data-model.md §0.
    /// `b2 explain` renders it; `b2 neighbors` carries it too.
    pub origin: String,
}

/// A note's full connection picture for `b2 explain`: the note itself (resolved to
/// its identity + display fields) and every active connection with its "why". A
/// thin header over [`NeighborView`] — `explain`'s job is to present a note's typed
/// edges and their explanations, so it reuses the same per-edge shape `neighbors`
/// returns rather than a parallel one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainView {
    pub b2id: String,
    pub path: String,
    pub title: Option<String>,
    /// Outbound edges first, then inbound (as [`graph::neighbors`] orders them),
    /// each with its label, target, and explanation. Empty for an isolated note.
    pub connections: Vec<NeighborView>,
}

/// One search hit, resolved to the note it belongs to with a text snippet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResult {
    pub b2id: String,
    pub path: String,
    pub title: Option<String>,
    /// Fused relevance score; higher is better.
    pub score: f64,
    /// A one-line excerpt of the matched chunk.
    pub snippet: String,
}

/// One semantically-similar candidate for `b2 similar`: a note near the anchor in
/// embedding space that is **not** already connected to it, resolved for display
/// with the passage that made it similar. This is connection-discovery candidate
/// generation ([`discover::candidates`]) surfaced directly to the human — the
/// machine finds the candidate, you decide whether to `link` it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarView {
    pub b2id: String,
    pub path: String,
    pub title: Option<String>,
    /// Best chunk-pair similarity to the anchor; higher is nearer.
    pub score: f64,
    /// A one-line excerpt of the candidate chunk that achieved `score` — the
    /// evidence for *why* it surfaced.
    pub evidence: String,
}

/// What `b2 link` did: the committed typed edge, resolved for display. `created` is
/// `false` when the directed `(src, dst, type)` edge already existed, so nothing was
/// written (the command is idempotent).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkReport {
    pub src_path: String,
    pub dst_path: String,
    pub relation: String,
    pub created: bool,
}

impl Vault {
    /// Open the vault rooted at `vault_root` with the deterministic [`FakeEmbedder`]
    /// — the default for tests/dev. Creating `<root>/.b2/` if absent; idempotent.
    pub fn open(vault_root: &Path) -> Result<Self> {
        Self::open_with_embedder(vault_root, Box::new(FakeEmbedder::new(EMBED_DIM)))
    }

    /// Open the vault with a caller-supplied embedder — the seam the `b2` CLI uses
    /// to inject the real candle model while tests keep the fake.
    ///
    /// `open` **never mutates the embedding space** (the `open()`-time-drop fix,
    /// tasks.md / index-engine.md §8): shaping `chunks_vec` and any re-embed happen
    /// only on `reindex`. That way changing the configured model can never silently
    /// wipe vectors on the next command — a mismatch is caught, and fixed, at
    /// `reindex`; `search` fails fast on it (see [`search`](Self::search)).
    pub fn open_with_embedder(vault_root: &Path, embedder: Box<dyn Embedder>) -> Result<Self> {
        // `Connection::open` creates the DB file but not its parent; make `.b2/` first.
        fs::create_dir_all(vault_root.join(".b2"))?;
        let conn = db::open(&vault_root.join(".b2").join("b2.sqlite"))?;
        Ok(Self {
            root: vault_root.to_path_buf(),
            conn,
            embedder,
            idgen: UlidGen,
        })
    }

    /// Re-project every `.md` note under the vault root into the index (Flow ①):
    /// notes, chunks (+embeddings), and the typed graph. Stamps any missing `b2id`.
    /// **Incremental** — a note whose body is unchanged reuses its vectors rather
    /// than re-embedding (see [`reindex_with_progress`](Self::reindex_with_progress)
    /// to force a full re-embed or observe progress).
    pub fn reindex(&self) -> Result<ReindexReport> {
        self.reindex_with_progress(false, &mut |_| {})
    }

    /// [`reindex`](Self::reindex) with two knobs the CLI needs: `force` re-embeds
    /// every note even if unchanged (a full rebuild without dropping the index), and
    /// `on_progress` fires after each embed batch so a slow full reindex under the
    /// real model shows a live progress line instead of looking frozen.
    pub fn reindex_with_progress(
        &self,
        force: bool,
        on_progress: &mut dyn FnMut(ingest::ReindexProgress),
    ) -> Result<ReindexReport> {
        let ingested = ingest::ingest_vault_with_progress(
            &self.conn,
            &self.root,
            &self.idgen,
            self.embedder.as_ref(),
            force,
            on_progress,
        )?;
        Ok(ReindexReport {
            indexed: ingested.len(),
            embedded: ingested.iter().filter(|i| i.embedded).count(),
            stamped: ingested.iter().filter(|i| i.stamped).count(),
        })
    }

    /// Preview a reindex (`reindex --dry-run`): report what [`reindex`](Self::reindex)
    /// **would** do — how many notes it would index, (re)embed, and stamp — with
    /// **no** writes: no `b2id` stamped to the Markdown (B2's one vault write,
    /// data-model.md §1), no index/log mutation, no embedding. `force` previews a
    /// full rebuild (every note would re-embed). A pure read, so it needs no model
    /// (the CLI opens with the fake for it, like `neighbors`).
    pub fn plan_reindex(&self, force: bool) -> Result<ReindexPlan> {
        let planned = ingest::plan_reindex(&self.conn, &self.root, force)?;
        Ok(ReindexPlan {
            would_index: planned.len(),
            would_embed: planned.iter().filter(|p| p.would_embed).count(),
            would_stamp: planned.iter().filter(|p| p.would_stamp).count(),
        })
    }

    /// Active neighbors of the note referenced by `note_ref` (path **or** `b2id`),
    /// each resolved to the other note's path + title for display. Errors with
    /// [`Error::NoteNotFound`] when the ref matches no indexed note (distinct from
    /// a found note that simply has no neighbors → an empty list).
    pub fn neighbors(&self, note_ref: &str) -> Result<Vec<NeighborView>> {
        let b2id = self.resolve_ref(note_ref)?;
        self.neighbors_of(&b2id)
    }

    /// The active neighbors of an already-resolved `b2id`, each resolved to the
    /// other note's path + title for display. Shared by [`neighbors`](Self::neighbors)
    /// and [`explain`](Self::explain) so the two present the same edge shape.
    fn neighbors_of(&self, b2id: &str) -> Result<Vec<NeighborView>> {
        let mut out = Vec::new();
        for n in graph::neighbors(&self.conn, b2id)? {
            let path = db::resolve_b2id_to_path(&self.conn, &n.other)?.unwrap_or_default();
            let title = db::note_title(&self.conn, &n.other)?;
            out.push(NeighborView {
                b2id: n.other,
                path,
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
            });
        }
        Ok(out)
    }

    /// Explain a note's connections (`b2 explain`): the note referenced by
    /// `note_ref` (path **or** `b2id`) resolved to its identity + title, together
    /// with every active typed edge and its "why". Errors with
    /// [`Error::NoteNotFound`] when the ref matches no indexed note; a found note
    /// with no edges returns an [`ExplainView`] with an empty `connections`. A pure
    /// graph read — no embedding, like [`neighbors`](Self::neighbors).
    pub fn explain(&self, note_ref: &str) -> Result<ExplainView> {
        let b2id = self.resolve_ref(note_ref)?;
        let path = db::resolve_b2id_to_path(&self.conn, &b2id)?.unwrap_or_default();
        let title = db::note_title(&self.conn, &b2id)?;
        let connections = self.neighbors_of(&b2id)?;
        Ok(ExplainView {
            b2id,
            path,
            title,
            connections,
        })
    }

    /// Hybrid search (BM25 ⊕ vector → RRF) resolved to notes, best first, capped at
    /// `limit` *notes*. Results are note-level: chunk hits are deduped to the
    /// highest-scoring chunk per note, so one note never appears twice.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // A never-reindexed vault has no vector space yet → no hits, no error
        // (open() no longer shapes it). This keeps `search` honest before the first
        // `reindex` instead of tripping over a missing `chunks_vec`.
        if !db::embedding_space_exists(&self.conn)? {
            return Ok(Vec::new());
        }
        // Fail fast if the index was built with a different model than the active
        // one: its vectors are incomparable with the query vector we'd produce, so
        // returning results would be silently wrong. The fix is a `reindex`.
        if let Some((indexed_model, indexed_dim)) = db::recorded_embedder(&self.conn)? {
            if indexed_model != self.embedder.model_id() || indexed_dim != self.embedder.dim() {
                return Err(Error::ModelMismatch {
                    indexed: format!("{indexed_model} (dim {indexed_dim})"),
                    active: format!("{} (dim {})", self.embedder.model_id(), self.embedder.dim()),
                });
            }
        }
        // Pull a wider chunk pool than `limit` so dedup can still fill `limit`
        // distinct notes when several top chunks share a note.
        let pool = limit.saturating_mul(3).max(limit);
        let mut out: Vec<SearchResult> = Vec::new();
        for hit in search::hybrid_search(&self.conn, self.embedder.as_ref(), query, pool)? {
            if out.iter().any(|r| r.b2id == hit.note_b2id) {
                continue; // note already represented by a higher-scoring chunk
            }
            let path = db::resolve_b2id_to_path(&self.conn, &hit.note_b2id)?.unwrap_or_default();
            let title = db::note_title(&self.conn, &hit.note_b2id)?;
            let snippet = db::chunk_text(&self.conn, hit.chunk_id)?
                .map(|t| snippet(&t))
                .unwrap_or_default();
            out.push(SearchResult {
                b2id: hit.note_b2id,
                path,
                title,
                score: hit.score,
                snippet,
            });
            if out.len() == limit {
                break;
            }
        }
        Ok(out)
    }

    /// Surface the notes most semantically similar to `note_ref` (path **or** `b2id`)
    /// that are **not already connected** to it — connection-discovery candidate
    /// generation ([`discover::candidates`]) exposed directly: vector KNN over the
    /// stored embeddings, minus the anchor's 1-hop graph neighbors, ranked by best
    /// chunk-pair max-similarity. Each result carries the candidate's path + title and
    /// the passage that made it similar. A **pure read over stored vectors — no model
    /// call** (a prior `reindex` supplies them), like [`neighbors`](Self::neighbors);
    /// `limit` bounds the count. Errors with [`Error::NoteNotFound`] for an unknown
    /// ref; returns an empty list for a known note with no vectors or no candidates.
    pub fn similar(&self, note_ref: &str, limit: usize) -> Result<Vec<SimilarView>> {
        let b2id = self.resolve_ref(note_ref)?;
        let mut out = Vec::new();
        for c in discover::candidates(&self.conn, &b2id, limit)? {
            let path = db::resolve_b2id_to_path(&self.conn, &c.note_b2id)?.unwrap_or_default();
            let title = db::note_title(&self.conn, &c.note_b2id)?;
            let evidence = db::chunk_text(&self.conn, c.evidence_chunk_id)?
                .map(|t| snippet(&t))
                .unwrap_or_default();
            out.push(SimilarView {
                b2id: c.note_b2id,
                path,
                title,
                score: c.score,
                evidence,
            });
        }
        Ok(out)
    }

    /// Commit a typed connection `src --type--> dst` (`b2 link`, Flow ③): append a
    /// typed-link string to the **source note's frontmatter `relations:`** (Markdown
    /// first, **never the body** — data-model.md §0) and re-project it as an
    /// `origin='frontmatter'` active edge. Both ends resolve by path **or** `b2id`.
    /// `edge_type` must be a **core** verb (data-model.md §2) — the CLI defaults it to
    /// `references`; a non-core verb errors with [`Error::InvalidRelation`] rather than
    /// silently storing a typo. **Idempotent:** if the directed `(src, dst, type)` edge
    /// already exists, nothing is written (`created: false`).
    ///
    /// Re-projection re-reads the source note (a frontmatter-only edit skips
    /// re-embedding, but ingest still takes the embedder), so the CLI opens the vault
    /// with the same embedder the index was built with, as for `add`/`mv`.
    pub fn link(
        &self,
        src_ref: &str,
        dst_ref: &str,
        edge_type: &str,
        explanation: Option<&str>,
    ) -> Result<LinkReport> {
        if !relation::is_core(edge_type) {
            return Err(Error::InvalidRelation(edge_type.to_string()));
        }
        let src_id = self.resolve_ref(src_ref)?;
        let dst_id = self.resolve_ref(dst_ref)?;
        let src_path = db::resolve_b2id_to_path(&self.conn, &src_id)?
            .ok_or_else(|| Error::NoteNotFound(src_ref.to_string()))?;
        let dst_full = db::resolve_b2id_to_path(&self.conn, &dst_id)?
            .ok_or_else(|| Error::NoteNotFound(dst_ref.to_string()))?;
        // The link path drops the `.md` Obsidian omits (matches how `[[links]]` are written).
        let dst_path = dst_full
            .strip_suffix(".md")
            .unwrap_or(&dst_full)
            .to_string();

        // Idempotent: don't append a duplicate frontmatter line for an existing edge.
        if db::edge_exists(&self.conn, &src_id, &dst_id, edge_type)? {
            return Ok(LinkReport {
                src_path,
                dst_path,
                relation: edge_type.to_string(),
                created: false,
            });
        }

        // Build the typed-link spec from the dst's current path + title.
        let link = match db::note_title(&self.conn, &dst_id)? {
            Some(title) => format!("[[{dst_path}|{title}]]"),
            None => format!("[[{dst_path}]]"),
        };
        let spec = match explanation {
            Some(e) => format!("{edge_type} {link} — {e}"),
            None => format!("{edge_type} {link}"),
        };

        // 1. Markdown first: append to frontmatter relations: (never the body, §0).
        let abs = self.root.join(&src_path);
        let mut parsed = note::parse(&fs::read_to_string(&abs)?);
        parsed.add_relation(&spec)?;
        fs::write(&abs, parsed.as_str())?;

        // 2. Re-project the source note so the edge re-materializes from the Markdown
        //    as origin='frontmatter' — a projection of the line just written, not an
        //    index write (data-model.md §3).
        ingest::ingest_file(
            &self.conn,
            &self.root,
            &src_path,
            &self.idgen,
            self.embedder.as_ref(),
        )?;

        Ok(LinkReport {
            src_path,
            dst_path,
            relation: edge_type.to_string(),
            created: true,
        })
    }

    /// Move/rename the note `note_ref` (path **or** `b2id`) to `to` (a
    /// vault-relative path; a `.md` suffix is optional), rewriting every inbound
    /// `[[oldpath|alias]]` link to the new path and re-projecting the index
    /// (user-stories.md Story 1). The graph never breaks — edges key on `b2id`, so
    /// `neighbors`/backlinks show the same set before and after; only the human
    /// convenience-copy link text is repaired. Errors with [`Error::NoteNotFound`]
    /// for an unknown source, or [`Error::MoveDestination`] /
    /// [`Error::MoveTargetExists`] for a bad or occupied destination.
    ///
    /// Rewriting an inbound file changes its body, so this **re-embeds** those
    /// files: the CLI opens the vault with the real model for `mv`, as for
    /// `reindex`/`link`.
    pub fn move_note(&self, note_ref: &str, to: &str) -> Result<MoveReport> {
        let b2id = self.resolve_ref(note_ref)?;
        let old_rel = db::resolve_b2id_to_path(&self.conn, &b2id)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))?;
        mv::move_note(
            &self.conn,
            &self.idgen,
            self.embedder.as_ref(),
            &self.root,
            &b2id,
            &old_rel,
            to,
        )
    }

    /// Create a new note (`b2 add`): write `path` (a vault-relative path; `.md`
    /// optional) with a minimal valid frontmatter (`type: note`, an optional
    /// `title`, today's `created`) and `content` as its body, then project it into
    /// the index — the created note is immediately searchable and in the graph.
    /// Errors with [`Error::AddDestination`] for a bad path or
    /// [`Error::AddTargetExists`] rather than clobber an existing file.
    ///
    /// Projection **embeds** the new note, so the CLI opens the vault with the real
    /// model for `add`, as for `reindex`/`link`/`mv`. The `b2id` is stamped by the
    /// ordinary ingest path, so the note is fully reconstructible from Markdown.
    pub fn add_note(
        &self,
        path: &str,
        title: Option<&str>,
        content: Option<&str>,
    ) -> Result<AddReport> {
        let created = self.today()?;
        add::add_note(
            &self.conn,
            &self.idgen,
            self.embedder.as_ref(),
            &self.root,
            path,
            title,
            content,
            &created,
        )
    }

    /// Today's date (`YYYY-MM-DD`) from **SQLite** — the same clock that stamps
    /// `indexed_at`, so `b2-core` needs no wall-clock crate and the façade is the
    /// determinism boundary (as it is for `idgen`). The vault convention for a note's
    /// `created:` field (data-model.md §1).
    fn today(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT strftime('%Y-%m-%d','now')", [], |r| r.get(0))?)
    }

    /// Resolve a note reference to a `b2id`: try it as a `b2id` first (exact PK
    /// lookup), then as a vault-relative path (`db::resolve_link_target` already
    /// tolerates the with/without-`.md` forms). Reuses the existing resolvers.
    fn resolve_ref(&self, note_ref: &str) -> Result<String> {
        if db::resolve_b2id_to_path(&self.conn, note_ref)?.is_some() {
            return Ok(note_ref.to_string());
        }
        db::resolve_link_target(&self.conn, note_ref)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))
    }
}

/// Collapse a chunk's text to a single-line, length-bounded snippet.
fn snippet(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SNIPPET_CHARS {
        flat
    } else {
        let cut: String = flat.chars().take(SNIPPET_CHARS).collect();
        format!("{}…", cut.trim_end())
    }
}
