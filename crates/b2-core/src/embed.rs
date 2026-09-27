//! The embedder seam (index-engine.md §6). The engine is tested against a deterministic
//! fake; `b2-embed`'s real model drops in here with no schema or flow change.

use crate::error::Result;

/// Turns note text into a vector. The dimension is fixed per model and recorded as
/// `meta.embed_dim` (ADR-0007).
///
/// Fallible because a real model can fail. [`embed`](Self::embed) embeds a passage and
/// [`embed_query`](Self::embed_query) a query, for asymmetric models.
pub trait Embedder {
    /// Stable identifier recorded in `meta.embed_model_id`. A change is a model swap
    /// (index-engine.md §8).
    fn model_id(&self) -> &str;
    /// Vector dimension; must equal the recorded `meta.embed_dim`.
    fn dim(&self) -> usize;
    /// Embed one document/passage for indexing.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Embed a search query. Symmetric by default; asymmetric models add their prefix.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.embed(text)
    }
    /// Embed a batch of passages, one vector per input in order. A real model overrides
    /// this with one batched forward pass. Batch boundaries never change a result.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

/// Deterministic embedder for tests and dev (`B2_EMBEDDER=fake`): identical text gives
/// an identical vector. Not semantic.
#[derive(Debug, Clone, Copy)]
pub struct FakeEmbedder {
    dim: usize,
}

impl FakeEmbedder {
    /// A fake of dimension `dim`, clamped to at least 1.
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }
}

impl Default for FakeEmbedder {
    /// A tiny dimension — cheap for tests that don't care about vector quality.
    fn default() -> Self {
        Self { dim: 8 }
    }
}

/// The fake embedder's model id. A vault recorded with it holds hash vectors, so
/// `Vault::similar` never grades it (GH #150/#197).
pub const FAKE_MODEL_ID: &str = "fake-deterministic-v1";

impl Embedder for FakeEmbedder {
    fn model_id(&self) -> &str {
        FAKE_MODEL_ID
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // blake3 XOF → dim little-endian u32 words → floats in [0,1).
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

/// Pack a vector as a little-endian float32 BLOB, the stored form of every vector
/// (index-engine.md §3).
pub fn pack_f32(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Inverse of [`pack_f32`]. A trailing partial group is truncated, not an error.
pub fn unpack_f32(bytes: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(bytes.len() / 4);
    unpack_f32_into(bytes, &mut out);
    out
}

/// [`unpack_f32`] into a reused scratch buffer: a fresh `Vec` per scanned row was
/// measurable (#38).
pub fn unpack_f32_into(bytes: &[u8], out: &mut Vec<f32>) {
    out.clear();
    out.extend(
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
    );
}

/// Squared Euclidean distance, the index's ranking key. A length mismatch scores the
/// shared prefix rather than panicking.
///
/// Eight accumulators: float addition is non-associative, so one running sum can't
/// autovectorize (~530 ms vs ~75 ms at the #38 scale).
pub fn l2_sq(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let mut acc = [0.0f32; 8];
    let chunks_a = a.chunks_exact(8);
    let chunks_b = b.chunks_exact(8);
    let (tail_a, tail_b) = (chunks_a.remainder(), chunks_b.remainder());
    for (xa, xb) in chunks_a.zip(chunks_b) {
        for i in 0..8 {
            let d = xa[i] - xb[i];
            acc[i] += d * d;
        }
    }
    let mut sum: f32 = acc.iter().sum();
    for (x, y) in tail_a.iter().zip(tail_b) {
        let d = x - y;
        sum += d * d;
    }
    sum
}

/// A note's centroid: the L2-normalized mean of its chunk vectors, summed in the given
/// order; `None` for an empty set. Discovery's first stage scans these (#38).
pub fn centroid_of(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = vectors.first()?;
    let mut mean = vec![0.0f32; first.len()];
    for v in vectors {
        // Fold the shared prefix on a length mismatch, as `l2_sq` does.
        for (m, x) in mean.iter_mut().zip(v) {
            *m += x;
        }
    }
    let n = vectors.len() as f32;
    for m in &mut mean {
        *m /= n;
    }
    let norm = mean.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for m in &mut mean {
            *m /= norm;
        }
    }
    Some(mean)
}
