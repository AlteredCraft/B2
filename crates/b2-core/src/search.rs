//! Hybrid retrieval (planning/index-engine.md §1, §5; build spec Flow ②).
//!
//! BM25 (over `chunks_fts`) and brute-force vector KNN (an in-process scan over the
//! stored `embeddings`) are retrieved in parallel and fused with **Reciprocal Rank
//! Fusion** (`Σ 1/(k+rank+1)`, k=60), borrowed wholesale from qmd. Results resolve
//! up from chunks to notes.
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
/// The raw query is sanitized into a safe FTS5 expression first (see
/// [`fts5_query`]): with real semantic search, callers pass natural-language
/// queries — apostrophes, punctuation, quotes — which are FTS5 *syntax* and would
/// otherwise raise a parse error. A query with no usable terms yields no hits (the
/// vector half still runs).
pub fn keyword_search(conn: &rusqlite::Connection, query: &str, limit: usize) -> Result<Vec<i64>> {
    let match_expr = fts5_query(query);
    if match_expr.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare("SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY rank LIMIT ?2")?;
    let rows = stmt.query_map(rusqlite::params![match_expr, limit as i64], |r| {
        r.get::<_, i64>(0)
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Turn arbitrary user text into a safe FTS5 `MATCH` expression: extract
/// alphanumeric terms, wrap each as a double-quoted string literal (so nothing in
/// the input is interpreted as FTS5 operators), and OR them for keyword recall —
/// the vector half supplies semantics, so the keyword half should be forgiving.
/// Returns an empty string when the query has no usable terms.
pub fn fts5_query(raw: &str) -> String {
    let terms: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        // A double-quoted FTS5 string is a literal term; internal quotes can't
        // occur here (split drops them), but double them defensively regardless.
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    terms.join(" OR ")
}

/// How wide a pool to pull from each signal before fusing (qmd keeps ~30).
fn pool_size(limit: usize) -> usize {
    (limit * 5).max(30)
}

/// Keyword-only search: BM25 over `chunks_fts` → top `limit`, resolved to notes —
/// the fallback that makes a **projected-but-unembedded** vault searchable
/// (projection-embedding-split.md §5): no query embedding, no model, no vectors.
/// Scores are the RRF of the single BM25 list, so they live on the same scale (and
/// sort the same way) as [`hybrid_search`]'s fused scores.
pub fn keyword_only_search(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    let bm25 = keyword_search(conn, query, pool_size(limit))?;
    tracing::debug!(
        target: "b2::search",
        bm25_hits = bm25.len(),
        pool = pool_size(limit),
        "keyword-only retrieval (no embedding space yet)"
    );
    let mut hits = Vec::new();
    for (chunk_id, score) in rrf_fuse(&[bm25], RRF_K).into_iter().take(limit) {
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

/// Hybrid search: BM25 ⊕ vector(query) → RRF → top `limit`, resolved to notes.
pub fn hybrid_search(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    let pool = pool_size(limit);
    let bm25 = keyword_search(conn, query, pool)?;
    let vector: Vec<i64> = db::vector_search(conn, &embedder.embed_query(query)?, pool)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    tracing::debug!(
        target: "b2::search",
        bm25_hits = bm25.len(),
        vector_hits = vector.len(),
        pool,
        "hybrid retrieval fusing BM25 ⊕ vector via RRF"
    );

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
/// either way is a candidate). Filtering is done by scanning the full ranked space
/// and keeping reachable notes — exact at vault scale (a full brute-force scan).
/// Chunk→note resolution is one bulk map load, not a per-ranked-row query: the walk
/// visits ranked chunks until `limit` reachable ones are found, which in the worst
/// case (a small neighborhood ranked deep) is the whole vault — the same N+1 shape
/// that once stalled `b2 similar` (#37).
pub fn graph_filtered_search(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    query: &str,
    anchor: &str,
    hops: usize,
    limit: usize,
) -> Result<Vec<Hit>> {
    let reachable = graph::reachable_within(conn, anchor, hops)?;
    let chunk_note = db::chunk_note_map(conn)?;

    let mut hits = Vec::new();
    for (chunk_id, distance) in db::vector_search_all(conn, &embedder.embed_query(query)?)? {
        let Some(note_b2id) = chunk_note.get(&chunk_id) else {
            continue;
        };
        if reachable.contains(note_b2id) {
            hits.push(Hit {
                chunk_id,
                note_b2id: note_b2id.clone(),
                score: -(distance as f64), // closer = higher
            });
            if hits.len() == limit {
                break;
            }
        }
    }
    Ok(hits)
}
