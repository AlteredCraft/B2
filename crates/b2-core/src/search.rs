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
    /// Which signals ranked this chunk, and how far its vector really sits from
    /// the query — the absolute readings RRF discards (invariants.md D2).
    pub provenance: HitProvenance,
}

/// Per-hit provenance carried **beside** the fused order, never folded into it
/// (invariants.md D2, GH #201).
///
/// RRF reduces each list to integer ranks and adds reciprocals, which is exactly
/// what makes a fused score read as confidence: rank 1 of a list that should
/// have been empty scores identically to rank 1 of a list full of real matches.
/// These are the absolute signals that reduction throws away. Recording them
/// changes no ordering — [`rrf_fuse`] is untouched — it only stops the evidence
/// from being unrecoverable by the time a surface has to decide what it vouches
/// for.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HitProvenance {
    /// 0-based rank in the BM25 list; `None` = the lexical half never ranked
    /// this chunk — the "dense-only hit" a nonsense query serves `limit` of.
    pub bm25_rank: Option<usize>,
    /// 0-based rank in the dense list; `None` = the vector half never ranked it,
    /// or never ran (a projected-but-unembedded vault).
    pub vector_rank: Option<usize>,
    /// L2 distance from the query vector to this chunk's, in
    /// [`db::vector_search`]'s units — `cosine = 1 - d²/2`, the model's vectors
    /// being unit-length. `None` whenever `vector_rank` is.
    pub distance: Option<f32>,
}

/// Build the per-chunk provenance map for a fusion over `(bm25, vector)`, the
/// dense list carrying its distances. Chunks absent from a list simply carry
/// `None` for it — absence *is* the reading.
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

/// Reciprocal Rank Fusion of ranked id lists: `score(id) = Σ 1/(k + rank + 1)`
/// over the lists it appears in (rank 0-based). Returns ids with fused scores,
/// best first.
///
/// **Exact ties are structural here**, not freak events: integer ranks put every
/// fused score on a discrete lattice of reachable sums, so two candidates with
/// mirrored ranks — (1, 3) against (3, 1) — collide bit-identically. The eval
/// corpus hit one on its second run (GH #156). Breaking
/// such a tie is a *policy choice about which signal to trust in a photo finish*:
/// the old key (ascending id — projection walk order) answered it with the
/// filesystem, which is deterministic and semantically arbitrary. The key now is
/// the candidate's rank in the **last** list — callers put the signal they trust
/// on ties last, which for [`hybrid_search`] is the dense/vector list: on the tie
/// the eval decomposed, the semantic half named the labelled answer and BM25
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
    let terms: Vec<String> = query_terms(raw).iter().map(|t| quoted(t)).collect();
    terms.join(" OR ")
}

/// The terms [`fts5_query`] ORs, in query order, before quoting: every maximal
/// alphanumeric run. Factored out because the evidence reading
/// ([`lexical_evidence`]) has to judge **exactly** the terms the `MATCH`
/// expression searches for — a second tokenizer here would let the two disagree
/// about what the query even said.
pub fn query_terms(raw: &str) -> Vec<String> {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// One term as a double-quoted FTS5 string literal, so nothing in it is read as
/// an operator. Internal quotes cannot occur (the split drops them) but are
/// doubled defensively regardless.
fn quoted(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

/// One query term's lexical reading: the term as written, and its **document
/// frequency** — how many chunks match it alone.
#[derive(Debug, Clone, PartialEq)]
pub struct TermEvidence {
    /// The term the sanitizer extracted (unstemmed; FTS5 stems it on the way in,
    /// so `df` is the stemmed reading of this surface form).
    pub term: String,
    /// Chunks matching this term. `0` = the vault has never seen the word, which
    /// is the strongest lexical statement available: not "ranked low", *absent*.
    pub df: usize,
}

/// The lexical half's **absolute** reading for one query (invariants.md D2, GH
/// #201) — what the OR-ed `MATCH` knows and then loses.
///
/// Matching at all is not evidence. [`keyword_search`] ORs every term for
/// recall, so a phrase-shaped query matches on its function words: on the eval
/// corpus the labelled negatives match 68 of 70 chunks through `a` / `to` / `my`
/// alone, and one reads a *better* best-BM25 than several positives off nothing
/// but mid-IDF function words (GH #201, Phase A). So neither a raw hit count nor
/// a raw best score is a lexical-anchor test.
///
/// Document frequency is, and it is the reading kept here — as a **weight**, not
/// a bin: a term is worth its IDF, so a word in most chunks counts for almost
/// nothing and a word in none counts for the most there is. That keeps the rule
/// working on a vault whose jargon is another vault's rare word, and it is what
/// a hard stopword ceiling could not do (see [`LexicalEvidence::idf`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LexicalEvidence {
    /// Chunks in the index — the scale every term's weight is read against.
    pub chunk_total: usize,
    /// Every distinct term the query contributed, in first-appearance order.
    pub terms: Vec<TermEvidence>,
}

impl LexicalEvidence {
    /// A term's inverse document frequency in this vault:
    /// `ln((chunks + 1) / (df + 1))`.
    ///
    /// This is the whole stopword policy, and it is **continuous** — there is no
    /// ceiling anywhere in it. A word in every chunk weighs ~0 and a word in none
    /// weighs the most there is, with everything between graded rather than
    /// sorted into two bins. The `+1`s keep both ends finite: a `df` of zero is
    /// the reading this rule most depends on, and an empty vault must not be a
    /// division by zero.
    ///
    /// A ceiling was tried first and **failed its transfer check** (GH #201): a
    /// "term in ≤ 10% of chunks is content" rule is scale-free in the vault's
    /// *size* but not in its *topical concentration*, and on the 15-chunk
    /// single-domain fixture the ceiling came to 1.5 chunks — so `drone` (df 3)
    /// and `comb` (df 7) were classed stopwords in a vault about beekeeping, the
    /// lexical half went inert, and the bar cut 3 of 15 queries naming notes the
    /// vault holds. The same geometry that broke every anchor-local z gate in GH
    /// #196, met again on the lexical axis. Weighting has no bin to put them in
    /// the wrong side of.
    pub fn idf(&self, df: usize) -> f64 {
        ((self.chunk_total as f64 + 1.0) / (df as f64 + 1.0)).ln()
    }

    /// How much of the query's **content** the vault actually holds: the share of
    /// its total term IDF carried by terms with `df >= 1`.
    ///
    /// Weighted, not counted, and that is what makes it a content test. Function
    /// words weigh nothing, so they neither grant coverage nor dilute it — the
    /// query "why parrots mimic speech" reads 0.08 on the eval corpus, almost all
    /// of it the `why`. Words the vault has never seen weigh the *most*, so a
    /// query is judged mostly on the words that would have mattered.
    ///
    /// `None` when no term carries any weight at all (every word in every chunk,
    /// or no usable terms) — that is *no reading*, not a coverage of zero, and
    /// the cosine half decides alone.
    pub fn term_coverage(&self) -> Option<f64> {
        let total: f64 = self.terms.iter().map(|t| self.idf(t.df)).sum();
        if total <= f64::EPSILON {
            return None;
        }
        // Folded from an explicit `0.0` rather than `.sum()`, whose additive
        // identity for `f64` is `-0.0`: an empty present-set is the *most*
        // common reading this function has (it is what a nonsense query looks
        // like), and a coverage of `-0.0` prints as `-0.00`.
        let present: f64 = self
            .terms
            .iter()
            .filter(|t| t.df >= 1)
            .map(|t| self.idf(t.df))
            .fold(0.0, |a, b| a + b);
        Some(present / total)
    }

    /// Whether the query has a **lexical anchor** — D2's lexical half.
    ///
    /// Coverage, not presence: "does the vault hold *any* of these words" is too
    /// weak a test, because an off-topic query shares one word with almost any
    /// vault. What separates a query the vault has material for from one it does
    /// not is how much of the query's own weight the vault carries.
    pub fn anchored(&self, min_term_coverage: f64) -> bool {
        self.term_coverage().is_some_and(|c| c >= min_term_coverage)
    }
}

/// Read the lexical evidence for `query`: every term's document frequency, over
/// the same sanitized terms [`keyword_search`] searches for.
///
/// One `count(*)` per distinct term — the probe FTS5 has no cheaper form of
/// without an `fts5vocab` shadow table, which would be schema surface bought for
/// a handful of counts per query.
pub fn lexical_evidence(conn: &rusqlite::Connection, query: &str) -> Result<LexicalEvidence> {
    let chunk_total: usize = conn
        .query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))?
        .try_into()
        .unwrap_or(0);
    let mut terms: Vec<TermEvidence> = Vec::new();
    for term in query_terms(query) {
        if terms.iter().any(|t| t.term == term) {
            continue; // a repeated term is one piece of evidence, not two
        }
        let df: i64 = conn.query_row(
            "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
            [&quoted(&term)],
            |r| r.get(0),
        )?;
        terms.push(TermEvidence {
            term,
            df: usize::try_from(df).unwrap_or(0),
        });
    }
    Ok(LexicalEvidence { chunk_total, terms })
}

/// The per-model evidence bar D2 judges a query against: what counts as a
/// lexical anchor, and how near the dense half must be to stand in for one.
///
/// **A distributional constant, so it is keyed to the model** (M2 — a swap
/// invalidates it) and owes process rule 5's real-vault transfer check. It lives
/// in code as a constant and is *measured* in the harness, never the other way
/// round: the GH #150 cosine floor was a comment quoting numbers that went stale
/// the first time the corpus grew a shape they were never read against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceBar {
    /// Share of the query's term IDF the vault must carry for the lexical half
    /// to count as evidence — see [`LexicalEvidence::term_coverage`].
    pub min_term_coverage: f64,
    /// Cosine the dense top-1 must reach for semantic proximity to stand as
    /// evidence on its own, for a query with no lexical anchor.
    pub min_cos: f64,
}

impl EvidenceBar {
    /// The calibrated bar for `model_id`, or `None` when the active embedder has
    /// none.
    ///
    /// `None` means **no verdict**, never a default one: a bar read off one
    /// model's distance distribution says nothing about another's (M2), and the
    /// fake embedder's hash vectors have no semantic geometry to hold a cosine
    /// bar over at all — the same reason `discover::candidates` is asked not to
    /// grade a fake-embedded space. A caller that gets `None` serves the list
    /// exactly as it always did.
    ///
    /// The **device suffix is stripped** before the lookup (`…@metal`, GH #40).
    /// A device switch is a model swap for *vector identity* — the stored
    /// vectors differ in their last bits, so mixing them is refused — but the
    /// bar is a claim about a distribution, and float-precision noise is not a
    /// distributional change. That is an assumption, so it is one the harness
    /// re-checks rather than one this comment asserts: the search evidence
    /// bake-off re-derives the admissible window on **every** run, `just
    /// eval-metal` runs it on the GPU build, and a bar that has drifted out of
    /// its window prints as such there.
    pub fn for_model(model_id: &str) -> Option<Self> {
        let base = model_id.split_once('@').map_or(model_id, |(id, _)| id);
        match base {
            "BAAI/bge-base-en-v1.5" => Some(BGE_BASE_EVIDENCE_BAR),
            _ => None,
        }
    }
}

/// The evidence bar for the shipped model, `BAAI/bge-base-en-v1.5`
/// (invariants.md D2, GH #201).
///
/// Read from the search evidence bake-off, and re-derived on every `just eval`
/// run rather than quoted here — `crates/b2-embed/examples/eval.rs`'s
/// `search evidence bake-off` block prints the admissible window and says
/// whether these three still sit inside it. Deliberately no numbers in this
/// comment: that is the mistake GH #187 named, where a floor's docstring froze
/// the day's readings and went stale the first time the corpus grew a shape they
/// were never measured against.
///
/// Where the margin lives is worth knowing, though, because it is not spread
/// evenly. The lexical half does nearly all the work and the cosine half is a
/// **thin backstop** for what it drops: ask the lexical half for more — a
/// stricter coverage — and the cosine window collapses, because the queries it
/// then has to rescue are exactly the ones with the weakest semantic evidence
/// too. So the safe band is a joint one, and the two are placed inside it
/// together, never tuned one at a time. Both sit toward the **serving** side of
/// their windows, because the two errors are not symmetric: serving a weak
/// result costs a little trust, and cutting a real one is the tripwire D2
/// asserts at zero.
pub const BGE_BASE_EVIDENCE_BAR: EvidenceBar = EvidenceBar {
    min_term_coverage: 0.20,
    min_cos: 0.54,
};

/// The query-level evidence reading Flow ② carries beside its order
/// (invariants.md D2): the two absolute signals a surface needs to say whether
/// it vouches for anything at all.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryEvidence {
    /// The lexical half's reading — see [`LexicalEvidence`].
    pub lexical: LexicalEvidence,
    /// Best cosine between the query vector and any scanned chunk vector: the
    /// dense half's strongest absolute claim about *this* query, as opposed to
    /// the rank it always has. `None` on a projected-but-unembedded vault, where
    /// there is no dense half and BM25's own emptiness is already honest.
    pub best_cos: Option<f64>,
}

impl QueryEvidence {
    /// D2's query-level verdict: does the vault hold **positive evidence** for
    /// this query — a lexical anchor, *or* semantic proximity clearing `bar`?
    ///
    /// Two independent signals, which is what keeps this out of the
    /// single-population trap GH #196 measured: a topic query in a vault full of
    /// that topic carries lexical evidence, semantic evidence, or both, from
    /// opposite ends of the same geometry — while nonsense carries neither. A
    /// one-signal test could not tell *nothing matches* from *everything
    /// matches*.
    pub fn vouched(&self, bar: EvidenceBar) -> bool {
        self.lexical.anchored(bar.min_term_coverage)
            || self.best_cos.is_some_and(|c| c >= bar.min_cos)
    }
}

/// A retrieval's two halves: the fused order, and the query-level evidence
/// behind it. One call, because the evidence is a by-product of the same two
/// list reads — asking for it separately would re-embed the query.
#[derive(Debug, Clone, PartialEq)]
pub struct Retrieval {
    pub hits: Vec<Hit>,
    pub evidence: QueryEvidence,
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
///
/// This path was already honest about zero — no lexical match, no results — and
/// the [`QueryEvidence`] it returns says so with `best_cos: None`: there is no
/// dense half here to overrule the lexical half's silence (invariants.md D2).
pub fn keyword_only_search(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> Result<Retrieval> {
    if limit == 0 {
        return Ok(Retrieval {
            hits: Vec::new(),
            evidence: QueryEvidence {
                lexical: lexical_evidence(conn, query)?,
                best_cos: None,
            },
        });
    }
    let pool = pool_size(limit);
    let bm25 = keyword_search(conn, query, pool)?;
    tracing::debug!(
        target: "b2::search",
        bm25_hits = bm25.len(),
        pool,
        "keyword-only retrieval (no embedding space yet)"
    );
    let provenance = provenance_of(&bm25, &[]);
    let hits = resolve_hits(conn, rrf_fuse(&[bm25], RRF_K), &provenance, limit)?;
    Ok(Retrieval {
        hits,
        evidence: QueryEvidence {
            lexical: lexical_evidence(conn, query)?,
            best_cos: None,
        },
    })
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
) -> Result<Retrieval> {
    if limit == 0 {
        return Ok(Retrieval {
            hits: Vec::new(),
            evidence: QueryEvidence {
                lexical: lexical_evidence(conn, query)?,
                best_cos: None,
            },
        });
    }
    let pool = pool_size(limit);
    let bm25 = keyword_search(conn, query, pool)?;
    let dense = db::vector_search(conn, &embedder.embed_query(query)?, pool)?;
    let vector: Vec<i64> = dense.iter().map(|&(id, _)| id).collect();
    // The dense list is nearest-first, so its head *is* the best cosine — the
    // absolute reading RRF is about to reduce to "rank 0".
    let best_cos = dense.first().map(|&(_, d)| cosine_of_distance(d));
    tracing::debug!(
        target: "b2::search",
        bm25_hits = bm25.len(),
        vector_hits = vector.len(),
        best_cos,
        pool,
        "hybrid retrieval fusing BM25 ⊕ vector via RRF"
    );

    let provenance = provenance_of(&bm25, &dense);
    let hits = resolve_hits(conn, rrf_fuse(&[bm25, vector], RRF_K), &provenance, limit)?;
    Ok(Retrieval {
        hits,
        evidence: QueryEvidence {
            // Two `count(*)` probes per distinct term, run beside a call that
            // has already scanned every stored vector in the vault — the
            // lexical reading is noise against the dense scan it rides on.
            lexical: lexical_evidence(conn, query)?,
            best_cos,
        },
    })
}

/// Cosine similarity from an L2 distance between **unit** vectors:
/// `cos = 1 - d²/2`. The stored vectors are model output, which bge normalizes,
/// so this is an identity rather than an approximation — and it is what puts the
/// dense half's absolute reading on the one scale a bar can be written in
/// (`db::vector_search` ranks by distance, where *smaller* is better).
pub fn cosine_of_distance(distance: f32) -> f64 {
    let d = distance as f64;
    1.0 - (d * d) / 2.0
}

/// Vector-only search: the dense half of [`hybrid_search`] alone — embed the
/// query, exact-scan the stored vectors, resolve the top `limit` to notes.
///
/// **An ablation instrument, not a product surface** (GH #158): the eval harness
/// scores it beside bm25-only and hybrid on every run, so fusion has a measured
/// single-signal baseline to answer to — the eval's finding that RRF can demote
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
    provenance: &HashMap<i64, HitProvenance>,
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
                provenance: provenance.get(&chunk_id).copied().unwrap_or_default(),
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
                // One list, walked in rank order: `hits.len()` is this chunk's
                // rank among the reachable ones, which is the only rank this
                // path has (there is no lexical half to be absent from).
                provenance: HitProvenance {
                    bm25_rank: None,
                    vector_rank: Some(hits.len()),
                    distance: Some(distance),
                },
            });
        }
    }
    Ok(hits)
}
