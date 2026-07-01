//! `b2-embed` — B2's real, local embedder.
//!
//! This is the deferred "quality half" of build-spec steps 3 & 5 and the one place
//! the architecture meets real friction (index-engine.md §6): producing embeddings
//! inside a single binary. It sits **behind the [`b2_core::embed::Embedder`] seam**,
//! so the store, the flows, and the whole `b2-core` test suite never see it — they
//! run against the deterministic `FakeEmbedder`. The `b2` CLI is the only client
//! that wires the real model in.
//!
//! Decisions (locked 2026-06-30, tasks.md "Next up"):
//! - **Runtime = `candle` + `hf-hub`** — pure-Rust inference compiled into the
//!   binary; no external ONNX runtime to ship. `hf-hub` is the download seam.
//! - **Model = a BERT-family sentence embedder**, default **BAAI/bge-base-en-v1.5**
//!   @ dim 768. (EmbeddingGemma-300M was the first choice but is gated on Hugging
//!   Face — HTTP 401 without a token + license click — which defeats a friction-free
//!   `b2 init`; bge was the pre-authorized fallback and validated in the spike.)
//! - **Not bundled** — an explicit [`provision`] (`b2 init`) downloads + verifies the
//!   model into a shared XDG cache; [`LocalEmbedder::load`] **fails fast** if absent.
//! - **Configurable** via a global TOML at `$XDG_CONFIG_HOME/b2/config.toml`
//!   (`[embedder] model / source / cache_dir`), source overridable to a mirror, an
//!   alternate repo, or a local path (fully-offline install).

mod config;
mod model;
mod provision;

pub use config::{EmbedConfig, Source};
pub use model::LocalEmbedder;
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
}

pub type Result<T> = std::result::Result<T, EmbedError>;
