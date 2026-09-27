//! The opt-in A/Bs, each judged against the default config's passes and logged as its own
//! row: `--stemmer` (FTS tokenizer, GH #157) and `--sweep` (chunker, GH #44).

use crate::common::append_result;
use crate::discovery::score_similar;
use crate::instrument::timed_embed;
use crate::labels::{Labelled, SimilarSet};
use crate::report::{print_rank_moves, result_row, RowInputs, RunId};
use crate::retrieval::{score_pass, Pass, Retrieval};
use b2_core::chunk::ChunkConfig;
use b2_core::db::FtsTokenizer;
use b2_core::vault::Vault;
use std::path::Path;

/// The default config's scored run, which every ablation row is judged against. A
/// short-lived, read-only view over values `run` owns.
#[derive(Clone, Copy)]
pub struct Baseline<'a> {
    pub log: &'a Path,
    pub run: RunId<'a>,
    pub notes: usize,
    pub chunks: usize,
    pub embed_secs: f64,
    pub queries: &'a [Labelled],
    pub bm25: &'a Pass,
    pub vector: &'a Pass,
    pub hybrid: &'a Pass,
}

/// The FTS tokenizer ablation (GH #157): shipped `porter unicode61` against unstemmed
/// `unicode61` over identical chunks and vectors. Discovery never touches FTS, so it isn't
/// re-scored; the dense ablation is, as a check that the harness itself is sound.
///
/// `bm25_unstemmed` is scored by `run` before embedding, so it is BM25-only.
pub fn stemmer(
    vault: &Vault,
    base: &Baseline,
    bm25_unstemmed: &Pass,
) -> Result<(), Box<dyn std::error::Error>> {
    let positives = base.queries;
    vault.rebuild_fts(FtsTokenizer::Unicode61)?;
    let vec_unstemmed = score_pass(vault, positives, Retrieval::VectorOnly)?;
    let hybrid_unstemmed = score_pass(vault, positives, Retrieval::Fused)?;

    println!("\n{}", "=".repeat(78));
    println!(
        "stemmer ablation — chunks_fts rebuilt `unicode61` (unstemmed) over identical chunks + vectors (GH #157)"
    );
    let dense_moved = (0..positives.len())
        .filter(|&i| vec_unstemmed.scores[i].note != base.vector.scores[i].note)
        .count();
    if dense_moved == 0 {
        println!("  [check] dense ablation identical across the flip, as it must be");
    } else {
        println!(
            "  [FAULT] {dense_moved} dense rank(s) moved across an FTS-only flip — \
             the harness is broken; distrust this run"
        );
    }
    println!("  bm25-only, unstemmed vs shipped porter:");
    print_rank_moves(positives, base.bm25, bm25_unstemmed);
    println!("  hybrid, unstemmed vs shipped porter:");
    print_rank_moves(positives, base.hybrid, &hybrid_unstemmed);
    println!(
        "  unstemmed aggregates: bm25 note {:.2} / {:.3}   hybrid note {:.2} / {:.3}   chunk {:.2} / {:.3}",
        bm25_unstemmed.note.hit1(),
        bm25_unstemmed.note.mrr(),
        hybrid_unstemmed.note.hit1(),
        hybrid_unstemmed.note.mrr(),
        hybrid_unstemmed.chunk.hit1(),
        hybrid_unstemmed.chunk.mrr(),
    );
    append_result(
        base.log,
        &result_row(
            base.run,
            RowInputs {
                label: "unicode61",
                cfg: &ChunkConfig::default(),
                tokenizer: FtsTokenizer::Unicode61.sql(),
                notes: base.notes,
                chunks: base.chunks,
                embed_secs: base.embed_secs,
                queries: positives,
                bm25: Some(bm25_unstemmed),
                vector: Some(&vec_unstemmed),
                hybrid: &hybrid_unstemmed,
                similar: None,
                calibration: None,
            },
        ),
    )?;
    // Restore the shipped tokenizer for a `--sweep` in the same run.
    vault.rebuild_fts(FtsTokenizer::PorterUnicode61)?;
    Ok(())
}

/// The in-process chunker sweep (the #44 A/B): re-chunk and re-embed the same vault
/// under each variant, and judge it on the paired per-query moves against the default.
pub fn sweep(
    vault: &mut Vault,
    base: &Baseline,
    sim_set: &SimilarSet,
) -> Result<(), Box<dyn std::error::Error>> {
    let positives = base.queries;
    // The #44 grid brackets each knob's default. `target-250+heading-path` pairs the heading
    // prefix (D3) with small chunks, where it most plausibly helps. `chars_per_token` and
    // `backscan_tokens` are calibration constants, not quality levers, so stay unswept.
    let variants: Vec<(&str, ChunkConfig)> = vec![
        (
            "target-250",
            ChunkConfig {
                target_tokens: 250,
                ..ChunkConfig::default()
            },
        ),
        (
            "target-350",
            ChunkConfig {
                target_tokens: 350,
                ..ChunkConfig::default()
            },
        ),
        (
            "target-600",
            ChunkConfig {
                target_tokens: 600,
                ..ChunkConfig::default()
            },
        ),
        (
            "overlap-0",
            ChunkConfig {
                overlap_frac: 0.0,
                ..ChunkConfig::default()
            },
        ),
        (
            "overlap-30",
            ChunkConfig {
                overlap_frac: 0.30,
                ..ChunkConfig::default()
            },
        ),
        (
            "prepend-heading-path",
            ChunkConfig {
                prepend_heading_path: true,
                ..ChunkConfig::default()
            },
        ),
        (
            "target-250+heading-path",
            ChunkConfig {
                target_tokens: 250,
                prepend_heading_path: true,
                ..ChunkConfig::default()
            },
        ),
    ];
    println!("\n{}", "=".repeat(78));
    println!("chunker sweep (same model, same corpus; default row above for reference)");
    // Per-mate MRR and strangers, because per-anchor hit@3 saturates (GH #183, #188).
    println!(
        "{:<24} {:>7} {:>8}   note h@1/MRR   vec h@1/MRR    chunk h@1/MRR   mate MRR   strangers",
        "config", "chunks", "embed_s"
    );
    for (label, cfg) in variants {
        vault.set_chunk_config(cfg.clone());
        vault.project(true)?; // force: re-chunk everything, clearing vectors
        let (chunks, embed_secs) = timed_embed(vault)?;
        let vec_pass = score_pass(vault, positives, Retrieval::VectorOnly)?;
        let pass = score_pass(vault, positives, Retrieval::Fused)?;
        let sim = score_similar(vault, sim_set)?;
        println!(
            "{:<24} {:>7} {:>8.1}   {:.2} / {:.3}    {:.2} / {:.3}    {:.2} / {:.3}    {:.3}      {}",
            label,
            chunks,
            embed_secs,
            pass.note.hit1(),
            pass.note.mrr(),
            vec_pass.note.hit1(),
            vec_pass.note.mrr(),
            pass.chunk.hit1(),
            pass.chunk.mrr(),
            sim.mate.mrr(),
            sim.strangers.len(),
        );
        // What the A/B is judged on: at this n an aggregate delta is 1–2 queries, so the
        // per-query moves are the data (docs/evals.md).
        print_rank_moves(positives, base.hybrid, &pass);
        append_result(
            base.log,
            &result_row(
                base.run,
                RowInputs {
                    label,
                    cfg: &cfg,
                    tokenizer: FtsTokenizer::PorterUnicode61.sql(),
                    notes: base.notes,
                    chunks,
                    embed_secs,
                    queries: positives,
                    bm25: None,
                    vector: Some(&vec_pass),
                    hybrid: &pass,
                    similar: Some(&sim),
                    calibration: None,
                },
            ),
        )?;
    }
    Ok(())
}
