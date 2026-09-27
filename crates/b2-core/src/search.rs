//! Hybrid retrieval (flow ②): BM25 over `chunks_fts` and brute-force vector KNN,
//! retrieved in parallel and fused by RRF, then resolved up from chunks to notes
//! (ADR-0008). A served result is a claim of evidence, which is what
//! [`lexical_evidence`] and [`EvidenceBar`] are for (ADR-0015).

use crate::db;
use crate::embed::Embedder;
use crate::error::Result;
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
    /// The absolute readings RRF discards (invariants.md D2).
    pub provenance: HitProvenance,
}

/// Per-hit provenance carried beside the fused order, never folded into it (ADR-0015,
/// GH #201). RRF's ranks can't tell a real rank 1 from one in a list that should have been
/// empty; these absolute signals can.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HitProvenance {
    /// 0-based rank in the BM25 list; `None` = a dense-only hit.
    pub bm25_rank: Option<usize>,
    /// 0-based rank in the dense list; `None` = not ranked, or no vectors yet.
    pub vector_rank: Option<usize>,
    /// L2 distance to the query vector (`cosine = 1 - d²/2` for unit vectors).
    pub distance: Option<f32>,
}

/// The per-chunk provenance map for a fusion over `(bm25, vector)`.
fn provenance_of(bm25: &[i64], vector: &[(i64, f32)]) -> HashMap<i64, HitProvenance> {
    let mut map: HashMap<i64, HitProvenance> = HashMap::new();
    for (rank, &id) in bm25.iter().enumerate() {
        map.entry(id).or_default().bm25_rank = Some(rank);
    }
    for (rank, &(id, distance)) in vector.iter().enumerate() {
        let entry = map.entry(id).or_default();
        entry.vector_rank = Some(rank);
        entry.distance = Some(distance);
    }
    map
}

/// Reciprocal Rank Fusion: `score(id) = Σ 1/(k + rank + 1)` (rank 0-based), best first.
///
/// Exact ties are common (mirrored ranks collide, GH #156), so ties break on rank in the
/// last list, where callers put the signal they trust (dense, for [`hybrid_search`]); then
/// on id.
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
        b.1.total_cmp(&a.1)
            .then(rank_of(a.0).cmp(&rank_of(b.0)))
            .then(a.0.cmp(&b.0))
    });
    out
}

/// BM25 keyword search over chunk text -> chunk ids, best first. The query is sanitized
/// first ([`fts5_query`]), since punctuation is FTS5 syntax.
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

/// Arbitrary text as a safe FTS5 `MATCH` expression: alphanumeric terms, each quoted,
/// OR-ed for recall. Empty when there are no usable terms.
pub fn fts5_query(raw: &str) -> String {
    let terms: Vec<String> = query_terms(raw).iter().map(|t| quoted(t)).collect();
    terms.join(" OR ")
}

/// The terms [`fts5_query`] ORs, unquoted. Shared so the evidence reading judges exactly
/// the terms searched for.
pub fn query_terms(raw: &str) -> Vec<String> {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// One term as a quoted FTS5 string literal. Quotes can't occur but are doubled anyway.
fn quoted(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

/// One query term and its document frequency.
#[derive(Debug, Clone, PartialEq)]
pub struct TermEvidence {
    /// The term as written; FTS5 stems it, so `df` is the stemmed reading.
    pub term: String,
    /// Chunks matching this term. `0` = the vault has never seen the word.
    pub df: usize,
}

/// The lexical half's absolute reading for one query (ADR-0015). Matching at all is not
/// evidence: OR-ed terms match on function words (GH #201). Each term is weighted by IDF
/// instead, so common words count for almost nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct LexicalEvidence {
    /// Chunks in the index.
    pub chunk_total: usize,
    /// Every distinct term the query contributed, in first-appearance order.
    pub terms: Vec<TermEvidence>,
}

impl LexicalEvidence {
    /// A term's IDF in this vault: `ln((chunks + 1) / (df + 1))`. The whole stopword policy,
    /// continuous by design: a df ceiling failed on topically concentrated vaults (GH #201).
    /// The `+1`s keep `df = 0` and an empty vault finite.
    pub fn idf(&self, df: usize) -> f64 {
        ((self.chunk_total as f64 + 1.0) / (df as f64 + 1.0)).ln()
    }

    /// The share of the query's total IDF carried by terms the vault holds (`df >= 1`).
    /// `None` when no term has any weight: no reading, not zero coverage.
    pub fn term_coverage(&self) -> Option<f64> {
        let total: f64 = self.terms.iter().map(|t| self.idf(t.df)).sum();
        if total <= f64::EPSILON {
            return None;
        }
        // Not `.sum()`: its `f64` identity is `-0.0`, which prints as `-0.00`.
        let present: f64 = self
            .terms
            .iter()
            .filter(|t| t.df >= 1)
            .map(|t| self.idf(t.df))
            .fold(0.0, |a, b| a + b);
        Some(present / total)
    }

    /// Whether the query has a lexical anchor (ADR-0015): coverage, not mere presence.
    pub fn anchored(&self, min_term_coverage: f64) -> bool {
        self.term_coverage().is_some_and(|c| c >= min_term_coverage)
    }
}

/// Read the lexical evidence for `query`: one `count(*)` per distinct term. Distinct to
/// FTS5 ([`fts_tokens`]), not `str::eq`, so `Memory`/`memories` count once (PR #205).
pub fn lexical_evidence(conn: &rusqlite::Connection, query: &str) -> Result<LexicalEvidence> {
    let chunk_total: usize = conn
        .query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))?
        .try_into()
        .unwrap_or(0);
    let raw = query_terms(query);
    let tokens = fts_tokens(conn, &raw)?;
    let mut terms: Vec<TermEvidence> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (term, token) in raw.into_iter().zip(tokens) {
        // Count a repeated token once, showing its first spelling.
        if seen.contains(&token) {
            continue;
        }
        let df: i64 = conn.query_row(
            "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
            [&quoted(&term)],
            |r| r.get(0),
        )?;
        seen.push(token);
        terms.push(TermEvidence {
            term,
            df: usize::try_from(df).unwrap_or(0),
        });
    }
    Ok(LexicalEvidence { chunk_total, terms })
}

/// Fold each term onto the token `chunks_fts` would index it as. FTS5 has no tokenize
/// function, so this uses a temp-schema table with the index's tokenizer (read, not
/// assumed: GH #157 swaps it), one row per term, read via `fts5vocab`. Temp only, so a
/// reader stays a reader (C1). A term with no tokens keeps its own spelling.
fn fts_tokens(conn: &rusqlite::Connection, terms: &[String]) -> Result<Vec<String>> {
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    // Named for its tokenizer, so a mid-session swap doesn't reuse a stale table.
    let tokenizer = db::index_tokenizer(conn)?;
    let table = format!("b2_qtok_{}", tokenizer.sql().replace(' ', "_"));
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.{table} USING fts5(
           t, tokenize = '{}'
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS temp.{table}_v USING fts5vocab(
           temp, {table}, instance
         );
         DELETE FROM temp.{table};",
        tokenizer.sql()
    ))?;
    {
        let mut insert = conn.prepare(&format!(
            "INSERT INTO temp.{table}(rowid, t) VALUES (?1, ?2)"
        ))?;
        for (i, term) in terms.iter().enumerate() {
            insert.execute(rusqlite::params![i as i64, term])?;
        }
    }
    // A row with no token keeps its spelling, staying distinct from every other term.
    let mut out: Vec<String> = terms.to_vec();
    let mut stmt = conn.prepare(&format!(
        "SELECT doc, term FROM temp.{table}_v ORDER BY doc, offset"
    ))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut tokenized: Vec<bool> = vec![false; terms.len()];
    for row in rows {
        let (doc, token) = row?;
        let Ok(i) = usize::try_from(doc) else {
            continue;
        };
        let Some(slot) = out.get_mut(i) else { continue };
        if tokenized[i] {
            slot.push('\u{1f}'); // a term that split: join its tokens, in order
            slot.push_str(&token);
        } else {
            *slot = token;
            tokenized[i] = true;
        }
    }
    Ok(out)
}

/// The per-model evidence bar a query is judged against (ADR-0015). Keyed to the model
/// (ADR-0007) and measured in the eval harness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceBar {
    /// Minimum [`LexicalEvidence::term_coverage`] for a lexical anchor.
    pub min_term_coverage: f64,
    /// Cosine the dense top-1 must reach to stand as evidence without a lexical anchor.
    pub min_cos: f64,
}

impl EvidenceBar {
    /// The calibrated bar for `model_id`. `None` means no verdict, never a default one.
    /// The device suffix (`…@metal`) is stripped: float noise is not a distributional
    /// change, an assumption `make eval-metal` re-checks.
    pub fn for_model(model_id: &str) -> Option<Self> {
        let base = model_id.split_once('@').map_or(model_id, |(id, _)| id);
        match base {
            "BAAI/bge-base-en-v1.5" => Some(BGE_BASE_EVIDENCE_BAR),
            _ => None,
        }
    }
}

/// The evidence bar for `BAAI/bge-base-en-v1.5` (ADR-0015), re-derived on every `make eval`
/// (no numbers here, GH #187). The two values form a joint band: tune them together, never
/// one at a time. Both lean toward serving, since cutting a real result is the worse error.
pub const BGE_BASE_EVIDENCE_BAR: EvidenceBar = EvidenceBar {
    min_term_coverage: 0.20,
    min_cos: 0.54,
};

/// The query-level evidence behind a search (ADR-0015), from a [`Retrieval`]'s `best_cos`
/// and a [`lexical_evidence`] read.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryEvidence {
    pub lexical: LexicalEvidence,
    /// See [`Retrieval::best_cos`].
    pub best_cos: Option<f64>,
}

impl QueryEvidence {
    /// Whether the vault holds positive evidence: a lexical anchor or semantic proximity
    /// clearing `bar` (ADR-0015). Two independent signals, since one can't tell "nothing
    /// matches" from "everything matches" (GH #196).
    pub fn vouched(&self, bar: EvidenceBar) -> bool {
        self.lexical.anchored(bar.min_term_coverage)
            || self.best_cos.is_some_and(|c| c >= bar.min_cos)
    }
}

/// A retrieval's fused order plus the dense half's best cosine, which comes free. The
/// lexical evidence costs a query per term, so [`lexical_evidence`] reads it on demand.
#[derive(Debug, Clone, PartialEq)]
pub struct Retrieval {
    pub hits: Vec<Hit>,
    /// Best cosine between the query and any chunk vector. `None` on an unembedded vault.
    pub best_cos: Option<f64>,
}

impl Retrieval {
    fn empty() -> Self {
        Self {
            hits: Vec::new(),
            best_cos: None,
        }
    }
}

/// How wide a pool to pull from each signal before fusing (qmd keeps ~30). `pub(crate)`
/// for the façade's candidate-pool measurements (GH #141, #142). Saturating: `limit` is
/// user input, and a wrapped product would silently shrink the pool.
pub(crate) fn pool_size(limit: usize) -> usize {
    limit.saturating_mul(5).max(30)
}

/// Keyword-only search, the fallback for an unembedded vault. Scores are single-list RRF,
/// on [`hybrid_search`]'s scale; `best_cos` is `None` (ADR-0015).
pub fn keyword_only_search(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> Result<Retrieval> {
    if limit == 0 {
        return Ok(Retrieval::empty());
    }
    let pool = pool_size(limit);
    let bm25 = keyword_search(conn, query, pool)?;
    tracing::debug!(
        target: "b2::search",
        bm25_hits = bm25.len(),
        pool,
        "keyword-only retrieval (no embedding space yet)"
    );
    fuse(conn, bm25, &[], limit)
}

/// Hybrid search: BM25 ⊕ vector(query) -> RRF -> top `limit`, resolved to notes
/// (ADR-0008). A `limit` of 0 returns before the costly query embedding.
pub fn hybrid_search(
    conn: &rusqlite::Connection,
    embedder: &dyn Embedder,
    query: &str,
    limit: usize,
) -> Result<Retrieval> {
    if limit == 0 {
        return Ok(Retrieval::empty());
    }
    let pool = pool_size(limit);
    let bm25 = keyword_search(conn, query, pool)?;
    let dense = db::vector_search(conn, &embedder.embed_query(query)?, pool)?;
    tracing::debug!(
        target: "b2::search",
        bm25_hits = bm25.len(),
        vector_hits = dense.len(),
        pool,
        "hybrid retrieval fusing BM25 ⊕ vector via RRF"
    );
    fuse(conn, bm25, &dense, limit)
}

/// The shared tail of both retrievals: RRF-fuse, keep provenance, resolve the first
/// `limit` to notes. Dense goes last, so it breaks ties.
fn fuse(
    conn: &rusqlite::Connection,
    bm25: Vec<i64>,
    dense: &[(i64, f32)],
    limit: usize,
) -> Result<Retrieval> {
    let provenance = provenance_of(&bm25, dense);
    // The dense list is nearest-first, so its head is the best cosine.
    let best_cos = dense.first().map(|&(_, d)| cosine_of_distance(d));
    let mut lists = vec![bm25];
    if !dense.is_empty() {
        lists.push(dense.iter().map(|&(id, _)| id).collect());
    }
    Ok(Retrieval {
        hits: resolve_hits(conn, rrf_fuse(&lists, RRF_K), &provenance, limit)?,
        best_cos,
    })
}

/// Cosine from an L2 distance between unit vectors: `cos = 1 - d²/2` (exact, as bge
/// normalizes its output).
pub fn cosine_of_distance(distance: f32) -> f64 {
    let d = distance as f64;
    1.0 - (d * d) / 2.0
}

/// Vector-only search: the dense half of [`hybrid_search`] alone. An eval ablation
/// instrument, not a product surface (ADR-0013, GH #158). Scores are negated L2 distance,
/// not comparable with RRF scores.
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
    let dense = db::vector_search(conn, &embedder.embed_query(query)?, pool)?;
    let ranked: Vec<(i64, f64)> = dense
        .iter()
        .map(|&(id, distance)| (id, -(distance as f64)))
        .collect();
    tracing::debug!(
        target: "b2::search",
        vector_hits = ranked.len(),
        pool,
        "vector-only retrieval (ablation instrument)"
    );
    let provenance = provenance_of(&[], &dense);
    resolve_hits(conn, ranked, &provenance, limit)
}

/// Resolve a ranked `(chunk_id, score)` list to the first `limit` [`Hit`]s that still
/// resolve to a note. An unresolved chunk (a concurrent reindex, C1) is skipped, not
/// charged against `limit` (GH #137).
fn resolve_hits(
    conn: &rusqlite::Connection,
    fused: Vec<(i64, f64)>,
    provenance: &HashMap<i64, HitProvenance>,
    limit: usize,
) -> Result<Vec<Hit>> {
    let mut hits = Vec::new();
    for (chunk_id, score) in fused {
        // Before the push, so `limit == 0` returns nothing.
        if hits.len() == limit {
            break;
        }
        if let Some(note_path) = db::note_for_chunk(conn, chunk_id)? {
            hits.push(Hit {
                chunk_id,
                note_path,
                score,
                provenance: provenance.get(&chunk_id).copied().unwrap_or_default(),
            });
        }
    }
    Ok(hits)
}
