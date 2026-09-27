//! Chat wiring (ADR-0005): which provider a chat command talks to, with Settings layered
//! over `b2_llm::LlmConfig::from_env` as a CLI flag would be. Adapter state only, so a chat
//! model swap costs no reindex.
//!
//! The API key lives in the Keychain, never in `chat.json` (GH #176), and never crosses to
//! the webview. `B2_LLM_API_KEY` outranks it; that ranking is the resolver's.

use crate::keychain::KeyStore;
use crate::state_file;
use b2_core::llm::LlmProvider;
use b2_llm::{ApiKeySource, LlmConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The state file holding the persisted half of [`ChatPrefs`].
const PREFS_FILE: &str = "chat.json";

/// The desktop's chat preferences plus the key in force. `None` means "whatever the
/// environment and defaults say", as the CLI with no flags.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPrefs {
    /// The OpenAI-compatible base URL the user typed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The chat model id the user typed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The tool-call cap the user set, if any (`LlmConfig::max_tool_calls`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<usize>,
    /// The bearer token in force for this run. `skip` both ways: it can never reach the
    /// file, and a file naming it can't install one.
    #[serde(skip)]
    pub api_key: Option<String>,
    /// Whether [`api_key`](Self::api_key) is also in the [`KeyStore`]. Read it through
    /// [`ChatPrefs::api_key_source`].
    #[serde(skip)]
    pub key_remembered: bool,
}

/// Hand-written, as `LlmConfig`'s is, so the key can never reach a log; only its presence
/// prints.
impl std::fmt::Debug for ChatPrefs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatPrefs")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tool_calls", &self.max_tool_calls)
            .field(
                "api_key",
                &match self.api_key {
                    Some(_) => "Some(<redacted>)",
                    None => "None",
                },
            )
            .field("api_key_source", &self.api_key_source())
            .finish()
    }
}

impl ChatPrefs {
    /// The configuration these preferences resolve to, over the shared env resolution.
    /// The key yields to `B2_LLM_API_KEY` instead (GH #176).
    pub fn config(&self) -> LlmConfig {
        LlmConfig::from_env()
            .with_overrides(self.base_url.as_deref(), self.model.as_deref())
            .with_max_tool_calls(self.max_tool_calls)
            .with_api_key(self.api_key.as_deref(), self.api_key_source())
    }

    /// Where this host's own key came from; never [`ApiKeySource::Environment`], which
    /// only [`LlmConfig::from_env`] claims.
    pub fn api_key_source(&self) -> ApiKeySource {
        match (&self.api_key, self.key_remembered) {
            (None, _) => ApiKeySource::None,
            (Some(_), true) => ApiKeySource::Stored,
            (Some(_), false) => ApiKeySource::Session,
        }
    }
}

/// The chat provider, by the same rule as the CLI. Unlike the CLI's `open_llm` it doesn't
/// probe: the desktop probes once when the chat surface opens, not per question.
pub fn provider(prefs: &ChatPrefs) -> Box<dyn LlmProvider> {
    b2_llm::provider(prefs.config())
}

/// Where the persisted half lives, or `None` when the platform has no data dir.
pub fn prefs_file() -> Option<PathBuf> {
    state_file::path(PREFS_FILE)
}

/// The preferences this launch starts from: the state file, and the key from the
/// [`KeyStore`]. Best-effort: a bad file is "nothing configured", a refusing store "no key".
pub fn read_prefs(keys: &dyn KeyStore) -> ChatPrefs {
    let file = prefs_file();
    read_prefs_from(file.as_deref(), keys)
}

/// [`read_prefs`] against an explicit path. `None` (no data dir) still reads the key.
pub fn read_prefs_from(file: Option<&Path>, keys: &dyn KeyStore) -> ChatPrefs {
    let mut prefs = match file.map(std::fs::read_to_string) {
        Some(Ok(text)) => match serde_json::from_str::<ChatPrefs>(&text) {
            Ok(prefs) => prefs,
            Err(e) => {
                eprintln!("[b2] ignoring unreadable chat settings: {e}");
                ChatPrefs::default()
            }
        },
        _ => ChatPrefs::default(),
    };
    // Silent, no prompt, when no key was ever saved.
    prefs.api_key = keys.load();
    prefs.key_remembered = prefs.api_key.is_some();
    prefs
}

/// Apply a save's key field, writing the [`KeyStore`] to match; returns
/// `(key, remembered)`. The field paints empty even when a key is set, so it is
/// three-state: `None` untouched, blank clears (the Remove button, which must be possible
/// or repointing `base_url` would leak the key to a new provider), a value sets.
///
/// A refused save degrades to [`ApiKeySource::Session`]. A refused clear keeps the key as
/// it was, or the next launch would read it back.
pub fn apply_key(
    prev: &ChatPrefs,
    typed: Option<&str>,
    keys: &dyn KeyStore,
) -> (Option<String>, bool) {
    let Some(typed) = typed else {
        return (prev.api_key.clone(), prev.key_remembered);
    };
    match typed.trim() {
        "" if keys.clear() => (None, false),
        // The store refused to let go: keep the key as it stood.
        "" => (prev.api_key.clone(), prev.key_remembered),
        key => (Some(key.to_string()), keys.save(key)),
    }
}

/// Apply a save's tool-call-cap field with [`apply_key`]'s three-state rule, so a save
/// that doesn't mention it can't reset it. A value `b2_llm::parse_max_tool_calls` refuses
/// leaves the cap as it stood.
pub fn apply_tool_cap(prev: &ChatPrefs, typed: Option<&str>) -> Option<usize> {
    match typed.map(str::trim) {
        None => prev.max_tool_calls,
        Some("") => None,
        Some(raw) => b2_llm::parse_max_tool_calls(raw).or(prev.max_tool_calls),
    }
}

/// Remember the endpoint and model. Best-effort: the change is already live in memory.
pub fn persist_prefs(prefs: &ChatPrefs) {
    state_file::update(PREFS_FILE, "remember chat settings", |file| {
        write_prefs_to(file, prefs)
    });
}

/// [`persist_prefs`] against an explicit path. The API key is skipped.
pub fn write_prefs_to(file: &Path, prefs: &ChatPrefs) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(prefs)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    state_file::write(file, json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::MemoryStore;

    #[test]
    fn prefs_round_trip_without_the_key_in_the_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        // The parent dir doesn't exist yet.
        let file = tmp.path().join("state/b2/chat.json");
        let prefs = ChatPrefs {
            base_url: Some("http://localhost:1234/v1".into()),
            model: Some("qwen2.5".into()),
            api_key: Some("sk-live-must-not-persist".into()),
            key_remembered: true,
            ..ChatPrefs::default()
        };
        write_prefs_to(&file, &prefs).unwrap();

        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert!(
            !on_disk.contains("sk-live-must-not-persist"),
            "a bearer token must never reach the settings file: {on_disk}"
        );
        let back = read_prefs_from(Some(&file), &MemoryStore::empty());
        assert_eq!(back.base_url.as_deref(), Some("http://localhost:1234/v1"));
        assert_eq!(back.model.as_deref(), Some("qwen2.5"));
        assert_eq!(
            back.api_key, None,
            "the settings file is structurally incapable of carrying a key"
        );
    }

    #[test]
    fn the_tool_call_cap_persists_and_layers_over_the_shared_resolution() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("chat.json");
        let prefs = ChatPrefs {
            max_tool_calls: Some(128),
            ..ChatPrefs::default()
        };
        write_prefs_to(&file, &prefs).unwrap();
        let back = read_prefs_from(Some(&file), &MemoryStore::empty());
        assert_eq!(back.max_tool_calls, Some(128));
        assert_eq!(back.config().max_tool_calls, 128);

        write_prefs_to(&file, &ChatPrefs::default()).unwrap();
        assert!(!std::fs::read_to_string(&file)
            .unwrap()
            .contains("max_tool_calls"));
        assert_eq!(
            ChatPrefs::default().config().max_tool_calls,
            LlmConfig::from_env().max_tool_calls
        );

        std::fs::write(&file, r#"{"max_tool_calls": 1000000000}"#).unwrap();
        let edited = read_prefs_from(Some(&file), &MemoryStore::empty());
        assert_eq!(
            edited.config().max_tool_calls,
            LlmConfig::from_env().max_tool_calls
        );
    }

    #[test]
    fn the_tool_call_cap_field_is_untouched_cleared_or_set() {
        let held = ChatPrefs {
            max_tool_calls: Some(128),
            ..ChatPrefs::default()
        };
        assert_eq!(apply_tool_cap(&held, None), Some(128), "untouched");
        assert_eq!(apply_tool_cap(&held, Some("  ")), None, "cleared");
        assert_eq!(apply_tool_cap(&held, Some(" 16 ")), Some(16), "set");
        for refused in ["0", "lots", "-3", "99999999"] {
            assert_eq!(
                apply_tool_cap(&held, Some(refused)),
                Some(128),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn the_key_comes_back_from_the_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("chat.json");
        std::fs::write(&file, r#"{"base_url":"https://api.example.com/v1"}"#).unwrap();

        let prefs = read_prefs_from(Some(&file), &MemoryStore::holding("sk-remembered"));
        assert_eq!(
            prefs.base_url.as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(prefs.api_key.as_deref(), Some("sk-remembered"));
        assert_eq!(prefs.api_key_source(), ApiKeySource::Stored);
    }

    #[test]
    fn a_missing_settings_file_does_not_lose_the_remembered_key() {
        let no_file = read_prefs_from(None, &MemoryStore::holding("sk-remembered"));
        assert_eq!(no_file.base_url, None);
        assert_eq!(no_file.api_key.as_deref(), Some("sk-remembered"));

        let tmp = tempfile::TempDir::new().unwrap();
        let broken = tmp.path().join("broken.json");
        std::fs::write(&broken, "{not json").unwrap();
        let prefs = read_prefs_from(Some(&broken), &MemoryStore::holding("sk-remembered"));
        assert_eq!(prefs.base_url, None);
        assert_eq!(prefs.api_key.as_deref(), Some("sk-remembered"));
    }

    #[test]
    fn applying_the_key_field_keeps_the_store_in_step() {
        let store = MemoryStore::empty();
        let none = ChatPrefs::default();

        // Set.
        let (key, remembered) = apply_key(&none, Some("sk-typed"), &store);
        assert_eq!(key.as_deref(), Some("sk-typed"));
        assert!(remembered);
        assert_eq!(store.peek().as_deref(), Some("sk-typed"));

        let set = ChatPrefs {
            api_key: key,
            key_remembered: remembered,
            ..ChatPrefs::default()
        };
        assert_eq!(set.api_key_source(), ApiKeySource::Stored);

        // Untouched.
        let (key, remembered) = apply_key(&set, None, &store);
        assert_eq!(key.as_deref(), Some("sk-typed"));
        assert!(remembered);
        assert_eq!(store.peek().as_deref(), Some("sk-typed"));

        // Blank: gone from both memory and the store.
        let (key, remembered) = apply_key(&set, Some("   "), &store);
        assert_eq!(key, None);
        assert!(!remembered);
        assert_eq!(store.peek(), None);
    }

    #[test]
    fn a_removal_the_store_refuses_does_not_pretend_to_have_happened() {
        let store = MemoryStore::refusing_holding("sk-wont-let-go");
        let held = ChatPrefs {
            api_key: Some("sk-wont-let-go".into()),
            key_remembered: true,
            ..ChatPrefs::default()
        };

        let (key, remembered) = apply_key(&held, Some(""), &store);
        assert_eq!(
            key.as_deref(),
            Some("sk-wont-let-go"),
            "a key the store still holds must stay in force, not vanish from the UI"
        );
        assert!(remembered);
        assert_eq!(store.peek().as_deref(), Some("sk-wont-let-go"));

        // The next launch agrees with what the panel reports.
        let next_launch = read_prefs_from(None, &store);
        assert_eq!(next_launch.api_key.as_deref(), Some("sk-wont-let-go"));
        assert_eq!(
            next_launch.api_key_source(),
            ChatPrefs {
                api_key: key,
                key_remembered: remembered,
                ..ChatPrefs::default()
            }
            .api_key_source(),
            "no launch may disagree with what the user was last shown"
        );
    }

    #[test]
    fn a_session_key_clears_even_against_a_refusing_store() {
        let store = MemoryStore::refusing();
        let session = ChatPrefs {
            api_key: Some("sk-never-stored".into()),
            key_remembered: false,
            ..ChatPrefs::default()
        };
        let (key, remembered) = apply_key(&session, Some(""), &store);
        assert_eq!(key, None);
        assert!(!remembered);
    }

    #[test]
    fn a_refusing_store_degrades_to_session_only() {
        let store = MemoryStore::refusing();
        let (key, remembered) = apply_key(&ChatPrefs::default(), Some("sk-typed"), &store);
        assert_eq!(key.as_deref(), Some("sk-typed"), "chat must still work");
        assert!(!remembered);
        assert_eq!(store.peek(), None);

        let prefs = ChatPrefs {
            api_key: key,
            key_remembered: remembered,
            ..ChatPrefs::default()
        };
        assert_eq!(prefs.api_key_source(), ApiKeySource::Session);
        // Stated as the layering, so it holds whatever the process env holds.
        assert_eq!(
            prefs.config(),
            LlmConfig::from_env().with_api_key(Some("sk-typed"), ApiKeySource::Session)
        );
    }

    #[test]
    fn debug_never_prints_the_api_key() {
        let prefs = ChatPrefs {
            base_url: Some("https://api.example.com/v1".into()),
            model: Some("some-model".into()),
            api_key: Some("sk-live-do-not-log-me".into()),
            key_remembered: true,
            ..ChatPrefs::default()
        };
        let rendered = format!("{prefs:?}");
        assert!(
            !rendered.contains("sk-live-do-not-log-me"),
            "a bearer token must never reach a log: {rendered}"
        );
        assert!(rendered.contains("redacted"), "{rendered}");
        // Presence and source still show.
        assert!(rendered.contains("api.example.com"), "{rendered}");
        assert!(rendered.contains("Stored"), "{rendered}");
        assert!(
            format!("{:?}", ChatPrefs::default()).contains("api_key: \"None\""),
            "a keyless configuration says so plainly"
        );
    }

    #[test]
    fn a_hand_written_key_in_the_file_is_ignored() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("chat.json");
        std::fs::write(
            &file,
            r#"{"base_url":"http://x/v1","api_key":"sk-smuggled","key_remembered":true}"#,
        )
        .unwrap();
        let prefs = read_prefs_from(Some(&file), &MemoryStore::empty());
        assert_eq!(prefs.base_url.as_deref(), Some("http://x/v1"));
        assert_eq!(prefs.api_key, None);
        assert_eq!(prefs.api_key_source(), ApiKeySource::None);
    }

    #[test]
    fn a_missing_or_broken_file_reads_as_nothing_configured() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(
            read_prefs_from(Some(&tmp.path().join("absent")), &MemoryStore::empty()),
            ChatPrefs::default()
        );
        let broken = tmp.path().join("broken.json");
        std::fs::write(&broken, "{not json").unwrap();
        assert_eq!(
            read_prefs_from(Some(&broken), &MemoryStore::empty()),
            ChatPrefs::default()
        );
    }

    #[test]
    fn empty_prefs_resolve_to_the_shared_default() {
        // Asserts the layering, which holds whatever the process env holds.
        assert_eq!(ChatPrefs::default().config(), LlmConfig::from_env());
        let pointed = ChatPrefs {
            base_url: Some("http://localhost:1234/v1".into()),
            ..ChatPrefs::default()
        };
        assert_eq!(pointed.config().base_url, "http://localhost:1234/v1");
        assert_eq!(pointed.config().model, LlmConfig::from_env().model);
    }
}
