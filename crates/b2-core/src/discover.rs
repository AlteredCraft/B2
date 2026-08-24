//! Connection-discovery candidate generation — the engine behind **`b2 similar`**. It
//! surfaces the notes to *consider* linking; the human is the precision gate, and
//! `b2 link` commits one (ADR-0009). It is the only discovery stage, and the only one
//! that reads the graph.
//!
//! A candidate is a note **semantically near the anchor but not already connected** —
//! the *complement* of the graph, not the intersection (that is
//! [`crate::search::graph_filtered_search`], a scoped-traversal primitive and the wrong
//! tool here). Generation is deliberately **recall-oriented**, and the **ranked list is
//! always served**: no statistic gates membership, and `limit` under-fills only for want
//! of scorable notes. ADR-0014 carries the retired existence gate's whole evidence trail
//! — including why an anchor-local statistic cannot tell *nothing is related* from
//! *everything is related*, and the terms any replacement signal must win on. The z
//! survives ungated as the strength band's input.
//!
//! Mechanics are **two-stage**:
//!
//! 1. **Coarse, O(notes):** rank every note by its stored *centroid*'s distance to the
//!    anchor's, minus the anchor and its 1-hop neighbours, and keep a shortlist many
//!    times larger than `limit`.
//! 2. **Exact, O(shortlist):** for each shortlisted note, load its chunk vectors and
//!    score the exact max-sim across the anchor's chunks, keeping the chunk that
//!    achieved it as evidence.
//!
//! Stage 2's scoring is the same exact max-sim a whole-space scan computed, so a
//! shortlist that covers the vault reproduces it exactly; what the shape buys is reading
//! N_notes centroid rows instead of N_chunks vector rows (#38). Discovery is vector-only
//! and **re-embeds nothing** — the anchor is represented by the vectors already stored,
//! never by an `embed_query` of its text (bge's asymmetric query prefix is the wrong
//! side). Graph distance beyond the 1-hop exclusion is **not** a ranking signal;
//! weighting it is a deferred, eval-gated experiment.

use crate::db;
use crate::embed::{centroid_of, l2_sq, unpack_f32_into};
use crate::error::Result;
use crate::graph;
use rusqlite::Connection;

/// The exclusion radius: a candidate must not already be *directly* linked to the
/// anchor. Fixed at 1 so triadic-closure candidates — two hops away, transitively
/// related but with no direct edge — stay in the pool.
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

/// Scored pools smaller than this carry no statistic: a z over a handful of distances
/// is noise, so under it every candidate is served **ungraded** and an adapter says so
/// rather than banding. It moves *banding only*, never membership — serving is
/// continuous in population size (ADR-0014; the retired gate shared this guard, which
/// made it a serve-everything/serve-nothing cliff at n = 12).
const STATS_MIN_POPULATION: usize = 12;

/// One discovery candidate: a note near the anchor and not already connected, ranked by
/// best-passage `score`. Owned, so the façade can resolve it to a
/// [`SimilarView`](crate::vault::SimilarView) without threading a lifetime through.
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
    /// The candidate's stage-2 best-passage z against the anchor's scored shortlist
    /// population. It **gates nothing** (ADR-0014): it is the strength band's input and
    /// nothing else. `None` when no statistic was computed (an ungraded space, a pool
    /// under [`STATS_MIN_POPULATION`], or zero variance). It is strictly monotonic in
    /// `score` within one query, so the band and the row order are one number by
    /// construction.
    pub z: Option<f64>,
}

/// Generate up to `limit` discovery candidates for `anchor`, strongest first by
/// best-passage distance (ties on `note_path`, for determinism). That is one order with
/// three names — the stage-2 `score`, the `z`, and the strength band an adapter paints,
/// each strictly monotonic in the others — so none of them can disagree with the row
/// order. **The ranked list is always served** (ADR-0014): `limit` is a cap, and the
/// list under-fills only for want of scorable notes.
///
/// `grade` asks for the per-candidate z beside the ranking, computed when the scored
/// population is at least [`STATS_MIN_POPULATION`] with nonzero variance. The façade
/// passes `false` for a fake-embedded space, whose hash vectors have no semantic
/// geometry to claim a statistic over. Grading changes what the rows *carry*, never
/// which rows exist or their order.
///
/// Returns empty when the vault has no embedding space, when the anchor has no stored
/// vectors, or when `limit` is 0 — there is nothing to search from.
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

    // Stage 1 ends here, and nothing judges it: the shortlist is a recall device, never
    // a quality gate (GH #192). It used to be both — cutting the coarse list on centroid
    // z meant max-sim could never rescue a candidate whose best passage is far nearer
    // than its centroid suggests, which is exactly a multi-topic note's shape (ADR-0014).
    coarse.truncate(
        limit
            .saturating_mul(SHORTLIST_PER_RESULT)
            .max(SHORTLIST_MIN),
    );

    // Stage 2 — exact max-sim over the whole shortlist: per note, the best (smallest
    // squared-L2) pair across the anchor's chunks and its own. Squared L2 is the same
    // ranking key without the per-comparison `sqrt`, applied once per surfaced candidate
    // below. Strictly-less keeps the earliest chunk on ties. A shortlisted note with no
    // stored chunk vectors (possible mid-embed) scores nothing and drops out.
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
    // Nearest-first, ties by path: the served order, and — because z below is affine in
    // this squared distance — also descending z. One sort key serves the row order and
    // the strength band, so the two can never disagree.
    scored.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    // The statistic, computed AFTER stage 2 on the best-passage distances (GH #192) and
    // gating nothing (ADR-0014): the population is every scored shortlist note, which on
    // a personal-scale vault is every unlinked note there is. Above SHORTLIST_MIN it is
    // the anchor's centroid-nearest slice, a bias the dogfooding obligation owns. z is
    // oriented so nearer = higher, and travels to the output as the band's input.
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
