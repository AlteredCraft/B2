//! The `Vault` façade — B2's one typed core API (vision-and-scope, "the
//! testability stack" point 1). Everything before this exists only as modules the
//! integration tests call directly; this is the single entry point the `b2` CLI
//! (and future adapters) are the sole clients of. It owns the open connection, the
//! durable `JsonlSink`, the embedder, and the id generator, and exposes *only what
//! this slice needs* — `open` / `reindex` / `neighbors` / `search`. Add operations
//! when a command needs them; do not pre-build a sprawling surface.
//!
//! A vault is one portable folder: the index and log live under `<root>/.b2/`
//! (data-model.md §4), so pointing B2 at a folder of Markdown is the whole setup.
//! The embedder is the deterministic [`FakeEmbedder`] for now — `search`'s BM25
//! (keyword) half is real; the vector half is *not yet semantic* until the local
//! model drops into the seam (index-engine.md §6), and callers must not overstate it.

use crate::db;
use crate::embed::{Embedder, FakeEmbedder};
use crate::error::{Error, Result};
use crate::event::JsonlSink;
use crate::graph::{self, Direction};
use crate::id::UlidGen;
use crate::{ingest, search};
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// The embedding dimension the fake embedder runs at. Small and fixed; the real
/// model will bring its own `dim` when it lands (and a model swap re-embeds — see
/// [`db::ensure_embedding_space`]).
const EMBED_DIM: usize = 64;

/// Longest snippet (in chars) shown for a search hit, so a result stays one line.
const SNIPPET_CHARS: usize = 160;

/// An open vault: the Markdown at `root`, projected into the disposable index at
/// `root/.b2/b2.sqlite`, with the durable event log beside it.
pub struct Vault {
    root: PathBuf,
    conn: Connection,
    sink: JsonlSink,
    // Concrete for now; every engine fn already takes `&dyn Embedder`, so swapping
    // in the real local model is a one-line change here (the "build for tomorrow's
    // model" seam, vision-and-scope).
    embedder: FakeEmbedder,
    idgen: UlidGen,
}

/// What `reindex` did: how many notes were projected, and how many needed a `b2id`
/// stamped (B2's one always-allowed write to the vault, data-model.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReindexReport {
    pub indexed: usize,
    pub stamped: usize,
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

impl Vault {
    /// Open the vault rooted at `vault_root`, creating `<root>/.b2/` (index + log)
    /// if absent. Idempotent: safe on a fresh folder or an already-built vault.
    pub fn open(vault_root: &Path) -> Result<Self> {
        // `Connection::open` creates the DB file but not its parent, and the log
        // sink creates `.b2/log/`; make `.b2/` first so both land in one place.
        fs::create_dir_all(vault_root.join(".b2"))?;
        let sink = JsonlSink::in_vault(vault_root)?;
        let conn = db::open(&vault_root.join(".b2").join("b2.sqlite"))?;
        let embedder = FakeEmbedder::new(EMBED_DIM);
        // Shape the embedding space now (idempotent; ingest does the same), so a
        // `search` on a freshly opened, not-yet-reindexed vault returns empty
        // rather than tripping over a missing `chunks_vec`.
        db::ensure_embedding_space(&conn, embedder.model_id(), embedder.dim())?;
        Ok(Self {
            root: vault_root.to_path_buf(),
            conn,
            sink,
            embedder,
            idgen: UlidGen,
        })
    }

    /// Re-project every `.md` note under the vault root into the index (Flow ①):
    /// notes, chunks (+embeddings), and the typed graph. Stamps any missing `b2id`.
    pub fn reindex(&self) -> Result<ReindexReport> {
        let ingested = ingest::ingest_vault(
            &self.conn,
            &self.root,
            &self.idgen,
            &self.sink,
            &self.embedder,
        )?;
        Ok(ReindexReport {
            indexed: ingested.len(),
            stamped: ingested.iter().filter(|i| i.stamped).count(),
        })
    }

    /// Active neighbors of the note referenced by `note_ref` (path **or** `b2id`),
    /// each resolved to the other note's path + title for display. Errors with
    /// [`Error::NoteNotFound`] when the ref matches no indexed note (distinct from
    /// a found note that simply has no neighbors → an empty list).
    pub fn neighbors(&self, note_ref: &str) -> Result<Vec<NeighborView>> {
        let b2id = self.resolve_ref(note_ref)?;
        let mut out = Vec::new();
        for n in graph::neighbors(&self.conn, &b2id)? {
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
            });
        }
        Ok(out)
    }

    /// Hybrid search (BM25 ⊕ vector → RRF) resolved to notes, best first, capped at
    /// `limit` *notes*. Results are note-level: chunk hits are deduped to the
    /// highest-scoring chunk per note, so one note never appears twice.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // Pull a wider chunk pool than `limit` so dedup can still fill `limit`
        // distinct notes when several top chunks share a note.
        let pool = limit.saturating_mul(3).max(limit);
        let mut out: Vec<SearchResult> = Vec::new();
        for hit in search::hybrid_search(&self.conn, &self.embedder, query, pool)? {
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
