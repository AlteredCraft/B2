//! The **onboarding corner** — deliberately Ollama-native, even though chat is
//! generic (GH #151 §"Errors, degraded modes, onboarding"; cut as GH #155).
//!
//! The chat path speaks the generic OpenAI-compatible `/v1` surface and will talk
//! to LM Studio, llama.cpp, vLLM or a cloud provider with a URL change. The
//! *setup card* speaks Ollama's **native** API: detect the daemon, list what is
//! installed (`GET /api/tags`), suggest a pull sized to the machine. That
//! asymmetry is by design and stated in the spec so nobody later "generalizes"
//! the card against an abstraction that cannot serve it — **guided setup is a
//! per-runtime feature, and Ollama is the runtime B2 guides**.
//!
//! Everything here is a *status*, never an error: [`probe_setup`] cannot fail,
//! because "the daemon isn't running" is the answer the card is asking for. The
//! typed [`LlmError`] still exists for the paths where a failure is a failure
//! (`probe`, a mid-answer break); this module turns it into something to draw.
//!
//! Adapter-level throughout (GH #151): nothing here is recorded in the vault or
//! the index, so a model swap costs no reindex (contrast M2).

use crate::{ApiKeySource, LlmConfig, LlmError, OpenAiCompatProvider};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Ollama's native model inventory — the one non-`/v1` endpoint B2 speaks, and
/// only for onboarding.
const TAGS_PATH: &str = "/api/tags";

/// Where a human who has *no* Ollama is sent. The quickstart rather than the
/// product page: someone reading this message has already found out that nothing
/// is listening, so the page they need is the one with the install command and
/// `ollama pull` on it, not the one with the download button.
///
/// One constant because two adapters print it — `b2-cli`'s `user_message` and the
/// desktop's setup card — and a link that drifts between them is a link one of
/// them gets wrong.
pub const OLLAMA_INSTALL_URL: &str = "https://docs.ollama.com/quickstart";

/// The port Ollama serves on. Recognizing it is what decides whether Ollama's own
/// commands (`ollama serve`, `ollama pull …`) belong in a message — advice about
/// the wrong program is worse than no advice, and `B2_LLM_URL` also points at LM
/// Studio, llama.cpp, vLLM and cloud providers.
const OLLAMA_PORT: &str = ":11434";

/// How long the onboarding round trip may take. Short for [`PROBE_TIMEOUT`]'s
/// reason: a setup card that hangs is worse than one that says "not running".
const TAGS_TIMEOUT: Duration = Duration::from_secs(5);

/// Does this endpoint look like the Ollama daemon — its port, or a host that
/// names it? It decides two things and no others: whether Ollama's own commands
/// belong in an error message, and whether [`probe_setup`] asks `/api/tags` at
/// all.
///
/// Deliberately a *guess about the runtime*, not a security check: being wrong
/// costs one refused HTTP request and a message that mentions the wrong program.
pub fn is_ollama(base_url: &str) -> bool {
    let endpoint = base_url.to_ascii_lowercase();
    endpoint.contains(OLLAMA_PORT) || endpoint.contains("ollama")
}

/// Is this endpoint on **this machine** — the **Local** configuration (invariant
/// M5)? Anything else is **Cloud models**: note passages leave the machine, which
/// is why the desktop shows the privacy copy beside exactly this answer.
///
/// Host-only, and permissive about *how* loopback is spelled (`localhost`,
/// `127.0.0.0/8`, `[::1]`), because the consequence of guessing "cloud" for a local URL
/// is a privacy warning the user doesn't need — while guessing "local" for a
/// remote one would hide a warning they do. So the test is a **membership** one:
/// only the known loopback spellings count as local.
pub fn is_local(base_url: &str) -> bool {
    let host = host_of(base_url);
    host == "localhost"
        || host == "::1"
        || host == "[::1]"
        || host == "0.0.0.0"
        || is_loopback_v4(&host)
        || host.ends_with(".localhost")
}

/// `127.0.0.0/8`, spelled as a dotted quad — and **nothing that merely begins
/// with `127.`**, because a registrable DNS name may (`127.notes.example.com`
/// resolves to wherever its owner points it). Parsing rather than prefix-matching
/// is the membership test [`is_local`] promises: the one direction that must
/// never happen is a remote host reading as local, which is exactly what the
/// prefix let through.
fn is_loopback_v4(host: &str) -> bool {
    matches!(host.parse::<std::net::Ipv4Addr>(), Ok(ip) if ip.is_loopback())
}

/// The host portion of a URL, lowercased and without scheme, port, path or
/// credentials. Hand-rolled rather than a URL crate: this crate's whole posture
/// is one wire shape and no dependency tree, and the two callers here need only
/// the authority's host.
fn host_of(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url)
        .to_ascii_lowercase();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    // IPv6 literals keep their brackets; everything else drops a `:port`.
    match authority.strip_prefix('[') {
        Some(v6) => format!("[{}]", v6.split(']').next().unwrap_or("")),
        None => authority.split(':').next().unwrap_or("").to_string(),
    }
}

/// Ollama's **native** root, derived from the configured OpenAI-compat base URL:
/// `http://localhost:11434/v1` → `http://localhost:11434`. The compat surface is
/// mounted under `/v1`; `/api/tags` is not.
///
/// Two derivations, in order, because the input is a URL a human typed:
///
/// 1. Drop a trailing `/v1`. This is the one that must come first, since it is
///    the only one that survives a **path-mounted** daemon — `https://gw/ollama/v1`
///    keeps its `/ollama` prefix, which the authority alone would throw away.
/// 2. Failing that, fall back to the **authority root**. What lands here is a base
///    URL that isn't the compat surface at all — most often a typo (`…/v1X`) — and
///    that is precisely when knowing whether the daemon is up is worth most: it is
///    the difference between "is Ollama running?" and "Ollama is running, your path
///    is wrong". Asking the wrong root would answer "not running" about a daemon
///    plainly serving requests.
fn ollama_root(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    match trimmed.strip_suffix("/v1") {
        Some(root) => root.to_string(),
        None => origin_of(trimmed),
    }
}

/// `scheme://authority` — the URL with every path segment dropped. A missing
/// scheme reads as `http`, which is what an Ollama URL without one means.
fn origin_of(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s, r),
        None => ("http", url),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    format!("{scheme}://{authority}")
}

/// How ready chat is, right now — the setup card's top-level branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatState {
    /// A server answered and serves the configured model. Chat works.
    Ready,
    /// Nothing usable is at the endpoint: the daemon isn't running, or the URL
    /// is wrong. Both readings live here because both are *the endpoint is not
    /// serving chat*, and the card's copy is where they part — a refusal
    /// ([`LlmError::Refused`]) never gets "is Ollama running?" advice about a
    /// daemon that plainly answered. Which is to say: the **state** picks the
    /// card, the **message** carries the fix.
    Unreachable,
    /// A server is there, but doesn't serve the configured model — the most
    /// common local-setup mistake (an un-pulled Ollama model).
    ModelMissing,
    /// `B2_LLM=fake` is in force: the deterministic provider answers, and no
    /// model is involved at all. Surfaced rather than hidden, for the same
    /// reason `b2 chat` prints its note — never overstate what answered.
    Fake,
}

/// One model an Ollama daemon has installed, as `/api/tags` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OllamaModel {
    /// The name to configure, tag included (`llama3.2:latest`).
    pub name: String,
    /// On-disk size in bytes, for "how much did this cost me" in the picker.
    pub size: u64,
    /// The parameter count as Ollama labels it (`"3.2B"`), when it says.
    pub parameters: Option<String>,
}

/// One rung of the picker heuristic (GH #151: *illustrative, non-binding*) —
/// deliberately data rather than prose, so the card can point at the rung the
/// machine actually sits on instead of printing a table and leaving the
/// arithmetic to the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelTier {
    /// Inclusive floor, in whole GB of system memory.
    pub min_ram_gb: u64,
    /// How the rung reads ("16 GB").
    pub ram: &'static str,
    /// The model size band ("7–8B").
    pub size: &'static str,
    /// A concrete model at that band — what `ollama pull` would be handed.
    pub model: &'static str,
}

/// The tiers, smallest first. **Illustrative and non-binding** (the spec's own
/// words): they are a starting point for someone who has never pulled a model,
/// not a hardware floor — B2 has none, and any OpenAI-compatible endpoint,
/// including a cloud one, satisfies the contract.
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

/// The rung `ram_gb` sits on — the highest tier whose floor it meets. Total
/// installed memory, not free: the question is what the machine can host, and a
/// suggestion that changed with whatever else is open would be noise.
pub fn tier_for_ram(ram_gb: u64) -> &'static ModelTier {
    MODEL_TIERS
        .iter()
        .rev()
        .find(|t| ram_gb >= t.min_ram_gb)
        .unwrap_or(&MODEL_TIERS[0])
}

/// The Ollama-native half of the card: what the daemon has, and what to pull.
/// `None` on the [`ChatSetup`] when the endpoint isn't Ollama's — there is
/// nothing honest to say about pulling models into LM Studio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OllamaSetup {
    /// The daemon's native root (`http://localhost:11434`) — what `/api/tags`
    /// was asked, and what a message names.
    pub root: String,
    /// Whether the native API answered at all.
    pub running: bool,
    /// Every installed model, as the daemon lists them. Empty when the daemon is
    /// running with nothing pulled — which is the "no model" empty state, and a
    /// different card from "no server".
    pub installed: Vec<OllamaModel>,
    /// Total system memory in whole GB, when the platform could be asked.
    pub ram_gb: Option<u64>,
    /// The whole heuristic, so the card can show the table it chose from.
    pub tiers: Vec<ModelTier>,
    /// The rung this machine sits on — `None` when memory couldn't be read, in
    /// which case the card shows the table without picking for the user.
    pub suggested: Option<ModelTier>,
}

/// Everything an adapter needs to draw the chat surface's setup card and its
/// Settings section: the resolved configuration, the verdict, and — when the
/// runtime is Ollama — the native inventory behind it.
///
/// A **status object, not a result**: every field is filled on every path, so an
/// unreachable daemon renders a card rather than raising.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatSetup {
    /// The OpenAI-compatible base URL in force (env + adapter overrides applied).
    pub base_url: String,
    /// The chat model id in force.
    pub model: String,
    /// `false` for the **Local** configuration, `true` for **Cloud models** —
    /// which is the flag the privacy copy hangs off (M5).
    pub cloud: bool,
    /// Whether a bearer token is configured and, when one is, which of the
    /// resolver's sources supplied it. **Never the token itself**: the key does
    /// not cross this boundary in either direction (the `LlmConfig` `Debug`
    /// impl's rule, applied to the view).
    ///
    /// Richer than the "is one set?" boolean it replaced because the Settings
    /// copy is different in each case (GH #176) — a key the environment
    /// supplies cannot be removed from inside the app, and a key the Keychain
    /// refused to take is gone at quit. Both are things a user has to be told
    /// *before* they wonder why.
    pub api_key_source: ApiKeySource,
    pub state: ChatState,
    /// A generic, actionable sentence when `state` isn't `Ready` (E4), else
    /// `None`. Phrased here rather than in each adapter so the CLI's wording and
    /// the app's stay one sentence.
    pub message: Option<String>,
    /// Models the endpoint says it serves, when it said — the list a "that model
    /// isn't there" card offers instead of the one that's missing.
    pub available: Vec<String>,
    /// The Ollama-native onboarding half; `None` for any other runtime.
    pub ollama: Option<OllamaSetup>,
}

impl ChatSetup {
    /// The setup a **fake** provider is in: `B2_LLM=fake`, no server, nothing to
    /// configure. Its own constructor because every other field would otherwise
    /// be a lie about a model that isn't answering.
    pub fn fake(config: &LlmConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            cloud: false,
            api_key_source: ApiKeySource::None,
            state: ChatState::Fake,
            message: Some(
                "The fake chat provider is in use (B2_LLM=fake) — answers are deterministic test \
                 scaffolding, not a model."
                    .to_string(),
            ),
            available: Vec::new(),
            ollama: None,
        }
    }
}

/// Ask the configured endpoint what it can do, and render the answer as a card's
/// worth of facts. One `GET /models` (the generic probe), plus one
/// `GET /api/tags` when the endpoint looks like Ollama's.
///
/// Never fails. A dead daemon, a wrong URL, a model nobody pulled: each is a
/// [`ChatState`] with a sentence, because each is a thing the human can fix and
/// the card exists to say how.
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
        // Something is listening and refused the probe. A different sentence from
        // "nothing is listening", and the one place the Ollama half is *evidence*
        // rather than onboarding: a daemon that answered `/api/tags` proves the
        // server is up, which turns a 404 from a guess into a diagnosis.
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
        Err(e) => (
            ChatState::Unreachable,
            Some(unreachable_message(&config.base_url, &e)),
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
    }
}

/// E4's own sentence, and the reason [`is_ollama`] exists: `ollama serve` is the
/// fix for Ollama and no help whatever for LM Studio, llama.cpp, vLLM or a cloud
/// endpoint — all of which the same setting points at.
fn unreachable_message(base_url: &str, _detail: &LlmError) -> String {
    if is_ollama(base_url) {
        format!(
            "Can't reach the model server at {base_url} — is Ollama running? \
             (`ollama serve`, or install: {OLLAMA_INSTALL_URL})"
        )
    } else {
        format!("Can't reach the model server at {base_url}. Check that it's running.")
    }
}

/// What to tell a human when the endpoint **answered a probe with a refusal** —
/// the sentence for [`LlmError::Refused`], phrased here so the CLI's wording and
/// the app's stay one sentence (as [`unreachable_message`] is).
///
/// Split by status, because these are different mistakes with different fixes and
/// a single "the server said no" would be true of all of them and useful for none:
///
/// - **401/403** — the path is right and the credential isn't. Previously this read
///   as *Connected* and failed at the first question.
/// - **404/405/410/501** — nothing serves `/models` here. Overwhelmingly a base URL
///   that isn't the compat surface (`…/v1X`, `…:11434` with no `/v1`), which is why
///   the fix names the path rather than the server.
/// - anything else — a server that is there and failing (5xx, a rate limit). Its own
///   words are the most useful thing available, so they are what gets shown.
///
/// `ollama_root` is `Some` only when the daemon **answered its native API**, which
/// is proof the machine is serving requests: with it, the 404 branch stops
/// speculating and names the URL that would work.
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
            // Only when it would name something *other* than what the user already
            // typed: echoing their own URL back as the fix is worse than silence.
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

fn model_missing_message(model: &str, base_url: &str) -> String {
    if is_ollama(base_url) {
        format!("The model server doesn't have '{model}'. Pull it with `ollama pull {model}`.")
    } else {
        format!("The model server at {base_url} doesn't serve '{model}'.")
    }
}

/// `ollama pull <model>` — the one command the card tells a human to run, spelled
/// in one place so the button's label, its copy-to-clipboard payload and any
/// message agree.
pub fn pull_command(model: &str) -> String {
    format!("ollama pull {model}")
}

/// The native half: `GET {root}/api/tags`, plus the machine's memory and the rung
/// it sits on. A daemon that doesn't answer is `running: false` with an empty
/// inventory — the "no server" card — and a daemon that answers with nothing
/// installed is `running: true` and still empty: the "no model" card. Those are
/// two different sentences, which is why this is a struct and not a bool.
fn ollama_setup(base_url: &str) -> OllamaSetup {
    let root = ollama_root(base_url);
    let installed = installed_models(&root);
    let ram_gb = system_ram_gb();
    OllamaSetup {
        root,
        running: installed.is_ok(),
        installed: installed.unwrap_or_default(),
        ram_gb,
        tiers: MODEL_TIERS.to_vec(),
        suggested: ram_gb.map(|gb| *tier_for_ram(gb)),
    }
}

/// What `GET {root}/api/tags` returns, of which B2 reads three fields. Ollama's
/// own shape, so it lives beside the one call that speaks it.
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

/// Every model the daemon at `root` has installed. `Err(())` means the native API
/// didn't answer — not that nothing is installed, which is a different card.
///
/// Its own agent rather than the provider's: this is a *different* API on a
/// *different* path, wanted at a different (shorter) timeout, and it must never
/// send the bearer token a cloud configuration might carry — `/api/tags` on an
/// Ollama daemon is not where a cloud key goes.
fn installed_models(root: &str) -> Result<Vec<OllamaModel>, ()> {
    let url = format!("{root}{TAGS_PATH}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(TAGS_TIMEOUT)
        .timeout_read(TAGS_TIMEOUT)
        .build();
    let body = match agent.get(&url).timeout(TAGS_TIMEOUT).call() {
        Ok(r) => r.into_string().map_err(|e| {
            tracing::debug!(target: "b2::llm", error = %e, url, "unreadable model inventory");
        })?,
        Err(e) => {
            tracing::debug!(target: "b2::llm", error = %e, url, "no native Ollama inventory");
            return Err(());
        }
    };
    let parsed: TagsResponse = serde_json::from_str(&body).map_err(|e| {
        tracing::debug!(target: "b2::llm", error = %e, url, "unparseable model inventory");
    })?;
    Ok(models_from(parsed))
}

/// Ollama's inventory shape, narrowed to the three fields the card shows. Split
/// from the HTTP call because it is the only real *parsing* in this module, and a
/// parse that can only be exercised through a live daemon is a parse nobody tests:
/// a nameless entry (which the daemon has been known to return for a partial pull)
/// is dropped rather than painted as a blank row, and an entry with no `details`
/// keeps its name instead of being lost with it.
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

/// Total system memory in whole GB, or `None` where the platform can't be asked.
///
/// Read from the OS rather than pulled in as a dependency (`sysinfo` and friends
/// bring a tree for one number), and best-effort by construction: the only thing
/// it feeds is a *non-binding suggestion*, so `None` costs the card its
/// highlighted row and nothing else.
pub fn system_ram_gb() -> Option<u64> {
    // Rounded, not truncated. The tiers are floors, and a machine sold as "16 GB"
    // does not always report 16 GiB of usable memory — Linux's `MemTotal` excludes
    // what the firmware reserved, so it reads ~15.6, and truncation would drop such
    // a machine a whole rung. Rounding puts it on the rung it was sold as.
    system_ram_bytes().map(|b| (b + GIB / 2) / GIB)
}

/// One gibibyte — what "GB" means everywhere in this module (and what `hw.memsize`
/// and `MemTotal` both report in).
const GIB: u64 = 1_073_741_824;

#[cfg(target_os = "macos")]
fn system_ram_bytes() -> Option<u64> {
    // `sysctl` is part of the base system; B2 ships on macOS (crates/b2-desktop),
    // so this is the platform's own answer rather than a guess.
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn system_ram_bytes() -> Option<u64> {
    // Not a shipping platform, but developers work here — and a suggestion that
    // silently vanished off-macOS would look like a bug in the card.
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
        // LM Studio, llama.cpp and the cloud endpoints must not be told to run
        // `ollama serve` — advice about the wrong program.
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
            // The failures that matter, both of the same shape: a host that merely
            // *looks* loopback must read as cloud, or the privacy copy hides on a
            // remote endpoint. `127.notes.example.com` is a perfectly registrable
            // name, and a `starts_with("127.")` test took it for the loopback range.
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

    /// The typo case, and why the fallback exists: `…/v1X` has no `/v1` to strip,
    /// and the daemon it needs to ask about is at the authority root.
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
        // A path-mounted daemon keeps its prefix — which is the whole reason the
        // `/v1` strip comes first rather than always taking the authority.
        assert_eq!(
            ollama_root("https://gw.example.com/ollama/v1"),
            "https://gw.example.com/ollama"
        );
    }

    /// The reported bug, at the layer that phrases it: a base URL that answers but
    /// isn't a chat API must produce a sentence about the **path**, never
    /// "is Ollama running?" about a daemon that is plainly running.
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
        // The daemon answered its native API, so the fix is a URL, not a guess.
        assert!(path.contains("try http://localhost:11434/v1"), "{path}");
        assert!(!path.contains("ollama serve"), "the daemon is up: {path}");

        // A key is a different fix, and used to read as Connected.
        let key = refusal_message("https://api.example.com/v1", 401, "invalid api key", None);
        assert!(key.contains("API key"), "{key}");
        assert!(!key.contains("endpoint path"), "one fix, not two: {key}");

        // Nothing to suggest: no Ollama evidence, so no invented advice.
        let bare = refusal_message("http://localhost:1234/v2", 404, "", None);
        assert!(bare.contains("endpoint path"), "{bare}");
        assert!(!bare.to_lowercase().contains("ollama"), "{bare}");

        // Anything else is the server's own words, which are the useful part.
        let busy = refusal_message("https://api.example.com/v1", 503, "at capacity", None);
        assert!(busy.contains("HTTP 503"), "{busy}");
        assert!(busy.contains("at capacity"), "{busy}");
    }

    /// The one sentence that would be silly: echoing back the URL the user typed
    /// as the thing to try instead.
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
        // Below the smallest rung is still the smallest rung — a 4 GB machine
        // gets the smallest suggestion, never no suggestion.
        assert_eq!(tier_for_ram(0).model, MODEL_TIERS[0].model);
    }

    /// The setup view is what crosses to a webview, so the one thing it must
    /// never carry is the bearer token — only whether there is one.
    /// The only real parsing in this module, and the reason `models_from` is split
    /// out of the HTTP call: a nameless entry is dropped rather than painted as a
    /// blank row, and an entry with no `details` keeps its name instead of going
    /// with it.
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
        // A body with no `models` key at all is an empty inventory, not a failure:
        // "the daemon answered and has nothing" is a card B2 draws.
        assert!(models_from(serde_json::from_str("{}").unwrap()).is_empty());
    }

    #[test]
    fn the_setup_view_reports_a_key_without_carrying_it() {
        let config = LlmConfig {
            base_url: "https://api.example.com/v1".into(),
            model: "some-model".into(),
            api_key: Some("sk-live-do-not-serialize-me".into()),
            api_key_source: ApiKeySource::Stored,
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
        };
        let json = serde_json::to_string(&setup).unwrap();
        assert!(!json.contains("sk-live-do-not-serialize-me"), "{json}");
        assert!(json.contains("\"api_key_source\":\"stored\""), "{json}");
        assert!(json.contains("\"cloud\":true"), "{json}");
    }

    /// The view's source is the **resolved** one, not a guess about it — which is
    /// what lets the Settings copy tell a user that the key in force is their
    /// shell's, and that the Remove button therefore cannot reach it (GH #176).
    #[test]
    fn the_setup_view_names_the_source_the_resolver_chose() {
        // `.invalid` is reserved (RFC 2606): the probe fails immediately and this
        // never meets a model server a developer happens to be running.
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
        // A keyless configuration says so plainly — what hides the Remove button
        // and the cloud privacy copy alike.
        assert_eq!(
            ChatSetup::fake(&LlmConfig::default()).api_key_source,
            ApiKeySource::None
        );
    }

    /// `B2_LLM=fake` is surfaced, not hidden: the CLI prints a note for the same
    /// reason (never overstate what answered).
    #[test]
    fn the_fake_setup_says_so() {
        let setup = ChatSetup::fake(&LlmConfig::default());
        assert_eq!(setup.state, ChatState::Fake);
        assert!(setup.message.is_some_and(|m| m.contains("B2_LLM=fake")));
    }

    #[test]
    fn messages_name_ollamas_own_fix_only_for_ollama() {
        let ollama = unreachable_message(
            "http://localhost:11434/v1",
            &LlmError::Unreachable {
                endpoint: "http://localhost:11434/v1".into(),
                detail: "connection refused".into(),
            },
        );
        assert!(ollama.contains("ollama serve"), "{ollama}");
        // The other half of "nothing is listening": the daemon may not be installed at
        // all, and the fix for that is a page, not a command.
        assert!(ollama.contains(OLLAMA_INSTALL_URL), "{ollama}");
        let other = unreachable_message(
            "http://localhost:1234/v1",
            &LlmError::Unreachable {
                endpoint: "http://localhost:1234/v1".into(),
                detail: "connection refused".into(),
            },
        );
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

    /// An unreachable endpoint is a *status*, not an error — the whole point of
    /// this module. Port 1 on a reserved-`.invalid` host can't resolve, so this
    /// stays hermetic and never meets a daemon a developer happens to be running.
    #[test]
    fn an_unreachable_endpoint_is_a_card_not_a_failure() {
        let setup = probe_setup(&LlmConfig {
            base_url: "http://b2-no-such-host.invalid:11434/v1".into(),
            model: "llama3.2".into(),
            ..LlmConfig::default()
        });
        assert_eq!(setup.state, ChatState::Unreachable);
        assert!(setup.message.is_some_and(|m| m.contains("ollama serve")));
        // The endpoint names Ollama's port, so the native half was attempted —
        // and honestly reports a daemon that isn't there.
        let ollama = setup
            .ollama
            .expect("an Ollama-shaped endpoint gets the card");
        assert!(!ollama.running);
        assert!(ollama.installed.is_empty());
        assert_eq!(ollama.tiers.len(), MODEL_TIERS.len());
    }
}
