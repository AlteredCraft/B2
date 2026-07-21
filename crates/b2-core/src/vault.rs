//! The `Vault` façade — B2's one typed core API (vision-and-scope, "the
//! testability stack" point 1). Everything before this exists only as modules the
//! integration tests call directly; this is the single entry point the `b2` CLI
//! (and future adapters) are the sole clients of. It owns the open connection, the
//! embedder, and the id generator, and exposes *only what the shipped commands need*
//! — `open` / `reindex` / `project` / `embed` / `read` / `write` / `neighbors` /
//! `explain` / `search` / `search_chunks` / `similar` / `link` / `add` / `create` /
//! `mv` / `rm`. Add
//! operations when a command needs them; do not pre-build a sprawling surface.
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
use crate::chunk::ChunkConfig;
use crate::db;
use crate::dirs;
use crate::discover;
use crate::embed::{Embedder, FakeEmbedder};
use crate::error::{Error, Result};
use crate::graph::{self, Direction};
use crate::id::UlidGen;
use crate::mv;
use crate::rm;
use crate::{ingest, note, relation, search};
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

/// Re-exported for the same reason: [`create_dir`](Vault::create_dir)'s report is
/// part of the façade contract.
pub use crate::dirs::DirCreateReport;
/// Re-exported so a `Vec<SkippedNote>` on [`ReindexReport`]/[`ProjectReport`] is
/// nameable through the façade — the one typed contract adapters import from.
pub use crate::ingest::SkippedNote;
/// Re-exported for the same reason: [`move_dir`](Vault::move_dir)'s report is
/// part of the façade contract.
pub use crate::mv::DirMoveReport;
/// Re-exported for the same reason: [`move_note`](Vault::move_note)'s and
/// [`move_resource`](Vault::move_resource)'s reports are part of the façade
/// contract.
pub use crate::mv::{MoveReport, ResourceMoveReport};
/// Re-exported for the same reason: the delete family's reports
/// ([`delete_note`](Vault::delete_note) / [`delete_resource`](Vault::delete_resource)
/// / [`delete_dir`](Vault::delete_dir)) are part of the façade contract.
pub use crate::rm::{DeleteReport, DirDeleteReport, ResourceDeleteReport};

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
    // The vault's one chunking policy (chunk.rs, spec §3 D5). Held here — not
    // re-defaulted per call — so every path that chunks (reindex/project/write/
    // add/link/mv) cuts identically under a *fixed* config and `incremental ≡
    // full rebuild` holds by construction. Across a `set_chunk_config` change the
    // guarantee is doc-enforced instead: an incremental pass would reuse chunks
    // cut under the old policy, so a config change must pair with
    // `project(force)` (as `set_chunk_config`'s doc requires and the eval does).
    // Defaults to `ChunkConfig::default()`; the retrieval eval is the one client
    // that overrides it, to A/B chunker levers in-process (specs/eval-strategy.md).
    chunk_config: ChunkConfig,
}

/// What `reindex` did: how many notes were projected, how many were actually
/// (re)embedded (the rest reused their vectors — incremental), and how many needed
/// a `b2id` stamped (B2's one always-allowed write to the vault, data-model.md §1).
///
/// `cancelled` is `true` when a cooperative cancel cut the embed phase short
/// (async-indexing.md §3): the counts then describe the partial work truthfully
/// (e.g. "indexed 1000, embedded 240, cancelled") — the index is still consistent
/// (keyword + graph complete, a prefix embedded) and an incremental re-run finishes
/// the rest. Always `false` for [`reindex`](Vault::reindex) and the CLI, which never
/// cancel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReindexReport {
    pub indexed: usize,
    pub embedded: usize,
    pub stamped: usize,
    pub cancelled: bool,
    /// Files skipped as unreadable this run (see [`SkippedNote`]) — a whole-vault
    /// reindex never fails on one bad file (non-UTF-8, permission-denied). Empty on a
    /// clean vault; each entry names the file and a short, file-level reason.
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned this run (#31): notes whose files were deleted outside b2
    /// with no replacement, reconciled so incremental equals a from-scratch rebuild.
    pub notes_pruned: usize,
    /// The resource inventory's counts (file-type support slice 1): resources seen
    /// this run, and stale inventory rows pruned.
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// What [`project`](Vault::project) did — the model-free half of a reindex
/// (projection-embedding-split.md §4): how many notes were projected and how many
/// needed a `b2id` stamped. No embed counts: projection never touches vectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectReport {
    pub indexed: usize,
    pub stamped: usize,
    /// Files skipped as unreadable this pass (see [`SkippedNote`]) — projecting a large
    /// vault never fails on one bad file. Empty on a clean vault; surfaced so an
    /// adapter can tell the user which files were left out and why.
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned this pass (#31): notes whose files were deleted outside b2
    /// with no replacement, reconciled so incremental equals a from-scratch rebuild.
    pub notes_pruned: usize,
    /// The resource inventory's counts (file-type support slice 1): resources seen
    /// this pass, and stale inventory rows pruned.
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// What [`embed`](Vault::embed) did — the model-bound half of a reindex: how many
/// notes had missing vectors filled (the rest were already complete), and whether a
/// cooperative cancel cut the pass short (the counts then describe the partial work
/// truthfully, and a re-run embeds exactly the remainder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EmbedReport {
    pub embedded: usize,
    pub cancelled: bool,
}

/// The vault's semantic-embedding coverage — how many of its notes are fully
/// embedded — for the honest "N/M embedded" signal (#26, projection-embedding-split.md
/// §5). Model-free: a pure count over the projection, so an adapter can surface
/// "keyword-only for now" *precisely* (not just via the binary "is a model installed"
/// flag) without loading the model. `embedded == total` (and `total > 0`) means
/// semantic ranking is complete; `embedded < total` means [`search`](Vault::search) is
/// running keyword-first over the unembedded remainder (`embedded == 0` = fully
/// keyword-only, a projected-but-unembedded vault).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EmbedStatus {
    /// Notes whose every chunk has a stored vector.
    pub embedded: usize,
    /// Every projected note (the denominator).
    pub total: usize,
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
    /// The other note's `created` date, if it has one — resolved from the
    /// projection (GH #22), so an adapter can date a neighbor without re-reading
    /// the file.
    pub created: Option<String>,
}

/// One outbound link a note authors at a **resource** (an image, a PDF — any
/// non-`.md` vault file), resolved for display. The third target kind an edge can
/// have (note / resource / dangling); surfaced on [`ExplainView`] so a note's file
/// links are visible from the note's side, not only as the resource's backlinks
/// (GH #22). Distinct from [`NeighborView`] — a resource has no `b2id`, no title,
/// and no direction (a resource never authors edges, so these are always outbound).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceLinkView {
    /// The resource's vault-relative path.
    pub path: String,
    /// Its inventory class (`image`/`pdf`/`html`/`text`/`media`/`binary`).
    pub class: String,
    /// The relation verb (`references` for a bare link/embed).
    pub relation: String,
    /// Edge origin — `"inline"` (a body link) or `"frontmatter"`.
    pub origin: String,
    /// The authored caption (alt text / `|caption`), if any.
    pub caption: Option<String>,
    /// Whether the link is an embed (`![…]` / `![[…]]`).
    pub embed: bool,
    pub explanation: Option<String>,
}

/// One outbound link a note authored that resolves to **nothing** — no note and no
/// resource exists at its target (a `[[Hermes]]` naming a *folder*, or a plain
/// typo). A note is one `.md` file (data-model.md §1), so a folder is never a valid
/// target; rather than silently drop such a link, B2 surfaces it as *unresolved* so
/// it reads as broken, not missing (GH #12). Has no `b2id`/`path` — the whole point
/// is that nothing resolved — so it is a distinct shape from [`NeighborView`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnresolvedLink {
    /// The target exactly as written in the Markdown (`[[target]]`) — e.g. `Hermes`.
    pub target: String,
    /// The relation verb (`references` for a bare link).
    pub relation: String,
    /// Edge origin — `"inline"` (a body link) or `"frontmatter"` (a `b2_relations:`
    /// entry).
    pub origin: String,
    pub explanation: Option<String>,
}

/// A note's full connection picture for `b2 explain`: the note itself (resolved to
/// its identity + display fields), every active connection with its "why", and any
/// **unresolved** outbound links (dangling — the target names no note or resource).
/// A thin header over [`NeighborView`] — `explain`'s job is to present a note's typed
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
    /// Outbound links at **resources** (images, PDFs, …) — the third target kind,
    /// so a note's file links are visible from the note's side (GH #22). Empty
    /// when the note links no files.
    pub resources: Vec<ResourceLinkView>,
    /// Outbound links that resolved to nothing (a folder target or a typo) — shown
    /// as broken rather than silently dropped (GH #12). Empty when every link
    /// resolves.
    pub unresolved: Vec<UnresolvedLink>,
}

/// A note's content + display metadata for a reader — the Desktop UI MVP's left
/// pane (specs/completed/desktop-ui-mvp.md §4), and the **one new façade op** that surface
/// adds. Carries the note's identity, the frontmatter fields worth showing a human,
/// and the **raw Markdown body read from disk** (the source of truth, not the index
/// projection) so an adapter renders Markdown → HTML itself. A pure read — no
/// embedding, like [`neighbors`](Vault::neighbors).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteView {
    pub b2id: String,
    pub path: String,
    pub title: Option<String>,
    pub r#type: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
    /// The note's Markdown body (frontmatter stripped), verbatim from disk.
    pub body: String,
    /// The raw frontmatter YAML **verbatim** (the text between the `---` fences,
    /// fences excluded), or `None` when the note has none. This is the byte-honest
    /// block, not a re-serialization of the projected fields above — so `b2_relations:`
    /// and any keys B2 doesn't model show as written. The Desktop UI renders it in a
    /// collapsible drawer (specs/completed/desktop-ui-mvp.md §4).
    pub frontmatter: Option<String>,
    /// blake3 of the **raw file bytes** at read time — the save-guard token
    /// (desktop-editing.md §3/§4): [`write`](Vault::write) refuses when the file on
    /// disk no longer hashes to the revision the edit was based on, so a save can
    /// never silently clobber an external edit. Whole-file (not just the body), so
    /// *any* out-of-band change conflicts honestly.
    pub revision: String,
}

/// One note's identity for a listing — `b2id`, vault-relative `path`, and display
/// `title` — with **no body** (the heavy field). This is what the desktop UI's file
/// tree renders: enough to show and open a note, cheap enough to fetch the whole
/// vault at once. The full body is a separate [`read`](Vault::read) when a note is
/// opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteSummary {
    pub b2id: String,
    pub path: String,
    pub title: Option<String>,
}

/// One resource's identity for the file tree (`Vault::list_resources`) — the
/// per-kind sibling of [`NoteSummary`], never a union type (research §9b #10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceSummary {
    pub path: String,
    pub class: String,
    pub size: i64,
    pub mtime: Option<i64>,
}

/// The fallback card's data (`Vault::explain_resource`, slice-1 spec §4):
/// the resource's inventory metadata plus its inbound backlinks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceExplainView {
    pub path: String,
    pub class: String,
    pub size: i64,
    pub mtime: Option<i64>,
    pub content_hash: String,
    pub backlinks: Vec<ResourceBacklink>,
}

/// One note that links at a resource, with the edge's authored context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceBacklink {
    pub b2id: String,
    pub path: String,
    pub title: Option<String>,
    pub r#type: String,
    pub caption: Option<String>,
    pub embed: bool,
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

/// One **chunk-level** search hit — the sub-note view of [`search`](Vault::search).
/// Same retrieval (BM25 ⊕ vector → RRF, keyword-only fallback), but ranked chunks
/// are returned as-is instead of deduped up to notes, so a caller can see *which
/// passage* matched and at what rank. The client is the out-of-CI retrieval eval
/// (specs/eval-strategy.md): note-rank scoring is blind to sub-note retrieval
/// quality — exactly what chunking levers move — so the eval scores passage ranks
/// through this view. Carries the chunk's **full text** (not a display snippet):
/// the eval anchors passage-containment scoring on it; an adapter wanting a
/// one-liner trims it itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChunkSearchResult {
    pub b2id: String,
    pub path: String,
    /// The chunk's heading breadcrumb (`"Fermentation > Vegetables"`), when the
    /// chunker recorded one.
    pub heading_path: Option<String>,
    /// Fused relevance score; higher is better.
    pub score: f64,
    /// The chunk's stored text, verbatim.
    pub text: String,
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

/// What [`write`](Vault::write) did: the saved note's vault-relative path and the
/// **new revision** (blake3 of the final on-disk bytes) — the token the editor
/// chains its next save on, so sequential saves never self-conflict
/// (desktop-editing.md §3, "last save wins — by construction").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteReport {
    pub path: String,
    pub revision: String,
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
    /// tasks.md / index-engine.md §8): shaping the vector tables and any re-embed happen
    /// only on `reindex`. That way changing the configured model can never silently
    /// wipe vectors on the next command — a mismatch is caught, and fixed, at
    /// `reindex`; `search` fails fast on it (see [`search`](Self::search)).
    pub fn open_with_embedder(vault_root: &Path, embedder: Box<dyn Embedder>) -> Result<Self> {
        // Every façade op opens a `tracing` span (target `b2::vault`): under a
        // subscriber with span-close events, each op reports its own duration, and
        // the per-query `b2::sqlite` events carry which op they ran under. Inert
        // (near-zero cost) until an adapter installs a subscriber — the determinism
        // boundary is unchanged, since the core itself never reads a clock for this.
        let _op = tracing::debug_span!(target: "b2::vault", "open").entered();
        // `Connection::open` creates the DB file but not its parent; make `.b2/` first.
        fs::create_dir_all(vault_root.join(".b2"))?;
        let conn = db::open(&vault_root.join(".b2").join("b2.sqlite"))?;
        Ok(Self {
            root: vault_root.to_path_buf(),
            conn,
            embedder,
            idgen: UlidGen,
            chunk_config: ChunkConfig::default(),
        })
    }

    /// Override the vault's chunking policy (default: [`ChunkConfig::default()`]).
    /// Every subsequent op that chunks — `project`/`reindex`/`write`/`add`/`link`/
    /// `mv` — cuts with this config, so the index stays self-consistent. The
    /// client is the out-of-CI retrieval eval, which sweeps chunker levers in one
    /// process (`set_chunk_config` → `project(force)` → `embed` → score;
    /// specs/eval-strategy.md); the shipped adapters never call it. Changing the
    /// config does **not** re-chunk by itself — pair it with `project(force)`.
    pub fn set_chunk_config(&mut self, cfg: ChunkConfig) {
        self.chunk_config = cfg;
    }

    /// Re-project every `.md` note under the vault root into the index (Flow ①):
    /// notes, chunks (+embeddings), and the typed graph. Stamps any missing `b2id`.
    /// **Incremental** — a note whose body is unchanged reuses its vectors rather
    /// than re-embedding (see [`reindex_with_progress`](Self::reindex_with_progress)
    /// to force a full re-embed or observe progress).
    pub fn reindex(&self) -> Result<ReindexReport> {
        self.reindex_with_progress(false, &mut |_| ControlFlow::Continue(()))
    }

    /// [`reindex`](Self::reindex) with three knobs its adapters need: `force`
    /// re-embeds every note even if unchanged (a full rebuild without dropping the
    /// index); `on_progress` fires after each embed batch so a slow full reindex under
    /// the real model shows a live progress line instead of looking frozen; and the
    /// callback's [`ControlFlow`] return **cooperatively cancels** the embed phase —
    /// returning [`ControlFlow::Break`] stops embedding at that batch boundary while
    /// Phase 2 still completes, leaving a consistent, resumable index
    /// (async-indexing.md §3). The desktop host maps a cancel flag to `Break`; the CLI
    /// always returns `Continue` (no behavior change for the non-cancel path, which
    /// stays byte-identical). A cancelled run sets [`ReindexReport::cancelled`].
    pub fn reindex_with_progress(
        &self,
        force: bool,
        on_progress: &mut dyn FnMut(ingest::ReindexProgress) -> ControlFlow<()>,
    ) -> Result<ReindexReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "reindex", force).entered();
        let ingested = ingest::ingest_vault_with_progress(
            &self.conn,
            &self.root,
            &self.idgen,
            &self.chunk_config,
            self.embedder.as_ref(),
            force,
            on_progress,
        )?;
        Ok(ReindexReport {
            indexed: ingested.notes.len(),
            embedded: ingested.notes.iter().filter(|i| i.embedded).count(),
            stamped: ingested.notes.iter().filter(|i| i.stamped).count(),
            cancelled: ingested.cancelled,
            skipped: ingested.skipped,
            notes_pruned: ingested.notes_pruned,
            resources_indexed: ingested.resources_indexed,
            resources_pruned: ingested.resources_pruned,
        })
    }

    /// The **projection pass** alone (projection-embedding-split.md §4): re-project
    /// every `.md` note into `notes`/`chunks`(+FTS)/`edges` — stamping missing
    /// `b2id`s — with **no model and no vector work**. After it returns, the file
    /// tree lists, notes open, keyword search answers, and the graph resolves; only
    /// vectors (and thus `similar` / semantic ranking) wait for
    /// [`embed`](Self::embed). `force` re-chunks every note (clearing its stale
    /// vectors), so `project(force)` + `embed` is a full rebuild.
    /// [`reindex`](Self::reindex) remains the composition of the two passes.
    ///
    /// *(The invariant's "index = projection of Markdown" still means the* full
    /// *index — `project` + `embed` together; this op is named for the row-projection
    /// pass it runs.)*
    pub fn project(&self, force: bool) -> Result<ProjectReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "project", force).entered();
        let outcome = ingest::project_vault(
            &self.conn,
            &self.root,
            &self.idgen,
            &self.chunk_config,
            force,
        )?;
        Ok(ProjectReport {
            indexed: outcome.notes.len(),
            stamped: outcome.notes.iter().filter(|n| n.stamped).count(),
            skipped: outcome.skipped,
            notes_pruned: outcome.notes_pruned,
            resources_indexed: outcome.resources_indexed,
            resources_pruned: outcome.resources_pruned,
        })
    }

    /// The **embed pass** alone: fill a vector for every chunk that lacks one — the
    /// pending set is derived from the index itself (chunks with no `embeddings`
    /// row), so this needs no prior [`project`](Self::project) call in the same
    /// process and heals any interruption (a cancelled embed, a crash between the
    /// passes) by embedding exactly what is still missing. Progress and cooperative
    /// cancel behave as in [`reindex_with_progress`](Self::reindex_with_progress),
    /// and progress is determinate from the first batch (the pending notes are
    /// counted up front). Runs under the vault's injected embedder — semantically
    /// useful only with the real model (the CLI/desktop wire it), deterministic
    /// under the fake (tests).
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

    /// Preview a reindex (`reindex --dry-run`): report what [`reindex`](Self::reindex)
    /// **would** do — how many notes it would index, (re)embed, and stamp — with
    /// **no** writes: no `b2id` stamped to the Markdown (B2's one vault write,
    /// data-model.md §1), no index/log mutation, no embedding. `force` previews a
    /// full rebuild (every note would re-embed). A pure read, so it needs no model
    /// (the CLI opens with the fake for it, like `neighbors`).
    pub fn plan_reindex(&self, force: bool) -> Result<ReindexPlan> {
        let _op = tracing::debug_span!(target: "b2::vault", "plan_reindex", force).entered();
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
        let _op = tracing::debug_span!(target: "b2::vault", "neighbors", note = note_ref).entered();
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
            let created = db::note_created(&self.conn, &n.other)?;
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
                created,
            });
        }
        Ok(out)
    }

    /// The resource links of an already-resolved `b2id` — every outbound edge at a
    /// non-`.md` file, resolved with its inventory class for display (GH #22).
    fn resource_links_of(&self, b2id: &str) -> Result<Vec<ResourceLinkView>> {
        Ok(db::outbound_resource_edges(&self.conn, b2id)?
            .into_iter()
            .map(
                |(path, class, relation, origin, caption, embed, explanation)| ResourceLinkView {
                    path,
                    class,
                    relation,
                    origin,
                    caption,
                    embed,
                    explanation,
                },
            )
            .collect())
    }

    /// The unresolved (dangling) outbound links of an already-resolved `b2id` —
    /// authored links whose target names no note and no resource (a folder or a
    /// typo). Shared by [`explain`](Self::explain) and
    /// [`unresolved_links`](Self::unresolved_links) so both present the same shape.
    fn unresolved_of(&self, b2id: &str) -> Result<Vec<UnresolvedLink>> {
        Ok(graph::unresolved_outbound(&self.conn, b2id)?
            .into_iter()
            .map(|u| UnresolvedLink {
                target: u.target,
                relation: u.edge_type,
                origin: u.origin,
                explanation: u.explanation,
            })
            .collect())
    }

    /// The unresolved (dangling) outbound links of the note referenced by `note_ref`
    /// (path **or** `b2id`): links it authored that resolve to no note and no
    /// resource — a `[[Hermes]]` naming a *folder*, or a typo (GH #12). Surfaced so a
    /// broken link is visible, not silently dropped; `b2 neighbors`/`b2 explain` show
    /// them. Errors with [`Error::NoteNotFound`] for an unknown ref; a note whose
    /// every link resolves returns an empty list. A pure graph read — no embedding.
    pub fn unresolved_links(&self, note_ref: &str) -> Result<Vec<UnresolvedLink>> {
        let _op = tracing::debug_span!(target: "b2::vault", "unresolved_links", note = note_ref)
            .entered();
        let b2id = self.resolve_ref(note_ref)?;
        self.unresolved_of(&b2id)
    }

    /// Explain a note's connections (`b2 explain`): the note referenced by
    /// `note_ref` (path **or** `b2id`) resolved to its identity + title, together
    /// with every active typed edge and its "why", its outbound **resource** links
    /// (images/PDFs — the third target kind, GH #22), plus any **unresolved**
    /// outbound links (dangling — a folder target or a typo, surfaced not
    /// dropped, GH #12).
    /// Errors with [`Error::NoteNotFound`] when the ref matches no indexed note; a
    /// found note with no edges returns an [`ExplainView`] with empty `connections`
    /// and `unresolved`. A pure graph read — no embedding, like
    /// [`neighbors`](Self::neighbors).
    pub fn explain(&self, note_ref: &str) -> Result<ExplainView> {
        let _op = tracing::debug_span!(target: "b2::vault", "explain", note = note_ref).entered();
        let b2id = self.resolve_ref(note_ref)?;
        let path = db::resolve_b2id_to_path(&self.conn, &b2id)?.unwrap_or_default();
        let title = db::note_title(&self.conn, &b2id)?;
        let connections = self.neighbors_of(&b2id)?;
        let resources = self.resource_links_of(&b2id)?;
        let unresolved = self.unresolved_of(&b2id)?;
        Ok(ExplainView {
            b2id,
            path,
            title,
            connections,
            resources,
            unresolved,
        })
    }

    /// Read a note for display (`Vault::read`) — the Desktop UI MVP's left pane and
    /// the one new façade op that surface adds (specs/completed/desktop-ui-mvp.md §4). Resolve
    /// `note_ref` (path **or** `b2id`) to its file and return the note's **raw
    /// Markdown body from disk** (the source of truth, not the index projection) plus
    /// the frontmatter metadata worth showing a reader. A pure read — no embedding,
    /// like [`neighbors`](Self::neighbors) — so an adapter needs no model just to
    /// render a note; path/`b2id` resolution is centralized here so the adapter never
    /// touches the filesystem itself. Errors with [`Error::NoteNotFound`] for an
    /// unknown ref.
    pub fn read(&self, note_ref: &str) -> Result<NoteView> {
        let _op = tracing::debug_span!(target: "b2::vault", "read", note = note_ref).entered();
        let b2id = self.resolve_ref(note_ref)?;
        let path = db::resolve_b2id_to_path(&self.conn, &b2id)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))?;
        let raw = fs::read_to_string(self.root.join(&path))?;
        let revision = revision_of(&raw);
        let parsed = note::parse(&raw);
        let fields = parsed.fields();
        // Display title is the filename (data-model.md §1); the frontmatter `title:`
        // is inert. Derived from the path here so even a not-yet-reindexed note shows
        // its filename in the pane header, matching the projected `notes.title`.
        let title = Some(note::display_title(&path));
        Ok(NoteView {
            b2id,
            path,
            title,
            r#type: fields.r#type.clone(),
            created: fields.created.clone(),
            updated: fields.updated.clone(),
            tags: fields.tags.clone(),
            body: parsed.body().to_string(),
            frontmatter: parsed.frontmatter().map(str::to_string),
            revision,
        })
    }

    /// Save a note's **body** (`Vault::write`, desktop-editing.md §4) — the editing
    /// surface's one write op. Markdown-first and **model-free**: validate that the
    /// file on disk still hashes to `base_revision` (else [`Error::WriteConflict`] —
    /// an external editor changed it; nothing is written), splice `body` in
    /// **verbatim** after the untouched frontmatter ([`note::ParsedNote::replace_body`]),
    /// write the file, and re-project the note ([`ingest::project_file`] — chunks +
    /// FTS + edges; a changed body's stale vectors are cleared and join the
    /// DB-derived pending set for any later [`embed`](Self::embed) to fill). No
    /// embedder is touched, so saving works with no model provisioned.
    ///
    /// Returns the **new revision** (hashing the *final* on-disk bytes — a
    /// missing-`b2id` stamp, the one write beyond the body, is reflected), which the
    /// editor chains its next save on: sequential saves never self-conflict, and
    /// only an external write trips the guard ("last save wins — by construction",
    /// desktop-editing.md §3).
    pub fn write(&self, note_ref: &str, body: &str, base_revision: &str) -> Result<WriteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "write", note = note_ref).entered();
        let b2id = self.resolve_ref(note_ref)?;
        let path = db::resolve_b2id_to_path(&self.conn, &b2id)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))?;
        let abs = self.root.join(&path);
        let raw = fs::read_to_string(&abs)?;

        // The guard: the bytes the edit was based on must still be the bytes on disk.
        if revision_of(&raw) != base_revision {
            return Err(Error::WriteConflict(path));
        }

        // Markdown first: the byte-honest splice (frontmatter bytes untouched).
        let mut parsed = note::parse(&raw);
        parsed.replace_body(body);
        fs::write(&abs, parsed.as_str())?;

        // Re-project model-free; stamps a missing b2id through the ordinary path.
        ingest::project_file(
            &self.conn,
            &self.root,
            &path,
            &self.idgen,
            &self.chunk_config,
        )?;

        // Hash the FINAL on-disk bytes (a stamp re-wrote the file after our splice).
        let final_raw = fs::read_to_string(&abs)?;
        Ok(WriteReport {
            path,
            revision: revision_of(&final_raw),
        })
    }

    /// Every indexed note as a lightweight [`NoteSummary`] (`b2id`, `path`, `title`;
    /// no body), ordered by `path` — the vault listing the desktop UI's file tree is
    /// built from (spec's navigation surface). A pure, model-free read like
    /// [`read`](Self::read): the tree shows exactly the notes the index knows, and
    /// every one is [`read`](Self::read)-resolvable, so a click always opens. A
    /// never-reindexed vault lists nothing (no error) — reindex populates it, the same
    /// index-first honesty as [`search`](Self::search).
    pub fn list_notes(&self) -> Result<Vec<NoteSummary>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_notes").entered();
        Ok(db::all_notes(&self.conn)?
            .into_iter()
            .map(|(b2id, path, title)| NoteSummary { b2id, path, title })
            .collect())
    }

    /// Every inventoried resource as a lightweight [`ResourceSummary`], ordered by
    /// `path` — the file tree's resource half (research §9b #10: a sibling of
    /// [`list_notes`](Self::list_notes), never a widened union; the adapters
    /// compose the tree). A pure, model-free read; a never-projected vault lists
    /// nothing, same index-first honesty as notes.
    pub fn list_resources(&self) -> Result<Vec<ResourceSummary>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_resources").entered();
        Ok(db::list_resources(&self.conn)?
            .into_iter()
            .map(|(path, class, size, mtime)| ResourceSummary {
                path,
                class,
                size,
                mtime,
            })
            .collect())
    }

    /// Every folder in the vault (vault-relative, sorted, empty ones included) —
    /// the file tree's structure half. Read **live off the filesystem, never the
    /// index**: folders are user-authored *structure* with no derived data
    /// (nothing to chunk, embed, or link), so the walk itself is the projection
    /// and the tree stays one-to-one with disk — a `mkdir` in Finder or a folder
    /// emptied by a move shows exactly as the filesystem has it. Dot-folders are
    /// skipped, the ingest walk's routing rule. Model-free and index-free (works
    /// on a never-reindexed vault).
    pub fn list_dirs(&self) -> Result<Vec<String>> {
        let _op = tracing::debug_span!(target: "b2::vault", "list_dirs").entered();
        dirs::list_dirs(&self.root)
    }

    /// The fallback card's data for one resource (`b2 explain <file>`, the desktop
    /// card — slice-1 spec §4/§6): inventory metadata plus the backlinks panel,
    /// straight off the materialized graph. `path` is a vault-relative path (the
    /// adapters dispatched here via [`crate::resource::doc_kind`]); errors with
    /// [`Error::ResourceNotFound`] when it is not inventoried.
    pub fn explain_resource(&self, path: &str) -> Result<ResourceExplainView> {
        let _op = tracing::debug_span!(target: "b2::vault", "explain_resource", path).entered();
        let (class, size, mtime, content_hash) = db::resource_detail(&self.conn, path)?
            .ok_or_else(|| Error::ResourceNotFound(path.to_string()))?;
        let backlinks = db::inbound_resource_edges(&self.conn, path)?
            .into_iter()
            .map(
                |(b2id, note_path, title, r#type, caption, embed)| ResourceBacklink {
                    b2id,
                    path: note_path,
                    title,
                    r#type,
                    caption,
                    embed,
                },
            )
            .collect();
        Ok(ResourceExplainView {
            path: path.to_string(),
            class,
            size,
            mtime,
            content_hash,
            backlinks,
        })
    }

    /// Move/rename a resource (`b2 mv <file> <to>`) — the note move minus the
    /// identity step (data-model.md §10): rewrite every inbound link's authored
    /// text (both syntaxes, each keeping its own relative-vs-root convention),
    /// move the file, update the inventory, re-project the touched notes. Errors
    /// with [`Error::ResourceNotFound`] for an uninventoried source; destination
    /// errors mirror [`move_note`](Self::move_note).
    pub fn move_resource(&self, path: &str, to: &str) -> Result<ResourceMoveReport> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "mv_resource", from = path, to).entered();
        if db::resource_detail(&self.conn, path)?.is_none() {
            return Err(Error::ResourceNotFound(path.to_string()));
        }
        mv::move_resource(
            &self.conn,
            &self.idgen,
            &self.chunk_config,
            self.embedder.as_ref(),
            &self.root,
            path,
            to,
        )
    }

    /// Hybrid search (BM25 ⊕ vector → RRF) resolved to notes, best first, capped at
    /// `limit` *notes*. Results are note-level: chunk hits are deduped to the
    /// highest-scoring chunk per note, so one note never appears twice.
    ///
    /// **Keyword-first fallback** (projection-embedding-split.md §5): when the
    /// vector space does not exist yet — a projected-but-unembedded vault — this
    /// runs BM25-only (no query embedding, no model) instead of returning nothing,
    /// so a vault is searchable the moment [`project`](Self::project) finishes.
    /// A never-indexed vault still yields no hits, no error (its FTS index is
    /// empty). `vault_info`-style callers should consult the `semantic` flag to
    /// present keyword-only results honestly.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let _op = tracing::debug_span!(target: "b2::vault", "search", query, limit).entered();
        // Pull a wider chunk pool than `limit` so dedup can still fill `limit`
        // distinct notes when several top chunks share a note.
        let pool = limit.saturating_mul(3).max(limit);
        let hits = self.retrieve(query, pool)?;
        let mut out: Vec<SearchResult> = Vec::new();
        for hit in hits {
            if out.iter().any(|r| r.b2id == hit.note_b2id) {
                continue; // note already represented by a higher-scoring chunk
            }
            let path = db::resolve_b2id_to_path(&self.conn, &hit.note_b2id)?.unwrap_or_default();
            let title = db::note_title(&self.conn, &hit.note_b2id)?;
            let snippet = db::chunk_text(&self.conn, hit.chunk_id)?
                .map(|t| query_snippet(&t, query))
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

    /// [`search`](Self::search) at **chunk** granularity: the top `limit` ranked
    /// chunks, resolved to their note + heading breadcrumb + full text, with **no
    /// note dedup** — one note may appear several times when several of its
    /// passages rank. Same retrieval, same fallback, same model-mismatch fail-fast
    /// (see [`ChunkSearchResult`] for who consumes this and why).
    pub fn search_chunks(&self, query: &str, limit: usize) -> Result<Vec<ChunkSearchResult>> {
        let _op =
            tracing::debug_span!(target: "b2::vault", "search_chunks", query, limit).entered();
        let mut out = Vec::new();
        for hit in self.retrieve(query, limit)? {
            // Both lookups can miss only on an inconsistent index (a hit whose row
            // vanished mid-call); drop such a hit rather than emit a half-resolved
            // one — a rank slot with an empty path would read as a real result.
            let Some(path) = db::resolve_b2id_to_path(&self.conn, &hit.note_b2id)? else {
                continue;
            };
            let Some((heading_path, text)) = db::chunk_detail(&self.conn, hit.chunk_id)? else {
                continue;
            };
            out.push(ChunkSearchResult {
                b2id: hit.note_b2id,
                path,
                heading_path,
                score: hit.score,
                text,
            });
        }
        Ok(out)
    }

    /// The shared retrieval core of [`search`](Self::search) and
    /// [`search_chunks`](Self::search_chunks): hybrid when the embedding space
    /// exists (failing fast on a model mismatch — the stored vectors would be
    /// incomparable with the query vector, so results would be silently wrong; the
    /// fix is a `reindex`), BM25-only on a projected-but-unembedded vault.
    fn retrieve(&self, query: &str, pool: usize) -> Result<Vec<search::Hit>> {
        if db::embedding_space_exists(&self.conn)? {
            if let Some((indexed_model, indexed_dim)) = db::recorded_embedder(&self.conn)? {
                if indexed_model != self.embedder.model_id() || indexed_dim != self.embedder.dim() {
                    return Err(Error::ModelMismatch {
                        indexed: format!("{indexed_model} (dim {indexed_dim})"),
                        active: format!(
                            "{} (dim {})",
                            self.embedder.model_id(),
                            self.embedder.dim()
                        ),
                    });
                }
            }
            search::hybrid_search(&self.conn, self.embedder.as_ref(), query, pool)
        } else {
            search::keyword_only_search(&self.conn, query, pool)
        }
    }

    /// The vault's semantic-embedding coverage as an [`EmbedStatus`] — the honest
    /// "N/M embedded" read (#26). A **pure model-free count** over the projection
    /// (`db::embed_progress`), so an adapter can tell the user semantic ranking is
    /// *partial* — flag results "keyword-only for now" — rather than silently
    /// under-ranking while a vault embeds behind the first tree paint
    /// (projection-embedding-split.md §5). `embedded == 0` on a projected-but-unembedded
    /// vault; `embedded == total` (with `total > 0`) once every note has vectors.
    pub fn embed_status(&self) -> Result<EmbedStatus> {
        let _op = tracing::debug_span!(target: "b2::vault", "embed_status").entered();
        let (embedded, total) = db::embed_progress(&self.conn)?;
        Ok(EmbedStatus { embedded, total })
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
        let _op =
            tracing::debug_span!(target: "b2::vault", "similar", note = note_ref, limit).entered();
        // A resource anchor is honest, not silent: resources become discoverable
        // when slice 3 gives them chunks + centroids (research §9b #7). Until
        // then an inventoried resource errs "not yet" — never an empty result —
        // and an unknown path falls through to the usual not-found.
        if crate::resource::doc_kind(note_ref) == crate::resource::DocKind::Resource
            && db::resource_detail(&self.conn, note_ref)?.is_some()
        {
            return Err(Error::ResourceUnsupported(note_ref.to_string()));
        }
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
    /// typed-link string to the **source note's frontmatter `b2_relations:`** (Markdown
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
        let _op = tracing::debug_span!(
            target: "b2::vault", "link",
            src = src_ref, dst = dst_ref, edge_type
        )
        .entered();
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

        // The typed-link spec targets the dst's path. A note's title is its filename
        // (data-model.md §1), so a bare `[[path]]` already reads as the title — B2
        // writes no alias, and a frontmatter `title:` (inert) is never consulted.
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
        //    as origin='frontmatter' — a projection of the line just written, not an
        //    index write (data-model.md §3).
        ingest::ingest_file(
            &self.conn,
            &self.root,
            &src_path,
            &self.idgen,
            &self.chunk_config,
            self.embedder.as_ref(),
        )?;

        Ok(LinkReport {
            src_path,
            dst_path,
            relation: edge_type.to_string(),
            created: true,
        })
    }

    /// Move/rename the whole folder `from` to `to` (both vault-relative
    /// directory paths; a trailing `/` is tolerated). One `fs::rename` moves the
    /// directory — unindexed files inside travel too — after every inbound link
    /// at the moved set (including vault-root wikilinks *between* co-moved
    /// notes) is rewritten, then the index re-projects; the graph never breaks
    /// (edges key on `b2id`). Errors with [`Error::DirNotFound`] for a missing
    /// source folder, [`Error::MoveDestination`] for an invalid destination
    /// (including one inside the moved folder), or [`Error::MoveTargetExists`]
    /// rather than merge into an existing entry.
    ///
    /// Rewriting an inbound file changes its body, so this **re-embeds** those
    /// files: the adapters open the vault with the real model for a dir move,
    /// as for `mv`/`reindex`/`link`.
    pub fn move_dir(&self, from: &str, to: &str) -> Result<DirMoveReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mv_dir", from, to).entered();
        mv::move_dir(
            &self.conn,
            &self.idgen,
            &self.chunk_config,
            self.embedder.as_ref(),
            &self.root,
            from,
            to,
        )
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
        let _op = tracing::debug_span!(target: "b2::vault", "mv", from = note_ref, to).entered();
        let b2id = self.resolve_ref(note_ref)?;
        let old_rel = db::resolve_b2id_to_path(&self.conn, &b2id)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))?;
        mv::move_note(
            &self.conn,
            &self.idgen,
            &self.chunk_config,
            self.embedder.as_ref(),
            &self.root,
            &b2id,
            &old_rel,
            to,
        )
    }

    /// Delete the note `note_ref` (path **or** `b2id`): the file leaves the disk,
    /// its projection rows leave the index, and every inbound link at it
    /// **dangles** — never rewritten, surfacing as an unresolved link (GH #12) —
    /// exactly the state an external `rm` plus a full reindex produces. Errors
    /// with [`Error::NoteNotFound`] for an unknown ref.
    ///
    /// **Model-free** (the `create_note`/`write` posture): no body changes, so the
    /// inbound re-projection touches no vectors and needs no model.
    pub fn delete_note(&self, note_ref: &str) -> Result<DeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm", note = note_ref).entered();
        let b2id = self.resolve_ref(note_ref)?;
        let rel = db::resolve_b2id_to_path(&self.conn, &b2id)?
            .ok_or_else(|| Error::NoteNotFound(note_ref.to_string()))?;
        rm::delete_note(
            &self.conn,
            &self.idgen,
            &self.chunk_config,
            &self.root,
            &b2id,
            &rel,
        )
    }

    /// [`delete_note`](Self::delete_note)'s resource sibling — same posture (file
    /// off disk, inventory row off the index, inbound links dangle, model-free).
    /// Errors with [`Error::ResourceNotFound`] for a path not in the inventory.
    pub fn delete_resource(&self, path: &str) -> Result<ResourceDeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm_resource", path).entered();
        rm::delete_resource(
            &self.conn,
            &self.idgen,
            &self.chunk_config,
            &self.root,
            path,
        )
    }

    /// Delete the whole folder `dir` (vault-relative) and everything inside it —
    /// one `fs::remove_dir_all`, so unindexed files inside go too — then every
    /// contained note's/resource's rows; surviving linkers outside the folder
    /// dangle, as for [`delete_note`](Self::delete_note). Model-free. Errors with
    /// [`Error::DirNotFound`] for a missing (or invalid) folder.
    pub fn delete_dir(&self, dir: &str) -> Result<DirDeleteReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "rm_dir", dir).entered();
        rm::delete_dir(&self.conn, &self.idgen, &self.chunk_config, &self.root, dir)
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
        let _op = tracing::debug_span!(target: "b2::vault", "add", path).entered();
        let created = self.today()?;
        add::add_note(
            &self.conn,
            &self.idgen,
            &self.chunk_config,
            self.embedder.as_ref(),
            &self.root,
            path,
            title,
            content,
            &created,
        )
    }

    /// Create a new, empty note **model-free** — the desktop's New-note action
    /// (its tree affordance / ⌘N), the create sibling of [`write`](Self::write):
    /// write `path` with the same minimal frontmatter as [`add_note`](Self::add_note)
    /// (no title — a note's display title is its filename, data-model.md §1) and
    /// project it via [`ingest::project_file`] with **no embedder touched**, so
    /// creation works with no model provisioned and a fake-opened vault can never
    /// write foreign vectors into a real-model embedding space. The note's chunks
    /// join the DB-derived missing-vector set, healed by any later
    /// [`embed`](Self::embed)/reindex (projection-embedding-split.md §7.2) — and an
    /// empty body has nothing to embed anyway. Same refusals as `add_note`:
    /// [`Error::AddDestination`] / [`Error::AddTargetExists`].
    pub fn create_note(&self, path: &str) -> Result<AddReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "create", path).entered();
        let created = self.today()?;
        add::create_note(
            &self.conn,
            &self.idgen,
            &self.chunk_config,
            &self.root,
            path,
            None,
            None,
            &created,
        )
    }

    /// Create the folder `dir` (vault-relative; missing parents included, like
    /// `mkdir -p`) — the desktop's New-folder action, the structure sibling of
    /// [`create_note`](Self::create_note). A folder is user-authored vault
    /// structure, so this writes the filesystem and touches **nothing else**: no
    /// index rows exist for a folder (see [`list_dirs`](Self::list_dirs)), and the
    /// new folder is real to every tool — Finder, the CLI, a sync — the moment
    /// this returns. Errors with [`Error::DirDestination`] for an invalid path or
    /// [`Error::DirTargetExists`] when anything already sits there.
    pub fn create_dir(&self, dir: &str) -> Result<DirCreateReport> {
        let _op = tracing::debug_span!(target: "b2::vault", "mkdir", dir).entered();
        dirs::create_dir(&self.root, dir)
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

/// A file's save-guard revision: blake3 of its raw bytes (desktop-editing.md §3).
/// One tiny fn so `read` (capture) and `write` (validate + return) can never drift.
fn revision_of(raw: &str) -> String {
    blake3::hash(raw.as_bytes()).to_hex().to_string()
}

/// Flatten a chunk's text to a single whitespace-collapsed line.
fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The head of a flattened chunk, bounded to one line.
fn head_snippet(flat: &str) -> String {
    if flat.chars().count() <= SNIPPET_CHARS {
        flat.to_string()
    } else {
        let cut: String = flat.chars().take(SNIPPET_CHARS).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Collapse a chunk's text to a single-line, length-bounded snippet (its head). Used
/// where there is no query to center on (e.g. `similar`'s evidence passage).
fn snippet(text: &str) -> String {
    head_snippet(&flatten(text))
}

/// Like [`snippet`] but windows the excerpt around the first query-term match, so a
/// section-sized chunk (qmd chunking, #19) still surfaces the matched text instead of
/// only its head. Falls back to the head when no term matches or the match is already
/// in view — a pure vector hit (query words absent from the chunk) keeps the head.
fn query_snippet(text: &str, query: &str) -> String {
    let flat = flatten(text);
    if flat.chars().count() <= SNIPPET_CHARS {
        return flat;
    }
    let lower = flat.to_lowercase();
    let match_pos = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 2)
        .filter_map(|t| {
            let byte = lower.find(&t.to_lowercase())?;
            Some(lower[..byte].chars().count())
        })
        .min();
    // A little lead-in so the match is not flush against the ellipsis.
    const LEAD: usize = 24;
    let Some(pos) = match_pos.filter(|p| *p > LEAD) else {
        return head_snippet(&flat);
    };
    let chars: Vec<char> = flat.chars().collect();
    // `pos` is a char index into the lowercased text, whose length can differ from
    // `flat` for exotic Unicode; clamp so the slice below can never go out of range.
    let start = (pos - LEAD).min(chars.len());
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let mut out = String::from("…");
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}
