//! `b2-relate` — B2's real, Claude-backed relator.
//!
//! This is the deferred "intelligence half" of connection discovery: the step that
//! classifies a candidate note pair into a *typed, explained* connection — or
//! declines it. It sits **behind the [`b2_core::relate::Relator`] seam**, so the
//! discovery pipeline, the suggestion lifecycle, and the whole `b2-core` test suite
//! never see it — they run against the deterministic `FakeRelator`. The `b2` CLI is
//! the only client that wires the real relator in (mirroring how it wires the
//! candle-backed `LocalEmbedder` from `b2-embed`).
//!
//! Decisions:
//! - **Backend = pluggable, Claude first.** The backend is config-selectable
//!   ([`config::Backend`]); the Claude Messages-API backend ([`ClaudeRelator`]) is
//!   the shipped default. A local/Ollama backend can drop in behind the same seam
//!   later without touching `b2-core`.
//! - **Transport = raw HTTP (`ureq`).** Rust has no official Anthropic SDK, so this
//!   talks to `POST /v1/messages` directly. `ureq` is synchronous — no `tokio` — and
//!   already in the workspace's dependency tree via `hf-hub`.
//! - **Structured output = forced tool use.** The request forces a single
//!   `classify_relation` tool whose input schema pins `relation` to the closed core
//!   verb set ([`b2_core::relation::CORE`]), so the model returns a clean, typed
//!   verdict instead of free text to parse.
//! - **Auth = `ANTHROPIC_API_KEY`.** Read from the environment and validated at
//!   construction ([`ClaudeRelator::from_config`]) so a missing key fails fast with
//!   an actionable message, never mid-run.

mod claude;
mod config;
mod prompt;

pub use claude::{ClaudeRelator, Usage};
pub use config::{Backend, RelateConfig, DEFAULT_BASE_URL, DEFAULT_MAX_TOKENS, DEFAULT_MODEL};

/// Errors from loading relator config or constructing the real relator. Classify-
/// *time* failures map into [`b2_core::Error::Relator`] so the discovery path
/// surfaces one error type; the setup-time errors here carry the actionable
/// guidance the CLI turns into user-facing messages.
#[derive(thiserror::Error, Debug)]
pub enum RelateError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    /// `ANTHROPIC_API_KEY` is unset or empty — the fail-fast the CLI turns into
    /// "set ANTHROPIC_API_KEY (or run with B2_RELATOR=fake)".
    #[error("ANTHROPIC_API_KEY is not set")]
    MissingApiKey,
}

pub type Result<T> = std::result::Result<T, RelateError>;
