//! Connection discovery, the engine behind `b2 similar`: notes semantically near the anchor
//! but not already linked, for the human to judge (ADR-0009). Recall-oriented: the ranked
//! list is always served and no statistic gates membership (ADR-0014); z only grades.
//!
//! Two stages (#38): a coarse O(notes) centroid scan, minus the anchor and its 1-hop
//! neighbours, keeps a wide shortlist; then exact chunk max-sim scores it, keeping the
//! winning chunk as evidence. Re-embeds nothing: the anchor is its stored vectors, never an
//! `embed_query` (bge's query prefix is the wrong side).

use crate::db;
use crate::embed::{centroid_of, l2_sq, unpack_f32_into};
use crate::error::Result;
use crate::graph;
use rusqlite::Connection;

/// The exclusion radius. 1, so two-hop (triadic-closure) candidates stay in the pool.
const EXCLUDE_HOPS: usize = 1;

/// Floor on the stage-1 shortlist, generous so the coarse stage never loses a nearby note.
/// At or below this many notes the result equals a whole-space scan.
const SHORTLIST_MIN: usize = 200;

/// Stage-1 shortlist per requested result, floored at [`SHORTLIST_MIN`]. Wide, because a
/// centroid can rank below its note's best chunk.
const SHORTLIST_PER_RESULT: usize = 20;

/// Below this pool size candidates are served ungraded: a z over a handful is noise.
/// Affects banding only, never membership (ADR-0014).
const STATS_MIN_POPULATION: usize = 12;

/// One discovery candidate, ranked by best-passage `score`.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateNote {
    /// The candidate's identity (L1).
    pub note_path: String,
    /// Best chunk-pair similarity, as negated L2 distance (like [`Hit`](crate::search::Hit)).
    pub score: f64,
    /// The candidate's chunk that achieved `score`, shown as evidence.
    pub evidence_chunk_id: i64,
    /// Best-passage z against the scored shortlist: the strength band's input, gating
    /// nothing (ADR-0014). Monotonic in `score`, so band and row order agree. `None` when
    /// ungraded.
    pub z: Option<f64>,
}

/// Mean and spread of the scored population's best-passage squared distances. Grades,
/// never gates (ADR-0014); explain grades passage pairs on the same yardstick.
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

/// One anchor's whole discovery field. [`candidates`] and [`explain`] both read it, so a
/// card and its explanation can't disagree.
struct Field {
    /// Every unlinked note with a centroid, nearest first, before the shortlist cut.
    coarse: Vec<String>,
    /// How many of `coarse` stage 2 was allowed to score.
    shortlist: usize,
    /// Stage 2: `(squared distance, note, evidence chunk)`, nearest first.
    scored: Vec<(f32, String, i64)>,
    /// `None` when ungraded.
    population: Option<Population>,
    /// The anchor and its 1-hop neighbours.
    excluded: std::collections::HashSet<String>,
}

/// Compute `anchor`'s field for a `limit`-sized list. `None` with no embedding space or
/// no anchor vectors.
fn field(conn: &Connection, anchor: &str, limit: usize, grade: bool) -> Result<Option<Field>> {
    if !db::embedding_space_exists(conn)? {
        return Ok(None);
    }
    // Stored vectors only (index-engine.md §3). The centroid is computed here, not read
    // back, so an anchor mid-embed still discovers from what it has.
    let (anchor_ids, anchor_vecs): (Vec<i64>, Vec<Vec<f32>>) =
        db::note_chunk_vectors(conn, anchor)?.into_iter().unzip();
    let Some(anchor_centroid) = centroid_of(&anchor_vecs) else {
        return Ok(None);
    };

    // The graph's only use here: subtract what's already linked.
    let excluded = graph::reachable_within(conn, anchor, EXCLUDE_HOPS)?;

    // Stage 1: coarse scan over centroids, skipping excluded notes so they take no slot.
    let mut coarse: Vec<(f32, String)> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    db::for_each_note_centroid(conn, |note, blob| {
        if excluded.contains(note) {
            return;
        }
        unpack_f32_into(blob, &mut scratch);
        coarse.push((l2_sq(&anchor_centroid, &scratch), note.to_string()));
    })?;
    coarse.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let coarse: Vec<String> = coarse.into_iter().map(|(_, note)| note).collect();

    // The shortlist is a recall device, never a quality gate (GH #192, ADR-0014).
    let shortlist = limit
        .saturating_mul(SHORTLIST_PER_RESULT)
        .max(SHORTLIST_MIN);

    // Stage 2: exact max-sim via [`nearest_pairs`], the kernel explain reads too. The
    // earliest chunk wins a tie; a note with no vectors yet drops out.
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
    // Nearest-first, ties by path. z is affine in this distance, so this is also
    // descending z.
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    // The statistic, after stage 2 on best-passage distances (GH #192). The population is
    // the scored shortlist: every unlinked note on a personal-scale vault, a
    // centroid-nearest slice above SHORTLIST_MIN.
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

/// Up to `limit` candidates for `anchor`, nearest best passage first (ties by path). The
/// list is always served; `limit` is only a cap (ADR-0014). `grade` adds z to each row
/// without changing which rows exist; the façade passes `false` for the fake embedder.
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
            score: -(dist_sq.sqrt() as f64),
            evidence_chunk_id,
            z: population.map(|p| p.z(dist_sq)),
        })
        .collect())
}

/// Where one note stands in an anchor's discovery field: why it is a card, or why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// The candidate is the anchor.
    SameNote,
    /// The anchor has no stored vectors, so there is nothing to compare from.
    AnchorUnembedded,
    /// Within one link of the anchor.
    Linked,
    /// The candidate has no stored vectors (or no centroid) yet.
    Unembedded,
    /// Its centroid rank fell past the stage-1 shortlist.
    NotShortlisted { shortlist: usize },
    /// Scored in stage 2: `rank` (1-based, the served order) among `of` scored notes.
    Ranked { rank: usize, of: usize },
}

/// [`PassagePair`] graded on the same yardstick as the cards.
#[derive(Debug, Clone, PartialEq)]
pub struct GradedPair {
    pub pair: PassagePair,
    /// The pair's z, or `None` when ungraded. The winning pair's z equals the row's.
    pub z: Option<f64>,
}

/// Everything [`explain`] reads for one (anchor, candidate) pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub standing: Standing,
    /// The candidate's 1-based centroid rank in stage 1. Beside its passage rank it shows
    /// a note whose one section matches far better than the whole.
    pub centroid_rank: Option<usize>,
    /// The candidate's z, when it was scored in a graded field.
    pub z: Option<f64>,
    /// Every scored note's z, descending. Empty when ungraded.
    pub population: Vec<f64>,
    /// One pair per candidate passage, nearest first ([`passage_pairs`]), each graded.
    pub pairs: Vec<GradedPair>,
}

/// Explain one note's place in `anchor`'s discovery field, from the same computation
/// [`candidates`] serves. Answers for any note, served or not. Re-embeds nothing.
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
        // Not scored and not past the shortlist: no centroid or vectors yet.
        Standing::Unembedded
    };
    Ok(out)
}

/// A candidate's chunk, the anchor chunk nearest to it, and how near.
#[derive(Debug, Clone, PartialEq)]
pub struct PassagePair {
    pub anchor_chunk_id: i64,
    pub candidate_chunk_id: i64,
    /// Negated L2 distance, as in [`CandidateNote::score`].
    pub score: f64,
}

/// Up to `limit` passage pairs between `anchor` and `candidate`, nearest first, one per
/// candidate chunk. The first is exactly the pair [`candidates`] scored the note on.
/// Re-embeds nothing.
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

/// [`passage_pairs`] with each squared distance kept, for grading.
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
    // Stable, so the earliest chunk wins a tie, as in `candidates`.
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

/// Discovery's max-sim kernel: per candidate chunk, the nearest anchor chunk, as
/// `(squared distance, anchor chunk, candidate chunk)`. Strictly-less keeps the earliest
/// anchor chunk on a tie.
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
