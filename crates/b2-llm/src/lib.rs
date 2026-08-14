//! `b2-llm` — B2's real chat provider.
//!
//! The wire half of the second AI seam: a hand-rolled **sync** OpenAI-compatible
//! streaming client sitting behind [`b2_core::llm::LlmProvider`], exactly as
//! `b2-embed` sits behind `Embedder`. The engine never sees it — flow ④
//! (`Vault::ask`) is built and tested against `FakeLlm`; the adapters wire this
//! in.
//!
//! Decisions (GH #151, the spec of record; cut as GH #154):
//! - **Build our own.** The MVP wire surface is *one* endpoint shape —
//!   `POST {base}/chat/completions` with `stream: true`, SSE frames until
//!   `[DONE]` — so a client library would buy a dependency tree, not leverage.
//!   Rig / genai / ollama-rs were evaluated and passed on; the question re-opens
//!   *inside this crate*, behind the unchanged trait, when the phase-4 agent
//!   loop is cut.
//! - **Sync end-to-end, over `ureq`** (already in the tree via `b2-embed`'s
//!   `hf-hub`). No tokio anywhere in the workspace, no async-to-sync bridge, and
//!   **cancellation is just returning early from a blocking read loop** —
//!   [`b2_core::llm::Completion`]'s cancelled marker, delivered at token
//!   granularity.
//! - **Any OpenAI-compatible URL.** Ollama is the *guided* default
//!   ([`DEFAULT_BASE_URL`]); LM Studio, llama.cpp, vLLM, OpenRouter and the
//!   cloud providers' compat endpoints are all a URL change. Note content leaves
//!   the machine only by explicit configuration (invariant M5) — a cloud base
//!   URL is that configuration, and nothing here defaults to one.
//! - **The wire quirks we chose to own are the ones we handle**: keep-alive
//!   comments, multi-line `data:` fields, delta shapes, mid-stream error frames,
//!   a stream that stops without `[DONE]`. They are unit-tested over canned
//!   frames in [`sse`] — the wire is the whole reason this crate exists, so it
//!   is the part with tests.
//!
//! Chat carries **no index identity** (contrast invariant M2): nothing here is
//! recorded in `meta`, nothing is cached, and swapping models never touches the
//! index — which is what makes "change models at any time" true by construction.

mod provider;
pub mod setup;
mod sse;

pub use provider::OpenAiCompatProvider;
pub use setup::{
    is_local, is_ollama, probe_setup, pull_command, ChatSetup, ChatState, ModelTier, OllamaModel,
    OllamaSetup, MODEL_TIERS,
};

/// Where the Ollama daemon serves its OpenAI-compatible surface. The *guided*
/// default per GH #151: local-first, no key, and the runtime B2's onboarding
/// speaks natively — while the chat path itself stays generic `/v1`.
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

/// The default chat model id. A 3B-class local model: the spec's picker
/// heuristic puts an 8 GB machine at "3–4B q4 or cloud opt-in", and a default
/// that runs on the smallest supported machine is the one that can be a
/// default. It is **not bundled or downloaded** — `ollama pull` is the user's,
/// and an absent model is an error the adapters phrase (E4).
pub const DEFAULT_MODEL: &str = "llama3.2";

/// Environment variable naming the OpenAI-compatible base URL.
pub const ENV_URL: &str = "B2_LLM_URL";
/// Environment variable naming the chat model id.
pub const ENV_MODEL: &str = "B2_LLM_MODEL";
/// Environment variable carrying the bearer token for a **cloud** endpoint.
/// Env-only, deliberately: a key passed as a CLI flag is a key in `ps` output
/// and in shell history.
pub const ENV_API_KEY: &str = "B2_LLM_API_KEY";

/// Which model to chat with, and where it lives. **Adapter-level state, never
/// vault or index state** (GH #151): nothing here is recorded anywhere in the
/// vault, so a config change costs no reindex and leaves no trace in the
/// Markdown.
///
/// Two named configurations, per the spec, are just two values of this type:
/// **Local** (the default — a localhost endpoint, no key) and **Cloud models**
/// (a provider endpoint + `api_key`, explicit opt-in only).
#[derive(Clone, PartialEq, Eq)]
pub struct LlmConfig {
    /// The OpenAI-compatible base URL, e.g. `http://localhost:11434/v1`.
    /// A trailing slash is tolerated (see [`LlmConfig::endpoint`]).
    pub base_url: String,
    /// The model id as the server names it, e.g. `llama3.2`.
    pub model: String,
    /// Bearer token for a cloud endpoint; `None` for a local runtime, which
    /// wants no auth (and where sending one would be noise).
    pub api_key: Option<String>,
}

/// Hand-written so the key **cannot** be logged. `Debug` is what an adapter
/// reaches for when something is wrong — a `tracing` field, a panic message, an
/// error report — and a derived one would put a live bearer token in whichever
/// of those the user then pastes into an issue. Only its presence is ever
/// printed, which is the part that helps diagnose "the cloud endpoint rejects
/// me". The repo's logging policy in one impl (root `CLAUDE.md`).
impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field(
                "api_key",
                &match self.api_key {
                    Some(_) => "Some(<redacted>)",
                    None => "None",
                },
            )
            .finish()
    }
}

impl Default for LlmConfig {
    /// The **Local** configuration: Ollama's compat endpoint, the default
    /// model, no key. Nothing leaves the machine under it (M5).
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            api_key: None,
        }
    }
}

impl LlmConfig {
    /// The defaults overlaid with [`ENV_URL`] / [`ENV_MODEL`] / [`ENV_API_KEY`].
    /// Environment resolution lives **here**, in one place, so both adapters
    /// (the CLI's flags, the desktop's settings) layer their own overrides over
    /// the same base rather than each spelling the variable names themselves.
    /// Blank values are ignored — an exported-but-empty var means "unset" to a
    /// shell user, and honoring it literally would produce an unusable URL.
    pub fn from_env() -> Self {
        let read = |key: &str| {
            std::env::var(key)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let base = Self::default();
        Self {
            base_url: read(ENV_URL).unwrap_or(base.base_url),
            model: read(ENV_MODEL).unwrap_or(base.model),
            api_key: read(ENV_API_KEY),
        }
    }

    /// Lay an adapter's **explicit** choices over this config — the
    /// `B2_VAULT_PATH` convention (a flag beats the environment beats the
    /// default). `None` keeps what's already there, so a caller passes its
    /// parsed flags straight through.
    #[must_use]
    pub fn with_overrides(mut self, base_url: Option<&str>, model: Option<&str>) -> Self {
        if let Some(url) = base_url {
            self.base_url = url.to_string();
        }
        if let Some(model) = model {
            self.model = model.to_string();
        }
        self
    }

    /// Lay an adapter's **explicit** bearer token over this config —
    /// [`with_overrides`](Self::with_overrides)'s sibling, separate because a key
    /// is not a setting like the other two: it is env-only for the CLI (a key in
    /// a flag is a key in `ps`), and the desktop holds one for the session
    /// without ever writing it down. `None` keeps whatever the environment
    /// supplied, so "I didn't type a key" never *clears* `B2_LLM_API_KEY`.
    #[must_use]
    pub fn with_api_key(mut self, api_key: Option<&str>) -> Self {
        if let Some(key) = api_key {
            self.api_key = Some(key.to_string());
        }
        self
    }

    /// Absolute URL for one API path (`"/chat/completions"`), tolerating a
    /// trailing slash on the configured base so `…/v1` and `…/v1/` behave the
    /// same — a difference nobody should have to debug from a 404.
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }
}

/// What can go wrong talking to a model server. Structured, and **the adapters
/// match on it** — which is the whole reason it isn't a string: E4's "can't
/// reach the model server at …" message is [`LlmError::Unreachable`] rendered,
/// not a substring test on someone's `io::Error`.
///
/// Note the seam boundary: these are the failures an adapter can *see*, because
/// it constructs the provider and probes it itself (the `b2-embed`
/// `NotProvisioned` precedent). Failures *inside* a `complete` call cross
/// `b2-core` as [`b2_core::Error::Llm`] — a message, by the seam's design — and
/// surface as the generic "the model call failed", with this type's `Display`
/// as the `B2_DEBUG` detail.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// Nothing is listening: connection refused, DNS failure, TLS failure, a
    /// timeout. The E4 case — "is Ollama running?" — and the reason
    /// [`OpenAiCompatProvider::probe`] exists.
    #[error("can't reach the model server at {endpoint}: {detail}")]
    Unreachable { endpoint: String, detail: String },

    /// The server answered, and said no: an HTTP error status. `message` is the
    /// provider's own explanation when it sent one (OpenAI-shaped errors carry
    /// `error.message`), else the raw body, truncated.
    #[error("the model server rejected the request (HTTP {status}): {message}")]
    Http { status: u16, message: String },

    /// The server is up but doesn't serve the configured model — the most
    /// common local-setup mistake (an un-pulled Ollama model), caught at probe
    /// time rather than as a 404 mid-answer. `available` is what it *does*
    /// serve, so an adapter can show the list.
    #[error("model '{model}' is not available at {endpoint}")]
    ModelMissing {
        model: String,
        endpoint: String,
        available: Vec<String>,
    },

    /// The stream itself was malformed — a `data:` frame that isn't JSON, or an
    /// I/O failure mid-read. Distinct from a *truncated* stream, which is not an
    /// error: that ends the completion honestly as cancelled (the partial text
    /// is real, and the seam has a marker for exactly this).
    #[error("malformed model stream: {0}")]
    Stream(String),

    /// The model server reported an error **inside** the stream, after the
    /// headers said 200 — the wire quirk a hand-rolled client has to own.
    #[error("the model server failed mid-answer: {0}")]
    Provider(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_api_key() {
        let config = LlmConfig {
            base_url: "https://api.example.com/v1".into(),
            model: "some-model".into(),
            api_key: Some("sk-live-do-not-log-me".into()),
        };
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("sk-live-do-not-log-me"),
            "a bearer token must never reach a log: {rendered}"
        );
        // Presence still shows: "is a key configured at all" is the question a
        // rejected cloud call actually needs answered.
        assert!(rendered.contains("redacted"), "{rendered}");
        assert!(rendered.contains("api.example.com"), "{rendered}");
        assert!(
            format!("{:?}", LlmConfig::default()).contains("api_key: \"None\""),
            "the local configuration says so plainly"
        );
    }
}
