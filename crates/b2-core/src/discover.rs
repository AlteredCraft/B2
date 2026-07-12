//! Connection-discovery candidate generation — the engine behind **`b2 similar`**
//! (planning/tasks.md ①, resolved 2026-07-01; vision-and-scope "Connection discovery
//! v1"). It surfaces the notes to *consider* linking; the human is the precision gate
//! and `b2 link` commits one. It is the only discovery stage, and the only one that
//! reads the graph.
//!
//! A candidate is a note **semantically near the anchor but not already connected**:
//! the *complement* of the graph, not the intersection. (The intersection —
//! semantic-nearest chunks *within* k hops — is [`crate::search::graph_filtered_search`],
//! a scoped-traversal primitive, the wrong tool here.) Generation is deliberately
//! **permissive**: it over-produces, and the human decides which are worth a link.
//!
//! Mechanics (tasks.md ①): score every stored chunk by its similarity to the anchor's
//! **nearest** stored chunk vector, keep each note's **best** such chunk (max-sim), and
//! subtract the anchor and its **direct (1-hop)** neighbors. This is one whole-space
//! pass over `chunks_vec` (`db::for_each_stored_vector`), computing squared-L2 in
//! process — *not* one SQL KNN scan per anchor chunk, which reread and re-sorted the
//! entire vector space once per anchor chunk (O(anchor × vault), the old `b2 similar`
//! stall). It is vector-only and **re-embeds nothing** — discovery is passage↔passage,
//! so the anchor is represented by the vectors already in `chunks_vec`, never by an
//! `embed_query` of its text (bge's asymmetric query prefix is the wrong side).
//! Graph distance beyond the 1-hop exclusion is **not** a ranking signal — graph-
//! distant "bridge" candidates ride along unboosted; weighting distance (closure vs.
//! serendipity) is a deferred, eval-gated experiment (tasks.md backlog).

use crate::db;
use crate::embed::{l2_sq, unpack_f32};
use crate::error::Result;
use crate::graph;
use rusqlite::Connection;
use std::collections::HashMap;

/// The exclusion radius: a candidate must not already be *directly* linked to the
/// anchor. Fixed at 1 by decision (tasks.md ①) so triadic-closure candidates — a note
/// two hops away, transitively related but with no direct edge — stay in the pool.
const EXCLUDE_HOPS: usize = 1;

/// One discovery candidate: a note near the anchor and not already connected, ranked
/// by `score`. Owned, so the façade can resolve it to a [`SimilarView`](crate::vault::SimilarView)
/// for `b2 similar` without threading a lifetime through generation.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateNote {
    /// The candidate note's `b2id`.
    pub note_b2id: String,
    /// Best chunk-pair similarity across the anchor's chunks × this note's chunks —
    /// higher is nearer (negated `sqlite-vec` distance, matching
    /// [`Hit`](crate::search::Hit)).
    pub score: f64,
    /// The candidate's chunk that achieved `score` — the passage that made this note
    /// similar, surfaced by `b2 similar` as the evidence for *why* it appeared.
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

    // The anchor's own stored vectors (re-embeds nothing — tasks.md ①), unpacked once
    // so the hot scan below reuses them across every stored chunk. A chunk with no
    // stored vector (shouldn't occur post-embed) is skipped; none at all ⇒ nothing to
    // search from.
    let mut anchor_vecs: Vec<Vec<f32>> = Vec::new();
    for chunk in anchor_chunks {
        if let Some(v) = db::chunk_vector(conn, chunk)? {
            anchor_vecs.push(v);
        }
    }
    if anchor_vecs.is_empty() {
        return Ok(Vec::new());
    }

    // Resolve chunk→note once, up front — the scan visits every vault chunk, so a
    // per-hit note_for_chunk query would be an O(vault) round-trip storm.
    let chunk_note = db::chunk_note_map(conn)?;

    // One whole-space pass (not one KNN scan per anchor chunk): score every stored
    // chunk by its nearest anchor vector and keep, per note, its best chunk — exact
    // max-sim, the brute force index-engine.md §4 specs as comfortable at vault scale.
    // `best`: note_b2id → (smallest squared L2 seen, the chunk that achieved it).
    // Squared L2 is the same ranking key as `vec_distance_l2` without the per-hit
    // `sqrt` (monotonic); the `sqrt` is applied once per candidate below.
    let mut best: HashMap<String, (f32, i64)> = HashMap::new();
    db::for_each_stored_vector(conn, |chunk_id, blob| {
        let Some(note) = chunk_note.get(&chunk_id) else {
            return;
        };
        if exclude.contains(note.as_str()) {
            return; // the anchor or a direct neighbor — already connected
        }
        // Decode this stored chunk once, then take its nearest anchor vector (min over
        // the anchor's chunks) — max-sim's inner min.
        let v = unpack_f32(blob);
        let Some(dist_sq) = anchor_vecs
            .iter()
            .map(|a| l2_sq(a, &v))
            .min_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
        else {
            return;
        };
        // Look up before inserting so the note id is cloned only on first sighting
        // (≤ once per distinct note), not on every one of the vault-many hits.
        match best.get_mut(note) {
            Some(cur) if dist_sq < cur.0 => *cur = (dist_sq, chunk_id),
            Some(_) => {}
            None => {
                best.insert(note.clone(), (dist_sq, chunk_id));
            }
        }
    })?;

    let mut out: Vec<CandidateNote> = best
        .into_iter()
        .map(|(note_b2id, (dist_sq, evidence_chunk_id))| CandidateNote {
            note_b2id,
            score: -(dist_sq.sqrt() as f64), // nearer = higher, matching Hit's -L2
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
