//! The opt-in A/Bs, each judged against the default config's passes and logged as rows
//! of their own beside its row: `--stemmer` (the FTS tokenizer ablation, GH #157) and
//! `--sweep` (the in-process chunker sweep, GH #44).

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

/// The default config's scored run — what every ablation row is judged against
/// (the paired per-query moves) and logged beside. A short-lived, read-only view
/// over values `run` owns, built once and handed to each A/B.
#[derive(Clone, Copy)]
pub struct Baseline<'a> {
    /// The results log every row is appended to.
    pub log: &'a Path,
    pub run: RunId<'a>,
    pub notes: usize,
    pub chunks: usize,
    pub embed_secs: f64,
    /// The labelled positives every pass below was scored over.
    pub queries: &'a [Labelled],
    pub bm25: &'a Pass,
    pub vector: &'a Pass,
    pub hybrid: &'a Pass,
}

/// The FTS tokenizer ablation (the #157 instrument).
///
/// One lever, isolated: `rebuild_fts` swaps the tokenizer over the identical
/// chunk rows and vectors — the shipped `porter unicode61` against the
/// unstemmed `unicode61` the A/B retired, kept measurable so the verdict can
/// be re-tried as the corpus grows. Discovery is deliberately not re-scored —
/// `similar` never touches FTS (centroid shortlist + chunk vectors), so its
/// numbers cannot move and the ablation row records no `similar` keys. The
/// dense ablation IS re-scored, as an instrument check: FTS cannot reach it
/// either, so a moved dense rank means the harness is broken, not the engine.
///
/// `bm25_unstemmed` is the lexical arm, which `run` scores while the vault is still
/// projected-but-unembedded (so `search` is honestly BM25-only under both tokenizers).
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
    // Hand the vault back untainted, so a `--sweep` in the same run (and the
    // reference numbers above) stay under the shipped default tokenizer.
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
    // The #44 grid: both directions on each swept knob, plus the one
    // interaction worth a row. `target_tokens` brackets the 450 default
    // (250 / 350 / 600); `overlap_frac` brackets 0.15 (0.0 / 0.30);
    // `target-250+heading-path` exists because a smaller chunk carries less
    // of its own context, which is exactly when the breadcrumb prefix (D3)
    // is most plausibly worth its tokens. `chars_per_token` and
    // `backscan_tokens` stay unswept: they are calibration constants of the
    // token proxy and the boundary search, not retrieval-quality levers.
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
    // `mate MRR` rather than the per-anchor `similar h@3` this column used
    // to carry: that number saturates (GH #183), so as a *comparison*
    // column across variants it could only ever print 1.00 (GH #188).
    // `strangers` replaced `neg clean` when GH #197 retired the existence
    // gate: under always-serve a negative anchor always serves, so that
    // column could only ever print 0/5 — the same cannot-move failure.
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
        // The readout the A/B is actually judged on: at this n every aggregate
        // delta above is 1–2 queries, so the aggregate is a smoke alarm and the
        // per-query win/loss list is the data (docs/evals.md, the
        // process rules).
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
