//! Connection-discovery candidate generation (planning/tasks.md ①, resolved
//! 2026-07-01; vision-and-scope "Connection discovery v1"). This is the first stage
//! of the three-stage pipeline — **candidate generation → [`Relator`] → review
//! loop** — and the only one that reads the graph.
//!
//! A candidate is a note **semantically near the anchor but not already connected**:
//! the *complement* of the graph, not the intersection. (The intersection —
//! semantic-nearest chunks *within* k hops — is [`crate::search::graph_filtered_search`],
//! a scoped-traversal primitive, the wrong tool here.) Generation is deliberately
//! **permissive**: it over-produces, and the [`Relator`](crate::relate::Relator) plus
//! the human are the precision gate ([`crate::suggest`]).
//!
//! Mechanics (tasks.md ①): for each of the anchor's **stored** chunk vectors, KNN the
//! vault, then score every other note by its **best** chunk-pair similarity
//! (max-sim), and finally subtract the anchor and its **direct (1-hop)** neighbors.
//! It is vector-only and **re-embeds nothing** — discovery is passage↔passage, so the
//! anchor is represented by the vectors already in `chunks_vec`, never by an
//! `embed_query` of its text (bge's asymmetric query prefix is the wrong side).
//! Graph distance beyond the 1-hop exclusion is **not** a ranking signal — graph-
//! distant "bridge" candidates ride along unboosted; weighting distance (closure vs.
//! serendipity) is a deferred, eval-gated experiment (tasks.md backlog).

use crate::db;
use crate::error::Result;
use crate::graph;
use rusqlite::Connection;
use std::collections::HashMap;

/// The exclusion radius: a candidate must not already be *directly* linked to the
/// anchor. Fixed at 1 by decision (tasks.md ①) so triadic-closure candidates — a note
/// two hops away, transitively related but with no direct edge — stay in the pool.
const EXCLUDE_HOPS: usize = 1;

/// One discovery candidate: a note near the anchor and not already connected, ranked
/// by `score`. Owned so the caller can build the borrowed
/// [`Candidate`](crate::relate::Candidate) view at relate-time without threading a
/// lifetime through generation.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateNote {
    /// The candidate note's `b2id`.
    pub note_b2id: String,
    /// Best chunk-pair similarity across the anchor's chunks × this note's chunks —
    /// higher is nearer (negated `sqlite-vec` distance, matching
    /// [`Hit`](crate::search::Hit)).
    pub score: f64,
    /// The candidate's chunk that achieved `score` — the evidence a
    /// [`Relator`](crate::relate::Relator) reads and the suggestion's provenance
    /// points at.
    pub evidence_chunk_id: i64,
}

/// Generate up to `limit` connection-discovery candidates for `anchor`, best score
/// first (ties broken by `note_b2id` for determinism).
///
/// Returns empty when the vault has no embedding space yet, when the anchor has no
/// chunks (unknown or empty note), or when `limit` is 0 — there is nothing to search
/// from. Excludes the anchor itself and its direct neighbors; everything else near in
/// vector space is a candidate.
pub fn candidates(conn: &Connection, anchor: &str, limit: usize) -> Result<Vec<CandidateNote>> {
    if limit == 0 || !db::embedding_space_exists(conn)? {
        return Ok(Vec::new());
    }
    let anchor_chunks = db::chunks_for_note(conn, anchor)?;
    if anchor_chunks.is_empty() {
        return Ok(Vec::new());
    }

    // The only use of the graph in generation: subtract what's already linked — the
    // anchor and everything within 1 hop (self + direct neighbors).
    let exclude = graph::reachable_within(conn, anchor, EXCLUDE_HOPS)?;

    // A full KNN pool per anchor chunk keeps max-sim exact at vault scale — the same
    // brute-force pool graph_filtered_search uses. The scale lever (a note-partition
    // column on chunks_vec for filtered KNN) is deferred (build spec §1.2 / §4).
    let pool = db::chunk_count(conn)?.max(1) as usize;

    // note_b2id → (best score so far, the chunk of that note which achieved it).
    let mut best: HashMap<String, (f64, i64)> = HashMap::new();
    for anchor_chunk in anchor_chunks {
        let Some(vector) = db::chunk_vector(conn, anchor_chunk)? else {
            continue; // a chunk with no stored vector (shouldn't occur post-embed)
        };
        for (hit_chunk, distance) in db::vector_search(conn, &vector, pool)? {
            let Some(note) = db::note_for_chunk(conn, hit_chunk)? else {
                continue;
            };
            if exclude.contains(&note) {
                continue; // the anchor or a direct neighbor — already connected
            }
            let score = -(distance as f64); // nearer = higher, matching Hit
            best.entry(note)
                .and_modify(|cur| {
                    if score > cur.0 {
                        *cur = (score, hit_chunk);
                    }
                })
                .or_insert((score, hit_chunk));
        }
    }

    let mut out: Vec<CandidateNote> = best
        .into_iter()
        .map(|(note_b2id, (score, evidence_chunk_id))| CandidateNote {
            note_b2id,
            score,
            evidence_chunk_id,
        })
        .collect();
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
