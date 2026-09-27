//! `b2-llm`: B2's real chat provider, a hand-rolled sync OpenAI-compatible streaming client
//! behind [`b2_core::llm::LlmProvider`] (ADR-0005, GH #151, GH #154). The wire is one
//! endpoint shape, so a client library would buy a dependency tree, not leverage. Sync over
//! `ureq` (ADR-0011): cancellation is an early return from the read loop.
//!
//! Ollama is the guided default; any compatible URL works. Note content leaves the machine
//! only by explicit configuration (M5). Chat carries no index identity (contrast ADR-0007).

mod provider;
mod setup;
mod sse;

use b2_core::llm::{FakeLlm, LlmProvider};
use serde::Serialize;

pub use provider::OpenAiCompatProvider;
pub use setup::{
    model_missing_message, probe_setup, pull_command, refusal_message, unreachable_message,
    ChatSetup, ChatState, ModelTier, OllamaModel, OllamaSetup, ToolCallCap, MODEL_TIERS,
    OLLAMA_INSTALL_URL,
};

/// `B2_LLM=fake` swaps in the deterministic [`FakeLlm`].
pub const ENV_LLM: &str = "B2_LLM";

/// What both adapters say when [`fake_requested`] is in force.
pub const FAKE_NOTICE: &str = "The fake chat provider is in use (B2_LLM=fake) — answers are \
                               deterministic test scaffolding, not a model.";

/// Whether `B2_LLM=fake` is in force; one reader so the adapters can't disagree.
pub fn fake_requested() -> bool {
    std::env::var_os(ENV_LLM).is_some_and(|v| v == "fake")
}

/// The provider a chat command talks to: the fake under [`fake_requested`], else the real
/// client. Not probed; see [`probed_provider`].
pub fn provider(config: LlmConfig) -> Box<dyn LlmProvider> {
    if fake_requested() {
        Box::new(FakeLlm)
    } else {
        Box::new(OpenAiCompatProvider::new(config))
    }
}

/// [`provider`], probed first, so a stopped server fails before the question.
pub fn probed_provider(config: LlmConfig) -> Result<Box<dyn LlmProvider>, LlmError> {
    if fake_requested() {
        return Ok(Box::new(FakeLlm));
    }
    let provider = OpenAiCompatProvider::new(config);
    provider.probe()?;
    Ok(Box::new(provider))
}

/// Ollama's OpenAI-compatible surface, the guided default (GH #151).
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

/// The default chat model: a 3B-class model that runs on the smallest supported (8 GB)
/// machine. Never downloaded by B2; an absent model is an error the adapters phrase (E4).
pub const DEFAULT_MODEL: &str = "llama3.2";

/// Environment variable naming the OpenAI-compatible base URL.
pub const ENV_URL: &str = "B2_LLM_URL";
/// Environment variable naming the chat model id.
pub const ENV_MODEL: &str = "B2_LLM_MODEL";
/// Environment variable carrying the bearer token for a cloud endpoint. Never a CLI flag
/// (it would show in `ps` and shell history). Outranks a stored key.
pub const ENV_API_KEY: &str = "B2_LLM_API_KEY";

/// Environment variable for [`LlmConfig::max_tool_calls`].
pub const ENV_MAX_TOOL_CALLS: &str = "B2_LLM_MAX_TOOL_CALLS";

/// The default for [`LlmConfig::max_tool_calls`]: far above a real reply, and a few KB.
pub const DEFAULT_MAX_TOOL_CALLS: usize = 64;

/// The highest [`LlmConfig::max_tool_calls`] from any source: a memory bound needs its own
/// bound.
pub const MAX_TOOL_CALLS_CEILING: usize = 4096;

/// Parse a tool-call cap, for both [`ENV_MAX_TOOL_CALLS`] and the Settings field: a whole
/// number from 1 to [`MAX_TOOL_CALLS_CEILING`].
pub fn parse_max_tool_calls(raw: &str) -> Option<usize> {
    raw.trim()
        .parse::<usize>()
        .ok()
        .filter(|&n| is_valid_tool_call_cap(n))
}

/// 1 (zero would refuse every call) to [`MAX_TOOL_CALLS_CEILING`].
fn is_valid_tool_call_cap(n: usize) -> bool {
    (1..=MAX_TOOL_CALLS_CEILING).contains(&n)
}

/// Where the bearer token in force came from: a fact about the key, safe to show and log
/// (GH #176). [`Environment`](Self::Environment) outranks a remembered key
/// ([`LlmConfig::with_api_key`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeySource {
    /// No key: the Local configuration.
    #[default]
    None,
    /// [`ENV_API_KEY`]: the CLI's only source, the desktop's override.
    Environment,
    /// Remembered in the desktop's macOS Keychain (GH #176).
    Stored,
    /// Held for this run only, when the Keychain refuses.
    Session,
}

/// Which model to chat with, and where. Adapter state, never vault or index state. Local
/// (localhost, no key) and Cloud (endpoint + `api_key`, opt-in) are two values of it.
#[derive(Clone, PartialEq, Eq)]
pub struct LlmConfig {
    /// The OpenAI-compatible base URL, e.g. `http://localhost:11434/v1`.
    pub base_url: String,
    /// The model id as the server names it, e.g. `llama3.2`.
    pub model: String,
    /// Bearer token for a cloud endpoint; `None` for a local runtime.
    pub api_key: Option<String>,
    /// Where [`api_key`](Self::api_key) came from; only the resolver knows.
    pub api_key_source: ApiKeySource,
    /// The most tool calls (and highest call `index`) one reply may make; past it,
    /// [`LlmError::TooManyToolCalls`]. A memory bound, not a tuning knob.
    pub max_tool_calls: usize,
}

/// Hand-written so the key can never be logged; only its presence is printed.
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
            .field("api_key_source", &self.api_key_source)
            .field("max_tool_calls", &self.max_tool_calls)
            .finish()
    }
}

impl Default for LlmConfig {
    /// The Local configuration; nothing leaves the machine (M5).
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            api_key: None,
            api_key_source: ApiKeySource::None,
            max_tool_calls: DEFAULT_MAX_TOOL_CALLS,
        }
    }
}

impl LlmConfig {
    /// The defaults overlaid with [`ENV_URL`] / [`ENV_MODEL`] / [`ENV_API_KEY`] /
    /// [`ENV_MAX_TOOL_CALLS`], the one base both adapters layer over. Blank reads as unset.
    pub fn from_env() -> Self {
        Self::resolve(|key| std::env::var(key).ok())
    }

    /// [`from_env`](Self::from_env) over any variable source, so tests needn't mutate the
    /// process env.
    fn resolve(var: impl Fn(&str) -> Option<String>) -> Self {
        let read = |key: &str| {
            var(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let base = Self::default();
        let api_key = read(ENV_API_KEY);
        // An invalid cap is a typo: keep the default, and say so.
        let max_tool_calls = match read(ENV_MAX_TOOL_CALLS) {
            None => base.max_tool_calls,
            Some(raw) => parse_max_tool_calls(&raw).unwrap_or_else(|| {
                tracing::warn!(
                    target: "b2::llm",
                    value = raw,
                    default = base.max_tool_calls,
                    "{ENV_MAX_TOOL_CALLS} is not a whole number from 1 to {MAX_TOOL_CALLS_CEILING}; using the default"
                );
                base.max_tool_calls
            }),
        };
        Self {
            base_url: read(ENV_URL).unwrap_or(base.base_url),
            model: read(ENV_MODEL).unwrap_or(base.model),
            api_key_source: match api_key {
                Some(_) => ApiKeySource::Environment,
                None => ApiKeySource::None,
            },
            api_key,
            max_tool_calls,
        }
    }

    /// Lay an adapter's explicit choices over this config (a flag beats the environment).
    /// `None` keeps what's there.
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

    /// Lay an adapter's tool-call cap over this config, as
    /// [`with_overrides`](Self::with_overrides) does. An invalid value is ignored, so a
    /// hand-edited settings file can't lift the ceiling.
    #[must_use]
    pub fn with_max_tool_calls(mut self, max_tool_calls: Option<usize>) -> Self {
        if let Some(n) = max_tool_calls.filter(|&n| is_valid_tool_call_cap(n)) {
            self.max_tool_calls = n;
        }
        self
    }

    /// Lay an adapter's bearer token over this config. Unlike
    /// [`with_overrides`](Self::with_overrides), the environment wins: `B2_LLM_API_KEY` is a
    /// per-run statement a stored key must not outrank (GH #176). `None` never clears a key.
    #[must_use]
    pub fn with_api_key(mut self, api_key: Option<&str>, source: ApiKeySource) -> Self {
        if self.api_key_source == ApiKeySource::Environment {
            return self;
        }
        if let Some(key) = api_key {
            self.api_key = Some(key.to_string());
            self.api_key_source = source;
        }
        self
    }

    /// Absolute URL for one API path, tolerating a trailing slash on the base.
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }
}

/// What can go wrong talking to a model server; the adapters match on it. Failures inside
/// a `complete` call cross `b2-core` as a [`b2_core::Error::Llm`] message instead.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    /// Nothing is listening (refused, DNS, TLS, timeout): E4's "is Ollama running?".
    #[error("can't reach the model server at {endpoint}: {detail}")]
    Unreachable { endpoint: String, detail: String },

    /// An HTTP error status mid-answer, with the server's own message when it sent one.
    #[error("the model server rejected the request (HTTP {status}): {message}")]
    Http { status: u16, message: String },

    /// A probe of `/models` was refused. Not [`LlmError::Http`]: a 404 here is a wrong base
    /// URL, while mid-answer it is "model not found" ([`crate::refusal_message`]).
    #[error("the model server at {endpoint} refused a probe (HTTP {status}): {message}")]
    Refused {
        endpoint: String,
        status: u16,
        /// The server's own explanation. In `Display` because that is the `B2_DEBUG` line;
        /// the user-facing sentence omits it.
        message: String,
    },

    /// The server doesn't serve the configured model (usually an un-pulled Ollama model).
    /// `available` is what it does serve.
    #[error("model '{model}' is not available at {endpoint}")]
    ModelMissing {
        model: String,
        endpoint: String,
        available: Vec<String>,
    },

    /// A malformed stream (a non-JSON frame, an I/O failure). A truncated stream is not
    /// an error: it ends as cancelled.
    #[error("malformed model stream: {0}")]
    Stream(String),

    /// One reply exceeded [`LlmConfig::max_tool_calls`]. Refused, not trimmed: running the
    /// first N calls would pass a broken server off as a normal turn.
    #[error(
        "the model asked for more than {limit} tool calls in one reply; raise {ENV_MAX_TOOL_CALLS} if that is expected"
    )]
    TooManyToolCalls { limit: usize },

    /// The server reported an error inside a 200 stream.
    #[error("the model server failed mid-answer: {0}")]
    Provider(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_call_cap_comes_from_the_environment_and_a_bad_value_keeps_the_default() {
        let with = |value: Option<&str>| {
            LlmConfig::resolve(|key| {
                (key == ENV_MAX_TOOL_CALLS)
                    .then(|| value.map(str::to_string))
                    .flatten()
            })
            .max_tool_calls
        };
        assert_eq!(with(None), DEFAULT_MAX_TOOL_CALLS);
        assert_eq!(with(Some("8")), 8);
        assert_eq!(
            with(Some(" 256 ")),
            256,
            "surrounding whitespace is a shell's, not a value"
        );
        for bad in ["0", "-1", "lots", "6.4", ""] {
            assert_eq!(with(Some(bad)), DEFAULT_MAX_TOOL_CALLS, "{bad:?}");
        }
    }

    #[test]
    fn one_parser_judges_the_cap_for_the_environment_and_for_settings() {
        assert_eq!(parse_max_tool_calls("8"), Some(8));
        assert_eq!(parse_max_tool_calls(" 256 "), Some(256));
        assert_eq!(
            parse_max_tool_calls(&MAX_TOOL_CALLS_CEILING.to_string()),
            Some(MAX_TOOL_CALLS_CEILING)
        );
        assert_eq!(
            parse_max_tool_calls(&(MAX_TOOL_CALLS_CEILING + 1).to_string()),
            None
        );
        for bad in ["0", "-1", "lots", "6.4", ""] {
            assert_eq!(parse_max_tool_calls(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_adapters_cap_beats_the_environments_and_none_keeps_it() {
        let env = LlmConfig::resolve(|key| (key == ENV_MAX_TOOL_CALLS).then(|| "8".to_string()));
        assert_eq!(env.max_tool_calls, 8);
        assert_eq!(
            env.clone().with_max_tool_calls(Some(128)).max_tool_calls,
            128
        );
        assert_eq!(env.clone().with_max_tool_calls(None).max_tool_calls, 8);
        assert_eq!(env.clone().with_max_tool_calls(Some(0)).max_tool_calls, 8);
        assert_eq!(
            env.with_max_tool_calls(Some(MAX_TOOL_CALLS_CEILING + 1))
                .max_tool_calls,
            8
        );
    }

    #[test]
    fn debug_never_prints_the_api_key() {
        let config = LlmConfig {
            base_url: "https://api.example.com/v1".into(),
            model: "some-model".into(),
            api_key: Some("sk-live-do-not-log-me".into()),
            api_key_source: ApiKeySource::Stored,
            ..LlmConfig::default()
        };
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("sk-live-do-not-log-me"),
            "a bearer token must never reach a log: {rendered}"
        );
        // Presence and source still show.
        assert!(rendered.contains("redacted"), "{rendered}");
        assert!(rendered.contains("api.example.com"), "{rendered}");
        assert!(rendered.contains("Stored"), "{rendered}");
        assert!(
            format!("{:?}", LlmConfig::default()).contains("api_key: \"None\""),
            "the local configuration says so plainly"
        );
    }

    /// GH #176.
    #[test]
    fn the_environments_key_outranks_a_remembered_one() {
        let from_env = LlmConfig {
            api_key: Some("sk-from-the-environment".into()),
            api_key_source: ApiKeySource::Environment,
            ..LlmConfig::default()
        };
        let resolved = from_env.with_api_key(Some("sk-from-the-keychain"), ApiKeySource::Stored);
        assert_eq!(resolved.api_key.as_deref(), Some("sk-from-the-environment"));
        assert_eq!(resolved.api_key_source, ApiKeySource::Environment);
    }

    #[test]
    fn a_remembered_key_is_used_and_named_when_the_environment_is_silent() {
        let keyless = LlmConfig::default();
        assert_eq!(keyless.api_key_source, ApiKeySource::None);

        let stored = keyless
            .clone()
            .with_api_key(Some("sk-from-the-keychain"), ApiKeySource::Stored);
        assert_eq!(stored.api_key.as_deref(), Some("sk-from-the-keychain"));
        assert_eq!(stored.api_key_source, ApiKeySource::Stored);

        // The Keychain refused: in force for this run only.
        let session = keyless
            .clone()
            .with_api_key(Some("sk-typed-just-now"), ApiKeySource::Session);
        assert_eq!(session.api_key_source, ApiKeySource::Session);

        // `None` never clears a key.
        assert_eq!(
            stored.with_api_key(None, ApiKeySource::None).api_key_source,
            ApiKeySource::Stored
        );
    }
}
