//! Real-vault discovery calibration — the instrument GH #197 (Phase 0a) promoted
//! out of GH #196's hand arithmetic. It runs against **any built vault**, needs no
//! labels, and prints the numbers every discovery-surfacing ruling has turned on:
//! per-anchor candidate-pool distributions (cosine min/median/max), each anchor's
//! leader cosine and z, what a z existence gate would serve versus what
//! always-serve does, and the strength bands the desktop would paint — plus a
//! vault-level summary. Process rule 3's dogfooding clause, made mechanical
//! (`docs/evals/README.md`, process rule 5): **a constant derived from a corpus's
//! score distribution is invalid until transfer-checked on a real vault**, and
//! this is the check.
//!
//! ```console
//! just calibrate ~/notes                 # per-anchor lines + the summary block
//! just calibrate ~/notes --json          # the same reading as one JSON object
//! just calibrate ~/notes --limit 5       # simulate a 5-card pane
//! just calibrate ~/notes --leader-z 1.5 --member-z 1.0   # replay a different gate
//! just calibrate ~/notes --mutual-k 5    # replay the fold at a different reciprocity depth
//! ```
//!
//! Beside the retired z gate, the instrument replays the **mutual-k reciprocity
//! fold** (GH #200, Phase A — the leading candidate for D1's *default disclosure
//! boundary*): candidate B is *reciprocal* for anchor A iff A ranks within B's
//! own top `mutual_k` candidates (default: `--limit`), and the replayed default
//! view is the ranked list's **longest reciprocal prefix** — it ends at the
//! first non-reciprocal candidate, so a reciprocal one ranked after that sits
//! below the fold too (prefix form: the fold is a cut in the served order,
//! never a filter that skips rows). A fold, not a gate —
//! under D1 everything below it stays served and reachable, so the replay prices
//! what the default view *would* show, never what exists. Rank-based, so it
//! carries no distributional constant to transfer-check; running this replay on
//! real vaults **is** its bake-off bench (GH #200's fourth row), beside the
//! orthogonal corpus, the dense fixture, and the labelled negatives.
//!
//! That bake-off has since **ruled, and no fold ships** (GH #200, 2026-08-22):
//! mutual-k's admissible window is empty on both eval corpora, and this
//! instrument supplied the reading that generalized the finding — the same `k`
//! is a different rule on every vault (`k = 10` discloses 36% of the cards on
//! the orthogonal corpus, 91% on the dense fixture, 98% on `fixtures/test-vault`,
//! where `k = 5` darkens 7 of 200 panes). The replay stays because the *next*
//! candidate is priced the same way, and because a real vault is still the
//! bench neither corpus can be.
//!
//! The **authored-edge reference bar** (GH #200's candidate 2) is replayed beside
//! it, and this is the only instrument that *can* replay it: the rule calibrates
//! "what related looks like in this vault" from the score distribution of the
//! human's own committed edges, and both eval corpora are link-free by
//! construction — engineered orthogonality leaves nothing to link — so the rule
//! has no population there. Here it has one. The bar is the population's lower
//! quartile (the whole distribution prints beside it, because a quantile is a
//! choice and a choice printed as one number is an assumption), and the replayed
//! default view is the longest prefix at or above it. Unlike reciprocity this is
//! a **distributional constant**, so process rule 5's transfer check binds it —
//! which is to say: this instrument is not an aside for candidate 2, it is the
//! whole of its evidence.
//!
//! **It is a pure read** — stored vectors only, no model call, no write beyond the
//! `.b2/` directory every read command ensures — so it runs in seconds on a vault
//! of any personal scale and never perturbs what it measures.
//!
//! The z here is **recomputed harness-side from the served scores** (`d² = score²`,
//! `z = (mean − d²) / σ` with the sample σ, leader self-included) — GH #196's
//! recovered arithmetic, kept independent of the engine's own statistics so the
//! instrument can also *check* them: where the engine ships a z beside a candidate,
//! the two are diffed and a drift is reported as a fault, the same posture as the
//! eval's `z_recheck`.
//!
//! The replayed gate defaults to the constants GH #197 retired (`leader_z` 1.96 /
//! `member_z` 1.49, inert under a 12-candidate population — GH #192's values), so
//! the acceptance reading reproduces GH #196's finding on its reporting vault:
//! leaders +1.358 / +1.522 / +1.529, cosine span 0.573 → 0.797, 16 of 17 anchors
//! dark. The gate is a **simulation** — since GH #197 the shipped surface serves
//! the ranked list and gates nothing; replaying the retired rule (or a candidate
//! Phase-2 one, via the flags) is exactly what this instrument is for.

use b2_core::vault::{SimilarView, Vault};
use b2_embed::{EmbedConfig, LocalEmbedder};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// How deep each anchor's pool is read. A bar is judged against the population it
/// has to cut, so the read must cover the whole candidate set, not a pane's
/// prefix; anything a personal vault holds fits far under this.
const SCAN_LIMIT: usize = 100_000;
/// The replayed gate is inert under this population — `discover.rs`'s statistics
/// guard, restated here because the replay must match the rule it prices.
const MIN_POPULATION: usize = 12;
/// The strength-band landmarks the desktop paints (`ui/src/strength.ts`, GH #182):
/// `●●●` at or above the labelled-mate population's upper quartile, `●●○` at or
/// above the retired leader bar. Restated constants, not imports — the bands are
/// UI copy, and `just eval`'s calibration block is where their values are
/// re-measured.
const BAND_STRONG_Z: f64 = 2.52;
const BAND_CLEAR_Z: f64 = 1.96;

/// The z existence gate being replayed — GH #197 retired it from the engine; the
/// instrument keeps it (and any variant the flags name) priceable.
struct GateSim {
    leader_z: f64,
    member_z: f64,
}

/// One anchor's complete reading: its whole candidate pool in rank order, with
/// the harness-side z when the population carries a statistic.
struct AnchorReading {
    path: String,
    /// (candidate path, cosine, engine z if shipped) in served (nearest-first) order.
    pool: Vec<(String, f64, Option<f64>)>,
    /// Harness-recomputed z per pool entry — `None` when the population is under
    /// [`MIN_POPULATION`] or has zero variance (no statistic exists).
    z: Option<Vec<f64>>,
}

impl AnchorReading {
    fn from_candidates(path: &str, cands: &[SimilarView]) -> Self {
        let d2: Vec<f64> = cands.iter().map(|c| c.score * c.score).collect();
        let z = (d2.len() >= MIN_POPULATION)
            .then(|| {
                let n = d2.len() as f64;
                let mean = d2.iter().sum::<f64>() / n;
                let var = d2.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
                let sd = var.sqrt();
                (sd > 0.0).then(|| d2.iter().map(|d| (mean - d) / sd).collect::<Vec<f64>>())
            })
            .flatten();
        Self {
            path: path.to_string(),
            pool: cands
                .iter()
                .map(|c| (c.path.clone(), cosine_of(c.score), c.z))
                .collect(),
            z,
        }
    }

    fn cosines(&self) -> Vec<f64> {
        self.pool.iter().map(|(_, cos, _)| *cos).collect()
    }

    /// How many candidates the replayed z gate would serve, capped at `limit` —
    /// 0 with the leader gate fired ("dark"), everything (to the cap) where the
    /// pool is too small for a statistic (the rule's own inertness).
    fn gate_serves(&self, gate: &GateSim, limit: usize) -> usize {
        match &self.z {
            None => self.pool.len().min(limit),
            Some(z) => {
                if z.first().copied().unwrap_or(f64::NEG_INFINITY) < gate.leader_z {
                    0
                } else {
                    z.iter()
                        .take_while(|&&v| v >= gate.member_z)
                        .count()
                        .min(limit)
                }
            }
        }
    }

    /// Band histogram over the top-`limit` cards always-serve shows:
    /// (strong ●●●, clear ●●○, near ●○○), or `None` for an ungraded pool —
    /// the A6 readout (do dense vaults compress every card into one band?).
    fn bands(&self, limit: usize) -> Option<(usize, usize, usize)> {
        let z = self.z.as_ref()?;
        let (mut strong, mut clear, mut near) = (0, 0, 0);
        for &v in z.iter().take(limit) {
            if v >= BAND_STRONG_Z {
                strong += 1;
            } else if v >= BAND_CLEAR_Z {
                clear += 1;
            } else {
                near += 1;
            }
        }
        Some((strong, clear, near))
    }

    /// Worst |engine z − recomputed z| across the pool — the drift check, biting
    /// only where the engine shipped a z at all.
    fn recheck_delta(&self) -> f64 {
        let Some(z) = &self.z else { return 0.0 };
        self.pool
            .iter()
            .zip(z)
            .filter_map(|((_, _, engine), ours)| engine.map(|e| (e - ours).abs()))
            .fold(0.0, f64::max)
    }

    /// How many pool entries the drift check actually compared — both z's
    /// present. Zero pairs must print as "nothing to cross-check", never as a
    /// pass: a check that reports success without having run is the
    /// advisory-but-exit-0 hole this repo's gates exist to close.
    fn recheck_pairs(&self) -> usize {
        if self.z.is_none() {
            return 0;
        }
        self.pool
            .iter()
            .filter(|(_, _, engine)| engine.is_some())
            .count()
    }
}

/// `similar` scores are negated L2 over unit vectors: `cos = 1 − d²/2`.
fn cosine_of(score: f64) -> f64 {
    1.0 - (score * score) / 2.0
}

fn pile_stats(pile: &[f64]) -> Option<(f64, f64, f64)> {
    if pile.is_empty() {
        return None;
    }
    let mut sorted = pile.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    };
    Some((sorted[0], median, sorted[sorted.len() - 1]))
}

/// **Candidate 2** of GH #200's bake-off, replayed: an *authored-edge reference
/// bar*. The idea is to calibrate "what related looks like **in this vault**"
/// from the one labelled population every real vault carries — the score
/// distribution of the human's own committed edges — and fold the default view
/// at the longest prefix scoring at or above it.
///
/// Priceable only where that population exists, which is why it lives here
/// rather than in `just eval`: **both eval corpora are link-free by
/// construction** (the token audit that keeps them orthogonal leaves nothing to
/// link), so the rule has nothing to calibrate from there. Its pair score is the
/// same statistic discovery ranks on — the best chunk pair across the two notes'
/// stored vectors — computed here over the *linked* pairs discovery never scores
/// (the 1-hop exclusion removes exactly them).
///
/// The bar is the population's **lower quartile**: the default view vouches for
/// a candidate that looks at least as related as the weaker quarter of what this
/// human has already been willing to link. The whole distribution prints beside
/// it, because the quantile is a choice and a choice printed as one number is an
/// assumption.
struct EdgeBar {
    /// Authored edges whose pair could be scored (both notes embedded).
    n: usize,
    min: f64,
    q1: f64,
    median: f64,
    max: f64,
}

impl EdgeBar {
    /// The bar itself — the lower quartile of the authored-edge cosines.
    fn bar(&self) -> f64 {
        self.q1
    }

    /// This vault's authored-edge pair cosines. Undirected and de-duplicated: an
    /// edge read from both endpoints is one relation, and counting it twice
    /// would weight the reciprocally-visible pairs double. `None` when the vault
    /// has no scorable authored edge — the rule has no population, which is a
    /// reading about the vault, not a failure of the instrument.
    fn read(vault: &Vault, conn: &Connection) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut cos: Vec<f64> = Vec::new();
        for note in vault.list_notes()? {
            let neighbors = vault.neighbors(&note.path)?;
            if neighbors.is_empty() {
                continue;
            }
            // Loaded once per source note and dropped with it: the population is
            // read edge by edge rather than by caching the whole vault's
            // vectors, so a large vault costs time here, never memory.
            let src: Vec<Vec<f32>> = b2_core::db::note_chunk_vectors(conn, &note.path)?
                .into_iter()
                .map(|(_, v)| v)
                .collect();
            for n in neighbors {
                let pair = if note.path <= n.path {
                    (note.path.clone(), n.path.clone())
                } else {
                    (n.path.clone(), note.path.clone())
                };
                if !seen.insert(pair) {
                    continue;
                }
                let dst: Vec<Vec<f32>> = b2_core::db::note_chunk_vectors(conn, &n.path)?
                    .into_iter()
                    .map(|(_, v)| v)
                    .collect();
                // Best-passage, the same statistic `similar` ranks on: the
                // nearest chunk pair across the two notes. A pair with an
                // unembedded side (or a dangling/resource target) scores nothing
                // and drops out rather than entering the population as a zero.
                let best = src
                    .iter()
                    .flat_map(|x| dst.iter().map(move |y| b2_core::embed::l2_sq(x, y)))
                    .fold(f32::INFINITY, f32::min);
                if best.is_finite() {
                    cos.push(1.0 - (best as f64) / 2.0);
                }
            }
        }
        if cos.is_empty() {
            return Ok(None);
        }
        cos.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let at = |q: f64| cos[(((cos.len() - 1) as f64) * q).round() as usize];
        Ok(Some(Self {
            n: cos.len(),
            min: cos[0],
            q1: at(0.25),
            median: at(0.5),
            max: cos[cos.len() - 1],
        }))
    }

    /// How many of `pool`'s leading candidates clear the bar — candidate 2's
    /// fold, in the same prefix form candidate 1 takes.
    fn fold(&self, pool: &[(String, f64, Option<f64>)], limit: usize) -> usize {
        pool.iter()
            .take(limit)
            .take_while(|(_, cos, _)| *cos >= self.bar())
            .count()
    }
}

/// English function words, probed for **where they weigh** against a query's
/// content (GH #201). The lexical-anchor rule weighs each term by its IDF, so
/// its whole premise is that a vault's function words weigh near nothing beside
/// its subject words. That is a claim about a vault, not about English, and this
/// is where it is checked.
const FUNCTION_WORDS: [&str; 12] = [
    "the", "and", "of", "to", "a", "in", "is", "it", "that", "for", "with", "on",
];

/// Built-in nonsense queries — the **strict** half of D2's negative pile, and
/// the only half that can be built in: an off-topic *phrase* is off-topic
/// relative to a particular vault ("choreographing a ballroom waltz" is a
/// labelled negative on the eval corpus and would be a positive in a dancer's
/// vault), so the eval corpus is where those live. These are strings no vault
/// holds, which makes them vault-independent.
const NONSENSE: [&str; 4] = [
    "shjfasd",
    "vrelqip zonktar wembleforth",
    "qwolbex frunstip",
    "zzzyqx",
];

/// How many notes contribute a title-as-query positive. A cap rather than a
/// sample: the walk takes them in `list_notes` order, which is deterministic, so
/// a re-run on an unchanged vault reads the same queries.
const MAX_TITLE_QUERIES: usize = 400;

/// One query's transfer reading — the same two absolute signals `just eval`'s
/// bake-off judges, read on a real vault instead of a labelled corpus.
struct SearchProbe {
    query: String,
    /// Share of the query's term IDF this vault carries. `None` when nothing in
    /// the query carries weight here.
    coverage: Option<f64>,
    /// Dense top-1 cosine; `None` on a vault with no embedding space.
    best_cos: Option<f64>,
    /// What the shipped bar would say — `true` = the default view vouches.
    vouched: bool,
}

/// The **search evidence transfer check** (invariants.md D2; GH #201) — process
/// rule 5's bench for the query-level bar, which is a distributional constant
/// and therefore invalid until a real vault has answered for it.
///
/// It needs no labels, and that is the point: the positives are each note's own
/// **title**, a query the vault demonstrably holds material for by construction,
/// and the negatives are [`NONSENSE`], strings no vault holds. Neither side is a
/// hand-label, so running this on someone's notes costs them nothing and the
/// reading cannot be tuned by relabelling — process rule 2's standing worry,
/// answered by there being nothing here to relabel.
///
/// What it can and cannot see is worth stating plainly. It **can** see the
/// tripwire direction — a bar that cuts queries a real vault holds material for
/// is the failure GH #196 punished, and title queries are the cheapest honest
/// probe of it. It **cannot** see the paraphrase case (a user's words for a note
/// they wrote in other words); generating those needs judgement, which is what
/// the labelled corpus is for. Read the two benches together, never either
/// alone.
fn print_search_transfer(
    vault: &Vault,
    conn: &Connection,
    model_id: &str,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("search evidence transfer check (D2 — process rule 5's bench for GH #201's bar)");
    let Some(bar) = b2_core::search::EvidenceBar::for_model(model_id) else {
        println!("  no calibrated bar for '{model_id}' — nothing to transfer-check");
        println!("  (M2: a bar read off one model's distances says nothing about another's)");
        return Ok(());
    };
    println!(
        "  bar under test: coverage ≥ {:.2}, cos ≥ {:.3}",
        bar.min_term_coverage, bar.min_cos
    );

    // Where this vault's function words weigh. One `lexical_evidence` call over
    // the joined list, so the dfs come back through exactly the path a query's
    // would.
    let probe = b2_core::search::lexical_evidence(conn, &FUNCTION_WORDS.join(" "))?;
    let heaviest = probe
        .terms
        .iter()
        .map(|t| probe.idf(t.df))
        .fold(0.0_f64, f64::max);
    // The weight a word the vault has never seen would carry — the scale every
    // function word above is read against, since that is the weight an absent
    // *content* word contributes to the same sum.
    let absent = probe.idf(0);
    println!("  chunks {}", probe.chunk_total);
    print!("  function-word weight (vs {absent:.2} for a word the vault lacks) ");
    for t in &probe.terms {
        print!("{} {:.2}  ", t.term, probe.idf(t.df));
    }
    println!();
    let share = 100.0 * heaviest / absent.max(f64::EPSILON);
    if absent > 0.0 && heaviest / absent <= 0.25 {
        println!("    → heaviest function word carries {share:.0}% of an absent word's weight —");
        println!("      the anchor test is reading subject words here, which is what it is for");
    } else {
        println!("    → [warn] heaviest function word carries {share:.0}% of an absent word's");
        println!("      weight — on this vault function words are not cheap, so every coverage");
        println!("      reading below is diluted by them");
    }

    let read = |query: &str| -> Result<SearchProbe, Box<dyn std::error::Error>> {
        let view = vault.search_evidence(query, limit)?;
        let idf = |df: usize| ((view.chunk_total as f64 + 1.0) / (df as f64 + 1.0)).ln();
        let total: f64 = view.terms.iter().map(|t| idf(t.df)).sum();
        let coverage = (total > f64::EPSILON).then(|| {
            view.terms
                .iter()
                .filter(|t| t.df >= 1)
                .map(|t| idf(t.df))
                .fold(0.0, |a, b| a + b)
                / total
        });
        Ok(SearchProbe {
            query: query.to_string(),
            coverage,
            best_cos: view.best_cos,
            // The engine's own verdict, not a restatement of it: this bench
            // prices what would actually ship.
            vouched: view.vouched.unwrap_or(true),
        })
    };

    let mut titles: Vec<String> = Vec::new();
    for note in vault.list_notes()? {
        if titles.len() == MAX_TITLE_QUERIES {
            break;
        }
        let title = note.title.clone().unwrap_or_else(|| {
            std::path::Path::new(&note.path)
                .file_stem()
                .map(|s| s.to_string_lossy().replace(['-', '_'], " "))
                .unwrap_or_default()
        });
        if !title.trim().is_empty() {
            titles.push(title);
        }
    }
    let positives: Vec<SearchProbe> = titles.iter().map(|t| read(t)).collect::<Result<_, _>>()?;
    let cut: Vec<&SearchProbe> = positives.iter().filter(|p| !p.vouched).collect();
    let covs: Vec<f64> = positives.iter().filter_map(|p| p.coverage).collect();
    let coss: Vec<f64> = positives.iter().filter_map(|p| p.best_cos).collect();
    println!(
        "  title-as-query positives (n={}, each note's own title)",
        positives.len()
    );
    match pile_stats(&covs) {
        Some((min, med, max)) => {
            println!("    coverage  min/med/max {min:.2}/{med:.2}/{max:.2}")
        }
        None => println!("    coverage  no reading (no note contributed a query)"),
    }
    match pile_stats(&coss) {
        Some((min, med, max)) => {
            println!("    best-cos  min/med/max {min:.3}/{med:.3}/{max:.3}")
        }
        None => println!(
            "    best-cos  no reading — no embedding space here, so only the bar's \
             model-free lexical half is being checked"
        ),
    }
    println!(
        "    the bar would cut {}/{}   ← the search-side TRIPWIRE direction (D2: zero, no headroom)",
        cut.len(),
        positives.len()
    );
    for p in cut.iter().take(10) {
        println!(
            "      cut: {:<44} cov {:>5}  cos {:>6}",
            truncate(&p.query, 44),
            p.coverage
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            p.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
        );
    }
    if cut.len() > 10 {
        println!("      … and {} more", cut.len() - 10);
    }
    // The closest calls, not a sample: a bar is placed against the queries that
    // nearly miss, and on a vault of any size those are the ten weakest — the
    // same reason the eval prints piles rather than means.
    let mut weakest: Vec<&SearchProbe> = positives.iter().collect();
    weakest.sort_by(|a, b| {
        a.best_cos
            .unwrap_or(f64::INFINITY)
            .partial_cmp(&b.best_cos.unwrap_or(f64::INFINITY))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    println!("    closest calls (lowest-cosine positives — where a bar placement bites first):");
    for p in weakest.iter().take(10) {
        println!(
            "      {:<44} cov {:>5}  cos {:>6}  → {}",
            truncate(&p.query, 44),
            p.coverage
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            p.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
            if p.vouched { "served" } else { "CUT" },
        );
    }

    let negatives: Vec<SearchProbe> = NONSENSE.iter().map(|q| read(q)).collect::<Result<_, _>>()?;
    let served = negatives.iter().filter(|p| p.vouched).count();
    println!(
        "  nonsense negatives (n={}, built-in — the vault-independent half of the pile)",
        negatives.len()
    );
    for p in &negatives {
        println!(
            "      {:<44} cov {:>5}  cos {:>6}  → {}",
            truncate(&p.query, 44),
            p.coverage
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            p.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
            if p.vouched { "SERVED" } else { "no matches" },
        );
    }
    println!("    the bar serves {served}/{}", negatives.len());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("calibrate failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vault_path: Option<PathBuf> = None;
    let mut limit = 10usize;
    let mut gate = GateSim {
        leader_z: 1.96,
        member_z: 1.49,
    };
    let mut mutual_k_flag: Option<usize> = None;
    let mut json = false;
    let mut search = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--limit" => limit = it.next().ok_or("--limit needs a value")?.parse()?,
            "--leader-z" => gate.leader_z = it.next().ok_or("--leader-z needs a value")?.parse()?,
            "--member-z" => gate.member_z = it.next().ok_or("--member-z needs a value")?.parse()?,
            "--mutual-k" => {
                mutual_k_flag = Some(it.next().ok_or("--mutual-k needs a value")?.parse()?)
            }
            "--json" => json = true,
            "--search" => search = true,
            other if vault_path.is_none() && !other.starts_with('-') => {
                vault_path = Some(PathBuf::from(other));
            }
            other => return Err(format!("unrecognized argument: {other}").into()),
        }
    }
    let vault_root = vault_path.ok_or(
        "usage: calibrate <vault> [--limit N] [--leader-z Z] [--member-z Z] [--mutual-k N] \
         [--search] [--json]",
    )?;
    // The reciprocity depth defaults to the simulated pane size, resolved after
    // parsing so flag order can't matter.
    let mutual_k = mutual_k_flag.unwrap_or(limit);

    // A pure read over stored vectors, so the fake-embedder open is correct — the
    // same posture as `b2 similar`. The recorded identity is read straight from
    // the index, because it is what the vectors actually are, not what an
    // injected embedder would be.
    let vault = Vault::open(&vault_root)?;
    let conn = b2_core::open(&vault_root.join(".b2").join("b2.sqlite"))?;
    let Some((model, dim)) = b2_core::db::recorded_embedder(&conn)? else {
        return Err(
            "this vault has no embedding space — run `b2 init` then `b2 reindex` first".into(),
        );
    };
    if model == b2_core::embed::FAKE_MODEL_ID {
        eprintln!(
            "[warn] this vault is fake-embedded — hash vectors have no semantic geometry, so every \
             distribution below is noise. Reindex with the real model before reading anything off this."
        );
    }

    let notes = vault.list_notes()?;
    let mut readings: Vec<AnchorReading> = Vec::new();
    let mut poolless: Vec<String> = Vec::new();
    for note in &notes {
        let cands = vault.similar(&note.path, SCAN_LIMIT)?;
        if cands.is_empty() {
            // No stored vectors on the anchor, or nothing unlinked with vectors
            // to compare against — a genuinely empty candidate set, named rather
            // than averaged in as a zero.
            poolless.push(note.path.clone());
        } else {
            readings.push(AnchorReading::from_candidates(&note.path, &cands));
        }
    }

    // The mutual-k reciprocity fold, replayed (GH #200, Phase A). Every note's
    // own top `mutual_k` candidate paths come from the same full-depth pools
    // just read, so reciprocity costs no extra discovery pass. Candidate B is
    // reciprocal for anchor A iff A sits in B's set; a candidate with no pool
    // of its own cannot reciprocate (and, having no stored vectors, could not
    // have been scored as a candidate either — the lookup's honesty is
    // belt-and-braces). The fold is the ranked list's longest reciprocal
    // prefix, capped at `limit` — prefix form is D1's admissibility
    // requirement: a fold that skipped rank 2 to admit rank 5 would visibly
    // disagree with the row order, so none is ever computed.
    let top_of: HashMap<&str, HashSet<&str>> = readings
        .iter()
        .map(|r| {
            (
                r.path.as_str(),
                r.pool
                    .iter()
                    .take(mutual_k)
                    .map(|(p, _, _)| p.as_str())
                    .collect::<HashSet<&str>>(),
            )
        })
        .collect();
    let folds: Vec<(usize, Vec<bool>)> = readings
        .iter()
        .map(|r| {
            let recip: Vec<bool> = r
                .pool
                .iter()
                .map(|(p, _, _)| {
                    top_of
                        .get(p.as_str())
                        .is_some_and(|s| s.contains(r.path.as_str()))
                })
                .collect();
            let fold = recip.iter().take(limit).take_while(|&&b| b).count();
            (fold, recip)
        })
        .collect();

    // Candidate 2 of the same bake-off (GH #200): the authored-edge reference
    // bar, priceable only where the human has committed edges — which is why it
    // is measured here and not in `just eval` (both eval corpora are link-free
    // by construction, so the rule has no population there).
    let edge_bar = EdgeBar::read(&vault, &conn)?;

    if json {
        print_json(
            &vault_root,
            &model,
            dim,
            limit,
            &gate,
            mutual_k,
            &readings,
            &folds,
            &edge_bar,
            &poolless,
        );
        return Ok(());
    }

    println!(
        "[calibrate] {}  model {model} (dim {dim})  {} notes, {} anchors with candidates, {} without",
        vault_root.display(),
        notes.len(),
        readings.len(),
        poolless.len()
    );
    println!(
        "[calibrate] replayed gate: leader_z {:.2} / member_z {:.2}, inert under {MIN_POPULATION} \
         candidates; replayed fold: mutual-k {mutual_k}, longest reciprocal prefix (simulations — \
         the shipped surface serves the ranked list, GH #197/#200)\n",
        gate.leader_z, gate.member_z
    );

    println!(
        "{:<44} {:>4}  {:^23}  {:^17}  {:>11}  {:>5}  {:>5}  {:>9}",
        "anchor", "n", "cos min/med/max", "leader cos / z", "gate serves", "fold", "e-bar", "bands"
    );
    println!("{}", "-".repeat(120));
    let mut dark = 0usize;
    let mut gate_served_total = 0usize;
    let mut fold_served_total = 0usize;
    let mut fold_empty = 0usize;
    let mut always_served_total = 0usize;
    let (mut strong, mut clear, mut near, mut ungraded) = (0usize, 0usize, 0usize, 0usize);
    let mut all_cos: Vec<f64> = Vec::new();
    let mut leader_zs: Vec<f64> = Vec::new();
    let mut worst_drift = 0.0f64;
    for (r, (fold, _)) in readings.iter().zip(&folds) {
        let cos = r.cosines();
        let (min, med, max) = pile_stats(&cos).unwrap_or((f64::NAN, f64::NAN, f64::NAN));
        all_cos.extend_from_slice(&cos);
        let leader_cos = cos.first().copied().unwrap_or(f64::NAN);
        let leader_z = r.z.as_ref().and_then(|z| z.first().copied());
        if let Some(z) = leader_z {
            leader_zs.push(z);
        }
        let served = r.gate_serves(&gate, limit);
        let always = r.pool.len().min(limit);
        gate_served_total += served;
        fold_served_total += fold;
        if *fold == 0 {
            fold_empty += 1;
        }
        always_served_total += always;
        if served == 0 {
            dark += 1;
        }
        let bands = r.bands(limit);
        match bands {
            Some((s, c, n)) => {
                strong += s;
                clear += c;
                near += n;
            }
            None => ungraded += 1,
        }
        worst_drift = worst_drift.max(r.recheck_delta());
        println!(
            "{:<44} {:>4}  {:>6.3}/{:>6.3}/{:>6.3}   {:>6.3} / {:>6}  {:>7}/{:<3}  {:>4}  {:>5}  {}",
            truncate(&r.path, 44),
            r.pool.len(),
            min,
            med,
            max,
            leader_cos,
            leader_z
                .map(|z| format!("{z:+.3}"))
                .unwrap_or_else(|| "—".into()),
            if served == 0 {
                "DARK".to_string()
            } else {
                served.to_string()
            },
            always,
            fold,
            edge_bar
                .as_ref()
                .map(|b| b.fold(&r.pool, limit).to_string())
                .unwrap_or_else(|| "—".into()),
            match bands {
                Some((s, c, n)) => format!("{s}●●● {c}●●○ {n}●○○"),
                None => "ungraded".to_string(),
            }
        );
    }
    for path in &poolless {
        println!(
            "{:<44}    0  (no candidate pool — no stored vectors to compare)",
            truncate(path, 44)
        );
    }

    println!("\n{}", "=".repeat(78));
    if let Some((min, med, max)) = pile_stats(&all_cos) {
        println!(
            "  cosine, all pools     min/med/max {min:.3}/{med:.3}/{max:.3}  (n={})",
            all_cos.len()
        );
    }
    match pile_stats(&leader_zs) {
        Some((min, med, max)) => println!(
            "  leader z              min/med/max {min:+.3}/{med:+.3}/{max:+.3}  (n={} graded anchors)",
            leader_zs.len()
        ),
        None => println!("  leader z              no graded anchor (every pool under {MIN_POPULATION} or zero variance)"),
    }
    println!(
        "  replayed z gate       {dark}/{} anchors dark, {gate_served_total} candidates served",
        readings.len()
    );
    println!(
        "  mutual-{mutual_k} fold         {fold_empty}/{} anchors fold to an empty default view, \
         {fold_served_total} candidates above the fold ({} below, still served — a fold, not a gate)",
        readings.len(),
        always_served_total.saturating_sub(fold_served_total)
    );
    println!(
        "  always-serve (top-{limit})  0/{} anchors dark, {always_served_total} candidates served",
        readings.len()
    );
    match &edge_bar {
        Some(bar) => {
            let (mut above, mut empty) = (0usize, 0usize);
            for r in &readings {
                let fold = bar.fold(&r.pool, limit);
                above += fold;
                if fold == 0 {
                    empty += 1;
                }
            }
            println!(
                "  authored-edge bar     {empty}/{} anchors fold to an empty default view, \
                 {above} candidates above the bar {:.3} (n={} edges, cos min/q1/med/max \
                 {:.3}/{:.3}/{:.3}/{:.3})",
                readings.len(),
                bar.bar(),
                bar.n,
                bar.min,
                bar.q1,
                bar.median,
                bar.max
            );
        }
        None => println!(
            "  authored-edge bar     UNPRICEABLE on this vault — no scorable authored edge, so \
             candidate 2 has no population to calibrate from"
        ),
    }
    println!(
        "  bands at top-{limit}       {strong} ●●● / {clear} ●●○ / {near} ●○○ across graded anchors ({ungraded} ungraded)"
    );
    let recheck_pairs: usize = readings.iter().map(|r| r.recheck_pairs()).sum();
    if worst_drift > 1e-3 {
        println!(
            "  [FAULT] engine z disagrees with the recomputation by up to {worst_drift:.3} — \
             the judged statistic moved; distrust the replay above"
        );
    } else if recheck_pairs == 0 {
        println!(
            "  [note]  the engine shipped no z on this vault (ungraded space or tiny pools) — \
             nothing to cross-check"
        );
    } else {
        println!(
            "  [check] engine z matches the recomputation over {recheck_pairs} candidates \
             (max Δ {worst_drift:.1e})"
        );
    }

    // The search-side bench (GH #201), opt-in because it is the one part of this
    // instrument that is **not** a pure read: judging the cosine half means
    // embedding a query, which means loading the real model. The lexical half
    // needs no model at all, so a fake-embedded vault still gets that much.
    if search {
        if model == b2_core::embed::FAKE_MODEL_ID {
            print_search_transfer(&vault, &conn, &model, limit)?;
        } else {
            let config = EmbedConfig::load()?;
            let embedder = LocalEmbedder::load(&config)?;
            let real = Vault::open_with_embedder(&vault_root, Box::new(embedder))?;
            print_search_transfer(&real, &conn, &model, limit)?;
        }
    }
    Ok(())
}

/// The same reading as one JSON object on stdout — for scripting a sweep (the A7
/// population-size sweep, a Phase-2 bake-off harness) without scraping the table.
#[allow(clippy::too_many_arguments)]
fn print_json(
    vault_root: &std::path::Path,
    model: &str,
    dim: usize,
    limit: usize,
    gate: &GateSim,
    mutual_k: usize,
    readings: &[AnchorReading],
    folds: &[(usize, Vec<bool>)],
    edge_bar: &Option<EdgeBar>,
    poolless: &[String],
) {
    let row = serde_json::json!({
        "vault": vault_root.display().to_string(),
        "model": model,
        "dim": dim,
        "limit": limit,
        "gate": { "leader_z": gate.leader_z, "member_z": gate.member_z, "min_population": MIN_POPULATION },
        // The replayed fold's depth (GH #200): candidate B is reciprocal iff
        // the anchor sits in B's own top `mutual_k`; `fold_serves` below is the
        // ranked list's longest reciprocal prefix, capped at `limit`.
        "mutual_k": mutual_k,
        // Candidate 2's population and the bar read off it (GH #200) — `null`
        // on a link-free vault, which is the rule's own reading there.
        "edge_bar": edge_bar.as_ref().map(|b| serde_json::json!({
            "n": b.n,
            "bar": (b.bar() * 1e4).round() / 1e4,
            "min": (b.min * 1e4).round() / 1e4,
            "q1": (b.q1 * 1e4).round() / 1e4,
            "median": (b.median * 1e4).round() / 1e4,
            "max": (b.max * 1e4).round() / 1e4,
        })),
        "anchors": readings.iter().zip(folds).map(|(r, (fold, recip))| serde_json::json!({
            "anchor": r.path,
            "n": r.pool.len(),
            "gate_serves": r.gate_serves(gate, limit),
            "fold_serves": fold,
            "edge_bar_serves": edge_bar.as_ref().map(|b| b.fold(&r.pool, limit)),
            "always_serves": r.pool.len().min(limit),
            "bands": r.bands(limit).map(|(s, c, n)| serde_json::json!({ "strong": s, "clear": c, "near": n })),
            "pool": r.pool.iter().enumerate().map(|(i, (path, cos, engine_z))| serde_json::json!({
                "path": path,
                "cos": (cos * 1e4).round() / 1e4,
                "z": r.z.as_ref().map(|z| (z[i] * 1e4).round() / 1e4),
                "engine_z": engine_z.map(|z| (z * 1e4).round() / 1e4),
                "reciprocal": recip[i],
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "no_pool": poolless,
    });
    println!("{row}");
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
