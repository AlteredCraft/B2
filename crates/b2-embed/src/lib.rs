//! `b2-embed` — B2's real, local embedder: candle + hf-hub producing embeddings inside the
//! single binary, with **BAAI/bge-base-en-v1.5** @ dim 768 as the default model (ADR-0020).
//!
//! It sits **behind the [`b2_core::embed::Embedder`] seam** (ADR-0005), so the store, the
//! flows and the whole `b2-core` suite never see it — they run against the deterministic
//! `FakeEmbedder`. The adapters are the only clients that wire the real model in.
//!
//! The model is **not bundled**: an explicit [`provision`] (`b2 init`) downloads and
//! verifies it into a shared XDG cache, and [`LocalEmbedder::load`] fails fast if it is
//! absent. Configurable via `$XDG_CONFIG_HOME/b2/config.toml`, whose `source` can point at
//! a mirror, an alternate repo, or a local path for a fully-offline install.

mod config;
mod model;
mod provision;

pub use config::{
    find_model, EmbedConfig, ModelChoice, ModelInfo, Source, AVAILABLE_MODELS, DEFAULT_MODEL,
};
pub use model::{active_device_label, LocalEmbedder};
pub use provision::{provision, ProvisionReport};

/// Errors from provisioning/loading the local model. Embed-*time* failures map into
/// [`b2_core::Error::Embed`] so the index path surfaces one error type; the
/// setup-time errors here carry the actionable "run `b2 init`" guidance.
#[derive(thiserror::Error, Debug)]
pub enum EmbedError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    /// The model is not in the cache yet — the fail-fast the CLI turns into
    /// "run `b2 init`". Carries the model id and the directory that was checked.
    #[error("embedding model '{model}' is not installed (looked in {dir}); run `b2 init`")]
    NotProvisioned { model: String, dir: String },

    #[error("model download failed: {0}")]
    Download(String),

    #[error("model load failed: {0}")]
    Load(String),

    /// A model id that isn't in [`AVAILABLE_MODELS`] was passed to
    /// [`EmbedConfig::set_model`] — refuse rather than write a config the loader could
    /// never provision. Reachable only from the desktop settings picker (the CLI never
    /// sets the model), so it maps to a generic "pick one from the list" message there.
    #[error("unknown embedding model '{0}'")]
    UnknownModel(String),
}

pub type Result<T> = std::result::Result<T, EmbedError>;
