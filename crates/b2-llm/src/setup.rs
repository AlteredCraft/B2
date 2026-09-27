//! Chat onboarding (GH #151, GH #155). Chat itself is generic `/v1`, but the setup card
//! speaks Ollama's native API (detect the daemon, list models, suggest a pull): guided
//! setup is a per-runtime feature, so don't generalize it.
//!
//! Everything here is a status, never an error: "the daemon isn't running" is the answer
//! [`probe_setup`] is asking for.

use crate::{ApiKeySource, LlmConfig, LlmError, OpenAiCompatProvider, FAKE_NOTICE};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Ollama's native model inventory, the one non-`/v1` endpoint B2 speaks.
const TAGS_PATH: &str = "/api/tags";

/// Where someone with no Ollama is sent: the page with the install command on it.
pub const OLLAMA_INSTALL_URL: &str = "https://docs.ollama.com/quickstart";

/// The port Ollama serves on, which decides whether Ollama's commands belong in a message.
const OLLAMA_PORT: &str = ":11434";

/// How long the onboarding round trip may take: a hanging card is worse than "not running".
const TAGS_TIMEOUT: Duration = Duration::from_secs(5);

/// Does this endpoint look like Ollama (its port, or a host naming it)? A guess, not a
/// security check: being wrong costs one refused request.
fn is_ollama(base_url: &str) -> bool {
    let endpoint = base_url.to_ascii_lowercase();
    endpoint.contains(OLLAMA_PORT) || endpoint.contains("ollama")
}

/// Is this endpoint on this machine (Local, M5)? Anything else is Cloud, where passages
/// leave the machine. A membership test of known loopback spellings: calling a remote host
/// local would hide the privacy warning.
fn is_local(base_url: &str) -> bool {
    let host = host_of(base_url);
    host == "localhost"
        || host == "::1"
        || host == "[::1]"
        || host == "0.0.0.0"
        || is_loopback_v4(&host)
        || host.ends_with(".localhost")
}

/// `127.0.0.0/8` as a dotted quad. Parsed, not prefix-matched: `127.notes.example.com` is
/// a DNS name.
fn is_loopback_v4(host: &str) -> bool {
    matches!(host.parse::<std::net::Ipv4Addr>(), Ok(ip) if ip.is_loopback())
}

/// A URL's scheme (when present) and authority. Hand-rolled to avoid a URL crate.
fn scheme_and_authority(url: &str) -> (Option<&str>, &str) {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, url),
    };
    (scheme, rest.split(['/', '?', '#']).next().unwrap_or(""))
}

/// The host portion of a URL, lowercased and without scheme, port, path or
/// credentials.
fn host_of(url: &str) -> String {
    let lowered = scheme_and_authority(url).1.to_ascii_lowercase();
    let authority = lowered.rsplit_once('@').map(|(_, h)| h).unwrap_or(&lowered);
    // IPv6 literals keep their brackets; everything else drops a `:port`.
    match authority.strip_prefix('[') {
        Some(v6) => format!("[{}]", v6.split(']').next().unwrap_or("")),
        None => authority.split(':').next().unwrap_or("").to_string(),
    }
}

/// Ollama's native root from the compat base URL: drop a trailing `/v1` (first, so a
/// path-mounted daemon keeps its prefix), else the authority root. The fallback catches a
/// typo'd path, where "Ollama is running, your path is wrong" is most useful.
fn ollama_root(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    match trimmed.strip_suffix("/v1") {
        Some(root) => root.to_string(),
        None => origin_of(trimmed),
    }
}

/// `scheme://authority`; a missing scheme reads as `http`.
fn origin_of(url: &str) -> String {
    let (scheme, authority) = scheme_and_authority(url);
    format!("{}://{authority}", scheme.unwrap_or("http"))
}

/// How ready chat is, right now — the setup card's top-level branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatState {
    /// A server answered and serves the configured model. Chat works.
    Ready,
    /// Nothing usable is at the endpoint: not running, or a wrong URL. The state picks the
    /// card; the message carries the fix.
    Unreachable,
    /// A server is there, but doesn't serve the configured model.
    ModelMissing,
    /// `B2_LLM=fake` is in force; surfaced so nobody overstates what answered.
    Fake,
}

/// One model an Ollama daemon has installed, as `/api/tags` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OllamaModel {
    /// The name to configure, tag included (`llama3.2:latest`).
    pub name: String,
    /// On-disk size in bytes.
    pub size: u64,
    /// The parameter count as Ollama labels it (`"3.2B"`), when it says.
    pub parameters: Option<String>,
}

/// One rung of the picker heuristic (GH #151), as data so the card can point at this
/// machine's rung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelTier {
    /// Inclusive floor, in whole GB of system memory.
    pub min_ram_gb: u64,
    /// How the rung reads ("16 GB").
    pub ram: &'static str,
    /// The model size band ("7–8B").
    pub size: &'static str,
    /// A concrete model at that band, for `ollama pull`.
    pub model: &'static str,
}

/// The tiers, smallest first. Illustrative and non-binding: a starting point, not a
/// hardware floor.
pub const MODEL_TIERS: [ModelTier; 3] = [
    ModelTier {
        min_ram_gb: 0,
        ram: "8 GB",
        size: "3–4B (4-bit)",
        model: "llama3.2",
    },
    ModelTier {
        min_ram_gb: 16,
        ram: "16 GB",
        size: "7–8B",
        model: "llama3.1:8b",
    },
    ModelTier {
        min_ram_gb: 32,
        ram: "32 GB or more",
        size: "12–14B and up",
        model: "gemma3:12b",
    },
];

/// The highest tier whose floor `ram_gb` (total, not free) meets.
fn tier_for_ram(ram_gb: u64) -> &'static ModelTier {
    MODEL_TIERS
        .iter()
        .rev()
        .find(|t| ram_gb >= t.min_ram_gb)
        .unwrap_or(&MODEL_TIERS[0])
}

/// The Ollama-native half of the card: what the daemon has, and what to pull.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OllamaSetup {
    /// The daemon's native root (`http://localhost:11434`).
    pub root: String,
    /// Whether the native API answered at all.
    pub running: bool,
    /// Every installed model; empty is the "no model" state, not "no server".
    pub installed: Vec<OllamaModel>,
    /// Total system memory in whole GB, when the platform could be asked.
    pub ram_gb: Option<u64>,
    /// The whole heuristic, so the card can show the table it chose from.
    pub tiers: Vec<ModelTier>,
    /// This machine's rung, or `None` when memory couldn't be read.
    pub suggested: Option<ModelTier>,
}

/// Everything an adapter needs for the chat setup card and Settings section. A status,
/// not a result: an unreachable daemon renders a card rather than raising.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatSetup {
    /// The OpenAI-compatible base URL in force (env + adapter overrides applied).
    pub base_url: String,
    /// The chat model id in force.
    pub model: String,
    /// `true` for Cloud models: the flag the privacy copy hangs off (M5).
    pub cloud: bool,
    /// Which source supplied the bearer token, never the token (GH #176).
    pub api_key_source: ApiKeySource,
    pub state: ChatState,
    /// A generic, actionable sentence when `state` isn't `Ready` (E4), shared by both
    /// adapters.
    pub message: Option<String>,
    /// Models the endpoint says it serves, when it said.
    pub available: Vec<String>,
    /// The Ollama-native onboarding half; `None` for any other runtime.
    pub ollama: Option<OllamaSetup>,
    /// The tool-call cap, as a Settings field needs it.
    pub tool_calls: ToolCallCap,
}

/// [`LlmConfig::max_tool_calls`] for a surface that edits it, with its default and ceiling
/// so the frontend never advertises a range the parser would refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ToolCallCap {
    /// The cap in force: Settings over the environment over the default.
    pub in_force: usize,
    /// [`crate::DEFAULT_MAX_TOOL_CALLS`].
    pub default: usize,
    /// [`crate::MAX_TOOL_CALLS_CEILING`].
    pub ceiling: usize,
}

impl ToolCallCap {
    /// The cap as `config` resolves it.
    pub fn of(config: &LlmConfig) -> Self {
        Self {
            in_force: config.max_tool_calls,
            default: crate::DEFAULT_MAX_TOOL_CALLS,
            ceiling: crate::MAX_TOOL_CALLS_CEILING,
        }
    }
}

impl ChatSetup {
    /// The setup under `B2_LLM=fake`: no server, nothing to configure.
    pub fn fake(config: &LlmConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            cloud: false,
            api_key_source: ApiKeySource::None,
            state: ChatState::Fake,
            message: Some(FAKE_NOTICE.to_string()),
            available: Vec::new(),
            ollama: None,
            tool_calls: ToolCallCap::of(config),
        }
    }
}

/// Ask the configured endpoint what it can do: `GET /models`, plus `GET /api/tags` when it
/// looks like Ollama. Never fails: each problem is a [`ChatState`] with a sentence.
pub fn probe_setup(config: &LlmConfig) -> ChatSetup {
    let ollama = is_ollama(&config.base_url).then(|| ollama_setup(&config.base_url));
    let (state, message, available) = match OpenAiCompatProvider::new(config.clone()).probe() {
        Ok(()) => (ChatState::Ready, None, Vec::new()),
        Err(LlmError::ModelMissing {
            model, available, ..
        }) => (
            ChatState::ModelMissing,
            Some(model_missing_message(&model, &config.base_url)),
            available,
        ),
        // Refused, not unreachable. A daemon that answered `/api/tags` proves the server
        // is up, which turns a 404 into a diagnosis.
        Err(LlmError::Refused {
            status, message, ..
        }) => (
            ChatState::Unreachable,
            Some(refusal_message(
                &config.base_url,
                status,
                &message,
                ollama
                    .as_ref()
                    .filter(|o| o.running)
                    .map(|o| o.root.as_str()),
            )),
            Vec::new(),
        ),
        Err(_) => (
            ChatState::Unreachable,
            Some(unreachable_message(&config.base_url)),
            Vec::new(),
        ),
    };
    ChatSetup {
        base_url: config.base_url.clone(),
        model: config.model.clone(),
        cloud: !is_local(&config.base_url),
        api_key_source: config.api_key_source,
        state,
        message,
        available,
        ollama,
        tool_calls: ToolCallCap::of(config),
    }
}

/// E4: nothing is listening at `base_url`. Suggests `ollama serve` only for Ollama. Both
/// adapters use it and append their own way to change the endpoint.
pub fn unreachable_message(base_url: &str) -> String {
    if is_ollama(base_url) {
        format!(
            "Can't reach the model server at {base_url} — is Ollama running? \
             (`ollama serve`, or install: {OLLAMA_INSTALL_URL})"
        )
    } else {
        format!("Can't reach the model server at {base_url}. Check that it's running.")
    }
}

/// The sentence for [`LlmError::Refused`], by status: 401/403 is the credential, 404-ish
/// is the base URL path, anything else quotes the server. `ollama_root` is `Some` only when
/// the daemon answered its native API, so the 404 branch can name the URL that would work.
pub fn refusal_message(
    base_url: &str,
    status: u16,
    detail: &str,
    ollama_root: Option<&str>,
) -> String {
    match status {
        401 | 403 => format!(
            "The model server at {base_url} refused the request (HTTP {status}). \
             Check the API key for this endpoint."
        ),
        404 | 405 | 410 | 501 => {
            let mut message = format!(
                "Something is running at {base_url}, but it isn't an OpenAI-compatible API \
                 (HTTP {status}). Check the endpoint path — it usually ends in `/v1`."
            );
            // Never echo the user's own URL back as the fix.
            if let Some(root) = ollama_root {
                let suggestion = format!("{root}/v1");
                if suggestion != base_url.trim_end_matches('/') {
                    message.push_str(&format!(
                        " The Ollama daemon at {root} is running — try {suggestion}."
                    ));
                }
            }
            message
        }
        _ if detail.is_empty() => {
            format!("The model server at {base_url} answered with HTTP {status}.")
        }
        _ => format!("The model server at {base_url} answered with HTTP {status}: {detail}"),
    }
}

/// The sentence for [`LlmError::ModelMissing`]; Ollama gets its own fix.
pub fn model_missing_message(model: &str, base_url: &str) -> String {
    if is_ollama(base_url) {
        format!(
            "The model server doesn't have '{model}'. Pull it with `{}`.",
            pull_command(model)
        )
    } else {
        format!("The model server at {base_url} doesn't serve '{model}'. Load it there.")
    }
}

/// `ollama pull <model>`, spelled once for the button, its clipboard payload and messages.
pub fn pull_command(model: &str) -> String {
    format!("ollama pull {model}")
}

/// The native half: `GET {root}/api/tags`, plus the machine's memory and rung.
/// `running: false` is the "no server" card; running and empty is "no model".
fn ollama_setup(base_url: &str) -> OllamaSetup {
    let root = ollama_root(base_url);
    let installed = installed_models(&root);
    let ram_gb = system_ram_gb();
    OllamaSetup {
        root,
        running: installed.is_some(),
        installed: installed.unwrap_or_default(),
        ram_gb,
        tiers: MODEL_TIERS.to_vec(),
        suggested: ram_gb.map(|gb| *tier_for_ram(gb)),
    }
}

/// What `GET {root}/api/tags` returns; B2 reads three fields.
#[derive(Debug, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagsModel>,
}

#[derive(Debug, Deserialize)]
struct TagsModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    details: Option<TagsDetails>,
}

#[derive(Debug, Deserialize)]
struct TagsDetails {
    #[serde(default)]
    parameter_size: Option<String>,
}

/// Every model the daemon at `root` has installed, or `None` when it didn't answer. Its own
/// agent: a shorter timeout, and it must never send a cloud bearer token.
fn installed_models(root: &str) -> Option<Vec<OllamaModel>> {
    let url = format!("{root}{TAGS_PATH}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(TAGS_TIMEOUT)
        .timeout_read(TAGS_TIMEOUT)
        .build();
    let response = agent
        .get(&url)
        .timeout(TAGS_TIMEOUT)
        .call()
        .map_err(|e| {
            tracing::debug!(target: "b2::llm", error = %e, url, "no native Ollama inventory");
        })
        .ok()?;
    let body = response
        .into_string()
        .map_err(|e| {
            tracing::debug!(target: "b2::llm", error = %e, url, "unreadable model inventory");
        })
        .ok()?;
    let parsed: TagsResponse = serde_json::from_str(&body)
        .map_err(|e| {
            tracing::debug!(target: "b2::llm", error = %e, url, "unparseable model inventory");
        })
        .ok()?;
    Some(models_from(parsed))
}

/// Ollama's inventory narrowed to what the card shows; split out so it is testable. A
/// nameless entry (a partial pull) is dropped.
fn models_from(parsed: TagsResponse) -> Vec<OllamaModel> {
    parsed
        .models
        .into_iter()
        .filter(|m| !m.name.is_empty())
        .map(|m| OllamaModel {
            name: m.name,
            size: m.size,
            parameters: m.details.and_then(|d| d.parameter_size),
        })
        .collect()
}

/// Total system memory in whole GB, or `None` where the platform can't be asked. Read
/// from the OS rather than a dependency; it only feeds a suggestion.
fn system_ram_gb() -> Option<u64> {
    // Rounded, not truncated: Linux's `MemTotal` excludes firmware-reserved memory, so a
    // "16 GB" machine reads ~15.6 and would drop a rung.
    system_ram_bytes().map(|b| (b + GIB / 2) / GIB)
}

/// One gibibyte: what "GB" means in this module.
const GIB: u64 = 1_073_741_824;

#[cfg(target_os = "macos")]
fn system_ram_bytes() -> Option<u64> {
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn system_ram_bytes() -> Option<u64> {
    // Not a shipping platform, but developers work here.
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn system_ram_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_is_recognized_by_port_or_name() {
        assert!(is_ollama("http://localhost:11434/v1"));
        assert!(is_ollama("http://127.0.0.1:11434"));
        assert!(is_ollama("https://ollama.example.com/v1"));
        assert!(!is_ollama("http://localhost:1234/v1"));
        assert!(!is_ollama("https://api.openai.com/v1"));
    }

    #[test]
    fn local_means_this_machine_and_nothing_else() {
        for local in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:11434/v1",
            "http://LOCALHOST:11434/v1",
        ] {
            assert!(is_local(local), "{local} is on this machine");
        }
        for cloud in [
            "https://api.openai.com/v1",
            "https://api.anthropic.com/v1",
            // Hosts that merely look loopback must read as cloud.
            "https://localhost.evil.example.com/v1",
            "https://127.notes.example.com/v1",
            "https://127.0.0.1.example.com/v1",
            "http://192.168.1.9:11434/v1",
        ] {
            assert!(!is_local(cloud), "{cloud} leaves this machine");
        }
    }

    #[test]
    fn the_native_root_drops_the_compat_suffix() {
        assert_eq!(
            ollama_root("http://localhost:11434/v1"),
            "http://localhost:11434"
        );
        assert_eq!(
            ollama_root("http://localhost:11434/v1/"),
            "http://localhost:11434"
        );
        assert_eq!(
            ollama_root("http://localhost:11434"),
            "http://localhost:11434"
        );
    }

    #[test]
    fn a_root_that_isnt_the_compat_surface_falls_back_to_the_authority() {
        assert_eq!(
            ollama_root("http://localhost:11434/v1X"),
            "http://localhost:11434"
        );
        assert_eq!(
            ollama_root("http://localhost:11434/api"),
            "http://localhost:11434"
        );
        // A path-mounted daemon keeps its prefix.
        assert_eq!(
            ollama_root("https://gw.example.com/ollama/v1"),
            "https://gw.example.com/ollama"
        );
    }

    #[test]
    fn a_refusal_names_the_mistake_it_actually_is() {
        let path = refusal_message(
            "http://localhost:11434/v1X",
            404,
            "404 page not found",
            Some("http://localhost:11434"),
        );
        assert!(path.contains("isn't an OpenAI-compatible API"), "{path}");
        assert!(path.contains("HTTP 404"), "{path}");
        assert!(path.contains("try http://localhost:11434/v1"), "{path}");
        assert!(!path.contains("ollama serve"), "the daemon is up: {path}");

        let key = refusal_message("https://api.example.com/v1", 401, "invalid api key", None);
        assert!(key.contains("API key"), "{key}");
        assert!(!key.contains("endpoint path"), "one fix, not two: {key}");

        let bare = refusal_message("http://localhost:1234/v2", 404, "", None);
        assert!(bare.contains("endpoint path"), "{bare}");
        assert!(!bare.to_lowercase().contains("ollama"), "{bare}");

        let busy = refusal_message("https://api.example.com/v1", 503, "at capacity", None);
        assert!(busy.contains("HTTP 503"), "{busy}");
        assert!(busy.contains("at capacity"), "{busy}");
    }

    #[test]
    fn a_refusal_never_suggests_the_url_it_was_given() {
        let same = refusal_message(
            "http://localhost:11434/v1",
            404,
            "",
            Some("http://localhost:11434"),
        );
        assert!(!same.contains("try "), "{same}");
    }

    #[test]
    fn tiers_pick_the_highest_rung_the_machine_meets() {
        assert_eq!(tier_for_ram(8).model, MODEL_TIERS[0].model);
        assert_eq!(tier_for_ram(15).model, MODEL_TIERS[0].model);
        assert_eq!(tier_for_ram(16).model, MODEL_TIERS[1].model);
        assert_eq!(tier_for_ram(24).model, MODEL_TIERS[1].model);
        assert_eq!(tier_for_ram(64).model, MODEL_TIERS[2].model);
        assert_eq!(tier_for_ram(0).model, MODEL_TIERS[0].model);
    }

    #[test]
    fn the_inventory_reads_ollamas_own_shape() {
        let body = r#"{"models":[
            {"name":"llama3.2:latest","size":2019393189,"details":{"parameter_size":"3.2B"}},
            {"name":"bare:latest","size":10},
            {"name":"","size":0}
        ]}"#;
        let models = models_from(serde_json::from_str(body).unwrap());
        assert_eq!(
            models,
            vec![
                OllamaModel {
                    name: "llama3.2:latest".into(),
                    size: 2_019_393_189,
                    parameters: Some("3.2B".into()),
                },
                OllamaModel {
                    name: "bare:latest".into(),
                    size: 10,
                    parameters: None,
                },
            ]
        );
        // No `models` key is an empty inventory, not a failure.
        assert!(models_from(serde_json::from_str("{}").unwrap()).is_empty());
    }

    /// The setup view crosses to a webview, so it must never carry the bearer token.
    #[test]
    fn the_setup_view_reports_a_key_without_carrying_it() {
        let config = LlmConfig {
            base_url: "https://api.example.com/v1".into(),
            model: "some-model".into(),
            api_key: Some("sk-live-do-not-serialize-me".into()),
            api_key_source: ApiKeySource::Stored,
            ..LlmConfig::default()
        };
        let setup = ChatSetup {
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            cloud: !is_local(&config.base_url),
            api_key_source: config.api_key_source,
            state: ChatState::Ready,
            message: None,
            available: Vec::new(),
            ollama: None,
            tool_calls: ToolCallCap::of(&config),
        };
        let json = serde_json::to_string(&setup).unwrap();
        assert!(!json.contains("sk-live-do-not-serialize-me"), "{json}");
        assert!(
            json.contains("\"tool_calls\":{\"in_force\":64,\"default\":64,\"ceiling\":4096}"),
            "{json}"
        );
        assert!(json.contains("\"api_key_source\":\"stored\""), "{json}");
        assert!(json.contains("\"cloud\":true"), "{json}");
    }

    /// GH #176: Settings tells the user when the key in force is their shell's.
    #[test]
    fn the_setup_view_names_the_source_the_resolver_chose() {
        // `.invalid` is reserved (RFC 2606), so the probe fails at once.
        let stored_under_an_env_key = LlmConfig {
            base_url: "http://b2-no-such-host.invalid:11434/v1".into(),
            api_key: Some("sk-from-the-environment".into()),
            api_key_source: ApiKeySource::Environment,
            ..LlmConfig::default()
        }
        .with_api_key(Some("sk-from-the-keychain"), ApiKeySource::Stored);
        assert_eq!(
            probe_setup(&stored_under_an_env_key).api_key_source,
            ApiKeySource::Environment
        );
        assert_eq!(
            ChatSetup::fake(&LlmConfig::default()).api_key_source,
            ApiKeySource::None
        );
    }

    #[test]
    fn the_fake_setup_says_so() {
        let setup = ChatSetup::fake(&LlmConfig::default());
        assert_eq!(setup.state, ChatState::Fake);
        assert!(setup.message.is_some_and(|m| m.contains("B2_LLM=fake")));
    }

    #[test]
    fn messages_name_ollamas_own_fix_only_for_ollama() {
        let ollama = unreachable_message("http://localhost:11434/v1");
        assert!(ollama.contains("ollama serve"), "{ollama}");
        assert!(ollama.contains(OLLAMA_INSTALL_URL), "{ollama}");
        let other = unreachable_message("http://localhost:1234/v1");
        assert!(!other.to_lowercase().contains("ollama"), "{other}");
        assert!(
            model_missing_message("llama3.2", "http://localhost:11434/v1")
                .contains("ollama pull llama3.2")
        );
        assert!(
            !model_missing_message("gpt-4o", "https://api.openai.com/v1")
                .to_lowercase()
                .contains("ollama")
        );
    }

    /// A reserved `.invalid` host can't resolve, so this stays hermetic.
    #[test]
    fn an_unreachable_endpoint_is_a_card_not_a_failure() {
        let setup = probe_setup(&LlmConfig {
            base_url: "http://b2-no-such-host.invalid:11434/v1".into(),
            model: "llama3.2".into(),
            ..LlmConfig::default()
        });
        assert_eq!(setup.state, ChatState::Unreachable);
        assert!(setup.message.is_some_and(|m| m.contains("ollama serve")));
        // Ollama's port, so the native half was attempted.
        let ollama = setup
            .ollama
            .expect("an Ollama-shaped endpoint gets the card");
        assert!(!ollama.running);
        assert!(ollama.installed.is_empty());
        assert_eq!(ollama.tiers.len(), MODEL_TIERS.len());
    }
}
