//! The per-hit search tail bake-off (GH #206): the candidate prefix-cut families, their
//! constraints re-derived on the labelled corpus and on the dense fixture's title lists,
//! and the cross-bench join. The ruling of record is **no tail fold ships**
//! (docs/evals.md); every constraint is still re-derived each run.

use crate::common::truncate;
use crate::dense::SearchProbe;
use crate::evidence::{QueryEvidence, SearchEvidence, ServedRow};

/// A candidate **per-hit tail rule** family (GH #206) — the fold where a real
/// query's evidence runs out, D2's per-hit half. Every family folds as a
/// **prefix cut** (invariants.md D1): the default view ends at the first served
/// row failing the family's test, and every row below folds with it — a passing
/// row under a failing one folds too, because a fold that punched holes in the
/// fused order would let row order and fold visibly disagree.
///
/// The consequence that does all the work below: a filler row served *above* a
/// keep-labelled row must pass the test too, or the keep row under it folds. So
/// each family's constraint is read over the **keep-prefix** — every row at or
/// above a list's deepest keep row — not over the keep rows alone.
#[derive(Clone, Copy, PartialEq)]
pub enum TailRule {
    /// Fold at the first row the lexical half never ranked (`bm25_rank: None`)
    /// — the dense-only fold, the signal GH #201 measured (0 of 410 positive
    /// rows vs 20 of 50 negative ones). Parameterless, so nothing
    /// distributional to place — but **not** scale-free: `bm25_rank` is a rank
    /// in a pool-truncated list (`search::pool_size`), so "never ranked" is
    /// partly a fact about pool depth against vault size. `make calibrate
    /// VAULT=<vault> ARGS=--search` measures that rather than assuming it
    /// (process rule 5's posture, owed even without a constant).
    Lexical,
    /// Fold at the first row that is dense-only **and** under a per-hit cosine
    /// bar — the shipped query rule's shape (lexical OR semantic) read per hit.
    LexOrCos,
    /// Fold at the first row under a per-hit cosine bar, lexical rank ignored
    /// (a row the dense half never ranked fails at any bar). The single-signal
    /// baseline the two-signal family must beat — the pure-cosine window at hit
    /// granularity, included to be retired the way GH #201 retired its
    /// query-level twin.
    Cos,
    /// Fold at the first row whose cosine sits more than δ under the list's own
    /// best — the "drop-off" shape, scale-adaptive in the query and still
    /// distributional in δ.
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

    /// Whether `row` passes this family's per-hit test at constant `p`
    /// (ignored by `Lexical`; `best` is the row's list's best served cosine,
    /// read only by `CosDrop`).
    pub fn passes(self, row: &ServedRow, p: f64, best: f64) -> bool {
        match self {
            TailRule::Lexical => row.bm25_rank.is_some(),
            TailRule::LexOrCos => row.bm25_rank.is_some() || row.cos.is_some_and(|c| c >= p),
            TailRule::Cos => row.cos.is_some_and(|c| c >= p),
            TailRule::CosDrop => row.cos.is_some_and(|c| c >= best - p),
        }
    }

    /// Where this family folds `rows` at constant `p`: the index of the first
    /// failing row, `rows.len()` when nothing folds. The prefix-cut definition
    /// lives here and nowhere else.
    pub fn fold(self, rows: &[ServedRow], p: f64) -> usize {
        let best = best_served_cos(rows);
        rows.iter()
            .position(|r| !self.passes(r, p, best))
            .unwrap_or(rows.len())
    }
}

/// The best served cosine of one list — [`TailRule::CosDrop`]'s reference
/// point. Read off the *served* rows, not the vault-wide dense top-1: the drop
/// rule is a claim about a list's own shape.
pub fn best_served_cos(rows: &[ServedRow]) -> f64 {
    rows.iter()
        .filter_map(|r| r.cos)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// The keep-prefix of one served list: every row at or above its deepest
/// keep-labelled row. Empty when nothing served is keep-labelled — such a list
/// constrains no rule (there is nothing a fold could wrongly hide).
pub fn keep_prefix(rows: &[ServedRow]) -> &[ServedRow] {
    match rows.iter().rposition(|r| r.keep) {
        Some(last) => &rows[..=last],
        None => &[],
    }
}

/// One family's constraint, re-derived from a set of served lists (GH #187's
/// idiom on the per-hit axis): the range of its constant, if any, at which no
/// keep-prefix row anywhere fails. Both edges carry the row that set them,
/// because a window edge is only arguable once the pair is named.
pub struct TailConstraint {
    /// A keep-prefix row that cannot pass at **any** constant — the family is
    /// dead on this bench (e.g. a row with no cosine under a cosine family).
    pub dead: Option<(String, String)>,
    /// `Cos`/`LexOrCos`: the highest admissible bar (the binding keep-prefix
    /// row's own cosine). `CosDrop`: the lowest admissible δ (the binding
    /// row's drop from its list's best). `None` = unconstrained — no
    /// keep-prefix row engages the family's test at all.
    pub edge: Option<f64>,
    /// The (query-or-title, path) that set `edge`.
    pub edge_row: Option<(String, String)>,
    /// `Lexical` only: keep-prefix rows failing its fixed test. Nonzero =
    /// inadmissible, and each failure is a keep row the fold would hide.
    pub lexical_violations: usize,
}

/// Read one family's [`TailConstraint`] over `(list name, full rows, keep-prefix
/// length)` triples, where every keep-prefix row must pass. The dense fixture
/// reuses this with whole lists as keep-prefixes: on a single-domain vault every
/// served row is a real match, so the absolute "truncate nothing" is the same
/// constraint with the keep-set saturated. The full list rides along because
/// [`TailRule::CosDrop`]'s reference is the **list's** best served cosine —
/// reading it off the prefix alone could understate a required δ when the best
/// row sits below the prefix, and the constraint must read exactly what the
/// fold would.
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
                // The cosine arms read the row's cosine through a finiteness
                // filter (PR #212 review): a NaN would sail through `c < e`
                // comparisons as silently-true-nowhere — worse, `is_none_or`
                // admits the FIRST row unconditionally, so a NaN first row
                // would seed the edge. A non-finite reading is *no reading*,
                // and the honest arm for that is `dead`, with the row named —
                // never a silent skip that reports the family unconstrained.
                TailRule::LexOrCos => {
                    if row.bm25_rank.is_none() {
                        match row.cos.filter(|c| c.is_finite()) {
                            // A served row is in at least one list, so a
                            // dense-only row carries a cosine unless it is
                            // non-finite — either way, no finite reading means
                            // this row can pass at no bar.
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

/// What a family buys at constant `p` on the labelled lists: rows cut, split by
/// label. At an admissible constant `kept_cut` is zero **by construction** —
/// the constraint above is exactly "every keep-prefix row passes" — so a
/// nonzero here is an arithmetic fault, printed as such rather than assumed
/// away.
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

/// The per-hit tail bake-off's labelled-corpus reading (GH #206): every
/// family's re-derived constraint plus its payoff at the constraint's own edge
/// — the most aggressive admissible point, since payoff is monotone in the
/// constant. Judged over the **positives**: the query-level bar already
/// answers the negatives whole (GH #201), so a tail rule ships riding on a
/// vouched verdict; the negatives' would-be reading is kept as context for
/// what the rule would buy where a query bar had missed.
pub struct TailBench {
    pub keep_rows: usize,
    pub filler_rows: usize,
    /// Positives whose keep-prefix is the whole served list — a fold has
    /// nothing to cut there under any admissible rule.
    pub saturated: usize,
    /// The **oracle** ceiling: rows below each list's last keep row — what a
    /// perfect prefix fold (one placed by the labels themselves) would cut.
    /// Every family's payoff is read against this, because "cuts N rows" means
    /// nothing until the reachable maximum is beside it.
    pub oracle: usize,
    pub families: Vec<TailFamilyReading>,
    /// Junk rows the parameterless lexical fold would cut on the negatives'
    /// served lists, of their total — the GH #201 `dense_only` reading, priced
    /// as a fold.
    pub neg_lexical_cut: usize,
    pub neg_rows: usize,
}

pub struct TailFamilyReading {
    pub rule: TailRule,
    pub constraint: TailConstraint,
    /// Payoff at the constraint's edge; `None` when the family is dead here or
    /// (for the constant families) unconstrained-and-therefore-identical to a
    /// simpler family.
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

/// The tail bake-off's JSON (`search_tail` in the orthogonal row): each
/// family's re-derived constraint and edge payoff. The served rows it was read
/// from are already in `search_evidence` per query, so any other constant is
/// re-derivable from the row — the `discovery_fold` convention.
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

/// One family's dense-fixture reading (GH #206): the same [`TailConstraint`]
/// read over **whole** title lists — every served row is a real match by
/// geometry, so the keep-prefix is the entire list and the constraint *is* the
/// absolute ("truncate nothing"). `lexical_cut` is the parameterless family's
/// visible cost here: rows its fold would remove from title queries' views.
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

/// Print the dense fixture's tail-transfer reading (GH #206) — the per-hit
/// sibling of the query bar's `search_transfer` block, and the bench that
/// carries D1's absolute for this bake-off.
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

/// The dense tail-transfer reading as JSON, nested under the dense row's
/// `search_transfer` key (additive subkey, per the row conventions).
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

/// The tail bake-off's **cross-bench join** (GH #206), printed once both
/// corpora have been read in the same run: a family ships only if some
/// constant is admissible on the labelled corpus *and* folds nothing on the
/// dense fixture *and* still cuts labelled filler there — read at the joint
/// edge, the most aggressive point both benches allow.
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
                        // Joint edge: the tighter bench binds. `None` = that bench
                        // leaves the constant free.
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
