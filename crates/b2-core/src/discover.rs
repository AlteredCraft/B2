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
//! [`DiscoveryFloor`] (GH #150): a **per-anchor z-score** rule, judged **after
//! stage 2** on the exact best-passage distances (GH #192; it judged the stage-1
//! centroid distances until the harness measured that no centroid bar separates
//! labelled mates from strangers — the distributions invert on any multi-topic
//! note, GH #187/#189 — while the best-passage unit separates cleanly on the
//! same dump). Z-scores are what make the floor
//! **model-relative by construction** — the rule compares each candidate against
//! the anchor's own score distribution, never against an absolute cosine, so it
//! survives a model or device swap with no recalibration (the measured failure
//! of absolute floors: eval-calibrated cosines kept 99–100% of a dense
//! real vault's candidates, GH #150). What it deliberately cannot
//! catch is a *pair-level* miscalibration — a single stranger the model scores
//! like a cluster-mate sits above any anchor-local statistic, and judging pair
//! scores directly makes that the floor's one exposed flank; the residue is
//! measured (the eval's watercolor ↔ stain-removal precedent, and the phishing
//! mate still priced under one stranger pair in the stage-2 unit) and belongs to
//! a discovery-side pair-scorer if the data ever demands one.
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

/// Scored pools smaller than this leave the floor inert: a z-score over a
/// handful of distances is statistical noise, and in a vault this small the human
/// can see everything anyway — serving every candidate is the honest posture for
/// a starter vault, not a quality failure. Judged against the stage-2 scored
/// population, which is the same count stage 1 shortlisted on any vault small
/// enough for this guard to matter.
const FLOOR_MIN_POPULATION: usize = 12;

/// The per-anchor discovery quality floor (index-engine.md §3's ruling, GH #150;
/// judged after stage 2 since GH #192).
///
/// Both thresholds are **z-scores against the anchor's own candidate population**
/// in stage-2 best-passage space: stage 2 computes every shortlisted note's exact
/// max-sim anyway, distances over unit vectors are affine in cosine
/// (`d² = 2 − 2·cos`), so the z is nearly free, and — because it is relative to
/// the anchor's own distribution — identical in meaning across models, devices,
/// and vault densities. Judging the passage rather than the centroid is what the
/// harness forced (GH #187/#189): a centroid averages a note's disagreeing
/// sections, so a multi-topic note lands *under* a centroid bar for the anchor
/// its one matching passage genuinely relates to, and *over* it for loner
/// anchors it does not relate to at all — the same note, wrong in both
/// directions, and no constant fixes a rule whose keep/cut populations invert.
/// The best passage is also the evidence the card shows, so the bar and the
/// human now read the same signal. Both bars were re-calibrated for the stage-2
/// unit on the eval's labelled anchors — with the journal-shaped
/// `week-log.md` in the corpus, the shape that killed the centroid rule — and
/// both are re-derived on every `just eval` run, which prints the current
/// admissible window for each and records it in `results.jsonl`
/// (`discovery_z`); `docs/evals/README.md` reads that instrument.
///
/// **The numbers live in the harness, not in this comment** (GH #187). Until
/// then the #150 windows were quoted here as if timeless, and the corpus
/// falsified them the first time it grew a note shape they were never measured
/// against. A constant belongs in code; where it came from belongs where it can
/// be re-measured.
///
/// The two bars answer to **different populations**, which is why they are two
/// fields and not one:
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryFloor {
    /// Suppress the *entire* list when the best candidate's z falls below this —
    /// the whole pool is one diffuse cloud, and "nothing relates" is the honest
    /// answer. Calibrated against **leaders**: it must cut every negative
    /// anchor's best candidate while keeping every positive anchor's. That is
    /// the pair the eval's negatives gate watches, so a bad value here fails
    /// the run.
    pub leader_z: f64,
    /// Keep only candidates at or above this z — the drop-off back into the
    /// anchor's noise floor ends the list. Calibrated against a **different**
    /// pair: strangers on positive anchors (which it must cut) against labelled
    /// mates (which it must keep). Nothing in the exit gate watches that pair —
    /// while `member_z ≤ leader_z` a negative anchor is clean iff its leader is
    /// cut, so this bar can move without the negatives noticing either way. It
    /// is also the bar that decides *existence*: a candidate below it is not
    /// demoted, it is never served — but since GH #192 it judges the same
    /// number the served card presents as evidence (the best passage), rather
    /// than a centroid the evidence never shows, which is what lets a buried
    /// gem exist at all.
    pub member_z: f64,
}

impl Default for DiscoveryFloor {
    fn default() -> Self {
        // Midpoints of the admissible windows the harness re-derived for the
        // stage-2 unit (GH #192) — equidistant from both measured edges, the
        // placement that survives the most corpus drift. The windows themselves
        // live in the harness (`just eval`'s floor-calibration block), not here.
        Self {
            leader_z: 1.96,
            member_z: 1.49,
        }
    }
}

/// One discovery candidate: a note near the anchor and not already connected, ranked
/// by `z` where the floor computed one and by `score` otherwise (see
/// [`candidates`]). Owned, so the façade can resolve it to a [`SimilarView`](crate::vault::SimilarView)
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
    /// shortlist population — how far its best pair stands above the anchor's own
    /// noise floor, the number the [`DiscoveryFloor`] judged (GH #192). `None`
    /// when the floor was off or inert (no statistics were computed). An adapter
    /// wanting to show *strength* shows a band derived from this, never the raw
    /// score (GH #150) — and it is strictly monotonic in `score` within one
    /// query (affine in the squared best-pair distance, which `score` negates
    /// the root of), so the band, the row order, and the gate are one number by
    /// construction.
    pub z: Option<f64>,
}

/// Generate up to `limit` connection-discovery candidates for `anchor`, strongest
/// first: by best-passage distance (ties break on `note_path`, for determinism),
/// which is one order with three names — the exact stage-2 `score`, the `z` the
/// floor judged, and the strength band an adapter paints, each strictly
/// monotonic in the others within one query, so none of them can disagree with
/// the row order (GH #150's coherence, held by construction since the GH #192
/// reorder).
/// `limit` is a cap, not a
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
    // affine in this squared distance — also descending z. One sort key serves the row
    // order, the strength band, and the floor, so none of the three can disagree
    // (the coherence GH #150 demanded, now by construction rather than by
    // comparator).
    scored.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    // The floor, judged AFTER stage 2 on the best-passage distances (GH #192): the
    // population is every scored shortlist note — on a personal-scale vault,
    // every unlinked note there is (the shortlist covers any vault at or below
    // SHORTLIST_MIN candidates; above that it is the anchor's centroid-nearest
    // slice, a bias process rule 3's dogfooding obligation owns). z is oriented so
    // nearer = higher, the same arithmetic the stage-1 rule used, one stage later.
    // When the statistics exist, every candidate's z travels to the output; the
    // gate then ends the list at the member bar — or empties it at the leader
    // gate, the "this whole pool is one diffuse cloud" verdict. Judging max-sim
    // instead of the centroid is what lets a buried gem clear the bar (its best
    // pair is the signal) while a diluted multi-topic centroid no longer tops a
    // loner's list (its best pair to a stranger is weak — GH #189's inversion,
    // measured in both units by the harness's `floor calibration` blocks).
    let mut zs: Option<Vec<f64>> = None;
    if let Some(floor) = floor {
        if scored.len() >= FLOOR_MIN_POPULATION {
            let n = scored.len() as f64;
            let mean = scored.iter().map(|(d, _, _)| *d as f64).sum::<f64>() / n;
            let var = scored
                .iter()
                .map(|(d, _, _)| (*d as f64 - mean).powi(2))
                .sum::<f64>()
                / (n - 1.0);
            let sd = var.sqrt();
            if sd > 0.0 {
                let mut z: Vec<f64> = scored
                    .iter()
                    .map(|(d, _, _)| (mean - *d as f64) / sd)
                    .collect();
                // scored is sorted nearest-first, so z[0] is the leader's.
                if z[0] < floor.leader_z {
                    return Ok(Vec::new());
                }
                let keep = z.partition_point(|&v| v >= floor.member_z);
                scored.truncate(keep);
                z.truncate(keep);
                zs = Some(z);
            }
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
