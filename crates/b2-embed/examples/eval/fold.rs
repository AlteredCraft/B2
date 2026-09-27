//! The discovery fold bake-off (GH #200): the default disclosure boundary, priced on every
//! bench. The ruling of record is that no fold ships (docs/evals.md); the window is
//! re-derived every run.

use crate::common::{cosine_of, truncate};
use crate::labels::{SimilarLabel, SimilarSet};
use crate::metrics::paths_match;
use crate::{SIM_K, Z_SCAN_LIMIT};
use b2_core::vault::Vault;
use std::collections::HashMap;

/// The reciprocity depths shown in the headline table: [`SIM_K`] and one either side.
/// Display only; the window is derived from the whole sweep.
pub const FOLD_MUTUAL_K: [usize; 3] = [3, 5, 10];

/// How far the reciprocity depth is swept. Half the orthogonal corpus's note count: past
/// that, most of the vault is "mutually near" and the rule is vacuous.
pub const FOLD_K_SWEEP: usize = 15;

/// A default disclosure rule: how much of the ranked list the default view vouches for
/// (D1, GH #200). Returns a depth, never a set, so a non-prefix rule is unrepresentable.
#[derive(Clone, Copy, PartialEq)]
pub enum FoldRule {
    /// Candidate 3, the incumbent: no fold, GH #197's always-serve.
    NoFold,
    /// Candidate 1: the longest prefix whose candidates each have the anchor in their own
    /// top `k`. Rank-based, so no cosine or z constant, but `k` measures scale-dependent.
    Mutual(usize),
}

impl FoldRule {
    /// The rule's one spelling in every table, row and window line.
    pub fn label(self) -> String {
        match self {
            FoldRule::NoFold => "no fold".to_string(),
            FoldRule::Mutual(k) => format!("mutual-{k}"),
        }
    }

    /// How many of `rows` this rule's default view vouches for.
    pub fn fold(self, rows: &[FoldRow]) -> usize {
        match self {
            FoldRule::NoFold => rows.len(),
            FoldRule::Mutual(k) => rows.iter().take_while(|r| r.reciprocal_at(k)).count(),
        }
    }
}

/// One served candidate on one anchor's list, with what a rule judges.
pub struct FoldRow {
    pub path: String,
    pub cos: f64,
    pub mate: bool,
    /// The anchor's rank in this candidate's full-depth list. A rank rather than a per-`k`
    /// flag, so one pass prices every `k`.
    pub recip_rank: Option<usize>,
}

impl FoldRow {
    /// Whether the anchor sits in this candidate's top `k`. Not sufficient for being above
    /// the fold, which is the longest reciprocal prefix.
    pub fn reciprocal_at(&self, k: usize) -> bool {
        self.recip_rank.is_some_and(|r| r <= k)
    }
}

/// One anchor's served list, read once so every rule is judged on identical rows.
pub struct FoldAnchor {
    pub anchor: String,
    /// A labelled loner: its correct default view is empty above the fold.
    pub negative: bool,
    pub expected: Vec<String>,
    /// The served prefix (`limit` = [`SIM_K`]) in rank order.
    pub rows: Vec<FoldRow>,
}

impl FoldAnchor {
    /// Labelled mates the served prefix reaches, as `(mate, rank)`. A mate past `limit` is
    /// always-serve's miss, not a fold's cost.
    pub fn served_mates(&self) -> Vec<(String, usize)> {
        self.expected
            .iter()
            .filter_map(|m| {
                self.rows
                    .iter()
                    .position(|r| paths_match(&r.path, m))
                    .map(|p| (m.clone(), p + 1))
            })
            .collect()
    }
}

/// What one rule reads on one bench, each quantity with its named list (process rule 1).
pub struct FoldReading {
    pub rule: FoldRule,
    /// Served labelled mates hidden by default, `(anchor, mate, rank)`: the fold's own cost,
    /// judged at zero (GH #200). Reported only until a fold ships.
    pub mates_folded: Vec<(String, String, usize)>,
    pub mates_above: usize,
    /// Strangers above the fold on positive anchors. Ungated, like the strangers count.
    pub strangers_above: Vec<(String, String)>,
    /// Loner anchors whose default view is empty.
    pub neg_empty: usize,
    /// Non-negative anchors whose default view is empty. Any entry disqualifies on the dense
    /// fixture.
    pub dark_panes: Vec<String>,
    /// Cards above the fold, summed.
    pub cards_above: usize,
}

/// The admissible-`k` window for candidate 1, re-derived each run. Shippable only if
/// `keep_min` (smallest `k` folding no mate and darkening no pane) ≤ `loner_max` (largest
/// `k` emptying every loner's view).
pub struct FoldWindow {
    pub keep_min: Option<usize>,
    pub loner_max: Option<usize>,
    /// The smallest `k` whose fold equals always-serve everywhere: the rule goes vacuous.
    pub vacuous_at: Option<usize>,
    /// The best loner claim, as `(empty, largest k still achieving it)`.
    pub loner_best: Option<(usize, usize)>,
    /// The largest `k` that still darkens a pane: the hard floor under any window (D1).
    pub dark_below: Option<usize>,
}

impl FoldWindow {
    /// Whether some swept `k` satisfies both bounds. Never true on a bench with no loner,
    /// which supplies only the lower bound.
    pub fn open(&self) -> bool {
        matches!((self.keep_min, self.loner_max), (Some(a), Some(b)) if a <= b)
    }
}

/// One bench's complete bake-off reading.
pub struct FoldBench {
    pub corpus: &'static str,
    pub anchors: Vec<FoldAnchor>,
    /// The headline rules: always-serve plus [`FOLD_MUTUAL_K`]'s detail depths.
    pub readings: Vec<FoldReading>,
    /// `(k, reading)` across the swept range, which the window is derived from.
    pub sweep: Vec<(usize, FoldReading)>,
    pub window: FoldWindow,
    pub neg_n: usize,
    /// Labelled mates never served at `limit`: always-serve's miss, charged to no rule.
    pub mates_unserved: usize,
    /// The median full-depth candidate pool. A `k` only compares across corpora as a
    /// fraction of it.
    pub pool_median: usize,
    /// Authored edges: candidate 2's calibration population. The eval corpora are link-free,
    /// so candidate 2 is only priced by `make calibrate` on a real vault.
    pub authored_edges: usize,
}

/// Every note's full-depth candidate list as `path → rank`, the reciprocity lookup. Over
/// every note, since most candidates are not anchors.
pub fn reciprocity_ranks(
    vault: &Vault,
) -> Result<HashMap<String, HashMap<String, usize>>, Box<dyn std::error::Error>> {
    let mut out = HashMap::new();
    for note in vault.list_notes()? {
        let ranks = vault
            .similar(&note.path, Z_SCAN_LIMIT)?
            .into_iter()
            .enumerate()
            .map(|(i, c)| (c.path, i + 1))
            .collect::<HashMap<String, usize>>();
        out.insert(note.path, ranks);
    }
    Ok(out)
}

/// Score the fold bake-off on one built vault. `sweep_all` reads every note as an anchor;
/// otherwise only the labelled anchors, matching
/// [`score_similar`](crate::discovery::score_similar).
pub fn score_fold(
    vault: &Vault,
    set: &SimilarSet,
    corpus: &'static str,
    sweep_all: bool,
) -> Result<FoldBench, Box<dyn std::error::Error>> {
    let recip = reciprocity_ranks(vault)?;

    let mut anchors: Vec<(String, Option<&SimilarLabel>)> = Vec::new();
    if sweep_all {
        for note in vault.list_notes()? {
            let label = set
                .anchors
                .iter()
                .find(|l| paths_match(&note.path, &l.anchor));
            anchors.push((note.path, label));
        }
    } else {
        for label in &set.anchors {
            anchors.push((label.anchor.clone(), Some(label)));
        }
    }

    // Outbound only: `neighbors` returns an edge from both endpoints, so counting all would
    // double it (PR #204).
    let mut authored_edges = 0;
    for note in vault.list_notes()? {
        authored_edges += vault
            .neighbors(&note.path)?
            .iter()
            .filter(|n| n.direction == "outbound")
            .count();
    }

    let mut rows_by_anchor: Vec<FoldAnchor> = Vec::new();
    let mut neg_n = 0;
    for (anchor, label) in anchors {
        let expected: Vec<String> = label.map(|l| l.expected.clone()).unwrap_or_default();
        // An unlabelled anchor on the dense sweep is not a negative.
        let negative = label.is_some() && expected.is_empty();
        if negative {
            neg_n += 1;
        }
        let rows = vault
            .similar(&anchor, SIM_K)?
            .into_iter()
            .map(|c| FoldRow {
                mate: expected.iter().any(|e| paths_match(&c.path, e)),
                recip_rank: recip.get(&c.path).and_then(|ranks| {
                    ranks
                        .iter()
                        .find(|(p, _)| paths_match(p, &anchor))
                        .map(|(_, r)| *r)
                }),
                cos: cosine_of(c.score),
                path: c.path,
            })
            .collect();
        rows_by_anchor.push(FoldAnchor {
            anchor,
            negative,
            expected,
            rows,
        });
    }

    let mates_unserved = rows_by_anchor
        .iter()
        .map(|a| a.expected.len() - a.served_mates().len())
        .sum();
    let readings = std::iter::once(FoldRule::NoFold)
        .chain(FOLD_MUTUAL_K.iter().map(|&k| FoldRule::Mutual(k)))
        .map(|rule| read_fold(&rows_by_anchor, rule))
        .collect();
    let sweep: Vec<(usize, FoldReading)> = (1..=FOLD_K_SWEEP)
        .map(|k| (k, read_fold(&rows_by_anchor, FoldRule::Mutual(k))))
        .collect();
    let window = FoldWindow {
        keep_min: sweep
            .iter()
            .find(|(_, r)| r.mates_folded.is_empty() && r.dark_panes.is_empty())
            .map(|(k, _)| *k),
        loner_max: (neg_n > 0)
            .then(|| {
                sweep
                    .iter()
                    .filter(|(_, r)| r.neg_empty == neg_n)
                    .map(|(k, _)| *k)
                    .next_back()
            })
            .flatten(),
        vacuous_at: sweep
            .iter()
            .find(|(_, r)| {
                rows_by_anchor
                    .iter()
                    .all(|a| r.rule.fold(&a.rows) == a.rows.len())
            })
            .map(|(k, _)| *k),
        loner_best: (neg_n > 0)
            .then(|| {
                let best = sweep.iter().map(|(_, r)| r.neg_empty).max()?;
                let last = sweep
                    .iter()
                    .filter(|(_, r)| r.neg_empty == best)
                    .map(|(k, _)| *k)
                    .next_back()?;
                Some((best, last))
            })
            .flatten(),
        dark_below: sweep
            .iter()
            .filter(|(_, r)| !r.dark_panes.is_empty())
            .map(|(k, _)| *k)
            .next_back(),
    };

    let pool_median = {
        let mut sizes: Vec<usize> = recip.values().map(|r| r.len()).collect();
        sizes.sort_unstable();
        sizes.get(sizes.len() / 2).copied().unwrap_or(0)
    };

    Ok(FoldBench {
        corpus,
        pool_median,
        anchors: rows_by_anchor,
        readings,
        sweep,
        window,
        neg_n,
        mates_unserved,
        authored_edges,
    })
}

/// Judge one rule against one bench's rows.
pub fn read_fold(anchors: &[FoldAnchor], rule: FoldRule) -> FoldReading {
    let mut reading = FoldReading {
        rule,
        mates_folded: Vec::new(),
        mates_above: 0,
        strangers_above: Vec::new(),
        neg_empty: 0,
        dark_panes: Vec::new(),
        cards_above: 0,
    };
    for a in anchors {
        let fold = rule.fold(&a.rows);
        reading.cards_above += fold;
        if fold == 0 {
            if a.negative {
                reading.neg_empty += 1;
            } else if !a.rows.is_empty() {
                reading.dark_panes.push(a.anchor.clone());
            }
        }
        for (mate, rank) in a.served_mates() {
            if rank <= fold {
                reading.mates_above += 1;
            } else {
                reading.mates_folded.push((a.anchor.clone(), mate, rank));
            }
        }
        if !a.negative && !a.expected.is_empty() {
            for row in a.rows.iter().take(fold) {
                if !row.mate {
                    reading
                        .strangers_above
                        .push((a.anchor.clone(), row.path.clone()));
                }
            }
        }
    }
    reading
}

/// The bake-off's printed readout: headline rules, the `k` sweep, and per-anchor folds.
pub fn print_fold_bench(bench: &FoldBench) {
    println!("\n{}", "=".repeat(78));
    println!(
        "discovery fold bake-off — the default disclosure boundary on the {} corpus \
         (GH #200, Phase B; invariants.md D1)",
        bench.corpus
    );
    println!(
        "  {} anchors read at limit={SIM_K} over a median {}-candidate pool; a fold is a PREFIX of \
         the served order and everything below it stays served (D1)",
        bench.anchors.len(),
        bench.pool_median
    );
    println!(
        "\n  {:<10} {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
        "rule", "cards above", "mates folded", "strangers above", "loners empty", "dark panes"
    );
    for r in &bench.readings {
        println!(
            "  {:<10} {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
            r.rule.label(),
            r.cards_above,
            r.mates_folded.len(),
            r.strangers_above.len(),
            format!("{}/{}", r.neg_empty, bench.neg_n),
            r.dark_panes.len(),
        );
    }
    println!(
        "  (mates folded: served within limit={SIM_K} but below the fold — the fold's OWN cost, and \
         the quantity GH #200 judges a candidate at 0 on. Reported, not gated: nothing folds today, \
         and GH #202 shipped no fold either, so the exit gate has nothing here to watch until one \
         does. {} further labelled mate(s) \
         rank past limit={SIM_K} under every rule, always-serve included, so no fold is charged \
         for them.)",
        bench.mates_unserved
    );

    // The named cost of every rule that has one.
    for r in &bench.readings {
        if r.mates_folded.is_empty() {
            continue;
        }
        println!("  {} folds labelled mates:", r.rule.label());
        for (anchor, mate, rank) in &r.mates_folded {
            println!("             rank {rank}   {anchor} → {mate}");
        }
    }

    // The swept window, derived from this run.
    println!(
        "\n  mutual-k sweep (k = 1..{FOLD_K_SWEEP}; the window a shippable k would have to sit in)"
    );
    println!(
        "  {:>3}  {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
        "k", "cards above", "mates folded", "strangers above", "loners empty", "dark panes"
    );
    for (k, r) in &bench.sweep {
        println!(
            "  {k:>3}  {:>11}  {:>12}  {:>15}  {:>12}  {:>10}",
            r.cards_above,
            r.mates_folded.len(),
            r.strangers_above.len(),
            format!("{}/{}", r.neg_empty, bench.neg_n),
            r.dark_panes.len(),
        );
    }
    let frac = |k: usize| {
        if bench.pool_median == 0 {
            String::new()
        } else {
            format!(
                " (= {:.2} of the {}-candidate pool)",
                k as f64 / bench.pool_median as f64,
                bench.pool_median
            )
        }
    };
    match bench.window.keep_min {
        Some(k) => println!(
            "    k ≥ {k}{}   folds no labelled mate and darkens no pane on this corpus",
            frac(k)
        ),
        None => println!("    (no swept k folds zero mates with zero dark panes on this corpus)"),
    }
    match bench.window.loner_max {
        Some(k) => println!(
            "    k ≤ {k}{}   folds every labelled loner's default view to empty",
            frac(k)
        ),
        None if bench.neg_n == 0 => {
            println!(
                "    (no labelled loner on this corpus — the upper bound is the other bench's)"
            )
        }
        None => println!("    (no swept k empties every loner's default view on this corpus)"),
    }
    if let Some(k) = bench.window.vacuous_at {
        println!(
            "    k ≥ {k}   the fold equals always-serve on every anchor here — the rule stops \
             claiming anything"
        );
    }
    if let Some(k) = bench.window.dark_below {
        println!(
            "    k ≤ {k}   darkens at least one pane here — disqualified outright where the labels \
             say everything relates (D1's absolute)"
        );
    }
    if let Some((best, last)) = bench.window.loner_best {
        println!(
            "    best loner claim: {best}/{} empty, holding to k = {last}",
            bench.neg_n
        );
    }
    // Name which bound failed; a bench with no loner can only supply the lower bound.
    println!(
        "    → window {}",
        match (bench.neg_n, bench.window.keep_min, bench.window.loner_max) {
            (0, Some(k), _) => format!(
                "UNDECIDABLE on this corpus alone — no loner here, so it supplies only the lower \
                 bound (k ≥ {k}) and the absolute above; the upper bound is the other bench's"
            ),
            (0, None, _) => "UNDECIDABLE on this corpus alone — no loner here, and no swept k is \
                 even clean on the labels"
                .to_string(),
            (_, _, None) => format!(
                "EMPTY — no swept k empties every loner's default view, so the fold never fully \
                 makes the claim it exists to make ({})",
                bench
                    .window
                    .keep_min
                    .map(|k| format!("its lower bound here is k ≥ {k}"))
                    .unwrap_or_else(|| "and no swept k is clean on the labels either".into())
            ),
            (_, Some(a), Some(b)) if a <= b => format!(
                "OPEN on this corpus at k ∈ [{a}, {b}] — the other benches are the rest \
                     of the claim"
            ),
            (_, Some(a), Some(b)) => format!(
                "EMPTY — the k that stops folding labelled mates (≥ {a}) is past the k that still \
                 empties every loner's view (≤ {b}), so no constant separates them"
            ),
            (_, None, Some(b)) => format!(
                "EMPTY — no swept k is clean on the labels at all, while the loner claim holds \
                 only to k ≤ {b}"
            ),
        }
    );

    println!("\n  per anchor — served / above the fold, by rule:");
    print!("  {:<40} {:>7}", "anchor", "served");
    for r in &bench.readings {
        if r.rule != FoldRule::NoFold {
            print!(" {:>9}", r.rule.label());
        }
    }
    println!();
    for a in &bench.anchors {
        print!(
            "  {:<40} {:>7}",
            format!(
                "{}{}",
                truncate(&a.anchor, 36),
                if a.negative { " [loner]" } else { "" }
            ),
            a.rows.len()
        );
        for r in &bench.readings {
            if r.rule != FoldRule::NoFold {
                let fold = r.rule.fold(&a.rows);
                let lost = a
                    .served_mates()
                    .iter()
                    .filter(|(_, rank)| *rank > fold)
                    .count();
                print!(
                    " {:>9}",
                    if lost > 0 {
                        format!("{fold}(-{lost})")
                    } else {
                        fold.to_string()
                    }
                );
            }
        }
        println!();
    }

    // Printed on every bench: "nothing to measure it on" is a finding, not an omission.
    println!(
        "\n  candidate 2 (authored-edge reference bar): {} authored edges in this corpus — {}",
        bench.authored_edges,
        if bench.authored_edges == 0 {
            "UNPRICEABLE here (a link-free corpus offers the rule no population to calibrate from; \
             it is measured where one exists, by `make calibrate` on a real vault)"
        } else {
            "priceable — see the calibrate replay"
        }
    );
}

/// The bake-off as one JSON subtree (`discovery_fold`), with the per-anchor rows so a
/// verdict can be re-derived without re-running the model.
pub fn fold_json(bench: &FoldBench) -> serde_json::Value {
    let reading = |r: &FoldReading| {
        serde_json::json!({
            "rule": r.rule.label(),
            "cards_above": r.cards_above,
            "mates_above": r.mates_above,
            "mates_folded": r.mates_folded.iter().map(|(a, m, rank)| serde_json::json!({
                "anchor": a, "mate": m, "rank": rank,
            })).collect::<Vec<_>>(),
            "strangers_above": r.strangers_above.iter().map(|(a, p)| serde_json::json!({
                "anchor": a, "path": p,
            })).collect::<Vec<_>>(),
            "neg_empty": r.neg_empty,
            "dark_panes": r.dark_panes,
        })
    };
    serde_json::json!({
        "corpus": bench.corpus,
        "limit": SIM_K,
        "pool_median": bench.pool_median,
        "neg_n": bench.neg_n,
        "mates_unserved": bench.mates_unserved,
        // Recorded even at zero, so the row doesn't read as untried.
        "authored_edges": bench.authored_edges,
        "rules": bench.readings.iter().map(reading).collect::<Vec<_>>(),
        "sweep": bench.sweep.iter().map(|(k, r)| serde_json::json!({
            "k": k,
            "reading": reading(r),
        })).collect::<Vec<_>>(),
        "window": {
            "swept_to": FOLD_K_SWEEP,
            "keep_min": bench.window.keep_min,
            "loner_max": bench.window.loner_max,
            "vacuous_at": bench.window.vacuous_at,
            "dark_below": bench.window.dark_below,
            "loner_best": bench.window.loner_best.map(|(empty, k)| serde_json::json!({
                "empty": empty, "of": bench.neg_n, "holds_to_k": k,
            })),
            "open": bench.window.open(),
        },
        "anchors": bench.anchors.iter().map(|a| serde_json::json!({
            "anchor": a.anchor,
            "negative": a.negative,
            "served": a.rows.len(),
            "folds": bench.readings.iter().map(|r| serde_json::json!({
                "rule": r.rule.label(),
                "fold": r.rule.fold(&a.rows),
            })).collect::<Vec<_>>(),
            "rows": a.rows.iter().map(|row| serde_json::json!({
                "path": row.path,
                "cos": (row.cos * 1e4).round() / 1e4,
                "mate": row.mate,
                "recip_rank": row.recip_rank,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
