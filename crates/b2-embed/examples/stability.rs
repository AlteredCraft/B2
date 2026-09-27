//! Rank-stability probe: the large-corpus half of the eval harness (GH #141).
//!
//! The scored eval's corpus is too small for candidate pools to truncate, so candidate-width
//! changes are invisible there. This runs on a vault big enough for the pool to bind and
//! reports:
//!
//! 1. **Pool sensitivity**: the same query asked at several depths, hence pool widths. RRF
//!    over a wider pool need not keep the shallow answer as a prefix; how often it breaks is
//!    what candidate width is worth here.
//! 2. **Baseline drift**: the shipped top-K against the committed baseline. `--bless`
//!    accepts it.
//!
//! The corpus is unlabelled: this says different, never better.
//!
//! ```console
//! cargo run -p b2-embed --example stability             # the probe (fake embedder)
//! cargo run -p b2-embed --example stability -- --verbose  # + the diverging lists
//! cargo run -p b2-embed --example stability -- --bless    # accept the current ranking
//! cargo run -p b2-embed --example stability -- --model     # real bge vectors, no baseline
//! cargo run -p b2-embed --example stability -- --vault path/to/vault
//! ```
//!
//! The fake embedder is the default because it is deterministic, so the baseline can be
//! committed (a real-model one is device-specific, ADR-0007). It exaggerates pool effects;
//! use `--model` for real magnitudes.
//!
//! Never a gate: exit status is 0 for any completed measurement.

mod common;

use b2_core::embed::Embedder;
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool, ChunkSearchResult, Vault};
use b2_embed::EmbedConfig;
use common::{git_short_sha, has_flag, load_or_provision, reject_unknown_flags, truncate};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The vault the committed baseline is defined over, relative to the repo root.
const DEFAULT_VAULT: &str = "fixtures/test-vault";
/// The depths each probe is asked at; each widens the pool it retrieves from (GH #142).
const DEPTHS: [usize; 3] = [4, 10, 30];
/// How deep a prefix the depths are compared over, capped by the shallowest ask.
const PREFIX: usize = DEPTHS[0];
/// The depth the committed baseline records: what an adapter's default search shows.
const BASELINE_K: usize = DEPTHS[1];
/// How much of a chunk's text goes into its baseline key (see [`chunk_key`]).
const KEY_CHARS: usize = 48;
const PROBE_COL: usize = 44;
const CELL_COL: usize = 26;
const KNOWN_FLAGS: [&str; 4] = ["--bless", "--model", "--verbose", "--vault"];

/// The hand-authored probe set (`evals/stability.json`) — plain queries, no labels.
#[derive(Deserialize)]
struct ProbeSet {
    probes: Vec<String>,
}

/// The committed ranking snapshot (`evals/stability-baseline.json`).
#[derive(Serialize, Deserialize)]
struct Baseline {
    #[serde(rename = "_note")]
    note: String,
    vault: String,
    embedder: String,
    k: usize,
    /// The commit the snapshot was blessed at, best-effort.
    blessed_at: Option<String>,
    probes: Vec<BaselineProbe>,
}

#[derive(Serialize, Deserialize)]
struct BaselineProbe {
    query: String,
    notes: Vec<String>,
    chunks: Vec<String>,
}

/// One probe's answer at one depth: ranked note paths and chunk keys.
struct Answer {
    notes: Vec<String>,
    chunks: Vec<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("stability probe failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // A bare `--` is dropped so a pasted `cargo run ... -- --verbose` in ARGS still works.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--").collect();
    let bless = has_flag(&args, "--bless");
    let real_model = has_flag(&args, "--model");
    let verbose = has_flag(&args, "--verbose");
    let vault_arg = flag_value(&args, "--vault")?;
    reject_unknown_flags(&args, &KNOWN_FLAGS, &["--vault"])?;

    // A baseline must be reproducible: fake embedder, committed vault.
    if bless && real_model {
        return Err("--bless needs the deterministic fake embedder; drop --model".into());
    }

    let evals_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let default_vault = repo_root.join(DEFAULT_VAULT);
    let source = match &vault_arg {
        Some(p) => PathBuf::from(p),
        None => default_vault.clone(),
    };
    if !source.is_dir() {
        return Err(format!("vault not found: {}", source.display()).into());
    }
    let on_default_vault = same_dir(&source, &default_vault);
    if bless && !on_default_vault {
        return Err(format!("--bless only applies to the committed {DEFAULT_VAULT}").into());
    }

    let probes: ProbeSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("stability.json"))?)?;
    if probes.probes.is_empty() {
        return Err("stability.json lists no probes".into());
    }

    // A throwaway copy, since indexing writes `.b2/` into the committed fixture.
    let tmp = tempfile::TempDir::new()?;
    let root = tmp.path().join("vault");
    copy_dir_all(&source, &root)?;

    let (vault, embedder_label) = if real_model {
        let config = EmbedConfig::load()?;
        let embedder = load_or_provision(&config)?;
        let label = embedder.model_id().to_string();
        (Vault::open_with_embedder(&root, Box::new(embedder))?, label)
    } else {
        (Vault::open(&root)?, "fake".to_string())
    };

    let mut chunks = 0usize;
    let t0 = Instant::now();
    let report = vault.reindex_with_progress(false, &mut |p| {
        chunks = p.chunks_done;
        ControlFlow::Continue(())
    })?;
    eprintln!(
        "[stability] vault = {} ({} notes / {chunks} chunks, embedder = {embedder_label}, {:.1}s)",
        display_from_repo(&source),
        report.indexed,
        t0.elapsed().as_secs_f64(),
    );
    // Checked against the widest pool (the note view's), so a partly-blind run warns too.
    if chunks <= note_candidate_pool(BASELINE_K) {
        eprintln!(
            "[warn] {chunks} chunks ≤ the {}-candidate pool the note view reaches (the passage view\n\
             \x20      reaches {}) — this vault is too small for pool width to bind, so probes below are\n\
             \x20      stable by construction, not by ranking quality (GH #141). That is the eval corpus'\n\
             \x20      situation; use a larger vault to measure.",
            note_candidate_pool(BASELINE_K),
            chunk_candidate_pool(BASELINE_K)
        );
    }
    eprintln!();

    // One retrieval per (probe, depth).
    let mut answers: Vec<Vec<Answer>> = Vec::with_capacity(probes.probes.len());
    for query in &probes.probes {
        let mut per_depth = Vec::with_capacity(DEPTHS.len());
        for depth in DEPTHS {
            per_depth.push(answer(&vault, query, depth)?);
        }
        answers.push(per_depth);
    }

    report_pool_sensitivity(&probes.probes, &answers, verbose);

    let baseline_path = evals_dir.join("stability-baseline.json");
    if bless {
        write_baseline(&baseline_path, &probes.probes, &answers)?;
        println!(
            "\nblessed {} probes into {}",
            probes.probes.len(),
            display_from_repo(&baseline_path)
        );
    } else if real_model || !on_default_vault {
        println!(
            "\nbaseline drift: skipped — the committed baseline is the fake embedder over {DEFAULT_VAULT}."
        );
    } else {
        report_baseline_drift(&baseline_path, &probes.probes, &answers)?;
    }

    Ok(())
}

/// Ask one probe at one depth, keeping only result identities. Scores aren't compared: they
/// move with the pool even when nothing reorders.
fn answer(vault: &Vault, query: &str, depth: usize) -> Result<Answer, Box<dyn std::error::Error>> {
    Ok(Answer {
        notes: vault
            .search(query, depth)?
            .into_iter()
            .map(|r| r.path)
            .collect(),
        chunks: vault
            .search_chunks(query, depth)?
            .iter()
            .map(chunk_key)
            .collect(),
    })
}

/// A chunk's identity across rebuilds: note, heading breadcrumb, and text head. Rowids are
/// per-build; a chunk cut differently is a different result.
fn chunk_key(hit: &ChunkSearchResult) -> String {
    let head: String = hit
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(KEY_CHARS)
        .collect();
    format!(
        "{}#{}#{head}",
        hit.path,
        hit.heading_path.as_deref().unwrap_or("")
    )
}

/// One label per adjacent depth pair: the per-signal candidate widths it spans for `pool`.
fn width_steps(pool: fn(usize) -> usize) -> Vec<String> {
    DEPTHS
        .windows(2)
        .map(|w| format!("{}→{}", pool(w[0]), pool(w[1])))
        .collect()
}

/// Section 1: does the answer to a probe survive the pool widening under it?
fn report_pool_sensitivity(probes: &[String], answers: &[Vec<Answer>], verbose: bool) {
    println!("{}", "=".repeat(96));
    println!("pool sensitivity — the same query asked at widening retrieval pools");
    println!(
        "  a cell is how many of the top-{PREFIX} results the two asks agree on, position by position;"
    );
    println!("  `=` means the shallow answer is an exact prefix of the deep one (pool-invariant).");
    // Each view has its own widths (GH #142).
    let chunk_steps = width_steps(chunk_candidate_pool);
    let note_steps = width_steps(note_candidate_pool);
    println!();
    let chunk_head = format!("chunks ({})", chunk_steps.join(" | "));
    let note_head = format!("notes ({})", note_steps.join(" | "));
    println!(
        "{:<PROBE_COL$}  {chunk_head:<CELL_COL$}  {note_head}",
        "probe"
    );
    println!("{}", "-".repeat(96));

    let mut moved_chunks = vec![0usize; chunk_steps.len()];
    let mut moved_notes = vec![0usize; note_steps.len()];
    let mut measured_chunks = vec![0usize; chunk_steps.len()];
    let mut measured_notes = vec![0usize; note_steps.len()];
    for (i, probe) in probes.iter().enumerate() {
        let per_depth = &answers[i];
        let mut chunk_cells = Vec::new();
        let mut note_cells = Vec::new();
        for s in 0..DEPTHS.len() - 1 {
            let (lo, hi) = (&per_depth[s], &per_depth[s + 1]);
            let c = prefix_cell(&lo.chunks, &hi.chunks);
            let n = prefix_cell(&lo.notes, &hi.notes);
            measured_chunks[s] += usize::from(c.measured);
            measured_notes[s] += usize::from(n.measured);
            if c.moved {
                moved_chunks[s] += 1;
            }
            if n.moved {
                moved_notes[s] += 1;
            }
            chunk_cells.push(c.label);
            note_cells.push(n.label);
        }
        println!(
            "{:<PROBE_COL$}  {:<CELL_COL$}  {}",
            truncate(probe, PROBE_COL),
            chunk_cells.join(" | "),
            note_cells.join(" | ")
        );
        if verbose {
            print_divergence(per_depth);
        }
    }

    println!();
    // Denominators count only measured probes (see [`prefix_cell`]).
    for s in 0..DEPTHS.len() - 1 {
        println!(
            "  ask {:>2}→{:<2}: {}/{} probes changed their top-{PREFIX} chunks ({} candidates/signal), \
             {}/{} their top-{PREFIX} notes ({})",
            DEPTHS[s],
            DEPTHS[s + 1],
            moved_chunks[s],
            measured_chunks[s],
            chunk_steps[s],
            moved_notes[s],
            measured_notes[s],
            note_steps[s],
        );
    }
    let unmeasured = measured_chunks
        .iter()
        .chain(&measured_notes)
        .any(|&m| m < probes.len());
    if unmeasured {
        println!(
            "  some probes returned nothing to compare (n/a above) and are outside those counts —\n\
             \x20 a query that matches no chunk measures no pool."
        );
    }
    if measured_chunks
        .iter()
        .chain(&measured_notes)
        .all(|&m| m == 0)
    {
        println!("  nothing was measured: no probe returned results on this vault.");
    } else if moved_chunks.iter().all(|&m| m == 0) && moved_notes.iter().all(|&m| m == 0) {
        println!(
            "  every measured probe is pool-invariant here — either the vault is no bigger than the\n\
             \x20 pool (the #141 blindness) or candidate width genuinely costs nothing on this corpus."
        );
    }
}

/// One depth-pair cell: how much of the shallow answer the deeper ask preserved. An empty
/// comparison is unmeasured, not stable, so a report that measured nothing can't claim
/// invariance.
struct Cell {
    label: String,
    moved: bool,
    measured: bool,
}

fn prefix_cell(shallow: &[String], deep: &[String]) -> Cell {
    let span = PREFIX.min(shallow.len()).min(deep.len());
    if span == 0 {
        return Cell {
            label: "n/a".to_string(),
            moved: false,
            measured: false,
        };
    }
    let same = (0..span).filter(|&i| shallow[i] == deep[i]).count();
    Cell {
        label: if same == span {
            format!("={span}")
        } else {
            format!("{same}/{span}")
        },
        moved: same != span,
        measured: true,
    }
}

/// `--verbose`: the two chunk rankings that disagreed (the un-deduped fusion output).
fn print_divergence(per_depth: &[Answer]) {
    for w in 0..DEPTHS.len() - 1 {
        let (lo, hi) = (&per_depth[w], &per_depth[w + 1]);
        if !prefix_cell(&lo.chunks, &hi.chunks).moved {
            continue;
        }
        println!(
            "      pool {} vs {}:",
            chunk_candidate_pool(DEPTHS[w]),
            chunk_candidate_pool(DEPTHS[w + 1])
        );
        for rank in 0..PREFIX {
            let a = lo.chunks.get(rank).map(String::as_str).unwrap_or("—");
            let b = hi.chunks.get(rank).map(String::as_str).unwrap_or("—");
            let mark = if a == b { ' ' } else { '≠' };
            println!("        {mark} {}. {}", rank + 1, truncate(a, 60));
            if a != b {
                println!("             → {}", truncate(b, 60));
            }
        }
    }
}

/// Section 2: the shipped top-K against the committed snapshot.
fn report_baseline_drift(
    path: &Path,
    probes: &[String],
    answers: &[Vec<Answer>],
) -> Result<(), Box<dyn std::error::Error>> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        println!(
            "\nbaseline drift: no baseline yet — `--bless` writes {}",
            display_from_repo(path)
        );
        return Ok(());
    };
    let baseline: Baseline = serde_json::from_str(&raw)?;
    // Otherwise drift would really be an instrument mismatch.
    if baseline.vault != DEFAULT_VAULT || baseline.embedder != "fake" {
        return Err(format!(
            "baseline was blessed from {} under the {} embedder, not {DEFAULT_VAULT} under fake — \
             re-bless it rather than comparing across instruments",
            baseline.vault, baseline.embedder
        )
        .into());
    }
    let depth_idx = DEPTHS
        .iter()
        .position(|&d| d == baseline.k)
        .ok_or_else(|| format!("baseline K={} is not one of {DEPTHS:?}", baseline.k))?;

    println!("\n{}", "=".repeat(96));
    println!(
        "baseline drift — top-{} vs {} (blessed at {})",
        baseline.k,
        display_from_repo(path),
        baseline.blessed_at.as_deref().unwrap_or("unknown")
    );
    let note_head = "notes";
    println!("{:<PROBE_COL$}  {note_head:<CELL_COL$}  chunks", "probe");
    println!("{}", "-".repeat(96));

    let mut drifted = 0usize;
    let mut unknown = 0usize;
    for (i, probe) in probes.iter().enumerate() {
        let Some(base) = baseline.probes.iter().find(|b| &b.query == probe) else {
            unknown += 1;
            println!(
                "{:<PROBE_COL$}  (new probe — no baseline)",
                truncate(probe, PROBE_COL)
            );
            continue;
        };
        let current = &answers[i][depth_idx];
        let notes = diff(&current.notes, &base.notes);
        let chunks = diff(&current.chunks, &base.chunks);
        if notes.moved || chunks.moved {
            drifted += 1;
        }
        println!(
            "{:<PROBE_COL$}  {:<CELL_COL$}  {}",
            truncate(probe, PROBE_COL),
            notes.label,
            chunks.label
        );
    }

    println!();
    let stale = baseline
        .probes
        .iter()
        .filter(|b| !probes.contains(&b.query))
        .count();
    if drifted == 0 && unknown == 0 && stale == 0 {
        println!("  no drift: every probe ranks exactly as blessed.");
    } else {
        println!(
            "  {drifted}/{} probes drifted{}{}.",
            probes.len(),
            if unknown > 0 {
                format!(", {unknown} new")
            } else {
                String::new()
            },
            if stale > 0 {
                format!(", {stale} blessed probe(s) no longer in stability.json")
            } else {
                String::new()
            }
        );
        println!(
            "  Drift is a signal, not a failure — this corpus is unlabelled, so it cannot say the\n\
             \x20 new ranking is worse or better. Once the change is the intended one, `--bless`."
        );
    }
    Ok(())
}

/// How one ranked list moved against its blessed self.
struct Diff {
    label: String,
    moved: bool,
}

fn diff(current: &[String], base: &[String]) -> Diff {
    let base_set: HashSet<&String> = base.iter().collect();
    let current_set: HashSet<&String> = current.iter().collect();
    let kept = current.iter().filter(|c| base_set.contains(c)).count();
    let entered = current.len() - kept;
    let left = base.iter().filter(|b| !current_set.contains(b)).count();
    let reordered = current != base;
    let label = if !reordered {
        format!("={}", base.len())
    } else if entered == 0 && left == 0 {
        format!("{kept}/{} kept, reordered", base.len())
    } else {
        format!("{kept}/{} kept, {entered} in, {left} out", base.len())
    };
    Diff {
        label,
        moved: reordered,
    }
}

fn write_baseline(
    path: &Path,
    probes: &[String],
    answers: &[Vec<Answer>],
) -> Result<(), Box<dyn std::error::Error>> {
    let depth_idx = DEPTHS
        .iter()
        .position(|&d| d == BASELINE_K)
        .ok_or("BASELINE_K must be one of DEPTHS")?;
    let baseline = Baseline {
        note: format!(
            "Blessed ranking snapshot for the rank-stability probe (crates/b2-embed/examples/stability.rs, \
             GH #141): the top-{BASELINE_K} notes and chunks each probe in stability.json returns from \
             {DEFAULT_VAULT} under the deterministic fake embedder. Committed so a later run can show \
             *movement* — any ranking change (candidate width, the RRF constant, chunking, resolution) shows up here \
             as drift rather \
             than being inferred. It scores nothing: the corpus is unlabelled, so drift means different, \
             never worse. Regenerate with `make stability-bless` once a ranking change is the intended one."
        ),
        vault: DEFAULT_VAULT.to_string(),
        embedder: "fake".to_string(),
        k: BASELINE_K,
        blessed_at: git_short_sha(),
        probes: probes
            .iter()
            .enumerate()
            .map(|(i, query)| BaselineProbe {
                query: query.clone(),
                notes: answers[i][depth_idx].notes.clone(),
                chunks: answers[i][depth_idx].chunks.clone(),
            })
            .collect(),
    };
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&baseline)?),
    )?;
    Ok(())
}

/// Recursive copy: the fixture has topic subfolders, which the eval's flat copy would drop.
fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let dest = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// `--vault <path>` / `--vault=<path>`, erroring when the value is missing.
fn flag_value(args: &[String], name: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    for (i, arg) in args.iter().enumerate() {
        if let Some(rest) = arg.strip_prefix(&format!("{name}=")) {
            return Ok(Some(rest.to_string()));
        }
        if arg == name {
            return match args.get(i + 1) {
                Some(v) if !v.starts_with("--") => Ok(Some(v.clone())),
                _ => Err(format!("{name} needs a path").into()),
            };
        }
    }
    Ok(None)
}

/// Same directory on disk, canonicalized.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `path` relative to the repo root where possible. A missing leaf (a baseline not yet
/// written) resolves through its parent, since canonicalize needs an existing path.
fn display_from_repo(path: &Path) -> String {
    let Ok(root) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
    else {
        return path.display().to_string();
    };
    let resolved = path.canonicalize().ok().or_else(|| {
        let parent = path.parent()?.canonicalize().ok()?;
        Some(parent.join(path.file_name()?))
    });
    match resolved {
        Some(p) => p
            .strip_prefix(&root)
            .map(|rel| rel.display().to_string())
            .unwrap_or_else(|_| p.display().to_string()),
        None => path.display().to_string(),
    }
}
