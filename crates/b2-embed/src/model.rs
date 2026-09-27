//! [`LocalEmbedder`]: the candle BERT sentence embedder behind the
//! [`b2_core::embed::Embedder`] seam. A missing model fails fast ("run `b2 init`"); the read
//! path never downloads.

use crate::config::EmbedConfig;
use crate::{EmbedError, Result};
use b2_core::embed::Embedder;
use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use tokenizers::{
    Encoding, PaddingParams, PaddingStrategy, Tokenizer, TruncationDirection, TruncationParams,
    TruncationStrategy,
};

/// The three files a BERT sentence model needs.
pub const REQUIRED_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];

/// Whether every [`REQUIRED_FILES`] entry is in `dir`: the one "installed" check, shared
/// by `load`, provisioning and the settings picker.
pub fn files_present(dir: &std::path::Path) -> bool {
    REQUIRED_FILES.iter().all(|f| dir.join(f).is_file())
}

/// BERT's positional limit, capped again by the model's own config.
const MAX_TOKENS: usize = 512;

/// A loaded local embedding model. Loading (which mmaps the weights) is the one-time cost.
pub struct LocalEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    /// The configured repo id, without the device tag.
    configured_model: String,
    /// The id recorded as `meta.embed_model_id`: the repo id, tagged by device.
    model_id: String,
    dim: usize,
    query_prefix: String,
}

impl LocalEmbedder {
    /// Load the provisioned model named by `config`, or [`EmbedError::NotProvisioned`].
    pub fn load(config: &EmbedConfig) -> Result<Self> {
        let dir = config.model_dir();
        if !files_present(&dir) {
            return Err(EmbedError::NotProvisioned {
                model: config.model.clone(),
                dir: dir.display().to_string(),
            });
        }

        let bert_config: Config =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json"))?)
                .map_err(|e| EmbedError::Load(format!("config.json: {e}")))?;
        let dim = bert_config.hidden_size;
        let max_len = MAX_TOKENS.min(bert_config.max_position_embeddings);

        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| EmbedError::Load(format!("tokenizer.json: {e}")))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: max_len,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                direction: TruncationDirection::Right,
            }))
            .map_err(|e| EmbedError::Load(format!("truncation: {e}")))?;
        // Pad a batch to its longest member so `embed_batch` can stack it; the attention
        // mask makes a padded row's CLS vector equal its single-encode vector. A single
        // `encode` stays unpadded. BERT's `[PAD]` id is 0.
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_id: 0,
            pad_type_id: 0,
            pad_token: "[PAD]".to_string(),
            ..Default::default()
        }));

        // The resolved device tags the model id (GH #40), so a device switch is a model
        // swap and CPU and GPU vectors never mix in one space.
        let (device, device_tag) = select_device();
        let model_id = tagged_model_id(&config.model, device_tag);
        // SAFETY: memory-maps the safetensors weights. Sound as long as the file is not
        // mutated while mapped; it is written once by `b2 init` and never touched again.
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
            configured_model: config.model.clone(),
            model_id,
            dim,
            query_prefix: config.query_prefix.clone(),
        })
    }

    /// The configured repo id, without the device tag [`model_id`](Embedder::model_id)
    /// carries: the id the config and settings picker use.
    pub fn configured_model(&self) -> &str {
        &self.configured_model
    }

    /// The L2-normalized embedding of one `text`, from a single unpadded encode.
    fn embed_inner(&self, text: &str) -> candle_core::Result<Vec<f32>> {
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| candle_core::Error::Msg(format!("tokenize: {e}")))?;
        self.forward_cls(std::slice::from_ref(&enc))?
            .pop()
            .ok_or_else(|| candle_core::Error::Msg("the model returned no embedding".into()))
    }

    /// Embed a batch in one forward pass, far cheaper than `B` single passes. Equal, per
    /// row, to [`embed_inner`](Self::embed_inner).
    fn embed_batch_inner(&self, texts: &[&str]) -> candle_core::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let encs = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| candle_core::Error::Msg(format!("tokenize: {e}")))?;
        self.forward_cls(&encs)
    }

    /// Run same-length encodings through BERT and take each row's CLS token (what bge is
    /// trained for), L2-normalized so the index's L2 distance ranks by cosine.
    fn forward_cls(&self, encs: &[Encoding]) -> candle_core::Result<Vec<Vec<f32>>> {
        let batch = encs.len();
        // Every encoding here has the same length (a batch was padded).
        let seq = encs.first().map_or(0, |e| e.get_ids().len());
        let mut ids = Vec::with_capacity(batch * seq);
        let mut mask = Vec::with_capacity(batch * seq);
        for e in encs {
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
        let rows: Vec<Vec<f32>> = hidden.i((.., 0))?.to_vec2()?;
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
        self.embed_inner(text).map_err(embed_err)
    }

    fn embed_query(&self, text: &str) -> b2_core::Result<Vec<f32>> {
        // Asymmetric: queries carry the retrieval instruction, documents don't.
        let prefixed = format!("{}{}", self.query_prefix, text);
        self.embed_inner(&prefixed).map_err(embed_err)
    }

    fn embed_batch(&self, texts: &[&str]) -> b2_core::Result<Vec<Vec<f32>>> {
        self.embed_batch_inner(texts).map_err(embed_err)
    }
}

/// A candle error as the core's error type. Not a `From` impl: both types are foreign.
fn embed_err(e: candle_core::Error) -> b2_core::Error {
    b2_core::Error::Embed(e.to_string())
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / norm).collect()
}

/// Try to open the Metal GPU (only in a `--features metal` build); any failure returns
/// `None` and the caller uses the CPU (GH #40). `announce` prints the fallback to stderr,
/// not a log, since it has a user-visible consequence (a Metal-built index reads as a
/// model change).
fn open_metal(announce: bool) -> Option<Device> {
    if candle_core::utils::metal_is_available() {
        match Device::new_metal(0) {
            Ok(d) => return Some(d),
            Err(e) if announce => {
                eprintln!("note: Metal GPU unavailable ({e}); embedding on CPU")
            }
            Err(_) => {}
        }
    }
    None
}

/// The inference device and a tag for what was actually resolved, so a fallback build
/// records CPU vectors.
fn select_device() -> (Device, &'static str) {
    match open_metal(true) {
        Some(d) => (d, "metal"),
        None => (Device::Cpu, "cpu"),
    }
}

/// `"Metal"` or `"CPU"` for the Settings badge (GH #40): [`select_device`], but silent.
pub fn active_device_label() -> &'static str {
    match open_metal(false) {
        Some(_) => "Metal",
        None => "CPU",
    }
}

/// The id recorded as `meta.embed_model_id`. CPU keeps the bare repo id (existing indexes
/// need no migration); other devices append `@<tag>`, a distinct embedding space. Never
/// in `config.model`.
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
        assert_eq!(
            tagged_model_id("BAAI/bge-base-en-v1.5", "cpu"),
            "BAAI/bge-base-en-v1.5"
        );
        assert_eq!(
            tagged_model_id("BAAI/bge-base-en-v1.5", "metal"),
            "BAAI/bge-base-en-v1.5@metal"
        );
    }

    #[test]
    fn files_present_is_loads_own_precondition() {
        // The desktop's `semantic` probe relies on this (GH #133): the file check never
        // overstates the model, and a corrupt model still fails in `load`.
        let tmp = tempfile::TempDir::new().unwrap();
        let config = EmbedConfig {
            model: "acme/not-a-real-model".to_string(),
            source: crate::Source::Local(tmp.path().to_path_buf()),
            cache_dir: tmp.path().to_path_buf(),
            query_prefix: String::new(),
        };

        assert!(!config.is_model_provisioned(&config.model));
        assert!(matches!(
            LocalEmbedder::load(&config),
            Err(EmbedError::NotProvisioned { .. })
        ));

        // Present but corrupt: the file check passes and `load` fails deeper.
        let dir = config.model_dir();
        std::fs::create_dir_all(&dir).unwrap();
        for f in REQUIRED_FILES {
            std::fs::write(dir.join(f), b"not a model").unwrap();
        }
        assert!(config.is_model_provisioned(&config.model));
        assert!(matches!(
            LocalEmbedder::load(&config),
            Err(EmbedError::Load(_))
        ));
    }

    #[test]
    fn select_device_falls_back_to_cpu_without_the_metal_feature() {
        if !candle_core::utils::metal_is_available() {
            let (device, tag) = select_device();
            assert_eq!(tag, "cpu");
            assert!(matches!(device, Device::Cpu));
            assert_eq!(active_device_label(), "CPU");
        }
    }
}
