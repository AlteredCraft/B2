//! Semantic-retrieval + discovery eval — the "separate, occasional pass" scoring
//! model quality out of CI (the eval harness, crates/b2-embed/evals/). It lives as an **example**,
//! not a test, so it never runs in the deterministic `cargo test` suite and model
//! quality can never flake CI (invariants.md). Run it on
//! demand:
//!
//! ```console
//! cargo run -p b2-embed --example eval             # score the configured model
//! cargo run -p b2-embed --example eval -- --sweep  # + chunker A/B (the #44 gate)
//! ```
//!
//! One run builds a throwaway vault from the hand-labelled corpus in `evals/` and
//! scores four things through the real pipeline:
//!
//! 1. **BM25 baseline** — after `project` only (no vectors), every query is scored
//!    keyword-only. The labelled queries avoid their target's keywords, so this is
//!    the floor the model must clear.
//! 2. **Hybrid retrieval** — after `embed`, the same queries through BM25 ⊕ vector
//!    → RRF. The delta vs. the baseline is the **semantic lift** — the measured
//!    value of the one AI seam.
//! 3. **Passage rank** — queries labelled with a verbatim `passage` are also
//!    scored at **chunk** level (`Vault::search_chunks`): note-rank is blind to
//!    sub-note retrieval, which is exactly what chunking levers move
//!    (index-engine.md, GH #44).
//! 4. **Discovery** — `evals/similar.json` anchors score `Vault::similar` (the
//!    centroid-shortlisted candidate generation, #38), which query-retrieval alone
//!    does not exercise.
//!
//! `--sweep` re-chunks + re-embeds the same vault under variant [`ChunkConfig`]s
//! (`Vault::set_chunk_config` → `project(force)` → `embed`) and reports the same
//! scores per config — the in-process chunker A/B the #44 gate runs on.
//!
//! Every scored run appends one JSON line to `evals/results.jsonl` (gitignored),
//! so runs accumulate into a comparable dataset: "tune from numbers" needs the
//! numbers kept.

use b2_core::chunk::ChunkConfig;
use b2_core::embed::Embedder;
use b2_core::vault::Vault;
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use serde::Deserialize;
use std::ops::ControlFlow;
use std::path::Path;
use std::time::Instant;

/// How deep we look for a relevant note/chunk when scoring.
const K: usize = 10;
/// How many `similar` candidates we look at per anchor.
const SIM_K: usize = 5;
/// The soft reference floor on the default config's hybrid note hit@1.
const FLOOR_HIT1: f64 = 0.75;

#[derive(Deserialize)]
struct QuerySet {
    queries: Vec<Labelled>,
}

#[derive(Deserialize)]
struct Labelled {
    query: String,
    relevant: Vec<String>,
    /// A short verbatim phrase from the target passage; when present the query is
    /// also scored at chunk level (does a top-K chunk of a relevant note contain
    /// it?). See queries.json's description for the labelling rules.
    #[serde(default)]
    passage: Option<String>,
}

#[derive(Deserialize)]
struct SimilarSet {
    anchors: Vec<SimilarLabel>,
}

#[derive(Deserialize)]
struct SimilarLabel {
    anchor: String,
    expected: Vec<String>,
}

/// One query's ranks in one retrieval mode: 1-based note rank, 1-based chunk rank
/// (only for passage-labelled queries), and the top note hit for display.
struct QueryScore {
    note: Option<usize>,
    chunk: Option<usize>,
    top: String,
}

/// Running hit@1 / hit@3 / MRR@K over a set of 1-based ranks. ("hit@k" — each
/// query has essentially one relevant target, so precision@k and recall@k
/// coincide with it.)
#[derive(Default)]
struct Agg {
    n: usize,
    hit1: usize,
    hit3: usize,
    rr: f64,
}

impl Agg {
    fn add(&mut self, rank: Option<usize>) {
        self.n += 1;
        if let Some(r) = rank {
            self.rr += 1.0 / r as f64;
            if r <= 1 {
                self.hit1 += 1;
            }
            if r <= 3 {
                self.hit3 += 1;
            }
        }
    }
    fn hit1(&self) -> f64 {
        self.hit1 as f64 / self.n.max(1) as f64
    }
    fn hit3(&self) -> f64 {
        self.hit3 as f64 / self.n.max(1) as f64
    }
    fn mrr(&self) -> f64 {
        self.rr / self.n.max(1) as f64
    }
}

/// A full pass over the query set in the vault's current state (keyword-only
/// before `embed`, hybrid after): per-query scores plus note- and chunk-level
/// aggregates.
struct Pass {
    scores: Vec<QueryScore>,
    note: Agg,
    chunk: Agg,
}

fn main() {
    match run() {
        Err(e) => {
            eprintln!("eval failed: {e}");
            std::process::exit(1);
        }
        Ok(passed) => {
            if !passed {
                std::process::exit(2);
            }
        }
    }
}

/// Returns whether the default-config hybrid pass cleared the reference floor.
fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let sweep = std::env::args().any(|a| a == "--sweep");
    let evals_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
    let corpus_dir = evals_dir.join("corpus");
    let results_path = evals_dir.join("results.jsonl");

    // Load the labelled sets.
    let set: QuerySet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("queries.json"))?)?;
    let sim_set: SimilarSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("similar.json"))?)?;

    // Ensure the model is available, then load it. (Provision is idempotent, so an
    // already-installed model is a no-op; a missing one is fetched here.)
    let config = EmbedConfig::load()?;
    provision(&config, |line| eprintln!("[init] {line}"))?;
    let embedder = LocalEmbedder::load(&config)?;
    let model_id = embedder.model_id().to_string();
    let dim = embedder.dim();
    eprintln!("[eval] model = {model_id} (dim {dim})\n");

    // Build a throwaway vault from the corpus.
    let tmp = tempfile::TempDir::new()?;
    let vault_root = tmp.path().join("vault");
    std::fs::create_dir_all(&vault_root)?;
    for entry in std::fs::read_dir(&corpus_dir)? {
        let entry = entry?;
        // Regular files only: `fs::copy` errors on a directory, so a future
        // corpus/ subfolder (or any stray non-file) must not abort the run.
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), vault_root.join(entry.file_name()))?;
        }
    }
    let mut vault = Vault::open_with_embedder(&vault_root, Box::new(embedder))?;

    // ---- Phase 1: projection only → the BM25-only baseline. ------------------
    // The vector space does not exist yet, so `search`/`search_chunks` run
    // keyword-only (index-engine.md) — the ablation costs nothing
    // extra: it is the same vault, paused between the two passes.
    let report = vault.project(false)?;
    let bm25 = score_pass(&vault, &set.queries)?;
    eprintln!(
        "[eval] projected {} notes; BM25-only baseline scored\n",
        report.indexed
    );

    // ---- Phase 2: embed → hybrid + passage + discovery. ----------------------
    let (chunks, embed_secs) = timed_embed(&vault)?;
    let hybrid = score_pass(&vault, &set.queries)?;
    let similar = score_similar(&vault, &sim_set)?;
    eprintln!("[eval] embedded {chunks} chunks in {embed_secs:.1}s\n");

    print_default_report(&set.queries, &bm25, &hybrid, &sim_set, &similar);

    let git = git_short_sha();
    append_result(
        &results_path,
        result_row(
            &git,
            &model_id,
            dim,
            "default",
            &ChunkConfig::default(),
            report.indexed,
            chunks,
            embed_secs,
            &set.queries,
            Some(&bm25),
            &hybrid,
            Some(&similar),
        ),
    )?;

    // ---- Optional: the in-process chunker sweep (the #44 A/B). ---------------
    if sweep {
        let variants: Vec<(&str, ChunkConfig)> = vec![
            (
                "prepend-heading-path",
                ChunkConfig {
                    prepend_heading_path: true,
                    ..ChunkConfig::default()
                },
            ),
            (
                "target-250",
                ChunkConfig {
                    target_tokens: 250,
                    ..ChunkConfig::default()
                },
            ),
        ];
        println!("\n{}", "=".repeat(78));
        println!("chunker sweep (same model, same corpus; default row above for reference)");
        println!(
            "{:<22} {:>7} {:>8}   note h@1/MRR   chunk h@1/MRR   similar h@3",
            "config", "chunks", "embed_s"
        );
        for (label, cfg) in variants {
            vault.set_chunk_config(cfg.clone());
            vault.project(true)?; // force: re-chunk everything, clearing vectors
            let (chunks, embed_secs) = timed_embed(&vault)?;
            let pass = score_pass(&vault, &set.queries)?;
            let sim = score_similar(&vault, &sim_set)?;
            println!(
                "{:<22} {:>7} {:>8.1}   {:.2} / {:.3}    {:.2} / {:.3}    {:.2}",
                label,
                chunks,
                embed_secs,
                pass.note.hit1(),
                pass.note.mrr(),
                pass.chunk.hit1(),
                pass.chunk.mrr(),
                sim.hit3(),
            );
            append_result(
                &results_path,
                result_row(
                    &git,
                    &model_id,
                    dim,
                    label,
                    &cfg,
                    report.indexed,
                    chunks,
                    embed_secs,
                    &set.queries,
                    None,
                    &pass,
                    Some(&sim),
                ),
            )?;
        }
    }

    eprintln!("\n[eval] appended run to {}", results_path.display());

    // The soft floor, on the DEFAULT config's hybrid pass — so this can double as
    // a manual quality gate. Not a CI test.
    if hybrid.note.hit1() < FLOOR_HIT1 {
        eprintln!(
            "\n[warn] hybrid hit@1 {:.2} is below the {FLOOR_HIT1} reference floor — inspect the misses above.",
            hybrid.note.hit1()
        );
        return Ok(false);
    }
    Ok(true)
}

/// Score every labelled query against the vault's current state: note rank via
/// `search`, and — for passage-labelled queries — chunk rank via `search_chunks`
/// (the first top-K chunk that belongs to a relevant note AND contains the
/// labelled phrase, case-insensitively).
fn score_pass(vault: &Vault, queries: &[Labelled]) -> Result<Pass, Box<dyn std::error::Error>> {
    let mut scores = Vec::with_capacity(queries.len());
    let mut note_agg = Agg::default();
    let mut chunk_agg = Agg::default();
    for q in queries {
        let results = vault.search(&q.query, K)?;
        let note = results
            .iter()
            .position(|r| q.relevant.iter().any(|rel| paths_match(&r.path, rel)))
            .map(|p| p + 1);
        let top = results
            .first()
            .map(|r| r.path.clone())
            .unwrap_or_else(|| "—".to_string());
        note_agg.add(note);

        let chunk = match &q.passage {
            None => None,
            Some(passage) => {
                let needle = passage.to_lowercase();
                let hits = vault.search_chunks(&q.query, K)?;
                let rank = hits
                    .iter()
                    .position(|h| {
                        q.relevant.iter().any(|rel| paths_match(&h.path, rel))
                            && h.text.to_lowercase().contains(&needle)
                    })
                    .map(|p| p + 1);
                chunk_agg.add(rank);
                rank
            }
        };
        scores.push(QueryScore { note, chunk, top });
    }
    Ok(Pass {
        scores,
        note: note_agg,
        chunk: chunk_agg,
    })
}

/// Score the discovery labels: for each anchor, the 1-based rank of the first
/// `expected` note among its top `SIM_K` `similar` candidates.
fn score_similar(vault: &Vault, set: &SimilarSet) -> Result<Agg, Box<dyn std::error::Error>> {
    let mut agg = Agg::default();
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, SIM_K)?;
        let rank = candidates
            .iter()
            .position(|c| label.expected.iter().any(|e| paths_match(&c.path, e)))
            .map(|p| p + 1);
        agg.add(rank);
    }
    Ok(agg)
}

/// Run the embed pass, timing it and counting the chunks it filled.
fn timed_embed(vault: &Vault) -> Result<(usize, f64), Box<dyn std::error::Error>> {
    let mut chunks = 0usize;
    let t0 = Instant::now();
    vault.embed(&mut |p| {
        chunks = p.chunks_done;
        ControlFlow::Continue(())
    })?;
    Ok((chunks, t0.elapsed().as_secs_f64()))
}

fn print_default_report(
    queries: &[Labelled],
    bm25: &Pass,
    hybrid: &Pass,
    sim_set: &SimilarSet,
    similar: &Agg,
) {
    println!(
        "{:>5} {:>6} {:>6}  {:<40}  top hybrid hit",
        "bm25", "hybrid", "chunk", "query"
    );
    println!("{}", "-".repeat(96));
    for (i, q) in queries.iter().enumerate() {
        println!(
            "{:>5} {:>6} {:>6}  {:<40}  {}",
            rank_str(bm25.scores[i].note),
            rank_str(hybrid.scores[i].note),
            match q.passage {
                Some(_) => rank_str(hybrid.scores[i].chunk),
                None => "".to_string(),
            },
            truncate(&q.query, 40),
            hybrid.scores[i].top,
        );
    }

    println!("\n{}", "=".repeat(78));
    println!("note rank (n={}, K={K}):", queries.len());
    println!(
        "  bm25-only  hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}",
        bm25.note.hit1(),
        bm25.note.hit3(),
        bm25.note.mrr()
    );
    println!(
        "  hybrid     hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}   semantic lift: {:+.2} hit@1",
        hybrid.note.hit1(),
        hybrid.note.hit3(),
        hybrid.note.mrr(),
        hybrid.note.hit1() - bm25.note.hit1(),
    );
    if hybrid.chunk.n > 0 {
        println!("chunk rank (passage-labelled, n={}):", hybrid.chunk.n);
        println!(
            "  bm25-only  hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}",
            bm25.chunk.hit1(),
            bm25.chunk.hit3(),
            bm25.chunk.mrr()
        );
        println!(
            "  hybrid     hit@1={:.2}  hit@3={:.2}  MRR@{K}={:.3}",
            hybrid.chunk.hit1(),
            hybrid.chunk.hit3(),
            hybrid.chunk.mrr()
        );
    }
    println!("similar (n={}, K={SIM_K}):", sim_set.anchors.len());
    println!(
        "  discovery  hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}",
        similar.hit1(),
        similar.hit3(),
        similar.mrr()
    );
}

/// One appendable JSONL row for a scored configuration.
#[allow(clippy::too_many_arguments)]
fn result_row(
    git: &Option<String>,
    model: &str,
    dim: usize,
    label: &str,
    cfg: &ChunkConfig,
    notes: usize,
    chunks: usize,
    embed_secs: f64,
    queries: &[Labelled],
    bm25: Option<&Pass>,
    hybrid: &Pass,
    similar: Option<&Agg>,
) -> serde_json::Value {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let agg = |a: &Agg| serde_json::json!({ "n": a.n, "hit1": a.hit1(), "hit3": a.hit3(), "mrr": a.mrr() });
    serde_json::json!({
        "ts": ts,
        "git": git,
        "model": model,
        "dim": dim,
        "config": {
            "label": label,
            "target_tokens": cfg.target_tokens,
            "overlap_frac": cfg.overlap_frac,
            "chars_per_token": cfg.chars_per_token,
            "backscan_tokens": cfg.backscan_tokens,
            "prepend_heading_path": cfg.prepend_heading_path,
        },
        "notes": notes,
        "chunks": chunks,
        "embed_secs": embed_secs,
        "note": {
            "bm25": bm25.map(|p| agg(&p.note)),
            "hybrid": agg(&hybrid.note),
        },
        "chunk": {
            "bm25": bm25.map(|p| agg(&p.chunk)),
            "hybrid": agg(&hybrid.chunk),
        },
        "similar": similar.map(agg),
        "queries": queries.iter().enumerate().map(|(i, q)| serde_json::json!({
            "q": q.query,
            "bm25": bm25.map(|p| p.scores[i].note),
            "hybrid": hybrid.scores[i].note,
            "chunk": hybrid.scores[i].chunk,
        })).collect::<Vec<_>>(),
    })
}

/// Append one row to the results log (creating it on first run). Append-only, so
/// runs accumulate into one dataset — the same convention as `B2_LOG_FILE`.
fn append_result(path: &Path, row: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{row}")?;
    Ok(())
}

/// The repo's short commit hash, best-effort (None outside a git checkout).
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

/// Corpus notes are copied flat into the vault, so a result path equals (or ends
/// with) the labelled relevant path.
fn paths_match(result_path: &str, relevant: &str) -> bool {
    result_path == relevant || result_path.ends_with(&format!("/{relevant}"))
}

fn rank_str(rank: Option<usize>) -> String {
    match rank {
        Some(1) => "✓1".to_string(),
        Some(r) => format!("·{r}"),
        None => format!("✗>{K}"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
