//! The embedder seam (planning/index-engine.md §6; the "build for tomorrow's
//! model" tenet in vision-and-scope). Producing embeddings inside a single binary
//! is the one genuinely hard part, and it is *orthogonal* to the store — so it
//! sits behind this trait. The engine is built and tested against a deterministic
//! fake; the real local model (`b2-embed`'s candle-backed `LocalEmbedder`,
//! index-engine.md §6) drops in through this same seam with no schema or flow change.

use crate::error::Result;

/// Turns note text into a vector. The dimension is fixed per model and pins the
/// `chunks_vec` column type via `meta.embed_dim` (build spec §1.0/§1.2).
///
/// `embed` is **fallible**: the fake never fails, but a real model runs tensor
/// math that can (e.g. a device/allocation error), and the index path must surface
/// that rather than panic. Retrieval is **asymmetric-ready**: [`embed`](Self::embed)
/// embeds a document/passage (indexing); [`embed_query`](Self::embed_query) embeds a
/// search query. Models that prefix the two differently (EmbeddingGemma's
/// `title:…|text:` vs `task:…|query:`; bge's query instruction, index-engine.md §5)
/// override `embed_query`; the default is symmetric.
pub trait Embedder {
    /// Stable identifier recorded in `meta.embed_model_id`. A change to it (or to
    /// `dim`) is a model swap → drop `chunks_vec` + re-embed (index-engine.md §8).
    fn model_id(&self) -> &str;
    /// Vector dimension; must equal the `FLOAT[N]` literal of `chunks_vec`.
    fn dim(&self) -> usize;
    /// Embed one document/passage for indexing. (Batching is a later perf concern;
    /// the real model can add it behind this same seam.)
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Embed a search query. Default is symmetric (query == document); asymmetric
    /// models override this to apply their query-side prompt prefix.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.embed(text)
    }
}

/// Deterministic, content-addressed embedder for tests/dev: identical text →
/// identical vector, so KNN is reproducible and drop-&-rebuild yields the same
/// vectors. It is **not** semantic — it stands in for a real local model behind
/// the seam (testability stack, point 4) until that model lands.
#[derive(Debug, Clone, Copy)]
pub struct FakeEmbedder {
    dim: usize,
}

impl FakeEmbedder {
    pub fn new(dim: usize) -> Self {
        assert!(dim > 0, "embedding dimension must be positive");
        Self { dim }
    }
}

impl Default for FakeEmbedder {
    /// A tiny dimension — cheap for tests that don't care about vector quality.
    fn default() -> Self {
        Self { dim: 8 }
    }
}

impl Embedder for FakeEmbedder {
    fn model_id(&self) -> &str {
        "fake-deterministic-v1"
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // blake3 XOF → dim little-endian u32 words → floats in [0,1). Deterministic
        // in text, so the same chunk always embeds to the same vector.
        let mut hasher = blake3::Hasher::new();
        hasher.update(text.as_bytes());
        let mut reader = hasher.finalize_xof();
        let mut buf = vec![0u8; self.dim * 4];
        reader.fill(&mut buf);
        Ok(buf
            .chunks_exact(4)
            .map(|b| {
                let u = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                (u as f64 / u32::MAX as f64) as f32
            })
            .collect())
    }
}

/// Pack a vector as the compact little-endian float32 BLOB that `sqlite-vec`
/// accepts for `vec0` columns (build spec §1.2). The query side packs the same
/// way so an exact match has distance 0.
pub fn pack_f32(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}
