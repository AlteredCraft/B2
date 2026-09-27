//! `b2-embed`: B2's real local embedder (candle, default BAAI/bge-base-en-v1.5, ADR-0020),
//! behind the [`b2_core::embed::Embedder`] seam (ADR-0005). Only the adapters wire it in.
//!
//! The model is not bundled: [`provision`] (`b2 init`) fetches it into a shared cache, and
//! [`LocalEmbedder::load`] fails fast if it is absent. `$XDG_CONFIG_HOME/b2/config.toml`
//! can point `source` at a mirror, another repo, or a local path.

mod config;
mod model;
mod provision;

pub use config::{EmbedConfig, ModelChoice, ModelInfo, Source, AVAILABLE_MODELS, DEFAULT_MODEL};
pub use model::{active_device_label, LocalEmbedder};
pub use provision::{provision, ProvisionReport};

/// `B2_EMBEDDER=fake` forces the deterministic fake embedder everywhere.
pub const ENV_EMBEDDER: &str = "B2_EMBEDDER";

/// Whether `B2_EMBEDDER=fake` is in force; one reader so the adapters can't disagree.
pub fn fake_requested() -> bool {
    std::env::var_os(ENV_EMBEDDER).is_some_and(|v| v == "fake")
}

/// The embedder a command should open its vault with. A command that embeds gets the real
/// [`LocalEmbedder`] (or [`EmbedError::NotProvisioned`]); everything else, and everything
/// under [`fake_requested`], gets `None`: open with the core's fake.
pub fn embedder_for(needs_semantic: bool) -> Result<Option<LocalEmbedder>> {
    if !needs_semantic || fake_requested() {
        return Ok(None);
    }
    LocalEmbedder::load(&EmbedConfig::load()?).map(Some)
}

/// Errors from provisioning or loading the model. Embed-time failures map into
/// [`b2_core::Error::Embed`] instead.
#[derive(thiserror::Error, Debug)]
pub enum EmbedError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    /// The model is not in the cache yet.
    #[error("embedding model '{model}' is not installed (looked in {dir}); run `b2 init`")]
    NotProvisioned { model: String, dir: String },

    #[error("model download failed: {0}")]
    Download(String),

    #[error("model load failed: {0}")]
    Load(String),

    /// [`EmbedConfig::set_model`] got an id not in [`AVAILABLE_MODELS`]. Reachable only
    /// from the desktop settings picker.
    #[error("unknown embedding model '{0}'")]
    UnknownModel(String),
}

pub type Result<T> = std::result::Result<T, EmbedError>;
