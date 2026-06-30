//! Hybrid retrieval (planning/index-engine.md §1, §5; build spec Flow ②).
//!
//! BM25 (over `chunks_fts`) and brute-force vector KNN (over `chunks_vec`) are
//! retrieved in parallel and fused with **Reciprocal Rank Fusion** (`Σ 1/(k+rank+1)`,
//! k=60), borrowed wholesale from qmd. Results resolve up from chunks to notes.
//!
//! The graph-filtered variant is B2's reason to exist: "nearest chunks whose note
//! is within k typed hops of note X" — the vector⨝graph join (index-engine.md §3)
//! that connection-discovery candidate generation runs on.
//!
//! Deferred, behind clean seams (changes *ordering*, not the store or candidate
//! set): a cross-encoder **reranker** over the fused top-N (the fast-follow, §5)
//! and query **expansion** (off by default). Both would cache in `llm_cache`,
//! which lands with the reranker.

use crate::db;
use crate::embed::Embedder;
use crate::error::Result;
use crate::graph;
use std::collections::HashMap;

/// The RRF constant, k=60 (index-engine.md §1).
pub const RRF_K: usize = 60;

/// A fused search result, resolved to the note it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub chunk_id: i64,
    pub note_b2id: String,
    /// Higher is better (RRF score for hybrid; negated distance for vector-only).
    pub score: f64,
}

/// Reciprocal Rank Fusion of ranked id lists: `score(id) = Σ 1/(k + rank + 1)`
/// over the lists it appears in (rank 0-based). Returns ids with fused scores,
/// best first; ties broken by id for determinism.
pub fn rrf_fuse(ranked_lists: &[Vec<i64>], k: usize) -> Vec<(i64, f64)> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for list in ranked_lists {
        for (rank, &id) in list.iter().enumerate() {
            *scores.entry(id).or_insert(0.0) += 1.0 / (k as f64 + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(i64, f64)> = scores.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    out
}

/// BM25 keyword search over chunk text → chunk ids, best first.
///
/// The query is passed to FTS5 `MATCH` as written; robust query parsing/escaping
/// is a later concern (callers currently use plain terms).
pub fn keyword_search(conn: &rusqlite::Connection, query: &str, limit: usize) -> Result<Vec<i64>> {
    let mut stmt = conn
        .prepare("SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY rank LIMIT ?2")?;
    let rows = stmt.query_map(rusqlite::params![query, limit as i64], |r| {
        r.get::<_, i64>(0)
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// How wide a pool to pull from each signal before fusing (qmd keeps ~30).
fn pool_size(limit: usize) -> usize {
    (limit * 5).max(30)
}

/// Hybrid search: BM25 ⊕ vector(query) → RRF → top `limit`, resolved to notes.
pub fn hybrid_search(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    let pool = pool_size(limit);
    let bm25 = keyword_search(conn, query, pool)?;
    let vector: Vec<i64> = db::vector_search(conn, &embedder.embed(query), pool)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();

    let mut hits = Vec::new();
    for (chunk_id, score) in rrf_fuse(&[bm25, vector], RRF_K).into_iter().take(limit) {
        if let Some(note_b2id) = db::note_for_chunk(conn, chunk_id)? {
            hits.push(Hit {
                chunk_id,
                note_b2id,
                score,
            });
        }
    }
    Ok(hits)
}

/// Graph-filtered vector search: the `limit` nearest chunks whose note is within
/// `hops` typed hops of `anchor` (the vector⨝graph discovery join).
///
/// Reachability is undirected over `active` edges (a note related to the anchor
/// either way is a candidate). Filtering is done by pulling a full KNN pool and
/// keeping reachable notes — exact at vault scale; the precise scale lever is a
/// partition column on `note_b2id` for filtered KNN (build spec §1.2 / §4).
pub fn graph_filtered_search(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    query: &str,
    anchor: &str,
    hops: usize,
    limit: usize,
) -> Result<Vec<Hit>> {
    let reachable = graph::reachable_within(conn, anchor, hops)?;
    let pool = db::chunk_count(conn)?.max(1) as usize;

    let mut hits = Vec::new();
    for (chunk_id, distance) in db::vector_search(conn, &embedder.embed(query), pool)? {
        let Some(note_b2id) = db::note_for_chunk(conn, chunk_id)? else {
            continue;
        };
        if reachable.contains(&note_b2id) {
            hits.push(Hit {
                chunk_id,
                note_b2id,
                score: -(distance as f64), // closer = higher
            });
            if hits.len() == limit {
                break;
            }
        }
    }
    Ok(hits)
}
