//! Hybrid retrieval (index-engine.md §1, §5; build spec Flow ②).
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
    /// The note the hit's chunk belongs to, by vault-relative path (L1).
    pub note_path: String,
    /// Higher is better (RRF score for hybrid; negated distance for vector-only).
    pub score: f64,
}

/// Reciprocal Rank Fusion of ranked id lists: `score(id) = Σ 1/(k + rank + 1)`
/// over the lists it appears in (rank 0-based). Returns ids with fused scores,
/// best first.
///
/// **Exact ties are structural here**, not freak events: integer ranks put every
/// fused score on a discrete lattice of reachable sums, so two candidates with
/// mirrored ranks — (1, 3) against (3, 1) — collide bit-identically. The eval
/// corpus hit one on its second run (docs/evals/runlog.md, 2026-08-10). Breaking
/// such a tie is a *policy choice about which signal to trust in a photo finish*:
/// the old key (ascending id — projection walk order) answered it with the
/// filesystem, which is deterministic and semantically arbitrary. The key now is
/// the candidate's rank in the **last** list — callers put the signal they trust
/// on ties last, which for [`hybrid_search`] is the dense/vector list: on the tie
/// the runlog decomposed, the semantic half named the labelled answer and BM25
/// named the wrong one (GH #156). A candidate absent from that list ranks below
/// any candidate present in it; id remains as the final key, so the sort stays
/// fully deterministic (single-list callers are unaffected — one list cannot
/// produce equal sums).
pub fn rrf_fuse(ranked_lists: &[Vec<i64>], k: usize) -> Vec<(i64, f64)> {
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for list in ranked_lists {
        for (rank, &id) in list.iter().enumerate() {
            *scores.entry(id).or_insert(0.0) += 1.0 / (k as f64 + rank as f64 + 1.0);
        }
    }
    let tiebreak: HashMap<i64, usize> = ranked_lists
        .last()
        .map(|list| list.iter().enumerate().map(|(r, &id)| (id, r)).collect())
        .unwrap_or_default();
    // Absent-from-the-tie-break-list sorts below present-at-any-rank.
    let rank_of = |id: i64| tiebreak.get(&id).copied().unwrap_or(usize::MAX);
    let mut out: Vec<(i64, f64)> = scores.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(rank_of(a.0).cmp(&rank_of(b.0)))
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
///
/// `pub(crate)` for two callers:
/// [`vault::note_candidate_pool`](crate::vault::note_candidate_pool) and
/// [`vault::chunk_candidate_pool`](crate::vault::chunk_candidate_pool) compose it
/// with each view's own headroom to state the *total* per-signal candidate depth
/// that view reaches — the number a measurement has to know to say whether a corpus
/// is big enough for fusion width to be observable (GH #141). It is this 5×
/// multiplication that makes a hit of façade headroom cost five candidates, and so
/// makes the two views' headroom a **ranking** choice, not a plumbing one (GH #142).
///
/// Saturating, like the façade's own widenings: `limit` is
/// user input (`b2 search --limit`, the desktop's page size) and the two widenings
/// compose, so a caller can reach a product that overflows `usize` — which debug
/// builds panic on and release builds *wrap*, turning an absurd ask into a silently
/// tiny pool. Saturation degrades honestly instead: an unreachable pool is just
/// every candidate there is.
pub(crate) fn pool_size(limit: usize) -> usize {
    limit.saturating_mul(5).max(30)
}

/// Keyword-only search: BM25 over `chunks_fts` → top `limit`, resolved to notes —
/// the fallback that makes a **projected-but-unembedded** vault searchable
/// (index-engine.md): no query embedding, no model, no vectors.
/// Scores are the RRF of the single BM25 list, so they live on the same scale (and
/// sort the same way) as [`hybrid_search`]'s fused scores.
pub fn keyword_only_search(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let pool = pool_size(limit);
    let bm25 = keyword_search(conn, query, pool)?;
    tracing::debug!(
        target: "b2::search",
        bm25_hits = bm25.len(),
        pool,
        "keyword-only retrieval (no embedding space yet)"
    );
    resolve_hits(conn, rrf_fuse(&[bm25], RRF_K), limit)
}

/// Hybrid search: BM25 ⊕ vector(query) → RRF → top `limit`, resolved to notes.
///
/// A `limit` of 0 returns before the query is embedded: with the real model that
/// call is the expensive part of a search, and asking for no results should cost
/// none. The same guard opens [`keyword_only_search`] and
/// [`graph_filtered_search`], so no retrieval path does work for an empty answer.
pub fn hybrid_search(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
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

    resolve_hits(conn, rrf_fuse(&[bm25, vector], RRF_K), limit)
}

/// Vector-only search: the dense half of [`hybrid_search`] alone — embed the
/// query, exact-scan the stored vectors, resolve the top `limit` to notes.
///
/// **An ablation instrument, not a product surface** (GH #158): the eval harness
/// scores it beside bm25-only and hybrid on every run, so fusion has a measured
/// single-signal baseline to answer to — the runlog's finding that RRF can demote
/// a chunk the dense signal ranked first is only a standing measurement if this
/// path stays callable. Scores are negated L2 distance (closer = higher), the
/// same convention as [`graph_filtered_search`] and **not** commensurable with
/// RRF scores. A vault with no embedding space yields no hits — there are no
/// vectors to scan, and pretending otherwise would rank on nothing.
pub fn vector_only_search(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let pool = pool_size(limit);
    let ranked: Vec<(i64, f64)> = db::vector_search(conn, &embedder.embed_query(query)?, pool)?
        .into_iter()
        .map(|(id, distance)| (id, -(distance as f64)))
        .collect();
    tracing::debug!(
        target: "b2::search",
        vector_hits = ranked.len(),
        pool,
        "vector-only retrieval (ablation instrument)"
    );
    resolve_hits(conn, ranked, limit)
}

/// The shared tail of [`keyword_only_search`], [`hybrid_search`], and
/// [`vector_only_search`]: resolve the ranked `(chunk_id, score)` list to
/// [`Hit`]s, best first, keeping the first `limit` **that still resolve** to a
/// note.
///
/// The walk is one pass over the whole ranking, stopping at `limit` — an
/// unresolved chunk is *skipped over*, never charged against the budget (GH
/// #137). A chunk can only fail to resolve when its row vanished between the
/// FTS/vector scan and this loop, i.e. a concurrent reindex replaced that note's
/// chunks mid-query — the posture C1 promises (readers are never refused while a
/// writer rebuilds; index-engine.md §3). Taking the top `limit` *first* would let
/// a dead chunk hold a slot a live, lower-ranked candidate could fill, so search
/// would quietly under-fill during that window. Steady state is unchanged: every
/// top-`limit` chunk resolves, so the same hits come back in the same order.
///
/// Per-hit resolution stays fine here — the loop still stops at `limit` (contrast
/// [`graph_filtered_search`], which walks the full ranked space by design and so
/// needs the bulk map).
fn resolve_hits(
    conn: &rusqlite::Connection,
    fused: Vec<(i64, f64)>,
    limit: usize,
) -> Result<Vec<Hit>> {
    let mut hits = Vec::new();
    for (chunk_id, score) in fused {
        // Tested *before* the push, not after: an after-the-push test can never
        // fire at a limit the loop starts below (`limit == 0`), which would turn
        // "give me nothing" into "give me the whole ranking".
        if hits.len() == limit {
            break;
        }
        if let Some(note_path) = db::note_for_chunk(conn, chunk_id)? {
            hits.push(Hit {
                chunk_id,
                note_path,
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
    if limit == 0 {
        return Ok(Vec::new());
    }
    let reachable = graph::reachable_within(conn, anchor, hops)?;
    let chunk_note = db::chunk_note_map(conn)?;

    let mut hits = Vec::new();
    for (chunk_id, distance) in db::vector_search_all(conn, &embedder.embed_query(query)?)? {
        // Before the push, not after: an after-the-push test can never fire at a
        // limit the loop starts below, which is what the entry guard above covers.
        if hits.len() == limit {
            break;
        }
        let Some(note_path) = chunk_note.get(&chunk_id) else {
            continue;
        };
        if reachable.contains(note_path) {
            hits.push(Hit {
                chunk_id,
                note_path: note_path.clone(),
                score: -(distance as f64), // closer = higher
            });
        }
    }
    Ok(hits)
}
