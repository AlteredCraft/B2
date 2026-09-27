//! The search evidence calibration and bake-off (invariants.md D2; ADR-0015, GH #201/#202):
//! per labelled query, the absolute signals RRF discards, the coverage × cosine grid swept
//! over them, and the shipped bar's reading the exit gate asserts.

use crate::common::{cosine_of, pile_stats, truncate};
use crate::gate::{MAX_NEGATIVES_SERVED, MAX_POSITIVES_CUT};
use crate::labels::Labelled;
use crate::metrics::{paths_match, Window};
use crate::K;
use b2_core::search::EvidenceBar;
use b2_core::vault::Vault;
use std::path::Path;

/// One served row of a labelled query's list (GH #206): per-hit provenance and relevance by
/// label. Rank is the position in `rows`, never stored: the fused order is what D1 binds.
pub struct ServedRow {
    pub path: String,
    /// 0-based rank in the BM25 list; `None` is a dense-only row.
    pub bm25_rank: Option<usize>,
    /// The row's cosine to the query; `None` when the dense half never ranked it. Never
    /// both `None`.
    pub cos: Option<f64>,
    /// In `relevant` ∪ `tail_relevant`. Exhaustive, so false is a judgement, not an omission.
    pub keep: bool,
}

/// One labelled query's evidence reading (D2, GH #201): the signals a query-level evidence
/// rule judges.
pub struct QueryEvidence {
    pub query: String,
    /// Chunks the OR-sanitized FTS5 expression matches. Not a lexical anchor: stopwords
    /// saturate it.
    pub bm25_hits: usize,
    /// Best BM25 score, sign-flipped so higher = better. `None` when nothing matches.
    pub bm25_best: Option<f64>,
    /// The dense top-1 cosine, which RRF discards before the surface sees it.
    pub best_cos: Option<f64>,
    /// The note that dense top-1 belongs to.
    pub top: String,
    /// The served list at [`K`] in fused order (GH #206).
    pub rows: Vec<ServedRow>,
    /// Chunks in the index, the denominator for every `df`.
    pub chunk_total: usize,
    /// Every query term with its document frequency, in query order: what the lexical
    /// anchor is derived from, since raw hits and best-BM25 failed to separate the piles.
    pub terms: Vec<(String, usize)>,
    /// The engine's own verdict (GH #202), which the exit gate counts on. `None` when the
    /// model has no calibrated bar. [`coverage`](Self::coverage) restates the rule for the
    /// sweep; [`read_shipped_bar`] flags any disagreement.
    pub vouched: Option<bool>,
}

impl QueryEvidence {
    /// What the shipped surface serves at [`K`]; on a negative query, the D2 defect.
    pub fn served(&self) -> usize {
        self.rows.len()
    }

    /// Served rows the lexical half never ranked, which the `lexical` tail rule folds on
    /// (GH #206).
    pub fn dense_only(&self) -> usize {
        self.rows.iter().filter(|r| r.bm25_rank.is_none()).count()
    }

    /// The share of this query's term IDF the vault carries: an independent restatement of
    /// [`b2_core::search::LexicalEvidence::term_coverage`], as a drift check. `None` when no
    /// term carries weight.
    pub fn coverage(&self) -> Option<f64> {
        let idf = |df: usize| ((self.chunk_total as f64 + 1.0) / (df as f64 + 1.0)).ln();
        let total: f64 = self.terms.iter().map(|(_, df)| idf(*df)).sum();
        if total <= f64::EPSILON {
            return None;
        }
        let present: f64 = self
            .terms
            .iter()
            .filter(|(_, df)| *df >= 1)
            .map(|(_, df)| idf(*df))
            .fold(0.0, |a, b| a + b);
        Some(present / total)
    }

    /// Whether this query has a lexical anchor at `min_coverage`.
    pub fn anchored(&self, min_coverage: f64) -> bool {
        self.coverage().is_some_and(|c| c >= min_coverage)
    }
}

/// The `min_term_coverage` grid the bake-off sweeps, spanning the rule's whole range rather
/// than a neighbourhood of the shipped constant.
pub const COVERAGES: [f64; 10] = [0.05, 0.10, 0.15, 0.20, 0.25, 0.34, 0.50, 0.67, 0.85, 1.00];

/// One coverage cell of the bake-off: how the lexical half splits the labelled piles, and
/// what the cosine half must do with the rest.
pub struct EvidenceCell {
    pub coverage: f64,
    pub pos_anchored: usize,
    /// Negatives the lexical half vouches for. Nonzero disqualifies the cell: no `min_cos`
    /// can cut an anchored query.
    pub neg_anchored: usize,
    /// Cosines of undecided positives, which a `min_cos` must keep.
    pub undecided_pos: Vec<f64>,
    /// Cosines of undecided negatives, which it must cut.
    pub undecided_neg: Vec<f64>,
}

impl EvidenceCell {
    /// The cosine window for `min_cos` over only the undecided queries: D2 is lexical OR
    /// semantic, so an anchored positive never needs its cosine kept.
    pub fn window(&self) -> Option<Window> {
        Window::read(&self.undecided_neg, &self.undecided_pos)
    }

    /// Whether some `min_cos` completes this cell into a rule that keeps every positive and
    /// cuts every negative. An empty undecided pile on either side is admissible.
    pub fn admissible(&self) -> bool {
        if self.neg_anchored > 0 {
            return false;
        }
        match self.window() {
            Some(w) => w.open(),
            None => true,
        }
    }

    /// The bar's measured floor. `None` when nothing is left to cut.
    pub fn cut_floor(&self) -> Option<f64> {
        let max = self
            .undecided_neg
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        max.is_finite().then_some(max)
    }

    /// The bar's measured ceiling. `None` when nothing is left to keep.
    pub fn keep_ceiling(&self) -> Option<f64> {
        let min = self
            .undecided_pos
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        min.is_finite().then_some(min)
    }
}

/// Sweep the lexical rule over [`COVERAGES`], reading each cell's piles from the
/// labelled queries (GH #201, Phase C).
pub fn bake_off(ev: &SearchEvidence) -> Vec<EvidenceCell> {
    let mut cells = Vec::new();
    for coverage in COVERAGES {
        let split = |rows: &[QueryEvidence]| -> (usize, Vec<f64>) {
            let anchored = rows.iter().filter(|r| r.anchored(coverage)).count();
            let undecided = rows
                .iter()
                .filter(|r| !r.anchored(coverage))
                .filter_map(|r| r.best_cos)
                .collect();
            (anchored, undecided)
        };
        let (pos_anchored, undecided_pos) = split(&ev.positives);
        let (neg_anchored, undecided_neg) = split(&ev.negatives);
        cells.push(EvidenceCell {
            coverage,
            pos_anchored,
            neg_anchored,
            undecided_pos,
            undecided_neg,
        });
    }
    cells
}

/// The search evidence dump (GH #201): positives the bar must keep, negatives it must cut.
pub struct SearchEvidence {
    pub positives: Vec<QueryEvidence>,
    pub negatives: Vec<QueryEvidence>,
}

/// One side's best-cos pile; queries with no dense reading drop out.
pub fn best_cos_pile(rows: &[QueryEvidence]) -> Vec<f64> {
    rows.iter().filter_map(|r| r.best_cos).collect()
}

/// Score the search evidence calibration (D2, GH #201), a pure read over the built vault.
pub fn score_search_evidence(
    vault_root: &Path,
    vault: &Vault,
    positives: &[Labelled],
    negatives: &[Labelled],
) -> Result<SearchEvidence, Box<dyn std::error::Error>> {
    // A second reader (C1): the probe wants FTS5's `rank`, which no façade read exposes.
    let conn = b2_core::open(&vault_root.join(".b2").join("b2.sqlite"))?;
    let rows = |queries: &[Labelled]| -> Result<Vec<QueryEvidence>, Box<dyn std::error::Error>> {
        queries
            .iter()
            .map(|q| {
                let (bm25_hits, bm25_best) = bm25_probe(&conn, &q.query)?;
                let dense = vault.search_vector_only(&q.query, 1)?;
                let best_cos = dense.first().map(|h| cosine_of(h.score));
                let top = dense
                    .first()
                    .map(|h| h.path.clone())
                    .unwrap_or_else(|| "—".to_string());
                // The façade's own evidence read, so the sweep judges what the engine would.
                let view = vault.search_evidence(&q.query, K)?;
                Ok(QueryEvidence {
                    query: q.query.clone(),
                    bm25_hits,
                    bm25_best,
                    best_cos,
                    top,
                    rows: view
                        .results
                        .iter()
                        .map(|r| ServedRow {
                            path: r.result.path.clone(),
                            bm25_rank: r.bm25_rank,
                            cos: r.cos,
                            keep: q
                                .relevant
                                .iter()
                                .chain(q.tail_relevant.iter())
                                .any(|rel| paths_match(&r.result.path, rel)),
                        })
                        .collect(),
                    chunk_total: view.chunk_total,
                    terms: view.terms.iter().map(|t| (t.term.clone(), t.df)).collect(),
                    vouched: view.vouched,
                })
            })
            .collect()
    };
    Ok(SearchEvidence {
        positives: rows(positives)?,
        negatives: rows(negatives)?,
    })
}

/// Match count and best BM25 (sign-flipped) for one query, probed over `chunks_fts` with the
/// engine's [`b2_core::search::fts5_query`]: the absolute signals RRF discards.
pub fn bm25_probe(
    conn: &rusqlite::Connection,
    query: &str,
) -> Result<(usize, Option<f64>), Box<dyn std::error::Error>> {
    let expr = b2_core::search::fts5_query(query);
    if expr.is_empty() {
        return Ok((0, None));
    }
    let hits: i64 = conn.query_row(
        "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
        [&expr],
        |r| r.get(0),
    )?;
    let hits = usize::try_from(hits).unwrap_or(0);
    if hits == 0 {
        return Ok((0, None));
    }
    let best: f64 = conn.query_row(
        "SELECT rank FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY rank LIMIT 1",
        [&expr],
        |r| r.get(0),
    )?;
    Ok((hits, Some(-best)))
}

/// Print the search evidence calibration (D2, GH #201). Reported, not gated. The
/// pure-cosine window overstates what a real bar must keep, since lexical evidence keeps
/// its own.
pub fn print_search_evidence(ev: &SearchEvidence) {
    println!(
        "  search evidence calibration (D2 — reported, nothing gates; the query bar is GH #201's \
         to earn)"
    );
    let pos_cos = best_cos_pile(&ev.positives);
    let neg_cos = best_cos_pile(&ev.negatives);
    for (label, pile, role) in [
        (
            "pos best-cos",
            &pos_cos,
            "a pure-cosine bar would have to KEEP",
        ),
        ("neg best-cos", &neg_cos, "any query bar would have to CUT"),
    ] {
        match pile_stats(pile) {
            Some((min, med, max)) => println!(
                "    {label:<12} n={:<4} min/med/max {min:.3}/{med:.3}/{max:.3}   ← {role}",
                pile.len()
            ),
            None => println!("    {label:<12} n=0    (nothing labelled — no reading)"),
        }
    }
    let hits: Vec<f64> = ev.positives.iter().map(|r| r.bm25_hits as f64).collect();
    if let Some((min, med, max)) = pile_stats(&hits) {
        println!(
            "    pos bm25     hits min/med/max {min:.0}/{med:.0}/{max:.0}   (OR-saturated: stopwords \
             match too, so a raw count is not a lexical anchor — GH #201 derives one from this dump)"
        );
    }
    match Window::read(&neg_cos, &pos_cos) {
        None => println!(
            "    cos window   no reading (a pile is empty — label negative queries to give GH #201 \
             its CUT side)"
        ),
        Some(w) if w.open() => println!(
            "    cos window   ({:.3}, {:.3}]  — open on THIS corpus for a pure-cosine query bar; \
             the real keep-set is smaller (lexical evidence keeps its own), and a real vault is the \
             other half of any such claim (process rule 5, `make calibrate`)",
            w.cut_max, w.keep_min
        ),
        Some(w) => println!(
            "    cos window   EMPTY for a pure-cosine bar — negatives reach {:.3} while positives \
             start at {:.3}; D2's two-signal rule (lexical OR semantic) is argued from the per-query \
             lines, not this window",
            w.cut_max, w.keep_min
        ),
    }
    if ev.negatives.is_empty() {
        println!(
            "    negatives    n=0   (none labelled — queries.json takes empty `relevant` as \
             \"no matches\")"
        );
    } else {
        println!(
            "    negatives (labelled answer: NO MATCHES — what the shipped surface serves instead):"
        );
        for r in &ev.negatives {
            println!(
                "      {:<40} bm25 {:>3} hit{}  best {:>6}  cos {:>6}  served {:>2}/{K}  top {}",
                truncate(&r.query, 40),
                r.bm25_hits,
                if r.bm25_hits == 1 { " " } else { "s" },
                r.bm25_best
                    .map(|b| format!("{b:.2}"))
                    .unwrap_or_else(|| "—".to_string()),
                r.best_cos
                    .map(|c| format!("{c:.3}"))
                    .unwrap_or_else(|| "—".to_string()),
                r.served(),
                r.top,
            );
        }
    }
}

/// Print the search evidence bake-off (ADR-0015, GH #201). The constant lives in
/// [`b2_core::search::BGE_BASE_EVIDENCE_BAR`]; its justification is recomputed here each run.
pub fn print_search_bakeoff(ev: &SearchEvidence, cells: &[EvidenceCell], model_id: &str) {
    println!(
        "  search evidence bake-off (D2 — the query-level rule, re-derived every run; GH #201)"
    );
    for line in [
        "the rule: serve iff the query has a LEXICAL ANCHOR (the vault carries ≥ coverage of the",
        "          query's term IDF — a word in most chunks weighs ~0, a word in none weighs the",
        "          most) OR its dense top-1 clears a cosine bar. Two signals on purpose: a",
        "          one-signal test cannot tell \"nothing matches\" from \"everything matches\" (#196).",
        "`cos need` reads over ONLY the queries the lexical half left undecided — a positive with",
        "          an anchor never needs its cosine kept, so a pure-cosine window overstates.",
    ] {
        println!("    {line}");
    }
    println!(
        "    n = {} positives / {} negatives labelled",
        ev.positives.len(),
        ev.negatives.len()
    );
    println!(
        "    {:>5}  {:>9} {:>9}  {:>15}  verdict",
        "cov", "pos anch", "neg anch", "cos need"
    );
    for cell in cells {
        let need = match (cell.cut_floor(), cell.keep_ceiling()) {
            (Some(cut), Some(keep)) => format!("({cut:.3},{keep:.3}]"),
            (Some(cut), None) => format!("> {cut:.3}"),
            (None, Some(keep)) => format!("≤ {keep:.3} (inert)"),
            (None, None) => "inert".to_string(),
        };
        let verdict = if cell.neg_anchored > 0 {
            format!(
                "✗ {} negative{} anchored — no cosine bar can rescue them",
                cell.neg_anchored,
                if cell.neg_anchored == 1 { "" } else { "s" }
            )
        } else if cell.admissible() {
            "✓ admissible".to_string()
        } else {
            "✗ cosine window empty".to_string()
        };
        println!(
            "    {:>5.2}  {:>4}/{:<4} {:>4}/{:<4}  {:>15}  {}",
            cell.coverage,
            cell.pos_anchored,
            ev.positives.len(),
            cell.neg_anchored,
            ev.negatives.len(),
            need,
            verdict,
        );
    }
    let admissible: Vec<&EvidenceCell> = cells.iter().filter(|c| c.admissible()).collect();
    if admissible.is_empty() {
        for line in [
            "→ NO admissible cell on this corpus: every lexical rule either vouches for a",
            "  labelled negative or leaves an empty cosine window. D2's bar is not earned,",
            "  and no rule ships (the GH #200 outcome, on search's side).",
        ] {
            println!("    {line}");
        }
    } else {
        println!(
            "    → {} of {} cells admissible; the widest cosine window belongs to {}",
            admissible.len(),
            cells.len(),
            widest(&admissible),
        );
    }
    print_shipped_bar(ev, model_id);
    let dense_only = |rows: &[QueryEvidence]| {
        (
            rows.iter().map(|r| r.dense_only()).sum::<usize>(),
            rows.iter().map(|r| r.served()).sum::<usize>(),
        )
    };
    let (pos_only, pos_served) = dense_only(&ev.positives);
    let (neg_only, neg_served) = dense_only(&ev.negatives);
    println!("    tail reading: served rows the lexical half never ranked —");
    println!(
        "      positives {pos_only}/{pos_served}, negatives {neg_only}/{neg_served}. The per-hit \
         rules over these are"
    );
    println!("      the tail bake-off's, below (GH #206).");
}

/// Name the plateau of admissible cells with the most cosine headroom and its strictest
/// coverage. Report the plateau, not a lone maximum, which would fit the grid's resolution.
pub fn widest(cells: &[&EvidenceCell]) -> String {
    let best = cells
        .iter()
        .map(|c| headroom(c))
        .fold(f64::NEG_INFINITY, f64::max);
    if !best.is_finite() {
        let inert = cells.iter().filter(|c| !headroom(c).is_finite()).count();
        return format!(
            "{inert} cell(s) where the cosine half is INERT — the lexical rule alone decides \
             every labelled query, so no `min_cos` is constrained there"
        );
    }
    let band: Vec<&&EvidenceCell> = cells
        .iter()
        .filter(|c| (headroom(c) - best).abs() < 1e-9)
        .collect();
    match band.iter().max_by(|a, b| {
        a.coverage
            .partial_cmp(&b.coverage)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        None => "—".to_string(),
        Some(c) => format!(
            "a plateau of {} cells at headroom {best:.3}; its strictest is coverage ≥ {:.2}",
            band.len(),
            c.coverage,
        ),
    }
}

/// A cell's cosine headroom between the negatives to cut and positives to keep; infinite
/// when the cosine half is inert.
pub fn headroom(cell: &EvidenceCell) -> f64 {
    match (cell.cut_floor(), cell.keep_ceiling()) {
        (Some(cut), Some(keep)) => keep - cut,
        _ => f64::INFINITY,
    }
}

/// Where the shipped bar stands against this run's piles. Gated at [`MAX_POSITIVES_CUT`] and
/// [`MAX_NEGATIVES_SERVED`] (GH #202); read here so the gate and the printer share one number.
pub struct ShippedBar {
    pub bar: EvidenceBar,
    pub pos_cut: usize,
    pub neg_served: usize,
    /// Queries where the engine's verdict and the harness's restatement disagree. Printed,
    /// not gated: it may be the instrument that drifted.
    pub faults: Vec<String>,
}

pub fn read_shipped_bar(ev: &SearchEvidence, model_id: &str) -> Option<ShippedBar> {
    let bar = EvidenceBar::for_model(model_id)?;
    // The engine's verdict. A `None` counts as served, as on the surfaces (M2).
    Some(ShippedBar {
        bar,
        pos_cut: ev
            .positives
            .iter()
            .filter(|r| r.vouched == Some(false))
            .count(),
        neg_served: ev
            .negatives
            .iter()
            .filter(|r| r.vouched != Some(false))
            .count(),
        faults: ev
            .positives
            .iter()
            .chain(ev.negatives.iter())
            .filter(|r| {
                let restated = r.anchored(bar.min_term_coverage)
                    || r.best_cos.is_some_and(|c| c >= bar.min_cos);
                r.vouched != Some(restated)
            })
            .map(|r| r.query.clone())
            .collect(),
    })
}

pub fn print_shipped_bar(ev: &SearchEvidence, model_id: &str) {
    let Some(ShippedBar {
        bar,
        pos_cut,
        neg_served,
        faults,
    }) = read_shipped_bar(ev, model_id)
    else {
        println!("    shipped bar: none for this model — no verdict is offered (M2)");
        return;
    };
    // The engine's verdict, so the numbers printed are the numbers gated.
    let vouches = |r: &QueryEvidence| r.vouched != Some(false);
    println!(
        "    shipped bar: coverage ≥ {:.2}, cos ≥ {:.3}",
        bar.min_term_coverage, bar.min_cos
    );
    println!(
        "      positives it would cut  {pos_cut}/{}   ← the search-side TRIPWIRE (D2: zero, no \
         headroom; GATED at ≤ {MAX_POSITIVES_CUT})",
        ev.positives.len()
    );
    println!(
        "      negatives it still serves {neg_served}/{}   ← the defect the bar exists to fix \
         (GATED at ≤ {MAX_NEGATIVES_SERVED})",
        ev.negatives.len()
    );
    for r in ev.positives.iter().filter(|r| !vouches(r)) {
        println!(
            "      [CUT] {:<40} cov {:>5}  cos {:>6}",
            truncate(&r.query, 40),
            r.coverage()
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            r.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
        );
    }
    if !faults.is_empty() {
        println!(
            "      [FAULT] the engine's verdict and this harness's restatement of the same rule \
             disagree on {} query/queries: {}. One of the two has drifted — read the engine's \
             `LexicalEvidence`, not this line's wording.",
            faults.len(),
            faults.join(", ")
        );
    }
    for r in &ev.negatives {
        println!(
            "      {:<40} cov {:>5}  cos {:>6}  → {}",
            truncate(&r.query, 40),
            r.coverage()
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            r.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
            if vouches(r) { "SERVED" } else { "no matches" },
        );
    }
}
