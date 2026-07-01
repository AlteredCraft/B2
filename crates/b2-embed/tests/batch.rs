//! Real-model check (out of CI): a batched embed equals the per-text single embed,
//! row for row. This is the correctness guarantee behind `LocalEmbedder::embed_batch`
//! — right-padding + the attention mask must leave each row's CLS vector unchanged.
//!
//! `#[ignore]`d because it needs the provisioned model (`b2 init`); run it with:
//!   cargo test -p b2-embed --test batch -- --ignored

use b2_core::embed::Embedder;
use b2_embed::{EmbedConfig, LocalEmbedder};

#[test]
#[ignore = "needs the provisioned model; run with --ignored"]
fn batched_equals_single_per_row() {
    let config = EmbedConfig::load().expect("load embed config");
    let model = LocalEmbedder::load(&config).expect("run `b2 init` to provision the model first");

    // Deliberately varied lengths, so batching pads short rows to the longest.
    let texts = [
        "Spaced repetition schedules reviews at increasing intervals.",
        "Sleep consolidates memory.",
        "Short.",
        "Focus and sustained attention shape what is later recalled from long-term memory across days.",
    ];
    let refs: Vec<&str> = texts.to_vec();
    let batched = model.embed_batch(&refs).unwrap();
    assert_eq!(batched.len(), texts.len());

    for (t, bv) in texts.iter().zip(&batched) {
        let sv = model.embed(t).unwrap();
        assert_eq!(bv.len(), sv.len(), "dim mismatch for {t:?}");
        // Both are L2-normalized, so the dot product is cosine similarity; padding
        // must not move it off ~1.0.
        let cos: f32 = bv.iter().zip(&sv).map(|(a, b)| a * b).sum();
        assert!(cos > 0.9999, "batched vs single cosine {cos} for {t:?}");
    }
}
