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

use crate::add::{self, AddReport};
use crate::chat;
use crate::chunk::ChunkConfig;
use crate::db;
use crate::dirs;
use crate::discover;
use crate::embed::{Embedder, FakeEmbedder};
use crate::error::{Error, Result};
use crate::graph::{self, Direction};
use crate::import;
use crate::llm::{ChatTurn, ContextPassage, LlmProvider};
use crate::mv;
use crate::rm;
use crate::{ingest, note, relation, search};
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

// Report types the façade returns — re-exported so adapters name them through the
// one typed contract.
pub use crate::dirs::DirCreateReport;
pub use crate::import::ImportReport;
pub use crate::ingest::SkippedNote;
pub use crate::mv::DirMoveReport;
pub use crate::mv::{MoveReport, ResourceMoveReport};
pub use crate::rm::{DeleteReport, DirDeleteReport, ResourceDeleteReport};

/// Embedding dimension of the *fake* embedder ([`Vault::open`]). The real model
/// brings its own (768); a model or dim swap re-embeds on `reindex` (ADR-0007).
const EMBED_DIM: usize = 64;

/// Longest snippet (in chars) shown for a search hit, so a result stays one line.
const SNIPPET_CHARS: usize = 160;

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
/// changed their top-4 passages across exactly that step (`just stability`). Wider
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

/// What `reindex` did: notes projected, and how many were actually (re)embedded
/// (the rest reused their vectors). It reports no vault writes because there are
/// none — a reindex reads (ADR-0004).
///
/// `cancelled` marks a cooperative cancel of the embed phase; the counts then
/// describe the partial work truthfully and the index is still consistent (keyword
/// and graph complete, a prefix embedded). Always `false` for
/// [`reindex`](Vault::reindex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReindexReport {
    pub indexed: usize,
    pub embedded: usize,
    pub cancelled: bool,
    /// Files skipped as unreadable this run — one bad file never fails the pass.
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned: notes whose files were deleted outside b2 (#31), so
    /// incremental equals a from-scratch rebuild.
    pub notes_pruned: usize,
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// What [`project`](Vault::project) did — the model-free half of a reindex. No
/// embed counts: projection never touches vectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectReport {
    pub indexed: usize,
    /// Files skipped as unreadable this pass, so an adapter can say which and why.
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned: notes whose files were deleted outside b2 (#31).
    pub notes_pruned: usize,
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// What [`embed`](Vault::embed) did — the model-bound half of a reindex: notes
/// whose missing vectors were filled, and whether a cooperative cancel cut the pass
/// short (the counts stay truthful, and a re-run embeds exactly the remainder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EmbedReport {
    pub embedded: usize,
    pub cancelled: bool,
}

/// The vault's embedding coverage — the honest "N/M embedded" signal (#26).
/// Model-free: a pure count over the projection, so an adapter can say
/// "keyword-only for now" precisely without loading a model. `embedded < total`
/// means [`search`](Vault::search) is running keyword-first over the remainder;
/// `embedded == 0` is a fully keyword-only vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EmbedStatus {
    /// Notes whose every chunk has a stored vector.
    pub embedded: usize,
    /// Every projected note (the denominator).
    pub total: usize,
}

/// What a reindex **would** do — the `reindex --dry-run` preview, computed
/// read-only. The `would_*` keys are the honesty signal: this is a forecast.
///
/// It forecasts work and nothing else. The dry-run's old columns (which notes would
/// be stamped, which files collide) existed because a real run wrote to the vault;
/// it no longer does (ADR-0004), so only the embedding is left to size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReindexPlan {
    /// Notes a real reindex would project (every `.md` file the walk collects).
    pub would_index: usize,
    /// …of which this many would be (re)embedded.
    pub would_embed: usize,
}

/// One neighbor of a note, resolved for display: the note at the other end of an
/// edge, with its path + title, so the adapter stays a dumb printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeighborView {
    /// The other note's vault-relative path — its identity (ADR-0003).
    pub path: String,
    pub title: Option<String>,
    /// The stored relation verb (outbound direction of the edge).
    pub relation: String,
    /// `"outbound"` (this note → other) or `"inbound"`.
    pub direction: String,
    /// Display label: the verb outbound, its inverse inbound (ADR-0010).
    pub label: String,
    pub explanation: Option<String>,
    /// Edge origin: `"inline"` (a body link) or `"frontmatter"` (ADR-0010).
    pub origin: String,
    /// The other note's `created` date, resolved from the projection (GH #22).
    pub created: Option<String>,
}

/// One outbound link a note authors at a **resource** (an image, a PDF — any
/// non-`.md` vault file), resolved for display. Surfaced on [`ExplainView`] so a
/// note's file links are visible from the note's side, not only as the resource's
/// backlinks (GH #22). Distinct from [`NeighborView`]: a resource has no title and
/// authors no edges, so these are always outbound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceLinkView {
    pub path: String,
    /// Its inventory class (`image`/`pdf`/`html`/`text`/`media`/`binary`).
    pub class: String,
    pub relation: String,
    pub origin: String,
    /// The authored caption (alt text / `|caption`), if any.
    pub caption: Option<String>,
    /// Whether the link is an embed (`![…]` / `![[…]]`).
    pub embed: bool,
    pub explanation: Option<String>,
}

/// One outbound link a note authored that resolves to **nothing** — no note and no
/// resource at its target (a `[[Hermes]]` naming a *folder*, or a typo). A note is
/// one `.md` file, so a folder is never a valid target; rather than drop such a link
/// B2 surfaces it as unresolved, so it reads as broken rather than missing (GH #12).
/// It has no `path` — that is the whole point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnresolvedLink {
    /// The target exactly as written in the Markdown (`[[target]]`).
    pub target: String,
    pub relation: String,
    pub origin: String,
    pub explanation: Option<String>,
}

/// A note's full connection picture for `b2 explain`: the note itself, every active
/// connection with its "why", its outbound resource links, and any unresolved
/// outbound links. A thin header over [`NeighborView`] — it reuses the per-edge
/// shape `neighbors` returns rather than a parallel one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainView {
    pub path: String,
    pub title: Option<String>,
    /// Outbound edges first, then inbound (as [`graph::neighbors`] orders them).
    pub connections: Vec<NeighborView>,
    /// Outbound links at resources — the third target kind (GH #22).
    pub resources: Vec<ResourceLinkView>,
    /// Outbound links that resolved to nothing (GH #12).
    pub unresolved: Vec<UnresolvedLink>,
}

/// A note's content + display metadata for a reader. Carries the note's identity,
/// the frontmatter fields worth showing a human, and the **raw Markdown body read
/// from disk** (the source of truth, not the projection) so an adapter renders
/// Markdown itself. A pure read — no embedding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteView {
    pub path: String,
    pub title: Option<String>,
    pub r#type: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
    /// The note's Markdown body (frontmatter stripped), verbatim from disk.
    pub body: String,
    /// The raw frontmatter YAML **verbatim** (the text between the `---` fences,
    /// fences excluded), not a re-serialization of the projected fields above — so
    /// `b2_relations:` and any keys B2 doesn't model show as written.
    pub frontmatter: Option<String>,
    /// Whether that block *reads* as YAML metadata
    /// ([`note::ParsedNote::frontmatter_readable`]): `false` means the raw bytes
    /// above are shown verbatim but the projected fields came back empty. Every
    /// read passes through here, so an external hand-edit surfaces the same warning
    /// as an in-app save (GH #79).
    pub frontmatter_readable: bool,
    /// blake3 of the **raw file bytes** at read time — the save-guard token
    /// [`write`](Vault::write) validates, so a save can never silently clobber an
    /// external edit. Whole-file, so *any* out-of-band change conflicts honestly.
    pub revision: String,
}

/// One note's identity for a listing — `path` + `title`, with **no body**: enough
/// to show and open a note, cheap enough to fetch the whole vault at once. The body
/// is a separate [`read`](Vault::read) when a note is opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteSummary {
    pub path: String,
    pub title: Option<String>,
}

/// One resource's identity for the file tree — the per-kind sibling of
/// [`NoteSummary`], never a union type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceSummary {
    pub path: String,
    pub class: String,
    pub size: i64,
    pub mtime: Option<i64>,
}

/// The resource fallback card's data: inventory metadata plus inbound backlinks.
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
    pub path: String,
    pub title: Option<String>,
    pub r#type: String,
    pub caption: Option<String>,
    pub embed: bool,
}

/// One search hit, resolved to the note it belongs to with a text snippet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub title: Option<String>,
    /// Fused relevance score; higher is better.
    pub score: f64,
    /// A one-line excerpt of the matched chunk.
    pub snippet: String,
}

/// A search's evidence reading — [`Vault::search_evidence`]'s return, and what a
/// surface needs to decide what it vouches for (ADR-0015).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchEvidenceView {
    /// The served results, whole and in fused order — evidence never reorders or
    /// removes a row.
    pub results: Vec<EvidencedResult>,
    /// Does the vault hold positive evidence for this query at the active model's
    /// bar? `None` = no calibrated bar for this embedder, so no verdict is offered
    /// rather than one guessed.
    pub vouched: Option<bool>,
    /// Chunks in the index — the scale every term's weight is read against.
    pub chunk_total: usize,
    /// Every query term with its document frequency, in query order.
    pub terms: Vec<QueryTermView>,
    /// Best cosine between the query and any chunk vector — the dense half's
    /// absolute claim. `None` on a projected-but-unembedded vault.
    pub best_cos: Option<f64>,
}

/// One served result with the provenance RRF discarded — which lists ranked its
/// chunk, and how near its vector actually was.
///
/// The query-level verdict on [`SearchEvidenceView`] is what ADR-0015's "no matches"
/// rests on; **this** is what a per-hit tail rule would be argued from (GH #201
/// Step 2, unshipped and deliberately so — the corpus does not yet label the
/// irrelevance of ranks 5–10). Until then it is an instrument reading.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvidencedResult {
    #[serde(flatten)]
    pub result: SearchResult,
    /// 0-based rank in the BM25 list; `None` = the lexical half never ranked it.
    pub bm25_rank: Option<usize>,
    /// 0-based rank in the dense list; `None` = the vector half never ranked it,
    /// or never ran.
    pub vector_rank: Option<usize>,
    /// This chunk's cosine to the query; `None` whenever `vector_rank` is.
    pub cos: Option<f64>,
}

/// One query term's lexical reading (see [`SearchEvidenceView`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueryTermView {
    pub term: String,
    /// Chunks matching this term alone; `0` = the vault has never seen the word.
    pub df: usize,
    /// This term's weight in the coverage reading — `ln((chunks+1)/(df+1))`; near
    /// zero for a ubiquitous word, largest for one the vault has never seen.
    pub idf: f64,
}

/// One **chunk-level** search hit — the sub-note view of [`search`](Vault::search).
/// Same retrieval, but ranked chunks are returned as-is instead of deduped up to
/// notes, so a caller can see *which passage* matched and at what rank. The client
/// is the out-of-CI retrieval eval (ADR-0013): note-rank scoring is blind to
/// sub-note quality, which is exactly what chunking levers move. Carries the chunk's
/// **full text**, which the eval's containment scoring anchors on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChunkSearchResult {
    pub path: String,
    /// The chunk's heading breadcrumb (`"Fermentation > Vegetables"`), when the
    /// chunker recorded one.
    pub heading_path: Option<String>,
    /// Fused relevance score; higher is better.
    pub score: f64,
    /// The chunk's stored text, verbatim.
    pub text: String,
}

/// The answer to one grounded-chat ask — flow ④'s display view: the model's
/// streamed text with its `[n]` citation markers resolved back to the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnswerView {
    /// The answer, verbatim as streamed — including any marker that did *not*
    /// resolve. Model output is untrusted content, but it is never rewritten here;
    /// an unmatched marker is simply absent from `citations`.
    pub answer: String,
    /// The resolved citations, ascending by marker; one entry per **distinct**
    /// marker that names a real passage.
    pub citations: Vec<Citation>,
    /// `true` when the caller's callback broke the stream mid-answer: `answer` then
    /// holds the partial text honestly, and citations resolve over what arrived.
    pub cancelled: bool,
}

/// One resolved `[n]` citation: the passage's note, plus a one-line excerpt of the
/// cited passage as display evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Citation {
    /// The marker as it appears in the answer text (1-based passage number).
    pub marker: usize,
    /// Vault-relative path of the cited note — its identity (ADR-0003), and what an
    /// adapter opens on click. A note renamed between answer and click goes stale,
    /// exactly as any path handle does.
    pub path: String,
    /// A one-line excerpt of the cited passage (its head, length-bounded).
    pub excerpt: String,
}

/// One semantically-similar candidate for `b2 similar`: a note near the anchor in
/// embedding space that is **not** already connected to it, resolved for display
/// with the passage that made it similar. The machine finds the candidate, the human
/// decides whether to `link` it (ADR-0009).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarView {
    pub path: String,
    pub title: Option<String>,
    /// Best chunk-pair similarity to the anchor; higher is nearer.
    pub score: f64,
    /// A one-line excerpt of the candidate chunk that achieved `score` — the
    /// evidence for *why* it surfaced.
    pub evidence: String,
    /// How far this candidate stands above the anchor's own candidate population —
    /// its best-passage z (GH #192), and the one honest input for a displayed
    /// *strength* band (GH #150). Non-increasing down the row order, so the band
    /// never contradicts the ranking. It **gates nothing** (ADR-0014): the band is a
    /// within-list grading, never a verdict on existence. The unit is load-bearing —
    /// a band calibrated in the retired centroid unit grades every card down
    /// (GH #182) — so a surface reads its landmarks off `just eval`'s calibration
    /// block. `None` when no statistic was computed (a fake-embedded space, a pool
    /// under the statistics minimum, or zero variance), which is the adapters' cue
    /// to say the list is *ungraded* rather than let bare cards read as uniformly
    /// weak; serialized only when present, so older JSON consumers see no change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
}

/// What [`write`](Vault::write) did: the saved note's path and the **new revision**
/// (blake3 of the final on-disk bytes) — the token the editor chains its next save
/// on, so sequential saves never self-conflict.
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
            let title = db::note_title(&self.conn, &n.other)?;
            let created = db::note_created(&self.conn, &n.other)?;
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
        let path = self.resolve_ref(note_ref)?;
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

        // Re-project model-free, through the ordinary path.
        ingest::project_file(self.ctx(), &path)?;

        // Read the final bytes back rather than assume them: the revision the editor
        // chains on must describe the file, not our intent.
        let final_raw = fs::read_to_string(&abs)?;
        Ok(WriteReport {
            path,
            revision: revision_of(&final_raw),
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
        let path = self.resolve_ref(note_ref)?;
        let abs = self.root.join(&path);
        let raw = fs::read_to_string(&abs)?;

        // The guard: the bytes the edit was based on must still be the bytes on disk.
        if revision_of(&raw) != base_revision {
            return Err(Error::WriteConflict(path));
        }
        if frontmatter
            .lines()
            .any(|l| l.trim_end_matches('\r') == "---")
        {
            return Err(Error::Frontmatter(
                "a `---` line inside frontmatter would end the block early".into(),
            ));
        }

        let mut parsed = note::parse(&raw);
        parsed.replace_frontmatter(frontmatter);
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

    /// The fallback card's data for one resource: inventory metadata plus the
    /// backlinks panel, straight off the materialized graph. `path` is vault-relative
    /// (the adapters dispatch here via [`crate::resource::doc_kind`]); errors with
    /// [`Error::ResourceNotFound`] when it is not inventoried.
    pub fn explain_resource(&self, path: &str) -> Result<ResourceExplainView> {
        let _op = tracing::debug_span!(target: "b2::vault", "explain_resource", path).entered();
        let detail = db::resource_detail(&self.conn, path)?
            .ok_or_else(|| Error::ResourceNotFound(path.to_string()))?;
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
        if db::resource_detail(&self.conn, path)?.is_none() {
            return Err(Error::ResourceNotFound(path.to_string()));
        }
        mv::move_resource(self.embed_ctx(), path, to)
    }

    /// Hybrid search (BM25 ⊕ vector → RRF) resolved to notes, best first, capped at
    /// `limit` *notes*: chunk hits dedup to the highest-scoring chunk per note, so
    /// one note never appears twice (ADR-0008).
    ///
    /// **Keyword-first fallback:** when the vector space does not exist yet — a
    /// projected-but-unembedded vault — this runs BM25-only rather than returning
    /// nothing, so a vault is searchable the moment [`project`](Self::project)
    /// finishes. A never-indexed vault yields no hits and no error; callers should
    /// consult the `semantic` flag to present keyword-only results honestly.
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
        Ok(self
            .resolve_note_hits(hits, query, limit)?
            .into_iter()
            .map(|(result, _)| result)
            .collect())
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
        Ok(self
            .resolve_note_hits(hits, query, limit)?
            .into_iter()
            .map(|(result, _)| result)
            .collect())
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

    /// The note-resolution tail shared by [`search`](Self::search) and
    /// [`search_vector_only`](Self::search_vector_only): dedup chunk hits to their
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
            // The note row can only be missing on a torn read; drop the hit rather
            // than name a note that is gone — the pool has headroom to backfill it
            // (GH #137).
            if !db::note_exists(&self.conn, &hit.note_path)? {
                continue;
            }
            let title = db::note_title(&self.conn, &hit.note_path)?;
            let snippet = db::chunk_text(&self.conn, hit.chunk_id)?
                .map(|t| query_snippet(&t, query))
                .unwrap_or_default();
            out.push((
                SearchResult {
                    path: hit.note_path,
                    title,
                    score: hit.score,
                    snippet,
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
            // Either lookup can miss only on an inconsistent index; drop such a hit
            // rather than emit a half-resolved one — a rank slot with an empty path
            // would read as a real result. The pool backfills it (GH #137).
            if !db::note_exists(&self.conn, &hit.note_path)? {
                continue;
            }
            let Some((heading_path, text)) = db::chunk_detail(&self.conn, hit.chunk_id)? else {
                continue;
            };
            out.push(ChunkSearchResult {
                path: hit.note_path,
                heading_path,
                score: hit.score,
                text,
            });
        }
        Ok(out)
    }

    /// Flow ④ — grounded chat over the vault: condense → retrieve → assemble →
    /// stream → cite, orchestrated here over the core logic in [`crate::chat`]. The
    /// provider is injected **per call**: chat is its sole consumer and, unlike the
    /// embedder, it carries no index identity (contrast ADR-0007), so nothing about
    /// it belongs on the open vault.
    ///
    /// - **Condense** (multi-turn only): one provider call rewrites the follow-up
    ///   into a standalone retrieval query; on failure it degrades to the raw
    ///   question, so that step can never break chat.
    /// - **Retrieve**: [`search_chunks`](Self::search_chunks) at
    ///   [`chat::ASK_PASSAGES`], holding `search`'s posture — chat is a reader.
    /// - **Stream**: tokens flow up through `on_token` as they arrive; returning
    ///   `ControlFlow::Break(())` cancels at token granularity and the result reports
    ///   it ([`AnswerView::cancelled`]).
    /// - **Cite**: distinct `[n]` markers resolve to `(path, excerpt)`; a
    ///   hallucinated marker resolves to nothing, and the text is never rewritten.
    ///
    /// Nothing model-derived is stored anywhere, and history is the caller's,
    /// session-only. Errors: retrieval as `search` raises it; a failed *answer* call
    /// as [`Error::Llm`](crate::Error::Llm).
    pub fn ask(
        &self,
        llm: &dyn LlmProvider,
        question: &str,
        history: &[ChatTurn],
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<AnswerView> {
        let _op = tracing::debug_span!(
            target: "b2::vault",
            "ask",
            question,
            multi_turn = !history.is_empty()
        )
        .entered();
        let query = if history.is_empty() {
            question.to_string()
        } else {
            chat::condense_query(llm, question, history)
        };
        let passages: Vec<ContextPassage> = self
            .search_chunks(&query, chat::ASK_PASSAGES)?
            .into_iter()
            .map(|c| ContextPassage {
                path: c.path,
                heading_path: c.heading_path,
                text: c.text,
            })
            .collect();
        tracing::debug!(
            target: "b2::chat",
            passages = passages.len(),
            "retrieved grounding passages"
        );
        let req = chat::build_request(question, history, passages);
        // Normalize: the trait returns the crate-wide `Result`, but every failure of
        // this call is a failed model call, and adapters match `Error::Llm` for the
        // "can't reach the model server" message. Enforced here, not hoped for.
        let completion = llm.complete(&req, on_token).map_err(|e| match e {
            Error::Llm(_) => e,
            other => Error::Llm(other.to_string()),
        })?;
        let citations = chat::cited_markers(&completion.text, req.passages.len())
            .into_iter()
            .filter_map(|marker| {
                // 1-based marker to 0-based passage; skip rather than index, so even
                // a broken invariant degrades to a missing citation.
                let p = req.passages.get(marker.checked_sub(1)?)?;
                Some(Citation {
                    marker,
                    path: p.path.clone(),
                    excerpt: snippet(&p.text),
                })
            })
            .collect();
        Ok(AnswerView {
            answer: completion.text,
            citations,
            cancelled: completion.cancelled,
        })
    }

    /// The shared retrieval core of [`search`](Self::search) and
    /// [`search_chunks`](Self::search_chunks): hybrid when the embedding space
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
        // Never claim a statistic over a fake-embedded space: hash vectors have no
        // semantic geometry, so a z there would be noise wearing a band. Judged by the
        // RECORDED identity — the space being searched — not the injected embedder.
        // Grading changes what the rows carry, never which rows exist (ADR-0014).
        let grade = !matches!(
            db::recorded_embedder(&self.conn)?,
            Some((model, _)) if model == crate::embed::FAKE_MODEL_ID
        );
        // A resource anchor is honest, not silent: resources become discoverable once
        // they have chunks + centroids. Until then an inventoried resource errs "not
        // yet" — never an empty result — and an unknown path falls through to
        // not-found.
        if crate::resource::doc_kind(note_ref) == crate::resource::DocKind::Resource
            && db::resource_detail(&self.conn, note_ref)?.is_some()
        {
            return Err(Error::ResourceUnsupported(note_ref.to_string()));
        }
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
/// section-sized chunk still surfaces the matched text instead of only its head.
/// Falls back to the head when no term matches or the match is already in view — a
/// pure vector hit keeps the head.
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
    // `pos` indexes the lowercased text, whose length can differ from `flat` for
    // exotic Unicode; clamp so the slice below can never go out of range.
    let start = (pos - LEAD).min(chars.len());
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let mut out = String::from("…");
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}
