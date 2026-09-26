//! The discovery fold bake-off (GH #200, Phase B) — the *default disclosure boundary*,
//! priced on every bench this harness carries. The ruling of record is **no fold ships**
//! (docs/evals.md); the bake-off keeps re-deriving the window every run.

use crate::common::{cosine_of, truncate};
use crate::labels::{SimilarLabel, SimilarSet};
use crate::metrics::paths_match;
use crate::{SIM_K, Z_SCAN_LIMIT};
use b2_core::vault::Vault;
use std::collections::HashMap;

/// The reciprocity depths GH #200's candidate 1 is shown at in the headline
/// table and the per-anchor lists — [`SIM_K`] (the pane itself) with one depth
/// either side of it. They are a *display* choice only: the window below is
/// derived from the whole sweep, so no verdict turns on these three.
pub const FOLD_MUTUAL_K: [usize; 3] = [3, 5, 10];

/// How far the reciprocity depth is swept when re-deriving candidate 1's
/// admissible window each run (GH #187's idiom on the disclosure axis — a
/// measurement in the harness, never a constant frozen into a docstring). Half
/// the orthogonal corpus's note count: a `k` past that admits most of the vault
/// as "mutually near", which is the rule going vacuous rather than a depth
/// anyone would ship.
pub const FOLD_K_SWEEP: usize = 15;

/// A **default disclosure rule**: a candidate answer to "how much of the ranked
/// list does the default view vouch for?" (invariants.md D1 as redrafted;
/// GH #200). Every variant is a **prefix** rule *by construction* — it returns a
/// depth, never a set — so a rule that admits rank 5 while folding rank 2 is not
/// expressible here. D1 states that admissibility requirement; this type makes
/// violating it unrepresentable, which is the difference between a check and a
/// guarantee.
#[derive(Clone, Copy, PartialEq)]
pub enum FoldRule {
    /// **Candidate 3, the incumbent**: no fold. The default view is the whole
    /// served prefix — GH #197's always-serve ruling, admissible per D1 and the
    /// thing the other candidates have to beat.
    NoFold,
    /// **Candidate 1**: mutual-kNN reciprocity in prefix form. Candidate B is
    /// *reciprocal* for anchor A iff A sits in B's own top `k` candidates, and
    /// the default view is the ranked list's longest reciprocal prefix. Rank-
    /// based (the hubness-correction family: mutual proximity, CSLS), so it
    /// carries no cosine or z constant — though `k` itself is measured below to
    /// be scale-dependent, which is a different objection and the decisive one.
    Mutual(usize),
}

impl FoldRule {
    /// The rule's name in every table, row and window line — one spelling, so a
    /// printed verdict and a recorded row can never name the same rule
    /// differently.
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

/// One served candidate on one anchor's list, carrying everything a candidate
/// rule judges: the label, the score, and — the reciprocity signal — **where the
/// anchor sits in this candidate's own ranked list**.
pub struct FoldRow {
    pub path: String,
    pub cos: f64,
    /// A labelled mate of this anchor (always `false` on a negative or
    /// unlabelled anchor).
    pub mate: bool,
    /// The **anchor's** rank in *this candidate's* full-depth candidate list, or
    /// `None` if the anchor is not in it at all. Storing the rank rather than a
    /// per-`k` boolean is what makes the whole sweep below free: reciprocity at
    /// depth `k` is `rank ≤ k`, so every `k` is priced off one discovery pass
    /// per note instead of one per note per depth.
    pub recip_rank: Option<usize>,
}

impl FoldRow {
    /// Whether this candidate is *reciprocal* at depth `k` — i.e. the anchor
    /// sits in its own top `k`. Reciprocity is necessary but not sufficient for
    /// being above the fold: the fold is the longest reciprocal **prefix**, so a
    /// reciprocal candidate ranked after a non-reciprocal one is below it too.
    pub fn reciprocal_at(&self, k: usize) -> bool {
        self.recip_rank.is_some_and(|r| r <= k)
    }
}

/// One anchor's served list, read once and judged by every candidate rule — so
/// the rules are compared on identical rows rather than on separate passes that
/// could drift.
pub struct FoldAnchor {
    pub anchor: String,
    /// A labelled **loner** (empty `expected`): its correct default view is
    /// *empty above the fold* — the assertion GH #197 retired, returning on the
    /// disclosure axis.
    pub negative: bool,
    /// Labelled mates in label order.
    pub expected: Vec<String>,
    /// The served prefix (`limit` = [`SIM_K`]) in rank order.
    pub rows: Vec<FoldRow>,
}

impl FoldAnchor {
    /// This anchor's labelled mates that the *served prefix* reaches, as
    /// `(mate, rank)`. A mate ranked past `limit` is not among them — it is
    /// always-serve's own miss at this depth, and counting it against a fold
    /// would charge every rule for a cost none of them caused.
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

/// What one rule reads on one bench — GH #200's judged quantities, each carrying
/// the per-anchor list it is argued from (process rule 1: the aggregate is a
/// smoke alarm, the named list is the data).
pub struct FoldReading {
    pub rule: FoldRule,
    /// Labelled mates the served prefix reaches but the default view does
    /// **not** vouch for: `(anchor, mate, rank)`. **The fold's own cost**, and
    /// the quantity GH #200 judges a candidate at zero on — a human-labelled
    /// relation hidden by default. Reported rather than gated while nothing
    /// folds (it reads a structural 0 under always-serve) — and GH #202 shipped
    /// no fold to charge, so it stays reporting-only; it becomes an exit-gate
    /// row with the first fold that does ship.
    pub mates_folded: Vec<(String, String, usize)>,
    /// Labelled mates above the fold — the other half of the same count.
    pub mates_above: usize,
    /// Unlabelled notes the default view vouches for on a *positive* anchor —
    /// the strangers count, read above the fold instead of over the whole served
    /// prefix. Expected to shrink; deliberately ungated for the standing reason
    /// (the cheapest way to shrink it is to label the stranger).
    pub strangers_above: Vec<(String, String)>,
    /// Negative (loner) anchors whose default view is empty — the honest answer
    /// their label always claimed, and what always-serve cannot assert.
    pub neg_empty: usize,
    /// Non-negative anchors whose default view is empty. On the dense fixture
    /// **any** entry here is disqualifying (a vault where everything relates may
    /// never *default* to "nothing relates"); on the orthogonal corpus it is the
    /// same event read through the labels.
    pub dark_panes: Vec<String>,
    /// Cards above the fold, summed — the default view's size against
    /// always-serve's.
    pub cards_above: usize,
}

/// The **admissible-`k` window** for candidate 1, re-derived from this run
/// (GH #187's idiom, moved onto the disclosure axis: print the window a rule
/// *would* have rather than freezing a constant into a docstring). Two bounds
/// that must overlap for any `k` to be shippable on this bench:
///
/// - `keep_min` — the smallest `k` that folds **no** labelled mate and darkens
///   **no** pane. Below it the rule hides relations a human labelled.
/// - `loner_max` — the largest `k` at which **every** labelled loner's default
///   view is empty. Above it the fold stops making the claim it exists to make,
///   and a loner's pane fills up again.
pub struct FoldWindow {
    pub keep_min: Option<usize>,
    pub loner_max: Option<usize>,
    /// The smallest swept `k` whose fold equals always-serve on every anchor —
    /// where the rule becomes vacuous (it vouches for everything).
    pub vacuous_at: Option<usize>,
    /// The best the loner claim ever gets on this bench, as `(empty, largest k
    /// still achieving it)` — printed because "never all of them" and "four of
    /// five up to k = 11" are different findings, and only the second says how
    /// close the rule came.
    pub loner_best: Option<(usize, usize)>,
    /// The largest swept `k` that still darkens a pane. Every `k` at or below it
    /// is disqualified outright on a corpus where everything relates (D1's
    /// absolute), so this is the hard floor under any window.
    pub dark_below: Option<usize>,
}

impl FoldWindow {
    /// Whether some swept `k` satisfies both bounds at once. A bench with no
    /// labelled loner has no upper bound to satisfy and so can never report an
    /// open window on its own — it contributes the lower bound to the
    /// bake-off's joint window, which is what the printed verdict says.
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
    /// `(k, reading)` across the whole swept range — what the window is derived
    /// from, and printed as a compact table so the shape of the trade is visible
    /// rather than asserted.
    pub sweep: Vec<(usize, FoldReading)>,
    pub window: FoldWindow,
    /// Negative anchors on this bench (0 on the dense fixture, which has no
    /// loner by construction).
    pub neg_n: usize,
    /// Labelled mates the served prefix never reaches at `limit` — always-serve's
    /// own miss, identical under every rule. Printed beside the folded count so
    /// no rule is charged for it.
    pub mates_unserved: usize,
    /// The median *full-depth* candidate pool an anchor here has — what a
    /// reciprocity depth is a fraction OF. A `k` is only meaningful against it:
    /// "top 7" of a 14-candidate pool and "top 7" of a 30-candidate one are
    /// different claims, and the two corpora's windows below are only
    /// comparable in this unit.
    pub pool_median: usize,
    /// Authored edges in the built vault — **candidate 2's** entire calibration
    /// population ("what related looks like in this vault", read off the human's
    /// own committed links). Both eval corpora are link-free by construction, so
    /// this reads 0 and candidate 2 is *unpriceable here*: it is measured where
    /// its population exists, by `make calibrate` on a real vault.
    pub authored_edges: usize,
}

/// Every note's own ranked candidate list as `path → rank`, read at full depth —
/// the reciprocity lookup candidate 1 is judged on. Read over **every note in
/// the vault**, not only the labelled anchors: reciprocity is decided by the
/// *candidate's* list, and most candidates are not anchors.
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

/// Score the fold bake-off on one built vault. `sweep_all` reads **every note**
/// as an anchor (the dense fixture's pane sweep, where the assertion is about
/// the surface rather than the labels); otherwise only the labelled anchors are
/// read, matching [`score_similar`](crate::discovery::score_similar)'s depth and population exactly.
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

    // Outbound only. `neighbors` answers "what is 1 hop from here" and so returns
    // an edge from *both* of its endpoints; summing the raw counts over every
    // note would census edge **endpoints** and report candidate 2's population at
    // twice its size (PR #204 review). Counting each edge once from its source is
    // the number the rule would calibrate from.
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
        // A *labelled* loner. An unlabelled anchor on the dense sweep is not a
        // negative — that fixture carries no loner at all, and reading "no
        // label" as "nothing relates" would invent the claim the bench exists
        // to test.
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
        // The precision side, read above the fold: unlabelled cards the default
        // view vouches for on a positive anchor.
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

/// The bake-off's printed readout: the headline rules side by side, the swept
/// `k` the window is derived from, and the per-anchor folds — which is the list
/// the verdict is argued from (process rule 1).
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

    // The named cost of every rule that has one — the list, not the count.
    for r in &bench.readings {
        if r.mates_folded.is_empty() {
            continue;
        }
        println!("  {} folds labelled mates:", r.rule.label());
        for (anchor, mate, rank) in &r.mates_folded {
            println!("             rank {rank}   {anchor} → {mate}");
        }
    }

    // The swept window — the #187 idiom on the disclosure axis: derive the
    // rule's admissible range from THIS run rather than quote a constant.
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
    // The verdict names *which* bound failed. "No k works" and "this corpus
    // cannot decide alone" are different sentences, and a bench with no loner
    // can only ever supply the lower bound.
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

    // The per-anchor lists at the headline depths.
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

    // Candidate 2's census — printed on every bench, because "we did not measure
    // it" and "there was nothing to measure it on" are different sentences and
    // only the second is an argument.
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

/// The bake-off as one JSON subtree (`discovery_fold`): every rule's reading,
/// the swept window it was judged against, and the per-anchor rows both were
/// computed from — so a verdict cited from a row can be re-derived without
/// re-running the model, which is what `results.jsonl` is for.
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
        // Always-serve's own miss at this limit, recorded apart from every
        // rule's cost so the two can never be added together by a later reader.
        "mates_unserved": bench.mates_unserved,
        // Candidate 2's population, recorded even at zero: "unpriceable here"
        // is a measurement, and a row that omitted it would read as untried.
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
                // The anchor's rank in THIS candidate's own list — the whole
                // reciprocity signal, from which any k is re-derivable.
                "recip_rank": row.recip_rank,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
