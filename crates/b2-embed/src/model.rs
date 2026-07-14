//! [`LocalEmbedder`] — the candle-backed BERT sentence embedder that satisfies the
//! [`b2_core::embed::Embedder`] seam. Loaded from the provisioned cache; a missing
//! model is a fail-fast "run `b2 init`", never a surprise mid-command download.

use crate::config::EmbedConfig;
use crate::{EmbedError, Result};
use b2_core::embed::Embedder;
use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use tokenizers::{
    PaddingParams, PaddingStrategy, Tokenizer, TruncationDirection, TruncationParams,
    TruncationStrategy,
};

/// The three files a BERT sentence model needs. Presence of all three in the flat
/// model dir *is* the "installed" check (fail-fast surface).
pub const REQUIRED_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];

/// BERT's positional limit; longer chunks are truncated so position embeddings are
/// never indexed out of range. Capped again by the model's own config.
const MAX_TOKENS: usize = 512;

/// A loaded local embedding model. Cheap to embed with once loaded; loading (which
/// mmaps the weights) is the one-time cost.
pub struct LocalEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    model_id: String,
    dim: usize,
    query_prefix: String,
}

impl LocalEmbedder {
    /// Load the provisioned model named by `config` from its cache dir. Fails fast
    /// with [`EmbedError::NotProvisioned`] if the files are absent — the read path
    /// never downloads.
    pub fn load(config: &EmbedConfig) -> Result<Self> {
        let dir = config.model_dir();
        for f in REQUIRED_FILES {
            if !dir.join(f).is_file() {
                return Err(EmbedError::NotProvisioned {
                    model: config.model.clone(),
                    dir: dir.display().to_string(),
                });
            }
        }

        let bert_config: Config =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json"))?)
                .map_err(|e| EmbedError::Load(format!("config.json: {e}")))?;
        let dim = bert_config.hidden_size;
        let max_len = MAX_TOKENS.min(bert_config.max_position_embeddings);

        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| EmbedError::Load(format!("tokenizer.json: {e}")))?;
        // Truncate long chunks so position embeddings stay in range.
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: max_len,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                direction: TruncationDirection::Right,
            }))
            .map_err(|e| EmbedError::Load(format!("truncation: {e}")))?;
        // Pad a batch to its longest member so `embed_batch` can stack sequences of
        // differing lengths into one tensor; the attention mask zeroes the pad
        // positions, so a padded row's CLS vector equals its single-encode vector.
        // `BatchLongest` leaves a single `encode` (batch of one) unpadded, so the
        // `embed`/`embed_query` path is unchanged. bge/BERT's `[PAD]` id is 0.
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_id: 0,
            pad_type_id: 0,
            pad_token: "[PAD]".to_string(),
            ..Default::default()
        }));

        // Pick the compute device (GH #40). Default build → CPU (with Accelerate BLAS, see
        // Cargo.toml); a `--features metal` build → the Apple-Silicon GPU, with a graceful CPU
        // fallback. The *resolved* device tags the recorded model id (`@metal`), so a device
        // switch is a model swap: `ensure_embedding_space` re-embeds and `search` fails fast
        // rather than mixing CPU and GPU vectors in one space.
        let (device, device_tag) = select_device();
        let model_id = tagged_model_id(&config.model, device_tag);
        // SAFETY: memory-maps the safetensors weights. Sound as long as the file is
        // not mutated while mapped; it is a read-only file in our XDG cache, written
        // once by `b2 init` (provision) and never touched again for the process's life.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], DTYPE, &device)
                .map_err(|e| EmbedError::Load(format!("weights: {e}")))?
        };
        let model = BertModel::load(vb, &bert_config)
            .map_err(|e| EmbedError::Load(format!("bert: {e}")))?;

        Ok(Self {
            model,
            tokenizer,
            device,
            model_id,
            dim,
            query_prefix: config.query_prefix.clone(),
        })
    }

    /// The pooled, L2-normalized embedding of `text`. CLS pooling (row 0) — what
    /// bge is trained for; normalized so the index's L2 distance ranks by cosine.
    fn embed_inner(&self, text: &str) -> candle_core::Result<Vec<f32>> {
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| candle_core::Error::Msg(format!("tokenize: {e}")))?;
        let ids = enc.get_ids();
        let input_ids = Tensor::new(ids, &self.device)?.unsqueeze(0)?;
        let token_type_ids = input_ids.zeros_like()?;
        let attention_mask = Tensor::new(enc.get_attention_mask(), &self.device)?.unsqueeze(0)?;
        // [1, seq, hidden] → CLS token → [hidden]
        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;
        let cls = hidden.i((0, 0))?;
        let v: Vec<f32> = cls.to_vec1()?;
        Ok(l2_normalize(&v))
    }

    /// Embed a batch in one forward pass: tokenize+pad to the batch's longest, run
    /// `[B, L]` through BERT, take each row's CLS token (position 0, unaffected by
    /// right-padding), and L2-normalize. One matmul over `B` texts is far cheaper on
    /// CPU than `B` single passes — the reindex win. Equivalent, per row, to
    /// [`embed_inner`](Self::embed_inner).
    fn embed_batch_inner(&self, texts: &[&str]) -> candle_core::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let encs = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| candle_core::Error::Msg(format!("tokenize: {e}")))?;
        let batch = encs.len();
        // Padding made every encoding the same length.
        let seq = encs.first().map_or(0, |e| e.get_ids().len());
        let mut ids = Vec::with_capacity(batch * seq);
        let mut mask = Vec::with_capacity(batch * seq);
        for e in &encs {
            ids.extend_from_slice(e.get_ids());
            mask.extend_from_slice(e.get_attention_mask());
        }
        let input_ids = Tensor::from_vec(ids, (batch, seq), &self.device)?;
        let attention_mask = Tensor::from_vec(mask, (batch, seq), &self.device)?;
        let token_type_ids = input_ids.zeros_like()?;
        // [B, seq, hidden] → CLS column [B, hidden].
        let hidden = self
            .model
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;
        let cls = hidden.i((.., 0))?;
        let rows: Vec<Vec<f32>> = cls.to_vec2()?;
        Ok(rows.iter().map(|r| l2_normalize(r)).collect())
    }
}

impl Embedder for LocalEmbedder {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, text: &str) -> b2_core::Result<Vec<f32>> {
        self.embed_inner(text)
            .map_err(|e| b2_core::Error::Embed(e.to_string()))
    }

    fn embed_query(&self, text: &str) -> b2_core::Result<Vec<f32>> {
        // Asymmetric: queries carry the retrieval instruction, documents don't.
        let prefixed = format!("{}{}", self.query_prefix, text);
        self.embed_inner(&prefixed)
            .map_err(|e| b2_core::Error::Embed(e.to_string()))
    }

    fn embed_batch(&self, texts: &[&str]) -> b2_core::Result<Vec<Vec<f32>>> {
        self.embed_batch_inner(texts)
            .map_err(|e| b2_core::Error::Embed(e.to_string()))
    }
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / norm).collect()
}

/// Try to open the Metal GPU — only when compiled `--features metal` (candle's
/// `metal_is_available()` is literally `cfg!(feature = "metal")`), and never a hard
/// requirement: any failure returns `None` and the caller uses the CPU (GH #40). `announce`
/// gates the one-line fallback notice so the load path can warn while the cheap capability
/// probe ([`active_device_label`]) stays silent. No `unwrap`: a failed `new_metal` is a soft
/// degrade (no-panic rule), not a load error.
fn open_metal(announce: bool) -> Option<Device> {
    if candle_core::utils::metal_is_available() {
        match Device::new_metal(0) {
            Ok(d) => return Some(d),
            Err(e) if announce => eprintln!("note: Metal GPU unavailable ({e}); embedding on CPU"),
            Err(_) => {}
        }
    }
    None
}

/// Pick the inference device and a short tag (`"cpu"`/`"metal"`) describing what we *actually*
/// got — so the recorded model id reflects the resolved device, and a fallback build honestly
/// records CPU vectors.
fn select_device() -> (Device, &'static str) {
    match open_metal(true) {
        Some(d) => (d, "metal"),
        None => (Device::Cpu, "cpu"),
    }
}

/// Human label for the compute device this build embeds on — `"Metal"` on a `--features metal`
/// build with a working GPU, else `"CPU"` (GH #40). The desktop Settings badge renders it.
/// Same resolution as [`select_device`] but silent; cheap — the compile-time gate
/// short-circuits on a CPU build before any GPU probe.
pub fn active_device_label() -> &'static str {
    match open_metal(false) {
        Some(_) => "Metal",
        None => "CPU",
    }
}

/// The id recorded as `meta.embed_model_id`, tagged by the resolved device. CPU keeps the
/// bare repo id (so existing indexes need no migration); any non-CPU device appends `@<tag>`
/// so its vectors live in a distinct embedding space — the swap that forces a re-embed and
/// makes `search` fail fast rather than mix devices. The tag is only in this id, never in
/// `config.model` (model-file lookup is unaffected).
fn tagged_model_id(base: &str, device_tag: &str) -> String {
    if device_tag == "cpu" {
        base.to_string()
    } else {
        format!("{base}@{device_tag}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_id_is_untagged_others_are_suffixed() {
        // CPU keeps the bare repo id — existing indexes must not be seen as a model swap.
        assert_eq!(
            tagged_model_id("BAAI/bge-base-en-v1.5", "cpu"),
            "BAAI/bge-base-en-v1.5"
        );
        // A non-CPU device tags a distinct embedding space, so a switch re-embeds + fails fast.
        assert_eq!(
            tagged_model_id("BAAI/bge-base-en-v1.5", "metal"),
            "BAAI/bge-base-en-v1.5@metal"
        );
    }

    #[test]
    fn select_device_falls_back_to_cpu_without_the_metal_feature() {
        // The default (no-feature) test build has `metal_is_available() == false`, so selection
        // resolves to CPU and the tag is "cpu" — this keeps the whole test suite on the CPU path.
        // (A `--features metal` build exercises the GPU branch out-of-CI; see the eval recipe.)
        if !candle_core::utils::metal_is_available() {
            let (device, tag) = select_device();
            assert_eq!(tag, "cpu");
            assert!(matches!(device, Device::Cpu));
            // The Settings-badge label agrees with the resolved device.
            assert_eq!(active_device_label(), "CPU");
        }
    }
}
