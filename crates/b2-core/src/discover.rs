//! Connection-discovery candidate generation — the engine behind **`b2 similar`**. It
//! surfaces the notes to *consider* linking; the human is the precision gate, and
//! `b2 link` commits one (ADR-0009). It is the only discovery stage, and the only one
//! that reads the graph.
//!
//! A candidate is a note **semantically near the anchor but not already connected** —
//! the *complement* of the graph, not the intersection. Generation is deliberately **recall-oriented**, and the **ranked list is
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

/// The statistic behind the strength band: the mean and spread of the scored
/// population's best-passage **squared** distances. It grades; it never gates
/// (ADR-0014). Any distance in the same space can be read against it, which is how a
/// passage pair in the explain view is graded on the same yardstick as the cards.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Population {
    mean: f64,
    sd: f64,
}

impl Population {
    /// The z of one squared distance, oriented so nearer is higher.
    fn z(&self, dist_sq: f32) -> f64 {
        (self.mean - dist_sq as f64) / self.sd
    }
}

/// One anchor's whole discovery field: the stage-1 order, the stage-2 scores, and the
/// statistic. [`candidates`] serves a prefix of it and [`explain`] reads one note's place
/// in it, so a card and its explanation are two reads of one computation and can never
/// disagree.
struct Field {
    /// Every unlinked note with a centroid, nearest centroid first (ties by path),
    /// before the shortlist cut.
    coarse: Vec<String>,
    /// How many of `coarse` stage 2 was allowed to score.
    shortlist: usize,
    /// Stage 2: `(squared distance, note, evidence chunk)` for every shortlisted note
    /// with stored vectors, nearest first (ties by path).
    scored: Vec<(f32, String, i64)>,
    /// `None` when ungraded: not asked for, a pool under [`STATS_MIN_POPULATION`], or
    /// zero variance.
    population: Option<Population>,
    /// The anchor and its 1-hop neighbours: excluded from discovery.
    excluded: std::collections::HashSet<String>,
}

/// Compute `anchor`'s field at the shortlist a `limit`-sized list uses. `None` when
/// there is nothing to search from: no embedding space, or an anchor with no stored
/// vectors.
fn field(conn: &Connection, anchor: &str, limit: usize, grade: bool) -> Result<Option<Field>> {
    if !db::embedding_space_exists(conn)? {
        return Ok(None);
    }
    // The anchor's own stored vectors, loaded once (re-embeds nothing — index-engine.md §3);
    // none ⇒ nothing to search from. Its centroid is computed in-process from them
    // rather than read back, so an anchor mid-embed still discovers from what it has.
    let (anchor_ids, anchor_vecs): (Vec<i64>, Vec<Vec<f32>>) =
        db::note_chunk_vectors(conn, anchor)?.into_iter().unzip();
    let Some(anchor_centroid) = centroid_of(&anchor_vecs) else {
        return Ok(None);
    };

    // The only use of the graph in generation: subtract what's already linked — the
    // anchor and everything within 1 hop (self + direct neighbors).
    let excluded = graph::reachable_within(conn, anchor, EXCLUDE_HOPS)?;

    // Stage 1 — coarse shortlist over note centroids: one O(notes) scan, excluded
    // notes skipped up front so they never occupy a shortlist slot.
    let mut coarse: Vec<(f32, String)> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    db::for_each_note_centroid(conn, |note, blob| {
        if excluded.contains(note) {
            return; // the anchor or a direct neighbor — already connected
        }
        unpack_f32_into(blob, &mut scratch);
        coarse.push((l2_sq(&anchor_centroid, &scratch), note.to_string()));
    })?;
    coarse.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let coarse: Vec<String> = coarse.into_iter().map(|(_, note)| note).collect();

    // Stage 1 ends here, and nothing judges it: the shortlist is a recall device, never
    // a quality gate (GH #192). It used to be both — cutting the coarse list on centroid
    // z meant max-sim could never rescue a candidate whose best passage is far nearer
    // than its centroid suggests, which is exactly a multi-topic note's shape (ADR-0014).
    let shortlist = limit
        .saturating_mul(SHORTLIST_PER_RESULT)
        .max(SHORTLIST_MIN);

    // Stage 2 — exact max-sim over the whole shortlist: per note, the best (smallest
    // squared-L2) of its [`nearest_pairs`] — the same kernel the explanation reads, so
    // the card's evidence is by construction the explanation's first pair. The earliest
    // chunk wins a tie. A shortlisted note with no stored chunk vectors (possible
    // mid-embed) scores nothing and drops out.
    let mut scored: Vec<(f32, String, i64)> = Vec::new();
    for note_path in coarse.iter().take(shortlist) {
        let pairs = nearest_pairs(
            &anchor_ids,
            &anchor_vecs,
            db::note_chunk_vectors(conn, note_path)?,
        );
        let best = pairs
            .into_iter()
            .reduce(|best, p| if p.0 < best.0 { p } else { best });
        if let Some((dist_sq, _, evidence_chunk_id)) = best {
            scored.push((dist_sq, note_path.clone(), evidence_chunk_id));
        }
    }
    // Nearest-first, ties by path: the served order, and — because z below is affine in
    // this squared distance — also descending z. One sort key serves the row order and
    // the strength band, so the two can never disagree.
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    // The statistic, computed AFTER stage 2 on the best-passage distances (GH #192) and
    // gating nothing (ADR-0014): the population is every scored shortlist note, which on
    // a personal-scale vault is every unlinked note there is. Above SHORTLIST_MIN it is
    // the anchor's centroid-nearest slice, a bias the dogfooding obligation owns. z is
    // oriented so nearer = higher, and travels to the output as the band's input.
    let mut population = None;
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
            population = Some(Population { mean, sd });
        }
    }

    Ok(Some(Field {
        coarse,
        shortlist,
        scored,
        population,
        excluded,
    }))
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
    if limit == 0 {
        return Ok(Vec::new());
    }
    let Some(field) = field(conn, anchor, limit, grade)? else {
        return Ok(Vec::new());
    };
    let population = field.population;
    Ok(field
        .scored
        .into_iter()
        .take(limit)
        .map(|(dist_sq, note_path, evidence_chunk_id)| CandidateNote {
            note_path,
            score: -(dist_sq.sqrt() as f64), // nearer = higher, matching Hit's -L2
            evidence_chunk_id,
            z: population.map(|p| p.z(dist_sq)),
        })
        .collect())
}

/// Where one note stands in an anchor's discovery field, which is the first thing an
/// explanation says: why it is a card, or why it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The candidate is the anchor.
    SameNote,
    /// The anchor has no stored vectors, so there is nothing to compare from.
    AnchorUnembedded,
    /// Within one link of the anchor: excluded from discovery as already known.
    Linked,
    /// The candidate has no stored vectors (or no centroid) yet.
    Unembedded,
    /// Its whole-note (centroid) rank fell past the stage-1 shortlist, so stage 2
    /// never scored it.
    NotShortlisted { shortlist: usize },
    /// Scored in stage 2: `rank` (1-based, the served order) among `of` scored notes.
    Ranked { rank: usize, of: usize },
}

/// [`PassagePair`] graded on the same yardstick as the cards.
#[derive(Debug, Clone, PartialEq)]
pub struct GradedPair {
    pub pair: PassagePair,
    /// The pair's z against the anchor's population, or `None` when ungraded. The
    /// winning pair's z is exactly the row's z: the same squared distance, read against
    /// the same population.
    pub z: Option<f64>,
}

/// Everything [`explain`] reads for one (anchor, candidate) pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub standing: Standing,
    /// The candidate's rank by whole-note centroid in stage 1 (1-based), when it entered
    /// stage 1 at all. Beside a best-passage rank it shows a buried gem: a note whose
    /// one section matches far better than the note as a whole.
    pub centroid_rank: Option<usize>,
    /// The candidate's z, when it was scored in a graded field.
    pub z: Option<f64>,
    /// Every scored note's z in served order (so descending): the field the band is
    /// relative to. Empty when ungraded.
    pub population: Vec<f64>,
    /// One pair per candidate passage, nearest first ([`passage_pairs`]), each graded.
    pub pairs: Vec<GradedPair>,
}

/// Explain one note's place in `anchor`'s discovery field, at the shortlist a
/// `limit`-sized list uses. The same computation [`candidates`] serves from, so a
/// served row's rank, z and winning pair are exactly the card's. Answers for any note,
/// not only a served one: a linked, unembedded or unshortlisted note says which, and its
/// passages are still compared when both sides have vectors. A pure read over stored
/// vectors, re-embedding nothing.
pub fn explain(
    conn: &Connection,
    anchor: &str,
    candidate: &str,
    limit: usize,
    grade: bool,
) -> Result<Explanation> {
    let mut out = Explanation {
        standing: Standing::SameNote,
        centroid_rank: None,
        z: None,
        population: Vec::new(),
        pairs: Vec::new(),
    };
    if anchor == candidate {
        return Ok(out);
    }
    let Some(field) = field(conn, anchor, limit, grade)? else {
        out.standing = Standing::AnchorUnembedded;
        return Ok(out);
    };
    let population = field.population;
    out.population = population
        .map(|p| field.scored.iter().map(|(d, _, _)| p.z(*d)).collect())
        .unwrap_or_default();
    out.pairs = passage_pairs_sq(conn, anchor, candidate, usize::MAX)?
        .into_iter()
        .map(|(dist_sq, pair)| GradedPair {
            pair,
            z: population.map(|p| p.z(dist_sq)),
        })
        .collect();
    out.centroid_rank = field
        .coarse
        .iter()
        .position(|n| n == candidate)
        .map(|i| i + 1);
    let scored_at = field.scored.iter().position(|(_, n, _)| n == candidate);
    out.standing = if field.excluded.contains(candidate) {
        Standing::Linked
    } else if let Some(i) = scored_at {
        out.z = population.map(|p| p.z(field.scored[i].0));
        Standing::Ranked {
            rank: i + 1,
            of: field.scored.len(),
        }
    } else if out.centroid_rank.is_some_and(|r| r > field.shortlist) {
        Standing::NotShortlisted {
            shortlist: field.shortlist,
        }
    } else {
        // Not scored and not past the shortlist: it has no centroid or no chunk
        // vectors yet.
        Standing::Unembedded
    };
    Ok(out)
}

/// One matched passage pair between an anchor and a candidate: the candidate's chunk,
/// the anchor's chunk nearest to it, and how near they are.
#[derive(Debug, Clone, PartialEq)]
pub struct PassagePair {
    /// The anchor's chunk nearest to `candidate_chunk_id`.
    pub anchor_chunk_id: i64,
    /// The candidate's chunk this pair is about.
    pub candidate_chunk_id: i64,
    /// Negated L2 distance between the two — higher is nearer, the same unit as
    /// [`CandidateNote::score`].
    pub score: f64,
}

/// The evidence behind one discovery row: up to `limit` passage pairs between `anchor`
/// and `candidate`, nearest first — **one pair per candidate chunk**, each matched to the
/// anchor chunk nearest to it. The first pair is exactly the pair [`candidates`] scored
/// the note on (same distances, same strictly-less tie rule, so the same
/// `evidence_chunk_id` at the same `score`): an explanation built from these is about
/// the passage the card showed, never a second ranking that could disagree with it.
///
/// [`candidates`] keeps only the winning candidate chunk because a list needs no more;
/// this is the read for the moment a human asks *why* one row is there, where the
/// anchor's half of the pair is the other half of the answer. A pure read over stored
/// vectors, re-embedding nothing. Empty when the vault has no embedding space, when
/// either note has no stored vectors, or when `limit` is 0.
pub fn passage_pairs(
    conn: &Connection,
    anchor: &str,
    candidate: &str,
    limit: usize,
) -> Result<Vec<PassagePair>> {
    Ok(passage_pairs_sq(conn, anchor, candidate, limit)?
        .into_iter()
        .map(|(_, pair)| pair)
        .collect())
}

/// [`passage_pairs`] with each pair's squared distance kept beside it, the unit the
/// population statistic reads.
fn passage_pairs_sq(
    conn: &Connection,
    anchor: &str,
    candidate: &str,
    limit: usize,
) -> Result<Vec<(f32, PassagePair)>> {
    if limit == 0 || !db::embedding_space_exists(conn)? {
        return Ok(Vec::new());
    }
    let (anchor_ids, anchor_vecs): (Vec<i64>, Vec<Vec<f32>>) =
        db::note_chunk_vectors(conn, anchor)?.into_iter().unzip();
    let mut pairs = nearest_pairs(
        &anchor_ids,
        &anchor_vecs,
        db::note_chunk_vectors(conn, candidate)?,
    );
    // Stable, so equal distances keep the candidate's chunk order — the earliest chunk
    // wins a tie, as it does in `candidates`.
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(pairs
        .into_iter()
        .take(limit)
        .map(|(dist_sq, anchor_chunk_id, candidate_chunk_id)| {
            (
                dist_sq,
                PassagePair {
                    anchor_chunk_id,
                    candidate_chunk_id,
                    score: -(dist_sq.sqrt() as f64),
                },
            )
        })
        .collect())
}

/// Discovery's max-sim kernel: for each candidate chunk, in order, the anchor chunk
/// nearest to it, as `(squared distance, anchor chunk, candidate chunk)`. Strictly-less
/// keeps the earliest anchor chunk on a tie. Stage 2 takes the minimum of this and the
/// explanation sorts it, so a card and its explanation read one computation. Squared L2
/// is the ranking key without the per-comparison `sqrt`; the readers take it once per
/// surfaced pair. Empty when either side has no vectors.
fn nearest_pairs(
    anchor_ids: &[i64],
    anchor_vecs: &[Vec<f32>],
    candidate: Vec<(i64, Vec<f32>)>,
) -> Vec<(f32, i64, i64)> {
    candidate
        .into_iter()
        .filter_map(|(candidate_chunk_id, v)| {
            anchor_ids
                .iter()
                .zip(anchor_vecs)
                .map(|(id, a)| (l2_sq(a, &v), *id))
                .reduce(|best, p| if p.0 < best.0 { p } else { best })
                .map(|(dist_sq, anchor_chunk_id)| (dist_sq, anchor_chunk_id, candidate_chunk_id))
        })
        .collect()
}
