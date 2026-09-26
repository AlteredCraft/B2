//! The display-ready views the façade returns: what the CLI prints (human and `--json`)
//! and what the desktop reuses as its IPC contract (`ui/src/types.ts` mirrors them).

use crate::ingest::SkippedNote;
use serde::Serialize;

// Doc links only: the views describe what these produce.
#[cfg(doc)]
use super::Vault;
#[cfg(doc)]
use crate::{graph, note};

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
    /// Notes with no chunk awaiting a vector. A note with **no chunks at all** — an empty
    /// body — counts here: there is no vector it could be waiting for, so it must be able
    /// to reach the numerator (see [`crate::db::embed_progress`]).
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
/// rests on; **this** is what the per-hit tail bake-off is argued from (GH #206).
/// That bake-off has run — the labels carry the per-hit depth (`tail_relevant`) —
/// and **no tail fold shipped**: the fused order is not an evidence order, so every
/// admissible prefix cut proved vacuous, and the tail complaint is ordering work
/// (the reranker seam), not disclosure work. This stays an instrument reading,
/// re-priced on every `make eval` run.
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
    /// The B2 tools that ran to produce this answer, in order — empty for a plain
    /// [`ask`](Vault::ask), which retrieves once and offers the model none. Serialized
    /// only when present, so existing JSON consumers see no change.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolUseView>,
}

/// One tool run during a tool-using chat turn — shown beside the answer so a human can
/// see what the explanation was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolUseView {
    /// The tool's name (`b2_passage_pairs`, `b2_read`, …).
    pub name: String,
    /// The call's arguments, as JSON text — the model's own, verbatim, for a call it made.
    pub arguments: String,
    /// `true` for the lookup B2 made itself before asking the model anything; `false`
    /// for a call the model chose.
    pub seeded: bool,
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
    /// (GH #182) — so a surface reads its landmarks off `make eval`'s calibration
    /// block. `None` when no statistic was computed (a fake-embedded space, a pool
    /// under the statistics minimum, or zero variance), which is the adapters' cue
    /// to say the list is *ungraded* rather than let bare cards read as uniformly
    /// weak; serialized only when present, so older JSON consumers see no change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
}

/// **Explain** for a *Similar & unlinked* card (GH #236): where one note stands in an
/// anchor's discovery field, and the passage pairs behind it. Model-free and read from
/// the same computation as [`Vault::similar`], so a served row's rank, z and best pair are
/// exactly the card's. Raw distances are never the point: every grade is a z on the same
/// yardstick as the strength band.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarExplainView {
    pub anchor: NoteSummary,
    pub candidate: NoteSummary,
    /// The list length the explanation is read at: the surface's own, so `served`
    /// answers "was this a card?".
    pub limit: usize,
    pub standing: SimilarStanding,
    /// The candidate's z when it was scored in a graded field (the card's band input).
    pub z: Option<f64>,
    /// The candidate's rank judged by whole-note average (stage 1), when it entered
    /// stage 1. Far worse than a ranked standing's `rank` means one section carries the
    /// match: a buried gem.
    pub centroid_rank: Option<usize>,
    /// Every scored note's z, nearest first: the field the band is relative to. Empty
    /// when ungraded (a fake space, a small pool, no spread).
    pub population: Vec<f64>,
    /// One pair per candidate passage, nearest first, each matched to the anchor passage
    /// nearest to it. Empty when either side has no vectors.
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
    /// Directly linked (either direction), so discovery leaves it out as already known.
    Linked,
    /// The candidate has no stored vectors yet.
    Unembedded,
    /// Its whole-note rank fell past the first-pass shortlist of `shortlist` notes, so
    /// it was never scored passage by passage.
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
    /// Both passages hold the same text (a template, a copy): a perfect match that says
    /// nothing about what the notes are about.
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
