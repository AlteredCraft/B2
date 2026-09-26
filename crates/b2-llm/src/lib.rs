//! `b2-llm` — B2's real chat provider: the wire half of the second AI seam (ADR-0005), a
//! hand-rolled **sync** OpenAI-compatible streaming client behind
//! [`b2_core::llm::LlmProvider`], exactly as `b2-embed` sits behind `Embedder`. The engine
//! never sees it — flow ④ is built and tested against `FakeLlm`.
//!
//! Decisions (GH #151, cut as GH #154):
//! - **Build our own.** The wire surface is *one* endpoint shape —
//!   `POST {base}/chat/completions` with `stream: true`, SSE frames until `[DONE]` — so a
//!   client library would buy a dependency tree, not leverage. Tool calls (ADR-0022) rode
//!   in on that same stream as `delta.tool_calls`, so they did not change the answer.
//! - **Sync end-to-end, over `ureq`** (ADR-0011): no runtime, no async-to-sync bridge, and
//!   **cancellation is just returning early from a blocking read loop**.
//! - **Any OpenAI-compatible URL.** Ollama is the *guided* default; LM Studio, llama.cpp,
//!   vLLM and the cloud compat endpoints are a URL change. Note content leaves the machine
//!   only by explicit configuration (M5), and nothing here defaults to one.
//! - **The wire quirks we own are the ones we handle**, unit-tested over canned frames in
//!   [`sse`] — the wire is the whole reason this crate exists, so it is the part with tests.
//!
//! Chat carries **no index identity** (contrast ADR-0007): nothing is recorded in `meta`,
//! nothing is cached, and swapping models never touches the index.

mod provider;
mod setup;
mod sse;

use b2_core::llm::{FakeLlm, LlmProvider};
use serde::Serialize;

pub use provider::OpenAiCompatProvider;
pub use setup::{
    is_ollama, model_missing_message, probe_setup, pull_command, refusal_message,
    unreachable_message, ChatSetup, ChatState, ModelTier, OllamaModel, OllamaSetup, ToolCallCap,
    MODEL_TIERS, OLLAMA_INSTALL_URL,
};

/// The environment variable that swaps in the deterministic [`FakeLlm`]: `B2_LLM=fake`,
/// `B2_EMBEDDER=fake`'s sibling for the chat seam.
pub const ENV_LLM: &str = "B2_LLM";

/// What an adapter says when [`fake_requested`] is in force — never overstate what
/// answered. One sentence, so the CLI's stderr note and the desktop's setup card agree.
pub const FAKE_NOTICE: &str = "The fake chat provider is in use (B2_LLM=fake) — answers are \
                               deterministic test scaffolding, not a model.";

/// Whether `B2_LLM=fake` is in force. Read in one place so the two adapters cannot
/// disagree about what the switch means.
pub fn fake_requested() -> bool {
    std::env::var_os(ENV_LLM).is_some_and(|v| v == "fake")
}

/// The provider a chat command talks to: the fake under [`fake_requested`], else the real
/// client over `config`. **No probe** — for a caller that has already checked the endpoint
/// (the desktop probes once, when its chat surface opens) or wants none.
pub fn provider(config: LlmConfig) -> Box<dyn LlmProvider> {
    if fake_requested() {
        Box::new(FakeLlm)
    } else {
        Box::new(OpenAiCompatProvider::new(config))
    }
}

/// [`provider`], **probed** before it is returned — the `b2 init` posture applied to chat:
/// a stopped server is an [`LlmError`] before the question, never a surprise after it. The
/// fake has nothing to probe.
pub fn probed_provider(config: LlmConfig) -> Result<Box<dyn LlmProvider>, LlmError> {
    if fake_requested() {
        return Ok(Box::new(FakeLlm));
    }
    let provider = OpenAiCompatProvider::new(config);
    provider.probe()?;
    Ok(Box::new(provider))
}

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
/// Never a CLI flag: a key passed as a flag is a key in `ps` output and in shell
/// history. It is also the **override** — see [`ApiKeySource::Environment`].
pub const ENV_API_KEY: &str = "B2_LLM_API_KEY";

/// Environment variable capping the tool calls one model reply may make — see
/// [`LlmConfig::max_tool_calls`].
pub const ENV_MAX_TOOL_CALLS: &str = "B2_LLM_MAX_TOOL_CALLS";

/// The default for [`LlmConfig::max_tool_calls`]. Far above what a real reply asks for
/// (B2 runs at most a handful per round), and small enough that the table a reply can
/// make B2 allocate stays a few kilobytes.
pub const DEFAULT_MAX_TOOL_CALLS: usize = 64;

/// The highest value [`LlmConfig::max_tool_calls`] accepts, from any source. The cap
/// exists to bound memory a server can make B2 allocate, so it needs a bound of its own:
/// at this size the table is a few hundred kilobytes, and no real reply comes near it.
pub const MAX_TOOL_CALLS_CEILING: usize = 4096;

/// Judge a typed tool-call cap — the **one** parser, shared by [`ENV_MAX_TOOL_CALLS`] and
/// an adapter's Settings field so the two can never accept different things. A whole
/// number from 1 (zero would refuse every call) to [`MAX_TOOL_CALLS_CEILING`]; anything
/// else is `None`.
pub fn parse_max_tool_calls(raw: &str) -> Option<usize> {
    raw.trim()
        .parse::<usize>()
        .ok()
        .filter(|&n| is_valid_tool_call_cap(n))
}

/// The range every tool-call cap must fall in, from any source: 1 (zero would refuse every
/// call) to [`MAX_TOOL_CALLS_CEILING`]. Shared by the parser and
/// [`LlmConfig::with_max_tool_calls`], so the two can never accept different values.
fn is_valid_tool_call_cap(n: usize) -> bool {
    (1..=MAX_TOOL_CALLS_CEILING).contains(&n)
}

/// Where the bearer token in force came from. A **fact about the key, never the key** —
/// the half of a cloud configuration that may safely be shown, logged and sent to a
/// webview, which is why it replaced a bare "is one set?" boolean (GH #176).
///
/// The order below is the resolution order, and the one interesting rule is at the top of
/// it: [`Environment`](Self::Environment) **wins**. An adapter that remembers a key for the
/// user still yields to `B2_LLM_API_KEY`, so a shell that exports one gets the key it
/// named. [`LlmConfig::with_api_key`] enforces that in one place.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeySource {
    /// No key at all — the **Local** configuration, and the default.
    #[default]
    None,
    /// [`ENV_API_KEY`], and therefore the key in force whatever else is
    /// configured. The CLI's only source; the desktop's override.
    Environment,
    /// Remembered by the adapter in the platform's own secret store — the
    /// desktop's macOS Keychain (GH #176). Survives quit, encrypted at rest.
    Stored,
    /// Held in memory for this run only. What the desktop degrades to when the
    /// Keychain is unavailable or refuses: chat still works with the key the
    /// user typed, it just won't be there next launch.
    Session,
}

/// Which model to chat with, and where it lives. **Adapter-level state, never vault or
/// index state**: nothing here is recorded in the vault, so a config change costs no
/// reindex. The spec's two named configurations are just two values of this type — Local
/// (a localhost endpoint, no key) and Cloud models (a provider endpoint + `api_key`,
/// explicit opt-in only).
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
    /// Where [`api_key`](Self::api_key) came from, and `None` when there is no
    /// key. Kept beside the secret rather than derived from it because only the
    /// *resolver* knows: an adapter that remembers keys has to be told which of
    /// its own sources answered, and the environment's answer outranks it.
    pub api_key_source: ApiKeySource,
    /// The most tool calls one model reply may make, which is also the highest call
    /// `index` it may name. A reply past it fails with [`LlmError::TooManyToolCalls`]
    /// rather than being trimmed. [`DEFAULT_MAX_TOOL_CALLS`] unless
    /// [`ENV_MAX_TOOL_CALLS`] says otherwise; it bounds memory a server can make B2
    /// allocate, so it is a ceiling to raise for an unusual model, not a tuning knob.
    pub max_tool_calls: usize,
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
            // Safe, and the useful half: "a key is set, from the environment"
            // is what turns a rejected cloud call into a diagnosis.
            .field("api_key_source", &self.api_key_source)
            .field("max_tool_calls", &self.max_tool_calls)
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
            api_key_source: ApiKeySource::None,
            max_tool_calls: DEFAULT_MAX_TOOL_CALLS,
        }
    }
}

impl LlmConfig {
    /// The defaults overlaid with [`ENV_URL`] / [`ENV_MODEL`] / [`ENV_API_KEY`] /
    /// [`ENV_MAX_TOOL_CALLS`].
    /// Environment resolution lives **here**, in one place, so both adapters
    /// (the CLI's flags, the desktop's settings) layer their own overrides over
    /// the same base rather than each spelling the variable names themselves.
    /// Blank values are ignored — an exported-but-empty var means "unset" to a
    /// shell user, and honoring it literally would produce an unusable URL.
    pub fn from_env() -> Self {
        Self::resolve(|key| std::env::var(key).ok())
    }

    /// [`from_env`](Self::from_env) over any variable source — the process environment
    /// in production, a closure in tests, so resolution is testable without mutating a
    /// process-global that parallel tests share.
    fn resolve(var: impl Fn(&str) -> Option<String>) -> Self {
        let read = |key: &str| {
            var(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let base = Self::default();
        let api_key = read(ENV_API_KEY);
        // A cap that doesn't parse, or parses to zero (which would refuse every tool
        // call), is a typo rather than a wish: keep the default and say so, since a
        // silently ignored setting is one nobody can debug.
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

    /// Lay an adapter's own tool-call cap over this config — the
    /// [`with_overrides`](Self::with_overrides) rule: an explicit choice beats the
    /// environment, `None` keeps what's there. A value outside what
    /// [`parse_max_tool_calls`] accepts is ignored rather than installed, so a
    /// hand-edited settings file cannot lift the ceiling the parser enforces.
    #[must_use]
    pub fn with_max_tool_calls(mut self, max_tool_calls: Option<usize>) -> Self {
        if let Some(n) = max_tool_calls.filter(|&n| is_valid_tool_call_cap(n)) {
            self.max_tool_calls = n;
        }
        self
    }

    /// Lay an adapter's own bearer token over this config —
    /// [`with_overrides`](Self::with_overrides)'s sibling, and deliberately **not** the
    /// same rule.
    ///
    /// For the URL and the model, an adapter's explicit choice beats the environment. A key
    /// inverts that: `B2_LLM_API_KEY` wins, and a key an adapter merely *remembered* yields
    /// to it (GH #176). A shell that exports a key is making a per-run statement — the only
    /// way to point one launch at a different provider — and a user who would rather B2
    /// kept no secret at all needs a way to say so that a stored key cannot outrank.
    ///
    /// `source` is where the caller's key came from, recorded so the configuration can
    /// *say* which one answered. `None` keeps whatever the environment supplied, so "I
    /// didn't type a key" never *clears* `B2_LLM_API_KEY`.
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

    /// Absolute URL for one API path (`"/chat/completions"`), tolerating a
    /// trailing slash on the configured base so `…/v1` and `…/v1/` behave the
    /// same — a difference nobody should have to debug from a 404.
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }
}

/// What can go wrong talking to a model server. Structured, and **the adapters match on
/// it** — which is why it isn't a string: the "can't reach the model server at …" message
/// is [`LlmError::Unreachable`] rendered, not a substring test on someone's `io::Error`.
///
/// Note the seam boundary: these are the failures an adapter can *see*, because it
/// constructs the provider and probes it itself. Failures *inside* a `complete` call cross
/// `b2-core` as [`b2_core::Error::Llm`] — a message, by the seam's design — and surface as
/// the generic "the model call failed", with this type's `Display` as the `B2_DEBUG` detail.
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

    /// A **probe** was refused: something is listening at `endpoint`, but it did not serve
    /// the OpenAI-compatible `/models` route. Its own variant rather than [`LlmError::Http`]
    /// because the *same status means different things in the two places it can arrive*: a
    /// 404 here is a base URL that isn't a chat API (the `…/v1X` typo), while a 404
    /// mid-answer is the server's own "model not found". An adapter that had to guess would
    /// give the wrong fix half the time, so the distinction is carried in the type — and
    /// turned into a sentence once, by [`crate::refusal_message`].
    #[error("the model server at {endpoint} refused a probe (HTTP {status}): {message}")]
    Refused {
        endpoint: String,
        status: u16,
        /// The server's own explanation, when it sent one. It rides in `Display` because
        /// that *is* the `B2_DEBUG` line — `user_message` prints `err.to_string()` and
        /// nothing else, so a field left out of the format is one no adapter can show. The
        /// user-facing sentence omits it: a 404 body says nothing a human can act on.
        message: String,
    },

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

    /// One reply asked for more tool calls than [`LlmConfig::max_tool_calls`] allows —
    /// or named a call `index` past it, which costs the same memory. Refused rather than
    /// trimmed: a reply this far outside the protocol is a broken or hostile server, and
    /// running the first N of its calls would pass that off as a normal turn.
    #[error(
        "the model asked for more than {limit} tool calls in one reply; raise {ENV_MAX_TOOL_CALLS} if that is expected"
    )]
    TooManyToolCalls { limit: usize },

    /// The model server reported an error **inside** the stream, after the
    /// headers said 200 — the wire quirk a hand-rolled client has to own.
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
        // Zero would refuse every tool call, and the rest aren't numbers: a typo keeps
        // the default rather than configuring an unusable client.
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
        // The cap exists to bound memory, so it has a ceiling of its own: a setting
        // that can be typed up to a billion is not a bound.
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
        // The `with_overrides` convention: an explicit adapter choice beats the env.
        assert_eq!(
            env.clone().with_max_tool_calls(Some(128)).max_tool_calls,
            128
        );
        assert_eq!(env.clone().with_max_tool_calls(None).max_tool_calls, 8);
        // A value no parser would have accepted is not installed by the back door.
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
        // Presence still shows: "is a key configured at all" is the question a
        // rejected cloud call actually needs answered — and now *which* key,
        // which is the follow-up question when the answer is "yes, and it's
        // being rejected".
        assert!(rendered.contains("redacted"), "{rendered}");
        assert!(rendered.contains("api.example.com"), "{rendered}");
        assert!(rendered.contains("Stored"), "{rendered}");
        assert!(
            format!("{:?}", LlmConfig::default()).contains("api_key: \"None\""),
            "the local configuration says so plainly"
        );
    }

    /// The one inversion of the layering rule (GH #176): an adapter's *remembered*
    /// key yields to `B2_LLM_API_KEY`, so a shell that exports one gets the key it
    /// named — the escape hatch a stored secret must never be able to shadow.
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

    /// With no key in the environment, a remembered one is simply the key — and
    /// it says where it came from, which is what the Settings copy reads.
    #[test]
    fn a_remembered_key_is_used_and_named_when_the_environment_is_silent() {
        let keyless = LlmConfig::default();
        assert_eq!(keyless.api_key_source, ApiKeySource::None);

        let stored = keyless
            .clone()
            .with_api_key(Some("sk-from-the-keychain"), ApiKeySource::Stored);
        assert_eq!(stored.api_key.as_deref(), Some("sk-from-the-keychain"));
        assert_eq!(stored.api_key_source, ApiKeySource::Stored);

        // The Keychain refused, so the same key is in force for this run only —
        // chat works, and the configuration is honest about how long it lasts.
        let session = keyless
            .clone()
            .with_api_key(Some("sk-typed-just-now"), ApiKeySource::Session);
        assert_eq!(session.api_key_source, ApiKeySource::Session);

        // And `None` is "I didn't type one", which must never *clear* a key.
        assert_eq!(
            stored.with_api_key(None, ApiKeySource::None).api_key_source,
            ApiKeySource::Stored
        );
    }
}
