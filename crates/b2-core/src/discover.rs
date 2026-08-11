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
//! link. *Surfacing* is not (index-engine.md §3, ruled 2026-08-05): **`limit` is a
//! cap, not a promise** — zero candidates is a legitimate, honest answer when
//! nothing in the vault genuinely relates. The projection of that ruling is
//! [`DiscoveryFloor`] (GH #150): a **per-anchor z-score** rule over the stage-1
//! centroid-distance population, calibrated from the eval's labelled anchors
//! (docs/evals/runlog.md, 2026-08-11). Z-scores are what make the floor
//! **model-relative by construction** — the rule compares each candidate against
//! the anchor's own score distribution, never against an absolute cosine, so it
//! survives a model or device swap with no recalibration (the measured failure
//! of absolute floors: eval-calibrated cosines kept 99–100% of a dense
//! real vault's candidates, runlog 2026-08-10). What it deliberately cannot
//! catch is a *pair-level* miscalibration — a single stranger the model scores
//! like a cluster-mate sits above any anchor-local statistic; that residue is
//! measured (the eval's watercolor ↔ stain-removal pair) and belongs to a
//! discovery-side pair-scorer if the data ever demands one.
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

/// Candidate pools smaller than this leave the floor inert: a z-score over a
/// handful of distances is statistical noise, and in a vault this small the human
/// can see everything anyway — serving every candidate is the honest posture for
/// a starter vault, not a quality failure.
const FLOOR_MIN_POPULATION: usize = 12;

/// The per-anchor discovery quality floor (index-engine.md §3's ruling, GH #150).
///
/// Both thresholds are **z-scores against the anchor's own candidate population**
/// in stage-1 centroid space: the coarse scan already computes every candidate's
/// centroid distance, distances over unit centroids are affine in cosine
/// (`d² = 2 − 2·cos`), so the z is free, and — because it is relative to the
/// anchor's own distribution — identical in meaning across models, devices, and
/// vault densities. Calibrated on the eval's labelled anchors (runlog 2026-08-11):
/// positive anchors put every labelled mate at z ≥ +1.90 while diffuse loners'
/// best candidates sit ≤ +1.73, and the same member bar trims a 228-note
/// single-author vault to 1–4 candidates. The default sits mid-window rather
/// than on either measured edge.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryFloor {
    /// Suppress the *entire* list when the best candidate's z falls below this —
    /// the whole pool is one diffuse cloud, and "nothing relates" is the honest
    /// answer. Measured window on the labelled corpus: (1.74, 1.88].
    pub leader_z: f64,
    /// Keep only candidates at or above this z — the drop-off back into the
    /// anchor's noise floor ends the list. Measured window: (1.16, 1.90].
    pub member_z: f64,
}

impl Default for DiscoveryFloor {
    fn default() -> Self {
        Self {
            leader_z: 1.85,
            member_z: 1.85,
        }
    }
}

/// One discovery candidate: a note near the anchor and not already connected, ranked
/// by `score`. Owned, so the façade can resolve it to a [`SimilarView`](crate::vault::SimilarView)
/// for `b2 similar` without threading a lifetime through generation.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateNote {
    /// The candidate note's `b2id`.
    pub note_b2id: String,
    /// Best chunk-pair similarity across the anchor's chunks × this note's chunks —
    /// higher is nearer (negated L2 distance, matching [`Hit`](crate::search::Hit)).
    pub score: f64,
    /// The candidate's chunk that achieved `score` — the passage that made this note
    /// similar, surfaced by `b2 similar` as the evidence for *why* it appeared.
    pub evidence_chunk_id: i64,
    /// The candidate's stage-1 z-score against the anchor's candidate population —
    /// how far it stands above the anchor's own noise floor, the number the
    /// [`DiscoveryFloor`] judged. `None` when the floor was off or inert (no
    /// statistics were computed). An adapter wanting to show *strength* shows a
    /// band derived from this, never the raw score (GH #150).
    pub z: Option<f64>,
}

/// Generate up to `limit` connection-discovery candidates for `anchor`, best score
/// first (ties broken by `note_b2id` for determinism). `limit` is a cap, not a
/// promise (index-engine.md §3): with a [`DiscoveryFloor`] the list ends where the
/// anchor's own score distribution says the candidates stop being signal — possibly
/// at zero — and without one it under-fills only for want of scorable notes (a
/// vault with fewer unlinked notes than `limit`, or a mid-embed vault whose
/// shortlisted notes still lack stored chunk vectors).
///
/// `floor: None` disables quality gating entirely — the caller's explicit "show me
/// the raw nearest" (the CLI's `--no-floor`, and the façade's choice for a vector
/// space with no semantic geometry, i.e. the fake embedder). Even with a floor the
/// gate stays inert on pools under [`FLOOR_MIN_POPULATION`] or with zero variance —
/// no meaningful statistic exists there, and serving everything is the honest
/// posture.
///
/// Returns empty when the vault has no embedding space yet, when the anchor has no
/// stored vectors (unknown, empty, or not-yet-embedded note), or when `limit` is 0 —
/// there is nothing to search from. Excludes the anchor itself and its direct
/// neighbors; everything else near in vector space is a candidate.
pub fn candidates(
    conn: &Connection,
    anchor: &str,
    limit: usize,
    floor: Option<&DiscoveryFloor>,
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

    // The floor, judged on the FULL stage-1 population before any truncation: the
    // coarse scan just scored every candidate in the vault, which is exactly the
    // distribution a per-anchor z is meaningful against (a truncated shortlist's
    // mean/σ would be biased toward the top). z is oriented so nearer = higher
    // (distance flipped), matching how the calibration was measured. When the
    // statistics exist, every candidate's z travels to the output; the gate then
    // ends the list at the member bar — or empties it at the leader gate, the
    // "this whole pool is one diffuse cloud" verdict.
    let mut zs: Option<Vec<f64>> = None;
    if let Some(floor) = floor {
        if coarse.len() >= FLOOR_MIN_POPULATION {
            let n = coarse.len() as f64;
            let mean = coarse.iter().map(|(d, _)| *d as f64).sum::<f64>() / n;
            let var = coarse
                .iter()
                .map(|(d, _)| (*d as f64 - mean).powi(2))
                .sum::<f64>()
                / (n - 1.0);
            let sd = var.sqrt();
            if sd > 0.0 {
                let mut z: Vec<f64> = coarse
                    .iter()
                    .map(|(d, _)| (mean - *d as f64) / sd)
                    .collect();
                // coarse is sorted nearest-first, so z[0] is the leader's.
                if z[0] < floor.leader_z {
                    return Ok(Vec::new());
                }
                let keep = z.partition_point(|&v| v >= floor.member_z);
                coarse.truncate(keep);
                z.truncate(keep);
                zs = Some(z);
            }
        }
    }

    coarse.truncate(
        limit
            .saturating_mul(SHORTLIST_PER_RESULT)
            .max(SHORTLIST_MIN),
    );

    // Stage 2 — exact max-sim over the shortlist only: per note, the best (smallest
    // squared-L2) pair across the anchor's chunks × its chunks. Squared L2 is the
    // same ranking key as L2 without the per-comparison `sqrt` (monotonic); the
    // `sqrt` is applied once per surfaced candidate below. Strictly-less keeps the
    // earliest (lowest-`seq`) chunk on ties, deterministically. A shortlisted note
    // with no stored chunk vectors (possible mid-embed) scores nothing and drops out.
    let mut out: Vec<CandidateNote> = Vec::new();
    for (i, (_, note_b2id)) in coarse.into_iter().enumerate() {
        let mut best: Option<(f32, i64)> = None;
        for (chunk_id, v) in db::note_chunk_vectors(conn, &note_b2id)? {
            for a in &anchor_vecs {
                let dist_sq = l2_sq(a, &v);
                if best.is_none_or(|(cur, _)| dist_sq < cur) {
                    best = Some((dist_sq, chunk_id));
                }
            }
        }
        if let Some((dist_sq, evidence_chunk_id)) = best {
            out.push(CandidateNote {
                note_b2id,
                score: -(dist_sq.sqrt() as f64), // nearer = higher, matching Hit's -L2
                evidence_chunk_id,
                z: zs.as_ref().and_then(|z| z.get(i)).copied(),
            });
        }
    }

    // Best score first; ties broken by id so the ranking (and thus `limit`'s prefix)
    // is deterministic.
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.note_b2id.cmp(&b.note_b2id))
    });
    out.truncate(limit);
    Ok(out)
}
