//! The display-ready views the façade returns: what the CLI prints (human and `--json`)
//! and what the desktop reuses as its IPC contract (`ui/src/types.ts` mirrors them).

use crate::ingest::SkippedNote;
use serde::Serialize;

// Doc links only: the views describe what these produce.
#[cfg(doc)]
use super::Vault;
#[cfg(doc)]
use crate::{graph, note};

/// What `reindex` did: notes projected and how many were (re)embedded. No vault writes
/// to report: a reindex reads (ADR-0004). After `cancelled`, the counts describe the
/// partial work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReindexReport {
    pub indexed: usize,
    pub embedded: usize,
    pub cancelled: bool,
    /// Files skipped as unreadable; one bad file never fails the pass.
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned: notes whose files were deleted outside b2 (#31).
    pub notes_pruned: usize,
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// What [`project`](Vault::project) did: the model-free half of a reindex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectReport {
    pub indexed: usize,
    /// Files skipped as unreadable.
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned: notes whose files were deleted outside b2 (#31).
    pub notes_pruned: usize,
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// What [`embed`](Vault::embed) did: notes whose missing vectors were filled, and whether
/// a cancel cut the pass short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EmbedReport {
    pub embedded: usize,
    pub cancelled: bool,
}

/// The vault's embedding coverage, "N/M embedded" (#26). `embedded < total` means
/// [`search`](Vault::search) is keyword-only over the remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EmbedStatus {
    /// Notes with no chunk awaiting a vector, empty notes included
    /// (see [`crate::db::embed_progress`]).
    pub embedded: usize,
    /// Every projected note (the denominator).
    pub total: usize,
}

/// What a reindex would do: the `reindex --dry-run` forecast, computed read-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReindexPlan {
    /// Notes a real reindex would project.
    pub would_index: usize,
    /// …of which this many would be (re)embedded.
    pub would_embed: usize,
}

/// One neighbor of a note, resolved for display.
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

/// One outbound link from a note to a resource (any non-`.md` file), resolved for
/// display (GH #22). Always outbound: resources author no edges.
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

/// One outbound link that resolves to no note or resource (a folder name, a typo),
/// surfaced so it reads as broken rather than missing (GH #12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnresolvedLink {
    /// The target exactly as written in the Markdown (`[[target]]`).
    pub target: String,
    pub relation: String,
    pub origin: String,
    pub explanation: Option<String>,
}

/// A note's full connection picture for `b2 explain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainView {
    pub path: String,
    pub title: Option<String>,
    /// Outbound edges first, then inbound (as [`graph::neighbors`] orders them).
    pub connections: Vec<NeighborView>,
    /// Outbound links at resources (GH #22).
    pub resources: Vec<ResourceLinkView>,
    /// Outbound links that resolved to nothing (GH #12).
    pub unresolved: Vec<UnresolvedLink>,
}

/// A note's raw Markdown from disk (not the projection) plus display metadata.
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
    /// The frontmatter YAML verbatim, fences excluded, so unmodelled keys show as
    /// written.
    pub frontmatter: Option<String>,
    /// Whether that block parses as YAML ([`note::ParsedNote::frontmatter_readable`]);
    /// `false` means the projected fields are empty (GH #79).
    pub frontmatter_readable: bool,
    /// blake3 of the whole file at read time: the save-guard token
    /// [`write`](Vault::write) validates, so a save never clobbers an external edit.
    pub revision: String,
}

/// One note's identity for a listing, with no body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteSummary {
    pub path: String,
    pub title: Option<String>,
}

/// One resource's identity for the file tree.
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

/// A search's evidence reading, for a surface to decide what it vouches for (ADR-0015).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchEvidenceView {
    /// The served results, whole and in fused order.
    pub results: Vec<EvidencedResult>,
    /// Whether the vault holds evidence for this query at the model's bar. `None`: no
    /// calibrated bar for this embedder, so no verdict.
    pub vouched: Option<bool>,
    /// Chunks in the index.
    pub chunk_total: usize,
    /// Every query term with its document frequency, in query order.
    pub terms: Vec<QueryTermView>,
    /// Best cosine between the query and any chunk. `None` on an unembedded vault.
    pub best_cos: Option<f64>,
}

/// One served result with the provenance RRF discarded: which lists ranked its chunk,
/// and how near its vector was. An instrument reading for `make eval`; no per-hit cut
/// shipped (GH #206).
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
    /// This term's weight, `ln((chunks+1)/(df+1))`.
    pub idf: f64,
}

/// One chunk-level search hit, not deduped to notes, with the chunk's full text. For
/// the retrieval eval, whose chunking levers note-rank scoring can't see (ADR-0013).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChunkSearchResult {
    pub path: String,
    /// The chunk's heading breadcrumb (`"Fermentation > Vegetables"`), if any.
    pub heading_path: Option<String>,
    /// Fused relevance score; higher is better.
    pub score: f64,
    /// The chunk's stored text, verbatim.
    pub text: String,
}

/// One grounded-chat answer (flow ④), with its `[n]` markers resolved to the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnswerView {
    /// The answer verbatim as streamed, never rewritten; an unresolved marker is just
    /// absent from `citations`.
    pub answer: String,
    /// One citation per distinct marker naming a real passage, ascending.
    pub citations: Vec<Citation>,
    /// The caller broke the stream; `answer` holds the partial text.
    pub cancelled: bool,
    /// The B2 tools that ran, in order; empty for a plain [`ask`](Vault::ask).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolUseView>,
}

/// One tool run during a chat turn, shown beside the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolUseView {
    /// The tool's name (`b2_passage_pairs`, `b2_read`, …).
    pub name: String,
    /// The call's arguments as JSON text, verbatim.
    pub arguments: String,
    /// `true` for B2's own lookup before asking the model; `false` for the model's call.
    pub seeded: bool,
}

/// One resolved `[n]` citation: the note and an excerpt of the passage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Citation {
    /// The marker as it appears in the answer text (1-based passage number).
    pub marker: usize,
    /// The cited note's path (ADR-0003).
    pub path: String,
    /// A one-line excerpt of the cited passage (its head, length-bounded).
    pub excerpt: String,
}

/// One `b2 similar` candidate: a note near the anchor, not linked to it, with the
/// passage that made it similar. The human decides whether to link it (ADR-0009).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarView {
    pub path: String,
    pub title: Option<String>,
    /// Best chunk-pair similarity to the anchor; higher is nearer.
    pub score: f64,
    /// An excerpt of the candidate chunk that achieved `score`.
    pub evidence: String,
    /// The best-passage z over the anchor's candidate population (GH #192), the input
    /// for the strength band (GH #150). Non-increasing down the list; gates nothing
    /// (ADR-0014). Band landmarks come from `make eval` (GH #182). `None` when
    /// ungraded, which adapters must say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
}

/// Explain for a Similar & unlinked card (GH #236): where a note stands in the anchor's
/// field and the passage pairs behind it. Same computation as [`Vault::similar`], so a
/// served row matches its card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarExplainView {
    pub anchor: NoteSummary,
    pub candidate: NoteSummary,
    /// The surface's list length, so `served` answers "was this a card?".
    pub limit: usize,
    pub standing: SimilarStanding,
    /// The candidate's z when it was scored in a graded field (the card's band input).
    pub z: Option<f64>,
    /// The whole-note (stage 1) rank. Far worse than `rank` means one section carries the
    /// match.
    pub centroid_rank: Option<usize>,
    /// Every scored note's z, nearest first. Empty when ungraded.
    pub population: Vec<f64>,
    /// Each candidate passage with its nearest anchor passage, nearest first.
    pub pairs: Vec<PassagePairView>,
    /// Notes both sides already link with (either direction).
    pub shared_neighbors: Vec<NoteSummary>,
}

/// Where a note stands in an anchor's discovery field: why it is a card, or why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SimilarStanding {
    /// The candidate is the anchor itself.
    SameNote,
    /// The anchor has no stored vectors yet: nothing to compare from.
    AnchorUnembedded,
    /// Directly linked (either direction), so already known.
    Linked,
    /// The candidate has no stored vectors yet.
    Unembedded,
    /// Its whole-note rank fell past the `shortlist`, so passages were never scored.
    NotShortlisted { shortlist: usize },
    /// Scored: `rank` (1-based) of `of` scored notes; `served` when `rank <= limit`.
    Ranked {
        rank: usize,
        of: usize,
        served: bool,
    },
}

/// One passage pair: a candidate passage and the anchor passage nearest to it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PassagePairView {
    pub anchor: PassageView,
    pub candidate: PassageView,
    /// Negated L2, higher is nearer: [`SimilarView::score`]'s unit.
    pub score: f64,
    /// The pair's z on the card's yardstick, `None` when ungraded.
    pub z: Option<f64>,
    /// Both passages hold the same text (a template, a copy): a meaningless match.
    pub identical: bool,
}

/// One passage, as stored in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassageView {
    /// The chunk's heading breadcrumb (`"Fermentation > Vegetables"`), when it has one.
    pub heading_path: Option<String>,
    /// The chunk's stored text, verbatim.
    pub text: String,
}

/// What [`write`](Vault::write) did: the saved path and the new revision the editor
/// chains its next save on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteReport {
    pub path: String,
    pub revision: String,
}

/// What `b2 link` did. `created` is `false` when the edge already existed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkReport {
    pub src_path: String,
    pub dst_path: String,
    pub relation: String,
    pub created: bool,
}
