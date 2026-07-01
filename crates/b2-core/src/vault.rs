//! The `Vault` façade — B2's one typed core API (vision-and-scope, "the
//! testability stack" point 1). Everything before this exists only as modules the
//! integration tests call directly; this is the single entry point the `b2` CLI
//! (and future adapters) are the sole clients of. It owns the open connection, the
//! durable `JsonlSink`, the embedder, and the id generator, and exposes *only what
//! the shipped commands need* — `open` / `reindex` / `neighbors` / `search`, plus
//! the suggestion lifecycle (`generate_suggestions` / `list_suggestions` /
//! `accept_suggestion` / `reject_suggestion`). Add operations when a command needs
//! them; do not pre-build a sprawling surface.
//!
//! A vault is one portable folder: the index and log live under `<root>/.b2/`
//! (data-model.md §4), so pointing B2 at a folder of Markdown is the whole setup.
//! The embedder is injected ([`open_with_embedder`](Vault::open_with_embedder)):
//! the `b2` CLI wires the candle-backed `LocalEmbedder` (real semantics), while
//! [`open`](Vault::open) defaults to the deterministic [`FakeEmbedder`] so the core
//! test suite stays fast and model-free (testability points 4–5). `search`'s BM25
//! (keyword) half is always real; the vector half is only semantic under a real
//! embedder — callers must not overstate the fake.

use crate::db;
use crate::discover::{self, GenerateOutcome};
use crate::embed::{Embedder, FakeEmbedder};
use crate::error::{Error, Result};
use crate::event::JsonlSink;
use crate::graph::{self, Direction};
use crate::id::UlidGen;
use crate::relate::FakeRelator;
use crate::{ingest, search, suggest};
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
/// `root/.b2/b2.sqlite`, with the durable event log beside it.
pub struct Vault {
    root: PathBuf,
    conn: Connection,
    sink: JsonlSink,
    // Injected through the seam: the CLI wires the real candle model; `open`
    // defaults to `FakeEmbedder` so the core tests stay deterministic and model-free
    // (the "build for tomorrow's model" seam, vision-and-scope).
    embedder: Box<dyn Embedder>,
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

/// One pending suggestion, resolved for display: the typed edge with both ends'
/// paths + titles and its decision fuel. `edge_id` is the handle `accept`/`reject`
/// take (so the CLI stays a dumb printer).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SuggestionView {
    pub edge_id: String,
    /// The source note (the anchor the edge points *from*).
    pub src_path: String,
    pub src_title: Option<String>,
    /// The target note (`dst_path` falls back to the raw link path if the target is
    /// dangling — no indexed note).
    pub dst_path: String,
    pub dst_title: Option<String>,
    /// The proposed relation verb (data-model.md §2 core set).
    pub relation: String,
    /// The relator's "why".
    pub explanation: Option<String>,
    /// `0.0–1.0` triage prior, if the relator gave one.
    pub confidence: Option<f64>,
    /// Provenance `by` — `agent:<model_id>` (data-model.md §4).
    pub by: String,
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
        // `Connection::open` creates the DB file but not its parent, and the log
        // sink creates `.b2/log/`; make `.b2/` first so both land in one place.
        fs::create_dir_all(vault_root.join(".b2"))?;
        let sink = JsonlSink::in_vault(vault_root)?;
        let conn = db::open(&vault_root.join(".b2").join("b2.sqlite"))?;
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
        self.reindex_with_progress(&mut |_| {})
    }

    /// [`reindex`](Self::reindex), but calls `on_progress` after each embed batch —
    /// the seam the CLI uses to show a live progress line, since embedding a large
    /// vault under the real model is the one slow step and would otherwise look
    /// frozen. The report is identical to `reindex`'s.
    pub fn reindex_with_progress(
        &self,
        on_progress: &mut dyn FnMut(ingest::ReindexProgress),
    ) -> Result<ReindexReport> {
        let ingested = ingest::ingest_vault_with_progress(
            &self.conn,
            &self.root,
            &self.idgen,
            &self.sink,
            self.embedder.as_ref(),
            on_progress,
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

    /// Generate connection-discovery suggestions across the whole vault (Flow ③
    /// generate): every note as an anchor, its complement candidates judged by the
    /// relator, each fired-and-core proposal written to the queue + durable log.
    /// **Idempotent** — a pair already active, pending, or rejected is never
    /// re-proposed — so it is safe to run repeatedly. `top_n` bounds the candidates
    /// considered per anchor. Returns the run tally.
    ///
    /// The relator is the deterministic [`FakeRelator`] for now; the real
    /// LLM-backed relator drops in through the same seam later (its own crate,
    /// mirroring the embedder). So these are **stub** proposals, not real
    /// judgments — the CLI must say so and never overstate them. Candidate
    /// generation reads the *stored* vectors, so this needs no live embedder (a
    /// prior `reindex` is enough).
    pub fn generate_suggestions(&self, top_n: usize) -> Result<GenerateOutcome> {
        let now = self.now()?;
        discover::generate_all(
            &self.conn,
            &self.sink,
            &self.idgen,
            &FakeRelator::new(),
            top_n,
            &now,
        )
    }

    /// The live review queue: every pending suggestion, each resolved to both ends'
    /// paths + titles for display, ordered oldest-evidence-first (as
    /// [`suggest::list_suggestions`] returns them).
    pub fn list_suggestions(&self) -> Result<Vec<SuggestionView>> {
        let mut out = Vec::new();
        for s in suggest::list_suggestions(&self.conn)? {
            let src_path = db::resolve_b2id_to_path(&self.conn, &s.src_id)?.unwrap_or_default();
            let src_title = db::note_title(&self.conn, &s.src_id)?;
            let (dst_path, dst_title) = match &s.dst_id {
                Some(d) => (
                    db::resolve_b2id_to_path(&self.conn, d)?
                        .unwrap_or_else(|| s.dst_path_raw.clone()),
                    db::note_title(&self.conn, d)?,
                ),
                None => (s.dst_path_raw.clone(), None),
            };
            out.push(SuggestionView {
                edge_id: s.edge_id,
                src_path,
                src_title,
                dst_path,
                dst_title,
                relation: s.edge_type,
                explanation: s.explanation,
                confidence: s.confidence,
                by: s.by,
            });
        }
        Ok(out)
    }

    /// Accept the pending suggestion `edge_id` (Flow ③): append its typed link to
    /// the source note's frontmatter `relations:` and re-project it as an active,
    /// authored edge. Returns `false` if `edge_id` is not a pending suggestion (or
    /// is dangling, with no concrete target to link).
    ///
    /// Re-projection **re-embeds the source note**, so the caller must have opened
    /// the vault with the same embedder the index was built with (the CLI loads the
    /// real model for `accept`, as for `reindex`).
    pub fn accept_suggestion(&self, edge_id: &str) -> Result<bool> {
        let now = self.now()?;
        suggest::accept_suggestion(
            &self.conn,
            &self.sink,
            self.embedder.as_ref(),
            &self.root,
            edge_id,
            &now,
        )
    }

    /// Reject the pending suggestion `edge_id`: tombstone it so the same pair+type
    /// is never proposed again. Returns `false` if `edge_id` is not a pending
    /// suggestion (nothing to reject). Touches no vectors — needs no embedder.
    pub fn reject_suggestion(&self, edge_id: &str) -> Result<bool> {
        if suggest::get_suggestion(&self.conn, edge_id)?.is_none() {
            return Ok(false);
        }
        let now = self.now()?;
        suggest::reject_suggestion(&self.conn, &self.sink, edge_id, &now)?;
        Ok(true)
    }

    /// The current UTC time as an ISO-8601 string, taken from **SQLite** — the same
    /// clock that stamps `indexed_at` — so `b2-core` needs no wall-clock crate and
    /// the engine ops still take the timestamp as a param (the façade is the
    /// determinism boundary, exactly as it is for `idgen`).
    fn now(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
                r.get(0)
            })?)
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
