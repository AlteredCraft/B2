//! The per-hit search tail bake-off (GH #206): candidate prefix-cut families, their
//! constraints on the labelled corpus and the dense fixture, and the cross-bench join. The
//! ruling of record is that no tail fold ships (docs/evals.md).

use crate::common::truncate;
use crate::dense::SearchProbe;
use crate::evidence::{QueryEvidence, SearchEvidence, ServedRow};

/// A candidate per-hit tail rule family (D2, GH #206). Every family is a prefix cut (D1):
/// the view ends at the first failing row. So a filler row above a keep row must pass too,
/// and constraints are read over the keep-prefix, not the keep rows alone.
#[derive(Clone, Copy, PartialEq)]
pub enum TailRule {
    /// Fold at the first dense-only row (GH #201). Parameterless but not scale-free: "never
    /// ranked" depends on `search::pool_size` against vault size, which `make calibrate
    /// ARGS=--search` measures.
    Lexical,
    /// Fold at the first row that is dense-only and under a cosine bar: the query rule's
    /// shape, per hit.
    LexOrCos,
    /// Fold at the first row under a cosine bar, lexical rank ignored: the single-signal
    /// baseline to beat.
    Cos,
    /// Fold at the first row more than δ under the list's best cosine.
    CosDrop,
}

impl TailRule {
    pub const ALL: [TailRule; 4] = [
        TailRule::Lexical,
        TailRule::LexOrCos,
        TailRule::Cos,
        TailRule::CosDrop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TailRule::Lexical => "lexical (dense-only fold)",
            TailRule::LexOrCos => "lex-or-cos ≥ c",
            TailRule::Cos => "cos ≥ c",
            TailRule::CosDrop => "cos ≥ best − δ",
        }
    }

    /// Whether `row` passes at constant `p` (ignored by `Lexical`). `best` is read only by
    /// `CosDrop`.
    pub fn passes(self, row: &ServedRow, p: f64, best: f64) -> bool {
        match self {
            TailRule::Lexical => row.bm25_rank.is_some(),
            TailRule::LexOrCos => row.bm25_rank.is_some() || row.cos.is_some_and(|c| c >= p),
            TailRule::Cos => row.cos.is_some_and(|c| c >= p),
            TailRule::CosDrop => row.cos.is_some_and(|c| c >= best - p),
        }
    }

    /// The index of the first failing row, or `rows.len()`. The one prefix-cut definition.
    pub fn fold(self, rows: &[ServedRow], p: f64) -> usize {
        let best = best_served_cos(rows);
        rows.iter()
            .position(|r| !self.passes(r, p, best))
            .unwrap_or(rows.len())
    }
}

/// The best served cosine of one list, [`TailRule::CosDrop`]'s reference point.
pub fn best_served_cos(rows: &[ServedRow]) -> f64 {
    rows.iter()
        .filter_map(|r| r.cos)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Every row at or above the deepest keep row; empty when none is kept.
pub fn keep_prefix(rows: &[ServedRow]) -> &[ServedRow] {
    match rows.iter().rposition(|r| r.keep) {
        Some(last) => &rows[..=last],
        None => &[],
    }
}

/// One family's constraint: the range of its constant at which no keep-prefix row fails,
/// with the row that set each edge.
pub struct TailConstraint {
    /// A keep-prefix row that passes at no constant: the family is dead on this bench.
    pub dead: Option<(String, String)>,
    /// `Cos`/`LexOrCos`: the highest admissible bar. `CosDrop`: the lowest admissible δ.
    /// `None` when no keep-prefix row engages the test.
    pub edge: Option<f64>,
    /// The (query-or-title, path) that set `edge`.
    pub edge_row: Option<(String, String)>,
    /// `Lexical` only: keep-prefix rows failing its test. Nonzero is inadmissible.
    pub lexical_violations: usize,
}

/// Read one family's [`TailConstraint`] over `(list name, full rows, keep-prefix length)`.
/// The dense fixture passes whole lists as keep-prefixes. The full list is needed because
/// [`TailRule::CosDrop`]'s reference is the whole list's best cosine.
pub fn tail_constraint<'a>(
    rule: TailRule,
    lists: impl Iterator<Item = (&'a str, &'a [ServedRow], usize)>,
) -> TailConstraint {
    let mut out = TailConstraint {
        dead: None,
        edge: None,
        edge_row: None,
        lexical_violations: 0,
    };
    for (name, rows, prefix_len) in lists {
        let best = best_served_cos(rows);
        for row in &rows[..prefix_len] {
            match rule {
                TailRule::Lexical => {
                    if row.bm25_rank.is_none() {
                        out.lexical_violations += 1;
                        if out.dead.is_none() {
                            out.dead = Some((name.to_string(), row.path.clone()));
                        }
                    }
                }
                // Filter non-finite cosines (PR #212): `is_none_or` admits the first row
                // unconditionally, so a NaN would seed the edge. No reading means `dead`.
                TailRule::LexOrCos => {
                    if row.bm25_rank.is_none() {
                        match row.cos.filter(|c| c.is_finite()) {
                            None => {
                                if out.dead.is_none() {
                                    out.dead = Some((name.to_string(), row.path.clone()));
                                }
                            }
                            Some(c) => {
                                if out.edge.is_none_or(|e| c < e) {
                                    out.edge = Some(c);
                                    out.edge_row = Some((name.to_string(), row.path.clone()));
                                }
                            }
                        }
                    }
                }
                TailRule::Cos => match row.cos.filter(|c| c.is_finite()) {
                    None => {
                        if out.dead.is_none() {
                            out.dead = Some((name.to_string(), row.path.clone()));
                        }
                    }
                    Some(c) => {
                        if out.edge.is_none_or(|e| c < e) {
                            out.edge = Some(c);
                            out.edge_row = Some((name.to_string(), row.path.clone()));
                        }
                    }
                },
                TailRule::CosDrop => match row.cos.filter(|c| c.is_finite()) {
                    None => {
                        if out.dead.is_none() {
                            out.dead = Some((name.to_string(), row.path.clone()));
                        }
                    }
                    Some(c) => {
                        let drop = best - c;
                        if out.edge.is_none_or(|e| drop > e) {
                            out.edge = Some(drop);
                            out.edge_row = Some((name.to_string(), row.path.clone()));
                        }
                    }
                },
            }
        }
    }
    out
}

/// Rows a family cuts at constant `p`, by label. At an admissible constant `kept_cut` is
/// zero by construction, so nonzero is printed as a fault.
pub struct TailPayoff {
    pub filler_cut: usize,
    pub kept_cut: usize,
}

pub fn tail_payoff(rule: TailRule, lists: &[&QueryEvidence], p: f64) -> TailPayoff {
    let mut out = TailPayoff {
        filler_cut: 0,
        kept_cut: 0,
    };
    for q in lists {
        let fold = rule.fold(&q.rows, p);
        for row in &q.rows[fold..] {
            if row.keep {
                out.kept_cut += 1;
            } else {
                out.filler_cut += 1;
            }
        }
    }
    out
}

/// The tail bake-off's labelled-corpus reading (GH #206): each family's constraint and its
/// payoff at the edge. Judged over the positives, since the query bar already cuts the
/// negatives (GH #201).
pub struct TailBench {
    pub keep_rows: usize,
    pub filler_rows: usize,
    /// Positives whose keep-prefix is the whole served list.
    pub saturated: usize,
    /// The oracle ceiling: rows below each list's last keep row, what a label-placed fold
    /// would cut.
    pub oracle: usize,
    pub families: Vec<TailFamilyReading>,
    /// Rows the lexical fold would cut on the negatives' lists.
    pub neg_lexical_cut: usize,
    pub neg_rows: usize,
}

pub struct TailFamilyReading {
    pub rule: TailRule,
    pub constraint: TailConstraint,
    /// Payoff at the constraint's edge; `None` when dead or unconstrained.
    pub payoff: Option<TailPayoff>,
}

pub fn score_search_tail(ev: &SearchEvidence) -> TailBench {
    let positives: Vec<&QueryEvidence> = ev.positives.iter().collect();
    let keep_rows = positives
        .iter()
        .flat_map(|q| q.rows.iter())
        .filter(|r| r.keep)
        .count();
    let filler_rows = positives
        .iter()
        .flat_map(|q| q.rows.iter())
        .filter(|r| !r.keep)
        .count();
    let saturated = positives
        .iter()
        .filter(|q| !q.rows.is_empty() && keep_prefix(&q.rows).len() == q.rows.len())
        .count();
    let oracle = positives
        .iter()
        .map(|q| q.rows.len() - keep_prefix(&q.rows).len())
        .sum();
    let families = TailRule::ALL
        .iter()
        .map(|&rule| {
            let constraint = tail_constraint(
                rule,
                positives.iter().map(|q| {
                    (
                        q.query.as_str(),
                        q.rows.as_slice(),
                        keep_prefix(&q.rows).len(),
                    )
                }),
            );
            let payoff = match rule {
                TailRule::Lexical => (constraint.lexical_violations == 0)
                    .then(|| tail_payoff(rule, &positives, f64::NAN)),
                _ => match (&constraint.dead, constraint.edge) {
                    (None, Some(edge)) => Some(tail_payoff(rule, &positives, edge)),
                    _ => None,
                },
            };
            TailFamilyReading {
                rule,
                constraint,
                payoff,
            }
        })
        .collect();
    let neg_lexical_cut = ev
        .negatives
        .iter()
        .map(|q| q.rows.len() - TailRule::Lexical.fold(&q.rows, f64::NAN))
        .sum();
    TailBench {
        keep_rows,
        filler_rows,
        saturated,
        oracle,
        families,
        neg_lexical_cut,
        neg_rows: ev.negatives.iter().map(|q| q.rows.len()).sum(),
    }
}

pub fn print_search_tail(bench: &TailBench) {
    println!(
        "  search tail bake-off (D2 per-hit — GH #206; the labels are GH #206's tail_relevant)"
    );
    for line in [
        "the rule under audition: end the DEFAULT VIEW at the first served row failing a",
        "          per-hit evidence test — a PREFIX CUT (D1), so a filler row above a keep row",
        "          must pass too or the keep row folds with it. Constraint re-derived per run",
        "          over the keep-prefixes; payoff read at the constraint's own edge. \"No tail",
        "          fold\" is an admissible winner (the GH #200 outcome, on search's side).",
    ] {
        println!("    {line}");
    }
    println!(
        "    served rows over the positives: {} keep / {} filler by label; {} list(s) saturated \
         (keep to the last served row)",
        bench.keep_rows, bench.filler_rows, bench.saturated
    );
    println!(
        "    oracle ceiling: a fold placed by the labels themselves (each list's last keep row) \
         would cut {} of {} — every payoff below is read against this",
        bench.oracle, bench.filler_rows
    );
    for f in &bench.families {
        let constraint = match f.rule {
            TailRule::Lexical => {
                if f.constraint.lexical_violations == 0 {
                    "admissible (no dense-only keep-prefix row)".to_string()
                } else {
                    let (q, p) = f.constraint.dead.as_ref().expect("violation names a row");
                    format!(
                        "✗ {} keep-prefix row(s) are dense-only — first: {} → {}",
                        f.constraint.lexical_violations,
                        truncate(q, 28),
                        p
                    )
                }
            }
            _ => match (&f.constraint.dead, f.constraint.edge) {
                (Some((q, p)), _) => {
                    format!(
                        "DEAD — {} → {} has no cosine to clear any bar",
                        truncate(q, 28),
                        p
                    )
                }
                (None, None) => "unconstrained (no keep-prefix row engages the test)".to_string(),
                (None, Some(edge)) => {
                    let (q, p) = f
                        .constraint
                        .edge_row
                        .as_ref()
                        .map(|(q, p)| (truncate(q, 28), p.clone()))
                        .unwrap_or_default();
                    match f.rule {
                        TailRule::CosDrop => format!("δ ≥ {edge:.3}  (set by {q} → {p})"),
                        _ => format!("c ≤ {edge:.3}  (set by {q} → {p})"),
                    }
                }
            },
        };
        let payoff = match &f.payoff {
            None => "—".to_string(),
            Some(p) if p.kept_cut > 0 => format!(
                "[FAULT] cuts {} keep row(s) at its own edge — the constraint arithmetic is wrong",
                p.kept_cut
            ),
            Some(p) => format!("cuts {} of {} filler rows", p.filler_cut, bench.filler_rows),
        };
        println!("    {:<24} {:<58} {}", f.rule.label(), constraint, payoff);
    }
    println!(
        "    negatives context: the lexical fold alone would cut {}/{} of their junk rows — the \
         query-level bar already cuts all of them (GH #201), so a tail rule is judged on what it \
         buys ABOVE that bar",
        bench.neg_lexical_cut, bench.neg_rows
    );
}

/// The tail bake-off's JSON (`search_tail`). The served rows are in `search_evidence`, so
/// any other constant is re-derivable from the row.
pub fn tail_json(bench: &TailBench) -> serde_json::Value {
    let round = |v: f64| (v * 1e4).round() / 1e4;
    serde_json::json!({
        "keep_rows": bench.keep_rows,
        "filler_rows": bench.filler_rows,
        "saturated_lists": bench.saturated,
        "oracle": bench.oracle,
        "neg_lexical_cut": bench.neg_lexical_cut,
        "neg_rows": bench.neg_rows,
        "families": bench.families.iter().map(|f| serde_json::json!({
            "rule": f.rule.label(),
            "dead": f.constraint.dead.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "edge": f.constraint.edge.map(round),
            "edge_row": f.constraint.edge_row.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "lexical_violations": f.constraint.lexical_violations,
            "filler_cut_at_edge": f.payoff.as_ref().map(|p| p.filler_cut),
            "kept_cut_at_edge": f.payoff.as_ref().map(|p| p.kept_cut),
        })).collect::<Vec<_>>(),
    })
}

/// One family's dense-fixture reading (GH #206), over whole title lists: every row is a real
/// match, so the constraint is "truncate nothing". `lexical_cut` is the lexical fold's cost.
pub struct TailFamilyTransfer {
    pub rule: TailRule,
    pub constraint: TailConstraint,
    pub lexical_cut: usize,
}

pub fn dense_tail_families(titles: &[SearchProbe]) -> Vec<TailFamilyTransfer> {
    TailRule::ALL
        .iter()
        .map(|&rule| TailFamilyTransfer {
            rule,
            constraint: tail_constraint(
                rule,
                titles
                    .iter()
                    .map(|t| (t.query.as_str(), t.rows.as_slice(), t.rows.len())),
            ),
            lexical_cut: match rule {
                TailRule::Lexical => titles
                    .iter()
                    .map(|t| t.rows.len() - rule.fold(&t.rows, f64::NAN))
                    .sum(),
                _ => 0,
            },
        })
        .collect()
}

/// Print the dense fixture's tail-transfer reading (GH #206).
pub fn print_dense_tail(titles: &[SearchProbe]) {
    println!(
        "  tail        the per-hit tail bench on this geometry (GH #206): every served row of a"
    );
    println!(
        "              title query is a real match, so a rule that folds one is disqualified —"
    );
    println!("              the GH #200 absolute, on search's side");
    for f in dense_tail_families(titles) {
        let reading = match f.rule {
            TailRule::Lexical => {
                if f.lexical_cut == 0 {
                    "cuts 0 title rows".to_string()
                } else {
                    let (q, p) = f.constraint.dead.as_ref().expect("cut names a row");
                    format!(
                        "✗ cuts {} title row(s) — first: {} → {}",
                        f.lexical_cut,
                        truncate(q, 24),
                        p
                    )
                }
            }
            _ => match (&f.constraint.dead, f.constraint.edge) {
                (Some((q, p)), _) => {
                    format!("DEAD — {} → {} has no cosine", truncate(q, 24), p)
                }
                (None, None) => "unconstrained (no row engages the test)".to_string(),
                (None, Some(edge)) => {
                    let (q, p) = f
                        .constraint
                        .edge_row
                        .as_ref()
                        .map(|(q, p)| (truncate(q, 24), p.clone()))
                        .unwrap_or_default();
                    match f.rule {
                        TailRule::CosDrop => {
                            format!("needs δ ≥ {edge:.3} to fold nothing (set by {q} → {p})")
                        }
                        _ => format!("needs c ≤ {edge:.3} to fold nothing (set by {q} → {p})"),
                    }
                }
            },
        };
        println!("              {:<24} {}", f.rule.label(), reading);
    }
}

/// The dense tail-transfer reading as JSON, under the dense row's `search_transfer`.
pub fn dense_tail_json(titles: &[SearchProbe]) -> serde_json::Value {
    let round = |v: f64| (v * 1e4).round() / 1e4;
    serde_json::json!(dense_tail_families(titles)
        .iter()
        .map(|f| serde_json::json!({
            "rule": f.rule.label(),
            "dead": f.constraint.dead.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "edge": f.constraint.edge.map(round),
            "edge_row": f.constraint.edge_row.as_ref().map(|(q, p)| serde_json::json!([q, p])),
            "lexical_cut": f.lexical_cut,
        }))
        .collect::<Vec<_>>())
}

/// The tail bake-off's cross-bench join (GH #206): a family survives only if some constant
/// is admissible on both benches and still cuts labelled filler at the joint edge.
pub fn print_tail_join(ev: &SearchEvidence, orth: &TailBench, titles: &[SearchProbe]) {
    println!("\n{}", "=".repeat(78));
    println!("search tail — the cross-bench join (GH #206; both corpora, one run)");
    let positives: Vec<&QueryEvidence> = ev.positives.iter().collect();
    let dense = dense_tail_families(titles);
    let mut winner = false;
    for (orth_f, dense_f) in orth.families.iter().zip(&dense) {
        let rule = orth_f.rule;
        let verdict =
            match rule {
                TailRule::Lexical => {
                    if orth_f.constraint.lexical_violations > 0 {
                        format!(
                        "✗ inadmissible on the labelled corpus ({} keep-prefix row(s) dense-only)",
                        orth_f.constraint.lexical_violations
                    )
                    } else if dense_f.lexical_cut > 0 {
                        format!(
                            "✗ disqualified on the dense fixture (cuts {} title rows)",
                            dense_f.lexical_cut
                        )
                    } else {
                        let cut = orth_f.payoff.as_ref().map(|p| p.filler_cut).unwrap_or(0);
                        if cut == 0 {
                            "✓ admissible and VACUOUS — cuts 0 of the labelled filler, so it buys \
                         nothing the query bar has not already bought"
                                .to_string()
                        } else {
                            winner = true;
                            format!(
                                "✓ admissible on both benches — cuts {cut} of the {} an oracle \
                                 fold reaches",
                                orth.oracle
                            )
                        }
                    }
                }
                _ => {
                    let orth_dead = orth_f.constraint.dead.is_some();
                    let dense_dead = dense_f.constraint.dead.is_some();
                    if orth_dead || dense_dead {
                        format!(
                            "✗ DEAD on the {} bench (a required row has no cosine)",
                            if orth_dead { "labelled" } else { "dense" }
                        )
                    } else {
                        // The tighter bench binds; `None` leaves the constant free.
                        let joint = match rule {
                            TailRule::CosDrop => {
                                match (orth_f.constraint.edge, dense_f.constraint.edge) {
                                    (Some(a), Some(b)) => Some(a.max(b)),
                                    (a, b) => a.or(b),
                                }
                            }
                            _ => match (orth_f.constraint.edge, dense_f.constraint.edge) {
                                (Some(a), Some(b)) => Some(a.min(b)),
                                (a, b) => a.or(b),
                            },
                        };
                        match joint {
                            None => {
                                "degenerates to the lexical fold (no row on either bench engages \
                                 the test) — see that family's verdict"
                                    .to_string()
                            }
                            Some(edge) => {
                                let payoff = tail_payoff(rule, &positives, edge);
                                if payoff.kept_cut > 0 {
                                    format!(
                                    "[FAULT] cuts {} keep row(s) at the joint edge {edge:.3} — \
                                     the join arithmetic is wrong",
                                    payoff.kept_cut
                                )
                                } else if payoff.filler_cut == 0 {
                                    format!(
                                    "✓ admissible to {} {edge:.3} and VACUOUS — cuts 0 labelled \
                                     filler there",
                                    if rule == TailRule::CosDrop { "δ ≥" } else { "c ≤" }
                                )
                                } else {
                                    winner = true;
                                    format!(
                                    "✓ admissible at {} {edge:.3} on both benches — cuts {} of \
                                     the {} an oracle fold reaches",
                                    if rule == TailRule::CosDrop { "δ ≥" } else { "c ≤" },
                                    payoff.filler_cut,
                                    orth.oracle
                                )
                                }
                            }
                        }
                    }
                }
            };
        println!("  {:<24} {}", rule.label(), verdict);
    }
    if winner {
        println!(
            "  → the ✓ family/families above survive both corpora at their joint edges. That is \
             ADMISSIBILITY, not a shipping order: a joint edge sits AT a bench's own binding row \
             (zero headroom — the constant placement the house sizing method forbids), each payoff \
             reads against the oracle ceiling above, and a shipped constant owes process rule 5's \
             real-vault reading besides (`make calibrate VAULT=<vault> ARGS=--search`, the tail block). The ruling of \
             record lives in docs/evals.md."
        );
    } else {
        println!(
            "  → NO family survives both benches with a nonzero payoff: the incumbent — no \
             per-hit tail fold — stands, the GH #200 outcome on search's side. The filler the \
             complaint names is above every admissible fold, so the honesty still rides on the \
             query-level bar and the copy."
        );
    }
}
