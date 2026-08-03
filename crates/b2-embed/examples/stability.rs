//! Rank-stability probe — the **large-corpus** half of the eval harness (GH #141).
//!
//! The scored eval (`--example eval`) runs on the hand-labelled corpus in
//! `evals/corpus/`: 20 notes / 26 chunks. Retrieval pulls
//! `vault::candidate_pool(10) = 150` candidates from *each* signal, so on a corpus
//! that small both halves of the hybrid return the entire index, the two ranked
//! lists RRF fuses are identical for any pool at least that wide, and every number
//! the eval prints is **invariant under pool/fusion width**. A change to
//! `vault::hit_pool`, `search::pool_size` or `search::RRF_K` therefore reads as "no
//! change" there while genuinely reordering a real vault — the worked example is
//! GH #140, whose 3× pool widening moved 5 of 7 probe top-10s on
//! `fixtures/test-vault` and printed bit-identical eval numbers.
//!
//! This probe measures the property that corpus cannot see. It runs on a vault big
//! enough for the pool to **bind** (`fixtures/test-vault`, ~200 notes / ~790
//! chunks) and reports two things:
//!
//! 1. **Pool sensitivity** — retrieval width is a function of the ask
//!    (`search_chunks(q, n)` reaches `candidate_pool(n)` candidates per signal), so
//!    asking the *same query* at several depths asks it at several pool widths. A
//!    pool-invariant retriever would answer a shallow ask with an exact prefix of
//!    the deep one; RRF over a widened pool does not, because a candidate ranked in
//!    *both* lists can outscore one ranked top of a single list (`2/121 > 1/61` at
//!    k = 60). How often the prefix breaks is how much fusion width is worth on
//!    this vault. No baseline, no config edit, no rebuild — one run answers it.
//! 2. **Baseline drift** — the shipped top-K against the committed
//!    `evals/stability-baseline.json`, so *any* future ranking change (fusion
//!    width, chunker, FTS sanitizer, resolution) is visible as movement rather than
//!    inferred. `--bless` accepts the current ranking as the new baseline.
//!
//! The corpus here is **unlabelled**: this scores no relevance and never says
//! *better*, only *different, and by how much*. Relevance is the scored eval's job;
//! sensitivity is this one's.
//!
//! ```console
//! cargo run -p b2-embed --example stability             # the probe (fake embedder)
//! cargo run -p b2-embed --example stability -- --verbose  # + the diverging lists
//! cargo run -p b2-embed --example stability -- --bless    # accept the current ranking
//! cargo run -p b2-embed --example stability -- --model     # real bge vectors, no baseline
//! cargo run -p b2-embed --example stability -- --vault path/to/vault
//! ```
//!
//! **Why the fake embedder by default.** It is content-addressed and deterministic
//! (blake3), so the committed baseline means the same thing on every machine — a
//! real-model baseline would be device-specific (a Metal build tags its own model
//! id, GH #40) and could not be committed. The cost is that fake vector ranking is
//! uncorrelated with BM25, which *exaggerates* how much a pool change moves
//! results; real bge rankings correlate, so the true shift is smaller. Read the
//! numbers as "the mechanism is live on this vault, at this magnitude given
//! uncorrelated signals", and use `--model` when the real magnitude is what the
//! decision needs (that run scores the same probes with bge vectors and skips the
//! baseline, which only exists for the deterministic mode).
//!
//! **The control experiment.** `--vault crates/b2-embed/evals/corpus` runs the same
//! probe on the eval corpus, where every prefix holds at every depth — that *is*
//! #141: not stability, blindness.
//!
//! Never a gate, and drift is not failure: a knob change is *supposed* to move
//! ranking, and an unlabelled corpus cannot say whether the movement was an
//! improvement. Exit status is 0 for any completed measurement.

use b2_core::embed::Embedder;
use b2_core::vault::{candidate_pool, ChunkSearchResult, Vault};
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The vault the committed baseline is defined over, relative to the repo root.
const DEFAULT_VAULT: &str = "fixtures/test-vault";
/// The depths each probe is asked at. Each one widens the pool it retrieves from:
/// `candidate_pool(4) = 60`, `candidate_pool(10) = 150`, `candidate_pool(30) = 450`.
/// The first pair is the ruling #142 has to make — `search_chunks`' shipped 3×
/// headroom (pool 150 for a 10-result ask) against the conservative `limit + 2`
/// reading of #137 (pool 60).
const DEPTHS: [usize; 3] = [4, 10, 30];
/// How deep a prefix the depths are compared over — capped by the shallowest ask.
const PREFIX: usize = DEPTHS[0];
/// The depth the committed baseline records: what an adapter's default search shows.
const BASELINE_K: usize = DEPTHS[1];
/// How much of a chunk's text goes into its baseline key (see [`chunk_key`]).
const KEY_CHARS: usize = 48;
/// Report column widths: the probe text, then one column per compared view.
const PROBE_COL: usize = 44;
const CELL_COL: usize = 26;

/// The hand-authored probe set (`evals/stability.json`) — plain queries, no labels.
#[derive(Deserialize)]
struct ProbeSet {
    probes: Vec<String>,
}

/// The committed ranking snapshot (`evals/stability-baseline.json`).
#[derive(Serialize, Deserialize)]
struct Baseline {
    /// Kept in the file so the artifact explains itself where it is read.
    #[serde(rename = "_note")]
    note: String,
    vault: String,
    embedder: String,
    k: usize,
    /// The commit the snapshot was blessed at, best-effort (`None` outside a checkout).
    blessed_at: Option<String>,
    probes: Vec<BaselineProbe>,
}

#[derive(Serialize, Deserialize)]
struct BaselineProbe {
    query: String,
    notes: Vec<String>,
    chunks: Vec<String>,
}

/// One probe's answer at one depth: the ranked note paths (`Vault::search`) and the
/// ranked chunk keys (`Vault::search_chunks`).
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bless = has_flag(&args, "--bless");
    let real_model = has_flag(&args, "--model");
    let verbose = has_flag(&args, "--verbose");
    let vault_arg = flag_value(&args, "--vault")?;
    reject_unknown_flags(&args)?;

    // A baseline is a *deterministic* artifact over a *committed* vault; blessing
    // one from a real-model run or a private vault would commit a number nobody
    // else can reproduce.
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

    // Work on a throwaway copy: indexing writes `.b2/` (and would stamp a missing
    // `b2id`), and the committed fixtures are never ours to mutate — the same
    // isolation the integration tests and `just compare-device` use.
    let tmp = tempfile::TempDir::new()?;
    let root = tmp.path().join("vault");
    copy_dir_all(&source, &root)?;

    let (vault, embedder_label) = if real_model {
        let config = EmbedConfig::load()?;
        provision(&config, |line| eprintln!("[init] {line}"))?;
        let embedder = LocalEmbedder::load(&config)?;
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
    if chunks < candidate_pool(BASELINE_K) {
        eprintln!(
            "[warn] {chunks} chunks < {}-candidate pool — this vault is too small for pool width to\n\
             \x20      bind, so every probe below is stable by construction, not by ranking quality\n\
             \x20      (GH #141). That is the eval corpus' situation; use a larger vault to measure.",
            candidate_pool(BASELINE_K)
        );
    }
    eprintln!();

    // One retrieval per (probe, depth); everything below is computed from these.
    let mut answers: Vec<Vec<Answer>> = Vec::with_capacity(probes.probes.len());
    for query in &probes.probes {
        let mut per_depth = Vec::with_capacity(DEPTHS.len());
        for depth in DEPTHS {
            per_depth.push(answer(&vault, query, depth)?);
        }
        answers.push(per_depth);
    }

    report_pool_sensitivity(&probes.probes, &answers, verbose);

    // The baseline is the deterministic mode's artifact over the committed vault;
    // any other run still gets the sensitivity report above, which needs no baseline.
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

/// Ask one probe at one depth, keeping only what identifies a result: the note path
/// and the chunk key. Scores are deliberately not compared — RRF scores move
/// whenever the pool does, so comparing them would report "different" for changes
/// that reorder nothing, and the question here is what the human *sees*.
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

/// A chunk's stable identity across index rebuilds: its note, its heading
/// breadcrumb, and the head of its text. Chunk rowids are per-build, so they cannot
/// key a committed baseline; the text head can, and it also makes a blessed diff
/// readable — a reviewer sees *which passage* moved, not an opaque id. Re-chunking
/// deliberately changes the key: a chunk cut differently is a different result.
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

/// Section 1: does the answer to a probe survive the pool widening under it?
fn report_pool_sensitivity(probes: &[String], answers: &[Vec<Answer>], verbose: bool) {
    println!("{}", "=".repeat(96));
    println!("pool sensitivity — the same query asked at widening retrieval pools");
    println!(
        "  a cell is how many of the top-{PREFIX} results the two asks agree on, position by position;"
    );
    println!("  `=` means the shallow answer is an exact prefix of the deep one (pool-invariant).");
    let steps: Vec<String> = DEPTHS
        .windows(2)
        .map(|w| format!("{}→{}", candidate_pool(w[0]), candidate_pool(w[1])))
        .collect();
    println!();
    let chunk_head = format!("chunks ({})", steps.join(" | "));
    let note_head = format!("notes ({})", steps.join(" | "));
    println!(
        "{:<PROBE_COL$}  {chunk_head:<CELL_COL$}  {note_head}",
        "probe"
    );
    println!("{}", "-".repeat(96));

    let mut moved_chunks = vec![0usize; steps.len()];
    let mut moved_notes = vec![0usize; steps.len()];
    for (i, probe) in probes.iter().enumerate() {
        let per_depth = &answers[i];
        let mut chunk_cells = Vec::new();
        let mut note_cells = Vec::new();
        for s in 0..DEPTHS.len() - 1 {
            let (lo, hi) = (&per_depth[s], &per_depth[s + 1]);
            let c = prefix_cell(&lo.chunks, &hi.chunks);
            let n = prefix_cell(&lo.notes, &hi.notes);
            if !c.stable {
                moved_chunks[s] += 1;
            }
            if !n.stable {
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
    for (s, step) in steps.iter().enumerate() {
        println!(
            "  {step:>9} candidates/signal: {}/{} probes changed their top-{PREFIX} chunks, {}/{} their top-{PREFIX} notes",
            moved_chunks[s],
            probes.len(),
            moved_notes[s],
            probes.len(),
        );
    }
    if moved_chunks.iter().all(|&m| m == 0) && moved_notes.iter().all(|&m| m == 0) {
        println!(
            "  every probe is pool-invariant here — either the vault is smaller than the pool\n\
             \x20 (the #141 blindness) or fusion width genuinely costs nothing on this corpus."
        );
    }
}

/// One depth-pair cell: how much of the shallow answer the deeper ask preserved.
struct Cell {
    label: String,
    stable: bool,
}

fn prefix_cell(shallow: &[String], deep: &[String]) -> Cell {
    let span = PREFIX.min(shallow.len()).min(deep.len());
    let same = (0..span).filter(|&i| shallow[i] == deep[i]).count();
    Cell {
        label: if same == span {
            format!("={span}")
        } else {
            format!("{same}/{span}")
        },
        stable: same == span,
    }
}

/// `--verbose`: the two rankings that disagreed, so the movement is inspectable
/// rather than a bare fraction. Only the chunk lists — they are the un-deduped view
/// the fusion actually produces.
fn print_divergence(per_depth: &[Answer]) {
    for w in 0..DEPTHS.len() - 1 {
        let (lo, hi) = (&per_depth[w], &per_depth[w + 1]);
        if prefix_cell(&lo.chunks, &hi.chunks).stable {
            continue;
        }
        println!(
            "      pool {} vs {}:",
            candidate_pool(DEPTHS[w]),
            candidate_pool(DEPTHS[w + 1])
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
    // The snapshot records what produced it, and that provenance is checked rather
    // than decorative: comparing today's ranking against one blessed from a
    // different vault or a different embedder would report drift that is really a
    // mismatch, and the reader would have no way to tell the two apart.
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
             *movement* — any change to fusion width, chunking or resolution shows up here as drift rather \
             than being inferred. It scores nothing: the corpus is unlabelled, so drift means different, \
             never worse. Regenerate with `just stability-bless` once a ranking change is the intended one."
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

/// Recursive copy — the fixture has topic subfolders, so the flat copy the scored
/// eval uses for its single-level corpus would silently drop 9 of 10 topics.
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

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// `--vault <path>` / `--vault=<path>`, erroring on a flag given without a value
/// rather than silently probing the default vault under a name the user did not mean.
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

/// A typo'd flag must not run a *different* measurement than the one asked for.
fn reject_unknown_flags(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    const KNOWN: [&str; 4] = ["--bless", "--model", "--verbose", "--vault"];
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if !arg.starts_with("--") {
            return Err(format!("unexpected argument {arg:?}").into());
        }
        let name = arg.split('=').next().unwrap_or(arg);
        if !KNOWN.contains(&name) {
            return Err(format!("unknown flag {arg:?}; known: {}", KNOWN.join(" ")).into());
        }
        skip_next = arg == "--vault";
    }
    Ok(())
}

/// Same directory on disk, canonicalized — so `--vault fixtures/test-vault` from the
/// repo root is recognized as the committed vault the baseline belongs to.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Paths are printed relative to the repo root where possible — an absolute
/// `/Users/…/B2/crates/…` line is noise in a report meant to be pasted into an issue.
///
/// A path that does not exist yet still resolves: the baseline is *named* before it
/// is written (`--bless` writes crates/…/stability-baseline.json), and canonicalize
/// only works on what exists — so a missing leaf falls back to its parent.
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

fn git_short_sha() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
