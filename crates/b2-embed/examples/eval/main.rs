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
//! Any other argument is refused (exit 1) rather than ignored: a typo'd `--swep` would
//! otherwise score only the default config while the reader believes the A/B ran.
//!
//! **`docs/evals.md` is the notebook of record** — the corpus, what the exit code
//! enforces, every verdict this harness has ruled, and the process rules. Read it before
//! touching the corpus, the labels, or a constant here. What this comment carries is only
//! what a reader of *this file* needs:
//!
//! One run builds throwaway vaults from the labelled corpora in `evals/` and scores, through
//! the real pipeline: a **BM25 baseline** (after `project` only — the floor the model must
//! clear, since the labelled queries avoid their target's keywords); **hybrid retrieval**
//! plus a **vector-only ablation** (GH #158), whose delta is the measured value of the one
//! AI seam; **passage rank** at chunk level, which is where chunking levers show; and
//! **discovery**, scored per labelled mate rather than per anchor (GH #183 — the per-anchor
//! metric saturates), with the strangers a positive anchor serves counted, named, and
//! deliberately ungated, since the cheapest way to shrink that count is to label one.
//!
//! Two calibration blocks re-derive their windows **every run** rather than quoting a
//! reading: the discovery **z dump** (GH #187) and the **search evidence bake-off**
//! (ADR-0015, GH #201/#202). That is the house rule — constants in code, measurements in the
//! harness — and it exists because the GH #150 floors were frozen into a docstring and went
//! stale the first time the corpus grew a shape they were never read against.
//!
//! The **dense single-domain fixture** (`evals/corpus-dense/`) is scored in its own vault
//! and its own row, never averaged in: fifteen genuinely inter-related notes with no loner,
//! the geometry that broke every anchor-local existence gate (ADR-0014) and killed the first
//! lexical evidence rule (ADR-0015). The orthogonal corpus is structurally incapable of
//! expressing topical concentration, so a run that judges a bar only there judges it on the
//! geometry it survives.
//!
//! What this corpus mostly **cannot** score is *candidate width*: while its chunk count sits
//! at or under a signal's candidate pool, that list is never truncated, widening it cannot
//! add a candidate, and every number is invariant under that view's headroom and
//! `search::pool_size` (GH #141). The corpus has since grown a few chunks past the narrower
//! (passage-view) pool (GH #183), so the run reads the live counts and warns (`pool_blind`)
//! only when blindness actually holds; the property itself is measured by
//! `--example stability`, on a vault big enough for the pools to bind. `RRF_K` re-weights
//! the *same* lists, so it does move scores here and needs no separate instrument.
//!
//! `--sweep` re-chunks + re-embeds the same vault under variant [`ChunkConfig`]s. `--stemmer`
//! swaps `chunks_fts` between the shipped `porter unicode61` and the unstemmed ablation over
//! **identical** chunk rows and vectors, so every rank move is the tokenizer's alone.
//!
//! Every scored run appends one JSON line to `evals/results.jsonl` (gitignored), so runs
//! accumulate into a comparable dataset.
//!
//! This file is the run's order of operations and nothing else; each block of the report
//! lives in the module named for it — `labels` (the sets and their lint), `retrieval`
//! and `metrics` (the per-query passes), `discovery` (per-mate ranks, strangers, the z
//! dump), `fold` (the GH #200 bake-off), `evidence` (the D2 bar's calibration and
//! bake-off), `tail` (the GH #206 bake-off and its cross-bench join), `dense` (the
//! single-domain fixture), `ablation` (`--stemmer`/`--sweep`), `report` (the default
//! table and the JSONL row), `instrument` (the checks the numbers rest on), and `gate`
//! (the exit gate's constants and assertions).

// `result_row`'s JSON literal (`report.rs`) is one `json!` expansion per key, and the row has
// grown a key per instrument (GH #158, #141, #183, #187, #188). Raising the
// limit keeps the row's shape legible in one place — the alternative is
// scattering its subtrees across helper functions to satisfy a macro, which
// costs the thing the row is for: a reader seeing every recorded field at once.
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
use b2_embed::{provision, EmbedConfig, LocalEmbedder};
use common::{append_result, git_short_sha, has_flag, reject_unknown_flags, ScratchVault};
use dense::{dense_row, print_dense_report, score_dense};
use discovery::{score_floor_z, score_similar};
use evidence::{bake_off, print_search_bakeoff, print_search_evidence, score_search_evidence};
use fold::{print_fold_bench, score_fold};
use instrument::{check_batch_matches_single, timed_embed, warn_if_pool_blind};
use labels::{lint_labels, Labelled, QuerySet, SimilarSet};
use report::{print_default_report, result_row, Calibration, RowInputs, RunId};
use retrieval::{score_pass, Retrieval};
use std::path::Path;
use tail::{print_search_tail, print_tail_join, score_search_tail};

/// How deep we look for a relevant note/chunk when scoring.
const K: usize = 10;

/// How many `similar` candidates we look at per anchor.
const SIM_K: usize = 5;

/// How deep the z dump reads (GH #187) — every candidate note in a corpus this
/// size, since a threshold is calibrated against the whole population it has to
/// cut, not against the prefix a human-facing `limit` would show.
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
    // A bare `--` is dropped rather than refused (`make stability`'s posture: a pasted
    // `cargo run … -- --sweep` must not fail on its own separator); anything else
    // unrecognised is refused, since a typo'd flag would otherwise run the default
    // measurement while the reader believes an A/B ran.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--").collect();
    reject_unknown_flags(&args, &["--sweep", "--stemmer"], &[])?;
    let sweep = has_flag(&args, "--sweep");
    let stemmer = has_flag(&args, "--stemmer");
    let evals_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
    let corpus_dir = evals_dir.join("corpus");
    let results_path = evals_dir.join("results.jsonl");

    // Load the labelled sets. Queries split on their labels: positives carry
    // rank labels; an empty `relevant` is a negative query (invariants.md D2,
    // GH #201), scored only by the search evidence calibration so its presence
    // moves no rank aggregate.
    let set: QuerySet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("queries.json"))?)?;
    let (negatives, positives): (Vec<Labelled>, Vec<Labelled>) =
        set.queries.into_iter().partition(|q| q.relevant.is_empty());
    let sim_set: SimilarSet =
        serde_json::from_str(&std::fs::read_to_string(evals_dir.join("similar.json"))?)?;
    let dense_set: SimilarSet = serde_json::from_str(&std::fs::read_to_string(
        evals_dir.join("similar-dense.json"),
    )?)?;

    // Lint the labels against the corpora they claim to describe, before
    // anything expensive runs. A typo'd label path fails nothing on its own —
    // it just reads as a permanent miss (a rank of `None`, a served row
    // downgraded to filler by label) and gets chased as an engine regression —
    // so the run refuses to score against labels the corpus cannot honour
    // (exit 1: the run broke, not the gate).
    lint_labels(
        &corpus_dir,
        &evals_dir.join("corpus-dense"),
        &positives,
        &negatives,
        &sim_set,
        &dense_set,
    )?;

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
    let scratch = ScratchVault::copy_flat(&corpus_dir)?;
    let vault_root = scratch.root();
    let mut vault = Vault::open_with_embedder(vault_root, Box::new(embedder))?;

    // ---- Phase 1: projection only → the BM25-only baseline. ------------------
    // The vector space does not exist yet, so `search`/`search_chunks` run
    // keyword-only (index-engine.md) — the ablation costs nothing
    // extra: it is the same vault, paused between the two passes.
    let report = vault.project(false)?;
    let bm25 = score_pass(&vault, &positives, Retrieval::Fused)?;
    eprintln!(
        "[eval] projected {} notes; BM25-only baseline scored\n",
        report.indexed
    );

    // The stemmer instrument's lexical arm is scored HERE, while the vault is
    // still projected-but-unembedded, so `search` is honestly BM25-only under
    // both tokenizers; the vault is handed back to the shipped default before
    // anything embeds.
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
    // The fold bake-off (GH #200, Phase B) — the candidate default-disclosure
    // rules judged on the same served lists `similar` above was scored from, so
    // no rule is compared against a surface the others did not see.
    let fold = score_fold(&vault, &sim_set, "orthogonal", false)?;
    // The z calibration dump (GH #187) — the same shipped surface read deep,
    // since the z travels ungated on it (GH #197).
    let floor_z = score_floor_z(&vault, &sim_set)?;
    // The search evidence dump (invariants.md D2, GH #201) — the query-side
    // sibling of the z calibration, read over the same built vault.
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
    // The per-hit tail bake-off (GH #206) — judged from the same served lists
    // the evidence dump above recorded, against the tail_relevant keep-set.
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
    // Its own throwaway vault, its own model load, its own results row (corpus
    // id `dense`) — the fixture measures a *vault-level* geometry, so nothing
    // about it may share state with the orthogonal corpus's run above.
    let dense = score_dense(&evals_dir, &dense_set)?;
    print_dense_report(&dense);
    print_fold_bench(&dense.fold);
    append_result(&results_path, &dense_row(run_id, &dense))?;
    // The tail bake-off's cross-bench join (GH #206) — printable only here,
    // where both corpora's readings exist in one run.
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
