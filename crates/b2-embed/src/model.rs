//! [`LocalEmbedder`] — the candle-backed BERT sentence embedder that satisfies the
//! [`b2_core::embed::Embedder`] seam. Loaded from the provisioned cache; a missing
//! model is a fail-fast "run `b2 init`", never a surprise mid-command download.

use crate::config::EmbedConfig;
use crate::{EmbedError, Result};
use b2_core::embed::Embedder;
use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use tokenizers::{Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy};

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
        // Truncate long chunks; batch size is 1 so no padding is needed.
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: max_len,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                direction: TruncationDirection::Right,
            }))
            .map_err(|e| EmbedError::Load(format!("truncation: {e}")))?;

        let device = Device::Cpu;
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
            model_id: config.model.clone(),
            dim,
            query_prefix: config.query_prefix.clone(),
        })
    }

    /// The pooled, L2-normalized embedding of `text`. CLS pooling (row 0) — what
    /// bge is trained for; normalized so `sqlite-vec`'s L2 distance ranks by cosine.
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
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / norm).collect()
}
