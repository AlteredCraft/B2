//! The embedder seam (planning/index-engine.md §6; the "build for tomorrow's
//! model" tenet in vision-and-scope). Producing embeddings inside a single binary
//! is the one genuinely hard part, and it is *orthogonal* to the store — so it
//! sits behind this trait. The engine is built and tested against a deterministic
//! fake; the real local model (`b2-embed`'s candle-backed `LocalEmbedder`,
//! index-engine.md §6) drops in through this same seam with no schema or flow change.

use crate::error::Result;

/// Turns note text into a vector. The dimension is fixed per model and recorded
/// as `meta.embed_dim` (build spec §1.0/§1.2).
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
    /// `dim`) is a model swap → drop the stored vectors + re-embed (index-engine.md §8).
    fn model_id(&self) -> &str;
    /// Vector dimension; must equal the recorded `meta.embed_dim`.
    fn dim(&self) -> usize;
    /// Embed one document/passage for indexing.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Embed a search query. Default is symmetric (query == document); asymmetric
    /// models override this to apply their query-side prompt prefix.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.embed(text)
    }
    /// Embed a batch of documents/passages for indexing, one vector per input in
    /// order. The default maps [`embed`](Self::embed) over the slice — always
    /// correct; a real model overrides this to run **one batched forward pass**,
    /// which is dramatically faster on CPU than N single passes (the reindex hot
    /// path). Chunk vectors are independent, so batch boundaries never change a
    /// result.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
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

/// Pack a vector as a compact little-endian float32 BLOB — the stored form of every
/// vector in the index (`embeddings.vector`, `note_centroids.centroid`; build spec
/// §1.2). The query side packs the same way so an exact match has distance 0.
pub fn pack_f32(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Inverse of [`pack_f32`]: read a stored BLOB (little-endian float32, no header)
/// back into a vector. Used to reuse a note's *stored* chunk vectors as discovery
/// queries without re-embedding (tasks.md ①). A trailing partial group can't occur
/// for a vector written by [`pack_f32`], so a non-multiple-of-4 length is simply
/// truncated rather than treated as an error.
pub fn unpack_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// [`unpack_f32`] into a caller-owned scratch buffer, reusing its capacity. The
/// whole-space scans decode one stored vector per visited row; a fresh `Vec` per row
/// was a measurable slice of the `b2 similar` stall (#38), where this costs nothing
/// after the first row.
pub fn unpack_f32_into(bytes: &[u8], out: &mut Vec<f32>) {
    out.clear();
    out.extend(
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
    );
}

/// Squared Euclidean distance between two equal-length vectors — the index's one
/// ranking key, minus the final `sqrt` (`sqrt` is monotonic, so dropping it never
/// changes an ordering; it is applied once per *surfaced* result, not per
/// comparison). A length mismatch (impossible for vectors from one embedding space)
/// scores the shared prefix rather than panicking.
///
/// Eight independent accumulators, summed at the end: float addition is
/// non-associative, so a single running sum forms one serial dependency chain the
/// compiler must execute as written — splitting it lets LLVM autovectorize.
/// Measured at the #38 scale (38.6k × 768-dim, 12 anchors) the naive iterator shape
/// cost ~530 ms; this shape ~75 ms.
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

/// The **centroid** of a note's chunk vectors: their arithmetic mean, L2-normalized
/// (the spherical mean — the standard coarse representative when the underlying
/// vectors are cosine-normalized, as b2-embed's are). `None` for an empty set — a
/// note with no stored vectors has no centroid row. Deterministic: summation runs in
/// the given (chunk `seq`) order. Discovery's first stage ranks whole *notes* by
/// centroid distance, so its heavy scan is O(notes), not O(chunks) (#38).
pub fn centroid_of(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = vectors.first()?;
    let mut mean = vec![0.0f32; first.len()];
    for v in vectors {
        // A length mismatch can't occur within one embedding space; fold the shared
        // prefix rather than panic, mirroring `l2_sq`.
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
