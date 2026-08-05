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
//!    does not exercise. Positive anchors score **rank** (did a labelled
//!    cluster-mate surface, and how high). **Negative anchors** — deliberate loner
//!    notes whose labelled answer is "nothing relates" — score **suppression**:
//!    does discovery return zero candidates, or serve strangers? And every
//!    surfaced candidate's score lands in one of two **cosine piles** — labelled
//!    related vs. everything else surfaced — the measured distributions the
//!    quality floor is calibrated from (index-engine.md §3's ruling, PR #145).
//!    Until that floor lands in `discover::candidates` the suppression metric is
//!    red by design: it is the failing target the floor is built against.
//!
//! What this corpus **cannot** score is *candidate width*. 29 chunks is no more
//! than the candidates each signal retrieves — `chunk_candidate_pool(K)` for the
//! passage view, `note_candidate_pool(K)` for the note view, the narrower of the two
//! being what has to bind — so neither list is truncated, widening the pool cannot
//! add a candidate, and every number above is invariant under either view's headroom
//! or `search::pool_size` — a change to any of them
//! prints bit-identical scores here while reordering a real vault (GH #141). A run
//! that is blind that way says so; the property is measured by the rank-stability
//! probe (`--example stability`) on a vault big enough for the pool to bind. Note
//! the scope: `RRF_K` re-weights the same lists rather than changing them, so it
//! *does* move scores here and needs no separate instrument.
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
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool, Vault};
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
    /// Corpus notes a human says belong next to `anchor`. **Empty = a negative
    /// anchor**: the labelled answer is "nothing relates", so the right result is
    /// zero candidates and everything surfaced is junk by label (similar.json).
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

/// A full pass over the discovery labels: rank aggregates over the positive
/// anchors, the suppression tally over the negative ones, and every surfaced
/// candidate's cosine sorted into the two calibration piles the quality floor
/// is read from (index-engine.md §3's ruling, PR #145).
#[derive(Default)]
struct SimilarPass {
    /// Rank of the first `expected` hit per positive anchor — the pre-existing
    /// hit@1 / hit@3 / MRR discovery metrics, unchanged.
    rank: Agg,
    /// Negative anchors asked.
    neg_n: usize,
    /// Negative anchors that (correctly) surfaced zero candidates.
    neg_clean: usize,
    /// Candidates surfaced across all negative anchors — cards whose labelled
    /// answer was "nothing".
    neg_cards: usize,
    /// Cosines of surfaced candidates a human labelled genuinely related.
    related: Vec<f64>,
    /// Cosines of everything else surfaced — non-expected candidates of positive
    /// anchors, and every candidate of a negative anchor.
    junk: Vec<f64>,
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

    // A correctness gate, not a score: every number below is computed from batched
    // embeddings, so they only mean anything if batching is faithful.
    check_batch_matches_single(&embedder)?;

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
    eprintln!(
        "[eval] embedded {chunks} chunks in {embed_secs:.1}s ({} candidates per signal at K={K}, \
         {} for the passage view)\n",
        note_candidate_pool(K),
        chunk_candidate_pool(K)
    );
    warn_if_pool_blind(chunks);

    print_default_report(&set.queries, &bm25, &hybrid, &similar);

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
            "{:<22} {:>7} {:>8}   note h@1/MRR   chunk h@1/MRR   similar h@3   neg clean",
            "config", "chunks", "embed_s"
        );
        for (label, cfg) in variants {
            vault.set_chunk_config(cfg.clone());
            vault.project(true)?; // force: re-chunk everything, clearing vectors
            let (chunks, embed_secs) = timed_embed(&vault)?;
            let pass = score_pass(&vault, &set.queries)?;
            let sim = score_similar(&vault, &sim_set)?;
            println!(
                "{:<22} {:>7} {:>8.1}   {:.2} / {:.3}    {:.2} / {:.3}    {:.2}          {}/{}",
                label,
                chunks,
                embed_secs,
                pass.note.hit1(),
                pass.note.mrr(),
                pass.chunk.hit1(),
                pass.chunk.mrr(),
                sim.rank.hit3(),
                sim.neg_clean,
                sim.neg_n,
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
    // a manual quality gate. Not a CI test. The discovery suppression metric is
    // deliberately NOT part of this gate yet: it is red by design until the
    // quality floor lands in `discover::candidates` (index-engine.md §3, PR #145),
    // and gating on it now would exit non-zero on every run, burying the
    // FLOOR_HIT1 signal the chunker sweep reads. When the floor ships, fold
    // `similar.neg_clean == similar.neg_n` in here.
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

/// Score the discovery labels. Positive anchors (non-empty `expected`): the
/// 1-based rank of the first `expected` note among the top `SIM_K` `similar`
/// candidates. Negative anchors (empty `expected`): a clean result is zero
/// candidates; anything surfaced is a stranger served where the labelled answer
/// is "nothing". Every surfaced candidate's score also lands in a cosine pile —
/// `related` if a human labelled it, `junk` otherwise — the distributions the
/// quality floor's cutoff is read from.
fn score_similar(
    vault: &Vault,
    set: &SimilarSet,
) -> Result<SimilarPass, Box<dyn std::error::Error>> {
    let mut pass = SimilarPass::default();
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, SIM_K)?;
        if label.expected.is_empty() {
            pass.neg_n += 1;
            if candidates.is_empty() {
                pass.neg_clean += 1;
            }
            pass.neg_cards += candidates.len();
            pass.junk
                .extend(candidates.iter().map(|c| cosine_of(c.score)));
            continue;
        }
        let rank = candidates
            .iter()
            .position(|c| label.expected.iter().any(|e| paths_match(&c.path, e)))
            .map(|p| p + 1);
        pass.rank.add(rank);
        for c in &candidates {
            if label.expected.iter().any(|e| paths_match(&c.path, e)) {
                pass.related.push(cosine_of(c.score));
            } else {
                pass.junk.push(cosine_of(c.score));
            }
        }
    }
    Ok(pass)
}

/// A `similar` score is negated L2 distance between L2-normalized vectors
/// (the real embedder normalizes every row), so it converts exactly:
/// `cos = 1 − d²/2`. Cosine is the unit the floor ruling is stated in and the
/// unit that survives a model swap comparison, so the piles are recorded in it.
fn cosine_of(score: f64) -> f64 {
    1.0 - (score * score) / 2.0
}

/// (min, median, max) of a pile, or None while it's empty.
fn pile_stats(pile: &[f64]) -> Option<(f64, f64, f64)> {
    if pile.is_empty() {
        return None;
    }
    let mut sorted = pile.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((
        sorted[0],
        sorted[sorted.len() / 2],
        sorted[sorted.len() - 1],
    ))
}

/// `LocalEmbedder::embed_batch` must be a faithful map of `embed`: right-padding
/// short rows to the batch's longest and masking them out has to leave each row's
/// CLS vector unchanged. The reindex path batches freely, so a regression here
/// would silently corrupt every stored vector — and every score this eval prints.
///
/// This lives in the eval rather than in `cargo test` because it needs the
/// provisioned model, which the fast suite deliberately never touches (root
/// `CLAUDE.md`, "Keep `cargo test` fast, deterministic, and model-free"). Running
/// it here means it actually runs, on every `just eval`, instead of sitting behind
/// an `#[ignore]` nobody passes `--ignored` to.
fn check_batch_matches_single(model: &LocalEmbedder) -> Result<(), Box<dyn std::error::Error>> {
    // Deliberately varied lengths, so batching pads the short rows to the longest.
    let texts = [
        "Spaced repetition schedules reviews at increasing intervals.",
        "Sleep consolidates memory.",
        "Short.",
        "Focus and sustained attention shape what is later recalled from long-term memory across days.",
    ];
    let refs: Vec<&str> = texts.to_vec();
    let batched = model.embed_batch(&refs)?;
    if batched.len() != texts.len() {
        return Err(format!(
            "embed_batch returned {} rows for {} texts",
            batched.len(),
            texts.len()
        )
        .into());
    }
    let mut worst = f32::INFINITY;
    for (text, batched_row) in texts.iter().zip(&batched) {
        let single = model.embed(text)?;
        if batched_row.len() != single.len() {
            return Err(format!("batched/single dim mismatch for {text:?}").into());
        }
        // Both rows are L2-normalized, so the dot product is cosine similarity;
        // padding must not move it off ~1.0. Non-finite is checked first and
        // explicitly: every comparison against a NaN is false, so `cos <= 0.9999`
        // alone would wave a NaN row *through* the gate — the one failure mode a
        // correctness check must not have.
        let cos: f32 = batched_row.iter().zip(&single).map(|(a, b)| a * b).sum();
        if !cos.is_finite() {
            return Err(format!("batched embedding is non-finite for {text:?}: {cos}").into());
        }
        worst = worst.min(cos);
        if cos <= 0.9999 {
            return Err(format!(
                "batched embedding differs from single for {text:?}: cosine {cos}"
            )
            .into());
        }
    }
    eprintln!("[eval] batch ≡ single: worst-row cosine {worst:.6}\n");
    Ok(())
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

/// State the one thing this corpus **cannot** measure, on every run that can't.
///
/// Retrieval pulls [`note_candidate_pool`] candidates from each signal before
/// fusing, or [`chunk_candidate_pool`] for the passage view. A corpus with no more
/// chunks than that truncates *neither* list — BM25 has fewer matches than its
/// `LIMIT`, the vector scan tops out at the stored vectors — so both lists are
/// already complete, widening the pool cannot add a candidate, and every score above
/// is invariant under **candidate width**. A change to either view's headroom or to
/// `search::pool_size` then prints bit-identical numbers here while genuinely
/// reordering a real vault (GH #141; the worked example is GH #140/#142).
///
/// Judged on the **narrower** of the two pools, which since #142 is the passage
/// view's: blindness is a claim about every number the run prints, and a corpus that
/// fits inside the narrower pool fits inside the wider one too.
///
/// Scoped deliberately to width. `RRF_K` re-weights the *same* two lists
/// (`Σ 1/(k+rank+1)`), so it reorders results on any corpus — measured on this one:
/// k = 60 → 10 moves note ranks across the query set. This eval sees that; it is
/// only blind to candidates it was never going to be handed.
///
/// A warning, not a gate: this eval is out-of-CI and human-read, and the point is
/// that a reader must not take an unmoved number as evidence of no change. The
/// property itself is measured by the rank-stability probe (`--example stability`),
/// which runs on a vault big enough for the pool to bind.
fn warn_if_pool_blind(chunks: usize) {
    let pool = chunk_candidate_pool(K).min(note_candidate_pool(K));
    if chunks <= pool {
        eprintln!(
            "[warn] {chunks} chunks ≤ {pool}-candidate pool — neither signal is truncated here, so a\n\
             \x20      candidate-width change (either hit pool, pool_size) cannot move any number in this run\n\
             \x20      (GH #141). `just stability` measures that property on a large vault. (RRF_K is\n\
             \x20      not in that set — it re-weights the same lists, and this corpus does see it.)\n"
        );
    }
}

fn print_default_report(queries: &[Labelled], bm25: &Pass, hybrid: &Pass, similar: &SimilarPass) {
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
    println!(
        "similar (n={} positive + {} negative, K={SIM_K}):",
        similar.rank.n, similar.neg_n
    );
    println!(
        "  discovery  hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}",
        similar.rank.hit1(),
        similar.rank.hit3(),
        similar.rank.mrr()
    );
    if similar.neg_n > 0 {
        // Suppression: a clean negative anchor surfaced zero candidates. Red by
        // design until the quality floor lands (index-engine.md §3, PR #145).
        println!(
            "  negatives  {}/{} anchors clean — {} cards surfaced where the label says \"nothing\"",
            similar.neg_clean, similar.neg_n, similar.neg_cards
        );
    }
    // The two calibration piles. If they separate, the gap IS the floor, read
    // off measured data; if they overlap, no simple floor can hold and the
    // escalation path (a discovery-side pair-scorer) is justified by data.
    if let (Some((r_min, r_med, r_max)), Some((j_min, j_med, j_max))) =
        (pile_stats(&similar.related), pile_stats(&similar.junk))
    {
        println!(
            "  piles(cos) related n={:<3} min/med/max {:.3}/{:.3}/{:.3}",
            similar.related.len(),
            r_min,
            r_med,
            r_max
        );
        println!(
            "             junk    n={:<3} min/med/max {:.3}/{:.3}/{:.3}",
            similar.junk.len(),
            j_min,
            j_med,
            j_max
        );
        let gap = r_min - j_max;
        println!(
            "             related-min − junk-max = {:+.3} ({})",
            gap,
            if gap > 0.0 {
                "piles separate — the gap is the floor"
            } else {
                "piles overlap — a simple floor cuts both"
            }
        );
    }
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
    similar: Option<&SimilarPass>,
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
        // The caveat travels with the numbers: a row whose corpus fit inside the
        // retrieval pool had both candidate lists complete, so no candidate-width
        // change could have affected it and comparing it across one proves nothing
        // (GH #141). Equality is blind too — a pool exactly the size of the corpus
        // truncates nothing either. Both depths are recorded because since #142 the
        // two views differ; the flat `pool` key of earlier rows is deliberately
        // *not* reused, so a reader of a mixed file sees a missing field rather than
        // one number silently meaning something narrower than it used to.
        "pool_note": note_candidate_pool(K),
        "pool_chunk": chunk_candidate_pool(K),
        "pool_blind": chunks <= chunk_candidate_pool(K).min(note_candidate_pool(K)),
        "embed_secs": embed_secs,
        "note": {
            "bm25": bm25.map(|p| agg(&p.note)),
            "hybrid": agg(&hybrid.note),
        },
        "chunk": {
            "bm25": bm25.map(|p| agg(&p.chunk)),
            "hybrid": agg(&hybrid.chunk),
        },
        // "similar" keeps its pre-negative shape (the positive anchors' rank agg)
        // so rows stay comparable across the change; the negative-anchor tally and
        // the calibration piles are NEW keys, absent from older rows rather than
        // redefining an existing one — the same convention as pool_note/pool_chunk.
        "similar": similar.map(|s| agg(&s.rank)),
        "similar_negatives": similar.map(|s| serde_json::json!({
            "n": s.neg_n, "clean": s.neg_clean, "cards": s.neg_cards,
        })),
        // Cosine, 4 decimals: enough to place a floor, short enough to keep rows
        // readable. Related = human-labelled matches; junk = everything else
        // surfaced (see score_similar).
        "similar_piles": similar.map(|s| serde_json::json!({
            "related": s.related.iter().map(|c| (c * 1e4).round() / 1e4).collect::<Vec<_>>(),
            "junk": s.junk.iter().map(|c| (c * 1e4).round() / 1e4).collect::<Vec<_>>(),
        })),
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
