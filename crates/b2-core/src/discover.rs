//! Connection-discovery candidate generation — the engine behind **`b2 similar`**
//! (index-engine.md §3, resolved 2026-07-01; invariants.md). It surfaces the notes to
//! *consider* linking; the human is the precision gate
//! and `b2 link` commits one. It is the only discovery stage, and the only one that
//! reads the graph.
//!
//! A candidate is a note **semantically near the anchor but not already connected**:
//! the *complement* of the graph, not the intersection. (The intersection —
//! semantic-nearest chunks *within* k hops — is [`crate::search::graph_filtered_search`],
//! a scoped-traversal primitive, the wrong tool here.) *Generation* is deliberately
//! **recall-oriented**: it over-produces, and the human decides which are worth a
//! link. *Surfacing* answers a **relative** question — "what in my vault belongs
//! next to this note?" — so the **ranked list is served** (GH #197, superseding
//! the GH #150 existence gate): `limit` is a cap, not a promise, and the list
//! under-fills only for want of scorable notes, never because a statistic ruled
//! the candidates unworthy. The retired rule — `DiscoveryFloor`, a per-anchor
//! z existence gate judged on the best-passage distances since GH #192 — asked
//! an *absolute* question ("does anything stand out from the background?") whose
//! model, a single-population outlier test, is valid only when related notes are
//! rare outliers in a dominant unrelated tail. GH #196 measured a single-domain
//! vault violating that assumption by construction: with no unrelated tail the
//! population mean is "moderately related" rather than noise, every leader's z
//! compresses under the gate, and 16 of 17 notes served nothing — including
//! three same-subject articles that are each other's top-ranked neighbours. The
//! gate could not distinguish *"nothing is related"* from *"everything is
//! related"*, and its member bar's admissible window had already been measured
//! empty on the eval corpus's own numbers (GH #187), so GH #197 deleted the bar
//! and retired the gate rather than re-tuning either. The z itself **survives as
//! a statistic**: it is the strength band's input (`SimilarView::z`), computed
//! whenever the population carries one and gating nothing. Any replacement
//! existence signal must win the evidence-gated bake-off GH #197 Phase 2
//! defines — judged on the orthogonal corpus, the dense fixture, and real vaults
//! via `just calibrate`, with "no gate at all" an admissible winner — and must
//! behave continuously in population size. What an anchor-local statistic could
//! never catch either way is a *pair-level* miscalibration — a stranger the
//! model scores like a cluster-mate; that residue is measured (the phishing
//! pair, served at best-passage rank 4 under always-serve, so it is ordering
//! quality now rather than existence) and belongs to a discovery-side
//! pair-scorer if the data ever demands one.
//!
//! **The first bake-off ran and nothing won** (GH #200, 2026-08-22), so the
//! code below is unchanged by it — which is the outcome being recorded. Its
//! question was narrower than existence: D1 as redrafted permits only a
//! *default disclosure* fold, a prefix of the list with everything below it
//! still served. The leading candidate — mutual-kNN reciprocity — was swept and
//! priced by `just eval`'s `discovery fold bake-off` block on both corpora, and
//! its admissible window is empty in both directions at once: the depth that
//! hides no labelled mate (`k = 14` on the orthogonal corpus, `k = 7` on the
//! dense fixture) sits past the depth that still folds a loner's view to empty
//! (`k ≤ 11`, and never all five of them), while every `k ≤ 5` darkens panes on
//! a vault where everything relates. The two safe depths are the same
//! *fraction* of their candidate pools rather than the same constant, which is
//! the reason to stop rather than re-tune: a reciprocity depth is a rank **in a
//! population**, so it transfers no better than the cosine and z constants
//! before it, and the fraction is not computable past `SHORTLIST_MIN` anyway.
//! `candidates` therefore returns what it always did — one undivided ranked
//! list, no per-row disclosure flag. What the sweep *did* establish is kept as
//! evidence for the pair-scorer above: reciprocity can tell a loner from a
//! dense-vault note (four of five loners folded to empty while every dense pane
//! stayed lit), the discrimination GH #196 proved no anchor-local statistic can
//! make.
//!
//! Mechanics are **two-stage** (#38; index-engine.md):
//!
//! 1. **Coarse, O(notes):** rank every note by the distance of its stored *centroid*
//!    (`note_centroids`, maintained by the embed pass) to the anchor's centroid,
//!    minus the anchor and its direct (1-hop) neighbors, and keep a shortlist many
//!    times larger than `limit`.
//! 2. **Exact, O(shortlist):** for each shortlisted note, load its chunk vectors and
//!    score the exact max-sim — the best pair across the anchor's chunks × that
//!    note's chunks — keeping the chunk that achieved it as evidence.
//!
//! Only the shortlist changes with stage 1; stage 2's scoring is the same exact
//! max-sim the previous whole-space scan computed, so a shortlist that covers the
//! vault (any small/test vault) reproduces it exactly. What the shape buys: the
//! per-open heavy pass reads N_notes centroid rows instead of N_chunks vector rows —
//! effectively flat as the vault grows (the previous exact scan was ~4.4 s at ~38.6k
//! chunks, #38). Discovery is vector-only and **re-embeds nothing** — the anchor is
//! represented by the vectors already stored, never by an `embed_query` of its text
//! (bge's asymmetric query prefix is the wrong side). Graph distance beyond the
//! 1-hop exclusion is **not** a ranking signal — graph-distant "bridge" candidates
//! ride along unboosted; weighting distance (closure vs. serendipity) is a deferred,
//! eval-gated experiment (GitHub Issues).

use crate::db;
use crate::embed::{centroid_of, l2_sq, unpack_f32_into};
use crate::error::Result;
use crate::graph;
use rusqlite::Connection;

/// The exclusion radius: a candidate must not already be *directly* linked to the
/// anchor. Fixed at 1 by decision (index-engine.md §3) so triadic-closure candidates — a note
/// two hops away, transitively related but with no direct edge — stay in the pool.
const EXCLUDE_HOPS: usize = 1;

/// Floor on the stage-1 shortlist. Generous relative to any `limit` a human-facing
/// surface asks for: discovery is recall-oriented (the human is the precision gate),
/// so the coarse stage must never be the reason a nearby note goes missing. On any
/// vault at or below this many candidate notes the two-stage result is *exactly*
/// the old whole-space scan's.
const SHORTLIST_MIN: usize = 200;

/// Stage-1 shortlist size per requested result: `limit × this`, floored at
/// [`SHORTLIST_MIN`]. A wide margin over `limit` because a note's centroid can rank
/// a few places below where its single best chunk deserves (the centroid smooths
/// over the note's chunks); the exact stage re-ranks whatever survives.
const SHORTLIST_PER_RESULT: usize = 20;

/// Scored pools smaller than this carry no statistic: a z-score over a handful
/// of distances is noise, so under this every candidate is served **ungraded**
/// (`z: None`) and an adapter says the list is ungraded rather than banding it.
/// This threshold moves *banding only*, never membership — serving is
/// continuous in population size (GH #197; the retired existence gate shared
/// this guard, which made it a serve-everything/serve-nothing cliff at n = 12,
/// GH #196's amplifier C). Judged against the stage-2 scored population, which
/// is the same count stage 1 shortlisted on any vault small enough for this
/// guard to matter.
const STATS_MIN_POPULATION: usize = 12;

/// One discovery candidate: a note near the anchor and not already connected,
/// ranked by best-passage `score` — the order `z`, where computed, is strictly
/// monotonic in (see [`candidates`]). Owned, so the façade can resolve it to a
/// [`SimilarView`](crate::vault::SimilarView)
/// for `b2 similar` without threading a lifetime through generation.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateNote {
    /// The candidate note's vault-relative path — its identity (L1).
    pub note_path: String,
    /// Best chunk-pair similarity across the anchor's chunks × this note's chunks —
    /// higher is nearer (negated L2 distance, matching [`Hit`](crate::search::Hit)).
    pub score: f64,
    /// The candidate's chunk that achieved `score` — the passage that made this note
    /// similar, surfaced by `b2 similar` as the evidence for *why* it appeared.
    pub evidence_chunk_id: i64,
    /// The candidate's stage-2 best-passage z-score against the anchor's scored
    /// shortlist population — how far its best pair stands above the anchor's
    /// own candidate distribution. Since GH #197 this **gates nothing**: it is
    /// the strength band's input and nothing else. `None` when no statistic was
    /// computed (an ungraded space, a pool under [`STATS_MIN_POPULATION`], or
    /// zero variance). An adapter wanting to show *strength* shows a band
    /// derived from this, never the raw score (GH #150) — and it is strictly
    /// monotonic in `score` within one query (affine in the squared best-pair
    /// distance, which `score` negates the root of), so the band and the row
    /// order are one number by construction.
    pub z: Option<f64>,
}

/// Generate up to `limit` connection-discovery candidates for `anchor`, strongest
/// first: by best-passage distance (ties break on `note_path`, for determinism),
/// which is one order with three names — the exact stage-2 `score`, the `z`, and
/// the strength band an adapter paints, each strictly monotonic in the others
/// within one query, so none of them can disagree with the row order (GH #150's
/// coherence, held by construction since the GH #192 reorder). **The ranked list
/// is always served** (GH #197): `limit` is a cap, not a promise, and the list
/// under-fills only for want of scorable notes (a vault with fewer unlinked
/// notes than `limit`, or a mid-embed vault whose shortlisted notes still lack
/// stored chunk vectors) — never because a statistic gated it.
///
/// `grade` asks for the per-candidate z beside the ranking — the strength band's
/// input, computed when the scored population is at least
/// [`STATS_MIN_POPULATION`] with nonzero variance and skipped otherwise (no
/// meaningful statistic exists there). The façade passes `false` for a
/// fake-embedded space, whose hash vectors have no semantic geometry to claim a
/// statistic over. Grading changes what the rows *carry*, never which rows
/// exist or their order.
///
/// Returns empty when the vault has no embedding space yet, when the anchor has no
/// stored vectors (unknown, empty, or not-yet-embedded note), or when `limit` is 0 —
/// there is nothing to search from. Excludes the anchor itself and its direct
/// neighbors; everything else near in vector space is a candidate.
pub fn candidates(
    conn: &Connection,
    anchor: &str,
    limit: usize,
    grade: bool,
) -> Result<Vec<CandidateNote>> {
    if limit == 0 || !db::embedding_space_exists(conn)? {
        return Ok(Vec::new());
    }
    // The anchor's own stored vectors, loaded once (re-embeds nothing — index-engine.md §3);
    // none ⇒ nothing to search from. Its centroid is computed in-process from them
    // rather than read back, so an anchor mid-embed still discovers from what it has.
    let anchor_vecs: Vec<Vec<f32>> = db::note_chunk_vectors(conn, anchor)?
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    let Some(anchor_centroid) = centroid_of(&anchor_vecs) else {
        return Ok(Vec::new());
    };

    // The only use of the graph in generation: subtract what's already linked — the
    // anchor and everything within 1 hop (self + direct neighbors).
    let exclude = graph::reachable_within(conn, anchor, EXCLUDE_HOPS)?;

    // Stage 1 — coarse shortlist over note centroids: one O(notes) scan, excluded
    // notes skipped up front so they never occupy a shortlist slot.
    let mut coarse: Vec<(f32, String)> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    db::for_each_note_centroid(conn, |note, blob| {
        if exclude.contains(note) {
            return; // the anchor or a direct neighbor — already connected
        }
        unpack_f32_into(blob, &mut scratch);
        coarse.push((l2_sq(&anchor_centroid, &scratch), note.to_string()));
    })?;
    coarse.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });

    // Stage 1 ends here, and nothing judges it: the shortlist is a recall device,
    // never a quality gate (GH #192). It used to be both — the floor cut the
    // coarse list on centroid z before stage 2 ran, which meant max-sim could
    // never rescue a candidate whose best passage is far nearer than its centroid
    // suggests. A multi-topic note is exactly that shape, and the harness measured
    // the consequence twice over: no centroid member bar separates labelled mates
    // from strangers (the two distributions invert, GH #187), and a journal-shaped
    // note defeats the centroid rule in both directions at once — its own gem
    // suppressed while its diluted, hub-like centroid tops loner anchors' lists on
    // content it does not contain (GH #189).
    coarse.truncate(
        limit
            .saturating_mul(SHORTLIST_PER_RESULT)
            .max(SHORTLIST_MIN),
    );

    // Stage 2 — exact max-sim over the whole shortlist: per note, the best
    // (smallest squared-L2) pair across the anchor's chunks × its chunks. Squared
    // L2 is the same ranking key as L2 without the per-comparison `sqrt`
    // (monotonic); the `sqrt` is applied once per surfaced candidate below.
    // Strictly-less keeps the earliest (lowest-`seq`) chunk on ties,
    // deterministically. A shortlisted note with no stored chunk vectors (possible
    // mid-embed) scores nothing and drops out.
    let mut scored: Vec<(f32, String, i64)> = Vec::new();
    for (_, note_path) in coarse {
        let mut best: Option<(f32, i64)> = None;
        for (chunk_id, v) in db::note_chunk_vectors(conn, &note_path)? {
            for a in &anchor_vecs {
                let dist_sq = l2_sq(a, &v);
                if best.is_none_or(|(cur, _)| dist_sq < cur) {
                    best = Some((dist_sq, chunk_id));
                }
            }
        }
        if let Some((dist_sq, evidence_chunk_id)) = best {
            scored.push((dist_sq, note_path, evidence_chunk_id));
        }
    }
    // Nearest-first, ties by path: the served order, and — because z below is
    // affine in this squared distance — also descending z. One sort key serves
    // the row order and the strength band, so the two can never disagree (the
    // coherence GH #150 demanded, now by construction rather than by
    // comparator).
    scored.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    // The statistic, computed AFTER stage 2 on the best-passage distances
    // (GH #192) and gating nothing (GH #197): the population is every scored
    // shortlist note — on a personal-scale vault, every unlinked note there is
    // (the shortlist covers any vault at or below SHORTLIST_MIN candidates;
    // above that it is the anchor's centroid-nearest slice, a bias process
    // rule 3's dogfooding obligation owns). z is oriented so nearer = higher.
    // Every candidate's z travels to the output as the strength band's input;
    // no bar consults it. The retired existence gate lived here — a leader gate
    // that emptied the list and a member bar that ended it — until GH #196
    // measured the model's assumption (related notes as rare outliers in a
    // dominant unrelated tail) failing wholesale on a single-domain vault, where
    // the same arithmetic reads "everything is related" as "nothing is".
    let mut zs: Option<Vec<f64>> = None;
    if grade && scored.len() >= STATS_MIN_POPULATION {
        let n = scored.len() as f64;
        let mean = scored.iter().map(|(d, _, _)| *d as f64).sum::<f64>() / n;
        let var = scored
            .iter()
            .map(|(d, _, _)| (*d as f64 - mean).powi(2))
            .sum::<f64>()
            / (n - 1.0);
        let sd = var.sqrt();
        if sd > 0.0 {
            zs = Some(
                scored
                    .iter()
                    .map(|(d, _, _)| (mean - *d as f64) / sd)
                    .collect(),
            );
        }
    }

    let out = scored
        .into_iter()
        .enumerate()
        .take(limit)
        .map(
            |(i, (dist_sq, note_path, evidence_chunk_id))| CandidateNote {
                note_path,
                score: -(dist_sq.sqrt() as f64), // nearer = higher, matching Hit's -L2
                evidence_chunk_id,
                z: zs.as_ref().and_then(|z| z.get(i)).copied(),
            },
        )
        .collect();
    Ok(out)
}
