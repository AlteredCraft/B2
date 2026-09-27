//! Instrument checks and plumbing the scored passes rest on: batch ≡ single embedding
//! faithfulness, the timed embed pass, and the candidate-width blindness warning (GH #141).

use crate::K;
use b2_core::embed::Embedder;
use b2_core::vault::{chunk_candidate_pool, note_candidate_pool, Vault};
use b2_embed::LocalEmbedder;
use std::ops::ControlFlow;
use std::rc::Rc;
use std::time::Instant;

/// One loaded model serving every throwaway vault of the run, instead of a load per vault.
///
/// Forwards every [`Embedder`] method, including the defaults `LocalEmbedder` overrides: a
/// method added to the trait must be forwarded here too, or the default silently runs.
#[derive(Clone)]
pub struct SharedEmbedder(Rc<LocalEmbedder>);

impl SharedEmbedder {
    pub fn new(model: LocalEmbedder) -> Self {
        Self(Rc::new(model))
    }
}

impl Embedder for SharedEmbedder {
    fn model_id(&self) -> &str {
        self.0.model_id()
    }
    fn dim(&self) -> usize {
        self.0.dim()
    }
    fn embed(&self, text: &str) -> b2_core::Result<Vec<f32>> {
        self.0.embed(text)
    }
    fn embed_query(&self, text: &str) -> b2_core::Result<Vec<f32>> {
        self.0.embed_query(text)
    }
    fn embed_batch(&self, texts: &[&str]) -> b2_core::Result<Vec<Vec<f32>>> {
        self.0.embed_batch(texts)
    }
}

/// `LocalEmbedder::embed_batch` must equal `embed` row by row: padding must not move a CLS
/// vector, or every stored vector and every score here is silently wrong. Lives here, not in
/// `cargo test`, because it needs the real model (ADR-0013).
pub fn check_batch_matches_single(model: &dyn Embedder) -> Result<(), Box<dyn std::error::Error>> {
    // Varied lengths, so batching pads.
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
        // Rows are L2-normalized, so the dot product is cosine. Check non-finite first: a
        // NaN fails every comparison, so `cos <= 0.9999` alone would let it through.
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

/// Warn when the corpus has no more chunks than a candidate pool: then no list is truncated
/// and a candidate-width change cannot move any number here (GH #141). Judged on the
/// narrower pool. `RRF_K` is unaffected; `--example stability` measures width.
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
