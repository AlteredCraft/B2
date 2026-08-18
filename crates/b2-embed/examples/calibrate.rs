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
//! ```
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
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--limit" => limit = it.next().ok_or("--limit needs a value")?.parse()?,
            "--leader-z" => gate.leader_z = it.next().ok_or("--leader-z needs a value")?.parse()?,
            "--member-z" => gate.member_z = it.next().ok_or("--member-z needs a value")?.parse()?,
            "--json" => json = true,
            other if vault_path.is_none() && !other.starts_with('-') => {
                vault_path = Some(PathBuf::from(other));
            }
            other => return Err(format!("unrecognized argument: {other}").into()),
        }
    }
    let vault_root = vault_path
        .ok_or("usage: calibrate <vault> [--limit N] [--leader-z Z] [--member-z Z] [--json]")?;

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

    if json {
        print_json(&vault_root, &model, dim, limit, &gate, &readings, &poolless);
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
         candidates (a simulation — the shipped surface serves the ranked list, GH #197)\n",
        gate.leader_z, gate.member_z
    );

    println!(
        "{:<44} {:>4}  {:^23}  {:^17}  {:>11}  {:>9}",
        "anchor", "n", "cos min/med/max", "leader cos / z", "gate serves", "bands"
    );
    println!("{}", "-".repeat(120));
    let mut dark = 0usize;
    let mut gate_served_total = 0usize;
    let mut always_served_total = 0usize;
    let (mut strong, mut clear, mut near, mut ungraded) = (0usize, 0usize, 0usize, 0usize);
    let mut all_cos: Vec<f64> = Vec::new();
    let mut leader_zs: Vec<f64> = Vec::new();
    let mut worst_drift = 0.0f64;
    for r in &readings {
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
            "{:<44} {:>4}  {:>6.3}/{:>6.3}/{:>6.3}   {:>6.3} / {:>6}  {:>7}/{:<3}  {}",
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
        "  always-serve (top-{limit})  0/{} anchors dark, {always_served_total} candidates served",
        readings.len()
    );
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
    readings: &[AnchorReading],
    poolless: &[String],
) {
    let row = serde_json::json!({
        "vault": vault_root.display().to_string(),
        "model": model,
        "dim": dim,
        "limit": limit,
        "gate": { "leader_z": gate.leader_z, "member_z": gate.member_z, "min_population": MIN_POPULATION },
        "anchors": readings.iter().map(|r| serde_json::json!({
            "anchor": r.path,
            "n": r.pool.len(),
            "gate_serves": r.gate_serves(gate, limit),
            "always_serves": r.pool.len().min(limit),
            "bands": r.bands(limit).map(|(s, c, n)| serde_json::json!({ "strong": s, "clear": c, "near": n })),
            "pool": r.pool.iter().enumerate().map(|(i, (path, cos, engine_z))| serde_json::json!({
                "path": path,
                "cos": (cos * 1e4).round() / 1e4,
                "z": r.z.as_ref().map(|z| (z[i] * 1e4).round() / 1e4),
                "engine_z": engine_z.map(|z| (z * 1e4).round() / 1e4),
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
