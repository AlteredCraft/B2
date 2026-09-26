//! Instrument checks and plumbing the scored passes rest on: batch ≡ single embedding
//! faithfulness, the timed embed pass, and the candidate-width blindness warning (GH #141).

use crate::K;
use b2_core::embed::Embedder;
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool, Vault};
use b2_embed::LocalEmbedder;
use std::ops::ControlFlow;
use std::time::Instant;

/// `LocalEmbedder::embed_batch` must be a faithful map of `embed`: right-padding short rows
/// to the batch's longest and masking them out has to leave each row's CLS vector unchanged.
/// The reindex path batches freely, so a regression here would silently corrupt every stored
/// vector — and every score this eval prints.
///
/// It lives in the eval rather than `cargo test` because it needs the provisioned model,
/// which the fast suite deliberately never touches (ADR-0013). Running it here means it
/// actually runs, instead of sitting behind an `#[ignore]` nobody passes `--ignored` to.
pub fn check_batch_matches_single(model: &LocalEmbedder) -> Result<(), Box<dyn std::error::Error>> {
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
pub fn timed_embed(vault: &Vault) -> Result<(usize, f64), Box<dyn std::error::Error>> {
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
/// A corpus with no more chunks than a signal's candidate pool truncates *neither* list, so
/// both are already complete, widening cannot add a candidate, and every score above is
/// invariant under **candidate width**: a change to either view's headroom or to
/// `search::pool_size` prints bit-identical numbers here while genuinely reordering a real
/// vault (GH #141). Judged on the **narrower** of the two pools, since blindness is a claim
/// about every number the run prints.
///
/// Scoped deliberately to width — `RRF_K` re-weights the *same* two lists, so it reorders
/// results on any corpus, and this eval sees that. A warning, not a gate: the point is that a
/// reader must not take an unmoved number as evidence of no change. The property itself is
/// measured by `--example stability`, on a vault big enough for the pool to bind.
pub fn warn_if_pool_blind(chunks: usize) {
    let pool = chunk_candidate_pool(K).min(note_candidate_pool(K));
    if chunks <= pool {
        eprintln!(
            "[warn] {chunks} chunks ≤ {pool}-candidate pool — neither signal is truncated here, so a\n\
             \x20      candidate-width change (either hit pool, pool_size) cannot move any number in this run\n\
             \x20      (GH #141). `make stability` measures that property on a large vault. (RRF_K is\n\
             \x20      not in that set — it re-weights the same lists, and this corpus does see it.)\n"
        );
    }
}
