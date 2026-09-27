//! Semantic-retrieval + discovery eval — the pass that scores model quality **out of CI**
//! (ADR-0013). It lives as an *example*, not a test, so it never runs in the deterministic
//! `cargo test` suite and quality can never flake CI.
//!
//! ```console
//! cargo run -p b2-embed --example eval               # score the configured model
//! cargo run -p b2-embed --example eval -- --sweep    # + chunker A/B (the #44 gate)
//! cargo run -p b2-embed --example eval -- --stemmer  # + FTS tokenizer A/B (the #157 gate)
//! ```
//!
//! Any other argument is refused (exit 1), so a typo'd flag can't pass for an A/B.
//!
//! `docs/evals.md` is the notebook of record: the corpus, what the exit code enforces, every
//! verdict, and the process rules. Read it before touching the corpus, labels or constants.
//!
//! One run builds throwaway vaults from `evals/` and scores, through the real pipeline, a
//! BM25 baseline, hybrid retrieval plus a vector-only ablation (GH #158), passage rank, and
//! discovery per labelled mate (GH #183). Calibration windows (the z dump, GH #187; the
//! search evidence bake-off, ADR-0015) are re-derived every run, never quoted in code. The
//! dense fixture (`evals/corpus-dense/`) is scored in its own vault and row, never averaged
//! in. Candidate width is mostly out of reach at this corpus size (`pool_blind`, GH #141);
//! `--example stability` measures it. Each run appends one line to `evals/results.jsonl`.
//!
//! This file is the run's order of operations; each report block lives in the module named
//! for it.

// `result_row`'s `json!` literal (`report.rs`) outgrows the default limit; raising it keeps
// every recorded field readable in one place.
#![recursion_limit = "256"]

mod ablation;
#[path = "../common/mod.rs"]
mod common;
mod dense;
mod discovery;
mod evidence;
mod fold;
mod gate;
mod instrument;
mod labels;
mod metrics;
mod report;
mod retrieval;
mod tail;

use ablation::Baseline;
use b2_core::chunk::ChunkConfig;
use b2_core::db::FtsTokenizer;
use b2_core::embed::Embedder;
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool, Vault};
use b2_embed::EmbedConfig;
use common::{
    append_result, git_short_sha, has_flag, load_or_provision, reject_unknown_flags, ScratchVault,
};
use dense::{dense_row, print_dense_report, score_dense};
use discovery::{score_floor_z, score_similar};
use evidence::{bake_off, print_search_bakeoff, print_search_evidence, score_search_evidence};
use fold::{print_fold_bench, score_fold};
use instrument::{check_batch_matches_single, timed_embed, warn_if_pool_blind, SharedEmbedder};
use labels::{lint_labels, Labelled, QuerySet, SimilarSet};
use report::{print_default_report, result_row, Calibration, RowInputs, RunId};
use retrieval::{score_pass, Retrieval};
use std::path::Path;
use tail::{print_search_tail, print_tail_join, score_search_tail};

/// How deep we look for a relevant note/chunk when scoring.
const K: usize = 10;

/// How many `similar` candidates we look at per anchor.
const SIM_K: usize = 5;

/// How deep the z dump reads (GH #187): every candidate, since a threshold is calibrated
/// against the whole population it cuts.
const Z_SCAN_LIMIT: usize = 500;

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

/// Returns whether the default config cleared every exit-gate row ([`gate::passes`]).
fn run() -> Result<bool, Box<dyn std::error::Error>> {
    // A bare `--` is dropped so a pasted `cargo run … -- --sweep` works.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--").collect();
    reject_unknown_flags(&args, &["--sweep", "--stemmer"], &[])?;
    let sweep = has_flag(&args, "--sweep");
    let stemmer = has_flag(&args, "--stemmer");
    let evals_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
    let corpus_dir = evals_dir.join("corpus");
    let results_path = evals_dir.join("results.jsonl");

    // An empty `relevant` is a negative query (D2, GH #201), scored only by the search
    // evidence calibration so it moves no rank aggregate.
    let set: QuerySet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("queries.json"))?)?;
    let (negatives, positives): (Vec<Labelled>, Vec<Labelled>) =
        set.queries.into_iter().partition(|q| q.relevant.is_empty());
    let sim_set: SimilarSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("similar.json"))?)?;
    let dense_set: SimilarSet = serde_json::from_str(&std::fs::read_to_string(
        evals_dir.join("similar-dense.json"),
    )?)?;

    // A typo'd label path reads as a permanent miss, not an error, so lint first (exit 1).
    lint_labels(
        &corpus_dir,
        &evals_dir.join("corpus-dense"),
        &positives,
        &negatives,
        &sim_set,
        &dense_set,
    )?;

    // Loaded once; both corpora's vaults share it.
    let config = EmbedConfig::load()?;
    let embedder = SharedEmbedder::new(load_or_provision(&config)?);
    let model_id = embedder.model_id().to_string();
    let dim = embedder.dim();
    eprintln!("[eval] model = {model_id} (dim {dim})\n");

    // Every number below comes from batched embeddings, so check batching first.
    check_batch_matches_single(&embedder)?;

    let scratch = ScratchVault::copy_flat(&corpus_dir)?;
    let vault_root = scratch.root();
    let mut vault = Vault::open_with_embedder(vault_root, Box::new(embedder.clone()))?;

    // ---- Phase 1: projection only → the BM25-only baseline. ------------------
    // No vectors yet, so search runs keyword-only (index-engine.md).
    let report = vault.project(false)?;
    let bm25 = score_pass(&vault, &positives, Retrieval::Fused)?;
    eprintln!(
        "[eval] projected {} notes; BM25-only baseline scored\n",
        report.indexed
    );

    // Scored here, before embedding, so both tokenizers are BM25-only; restored after.
    let bm25_unstemmed = if stemmer {
        vault.rebuild_fts(FtsTokenizer::Unicode61)?;
        let pass = score_pass(&vault, &positives, Retrieval::Fused)?;
        vault.rebuild_fts(FtsTokenizer::PorterUnicode61)?;
        Some(pass)
    } else {
        None
    };

    // ---- Phase 2: embed → vector-only ablation + hybrid + discovery. ---------
    let (chunks, embed_secs) = timed_embed(&vault)?;
    let vector = score_pass(&vault, &positives, Retrieval::VectorOnly)?;
    let hybrid = score_pass(&vault, &positives, Retrieval::Fused)?;
    let similar = score_similar(&vault, &sim_set)?;
    // The fold bake-off (GH #200), on the same served lists as `similar`.
    let fold = score_fold(&vault, &sim_set, "orthogonal", false)?;
    // The z calibration dump (GH #187, #197).
    let floor_z = score_floor_z(&vault, &sim_set)?;
    // The search evidence dump (D2, GH #201).
    let evidence = score_search_evidence(vault_root, &vault, &positives, &negatives)?;
    eprintln!(
        "[eval] embedded {chunks} chunks in {embed_secs:.1}s ({} candidates per signal at K={K}, \
         {} for the passage view)\n",
        note_candidate_pool(K),
        chunk_candidate_pool(K)
    );
    warn_if_pool_blind(chunks);

    print_default_report(&positives, &bm25, &vector, &hybrid, &similar, &floor_z);
    print_fold_bench(&fold);
    print_search_evidence(&evidence);
    print_search_bakeoff(&evidence, &bake_off(&evidence), &model_id);
    // The per-hit tail bake-off (GH #206), from the evidence dump's served lists.
    let tail = score_search_tail(&evidence);
    print_search_tail(&tail);

    let git = git_short_sha();
    let run_id = RunId {
        git: git.as_deref(),
        model: &model_id,
        dim,
    };
    append_result(
        &results_path,
        &result_row(
            run_id,
            RowInputs {
                label: "default",
                cfg: &ChunkConfig::default(),
                tokenizer: FtsTokenizer::PorterUnicode61.sql(),
                notes: report.indexed,
                chunks,
                embed_secs,
                queries: &positives,
                bm25: Some(&bm25),
                vector: Some(&vector),
                hybrid: &hybrid,
                similar: Some(&similar),
                calibration: Some(Calibration {
                    floor_z: &floor_z,
                    evidence: &evidence,
                    fold: &fold,
                    tail: &tail,
                }),
            },
        ),
    )?;

    // ---- Phase 3: the dense single-domain fixture (GH #196/#197, Phase 0b). --
    // Its own vault and results row: it measures vault-level geometry, so it shares no
    // state with the run above.
    let dense = score_dense(&evals_dir, &dense_set, embedder)?;
    print_dense_report(&dense);
    print_fold_bench(&dense.fold);
    append_result(&results_path, &dense_row(run_id, &dense))?;
    // Needs both corpora's readings (GH #206).
    print_tail_join(&evidence, &tail, &dense.search.titles);

    // ---- Optional: the A/Bs, each against the default passes above. ----------
    let base = Baseline {
        log: &results_path,
        run: run_id,
        notes: report.indexed,
        chunks,
        embed_secs,
        queries: &positives,
        bm25: &bm25,
        vector: &vector,
        hybrid: &hybrid,
    };
    // Scored only under `--stemmer`, while the vault was still unembedded.
    if let Some(bm25_unstemmed) = &bm25_unstemmed {
        ablation::stemmer(&vault, &base, bm25_unstemmed)?;
    }
    if sweep {
        ablation::sweep(&mut vault, &base, &sim_set)?;
    }

    eprintln!("\n[eval] appended run to {}", results_path.display());

    // The exit gate reads the DEFAULT config's passes only — never an A/B's.
    Ok(gate::passes(
        &hybrid, &similar, &dense, &evidence, &model_id,
    ))
}
