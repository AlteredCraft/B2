//! Chat wiring — the desktop's half of the second AI seam (GH #151, cut as
//! GH #155), and the sibling of `main.rs`'s embedder wiring.
//!
//! What lives here is *which provider a chat command talks to*, resolved the way
//! the CLI resolves it and layered the same way the vault root is: **flag beats
//! environment beats default**, with the app's Settings standing in for the
//! flags. Resolution itself is `b2_llm::LlmConfig::from_env`'s — one place, so
//! the two adapters cannot drift (root CLAUDE.md).
//!
//! Three things are deliberate and worth reading before changing them:
//!
//! * **Chat config is adapter state, never vault or index state** (GH #151).
//!   Nothing here is recorded in the vault, in `meta`, or anywhere the index can
//!   see it — which is what makes "change models at any time" true by
//!   construction: a chat model swap costs no reindex (contrast M2). The
//!   endpoint and the model id persist beside the remembered vault, in the app's
//!   own data dir, exactly like `last-vault`.
//! * **The API key is remembered in the Keychain, never in a file** (GH #176).
//!   A **Cloud models** configuration (M5 — the only way note content leaves the
//!   machine) needs a bearer token. It is held in memory for the run and, when
//!   the platform will take it, in the macOS Keychain so the next launch has it
//!   — `keychain.rs` argues that choice. What it is *not* is a field in
//!   `chat.json`: the endpoint and model persist there, and the key remains
//!   `#[serde(skip)]` on both halves, so it is structurally incapable of
//!   reaching that file and a hand-edited one cannot put a key back. It never
//!   crosses to the webview either — the status view carries an
//!   [`ApiKeySource`], never the key.
//! * **`B2_LLM_API_KEY` outranks the remembered key.** A shell that exports one
//!   gets the key it named, whatever is in the Keychain: it is the way to point
//!   a single launch at another provider, and the way to tell B2 to keep no
//!   secret of its own. The rule is `b2_llm::LlmConfig::with_api_key`'s, in the
//!   one resolver, so this adapter states its sources and doesn't rank them.

use crate::keychain::KeyStore;
use b2_core::llm::{FakeLlm, LlmProvider};
use b2_llm::{ApiKeySource, LlmConfig, OpenAiCompatProvider};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The desktop's chat preferences: the Settings section's two persisted fields,
/// plus the key in force. `None` means "whatever the environment and the
/// defaults say" — so a user who has never opened the section is exactly the CLI
/// with no flags, and clearing a field returns to that rather than to `""`.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPrefs {
    /// The OpenAI-compatible base URL the user typed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The chat model id the user typed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The bearer token for a cloud endpoint, as it stands for this run.
    ///
    /// `skip` on both halves, not merely `skip_serializing_if`: this type is
    /// what [`write_prefs_to`] writes, so the field must be structurally
    /// incapable of reaching that file, and a file that somehow named it must
    /// not be able to put one *back*. A key that outlives the run lives in the
    /// Keychain (`keychain.rs`), which is a different door entirely.
    #[serde(skip)]
    pub api_key: Option<String>,
    /// Whether [`api_key`](Self::api_key) is also in the [`KeyStore`] — i.e.
    /// whether it survives quit.
    ///
    /// Meaningless on its own (a store cannot remember a key that isn't set),
    /// which is why nothing reads it directly: [`ChatPrefs::api_key_source`] is
    /// the answer callers want, and deriving that from the pair is what keeps
    /// "a key with no source" unrepresentable. Skipped for its neighbor's
    /// reason — it is a fact about a secret, and the launch that reloads the
    /// key is what re-establishes it.
    #[serde(skip)]
    pub key_remembered: bool,
}

/// Hand-written for `LlmConfig`'s reason, applied to the type that actually holds
/// the key in force: **a secret kept in an encrypted store must not leak out of a
/// log line instead.** `#[serde(skip)]` above closes the settings-file path and
/// the Keychain closes the at-rest one; neither says anything
/// about formatting — and `Debug` is what an adapter reaches for when something is
/// wrong (a `tracing` field, a panic message, a `dbg!`), which in this host means
/// stderr and, under `B2_LOG_FILE`, a JSONL file the user may well paste into an
/// issue. Only the key's *presence* prints, which is the part that helps diagnose
/// "the cloud endpoint rejects me".
impl std::fmt::Debug for ChatPrefs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatPrefs")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
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
    /// The configuration these preferences resolve to: the shared env resolution
    /// with this host's explicit choices laid over it (the `B2_VAULT_PATH`
    /// convention — an explicit setting wins, the environment seeds it).
    ///
    /// The key is the one field that does **not** follow that rule, and the
    /// asymmetry is deliberate: `with_api_key` yields to `B2_LLM_API_KEY` (GH
    /// #176). This adapter passes what it has and where it got it; the ranking
    /// is the resolver's.
    pub fn config(&self) -> LlmConfig {
        LlmConfig::from_env()
            .with_overrides(self.base_url.as_deref(), self.model.as_deref())
            .with_api_key(self.api_key.as_deref(), self.api_key_source())
    }

    /// Where this host's own key came from — never [`ApiKeySource::Environment`],
    /// which only [`LlmConfig::from_env`] has the standing to claim. Derived
    /// rather than stored so the pair it reads can never disagree with it.
    pub fn api_key_source(&self) -> ApiKeySource {
        match (&self.api_key, self.key_remembered) {
            (None, _) => ApiKeySource::None,
            (Some(_), true) => ApiKeySource::Stored,
            (Some(_), false) => ApiKeySource::Session,
        }
    }
}

/// Whether the deterministic fake chat provider is forced (`B2_LLM=fake`) — the
/// `B2_EMBEDDER=fake` sibling, honored identically to the CLI so the two adapters
/// behave the same offline.
pub fn use_fake_llm() -> bool {
    matches!(std::env::var("B2_LLM").ok().as_deref(), Some("fake"))
}

/// Pick + wire the chat provider — `main.rs`'s `open_vault` for the second seam.
///
/// Unlike the CLI's `open_llm` this does **not** probe: the desktop probes once
/// when the chat surface opens (`chat_setup`), and paying a round trip per turn
/// would be a per-question tax on a cloud endpoint for an answer the pane already
/// has on screen. A server that dies between the probe and the question surfaces
/// as the same actionable message, from `Error::Llm` (see `error.rs`).
pub fn provider(prefs: &ChatPrefs) -> Box<dyn LlmProvider> {
    if use_fake_llm() {
        return Box::new(FakeLlm);
    }
    Box::new(OpenAiCompatProvider::new(prefs.config()))
}

/// Where the persisted half lives: `<data-dir>/b2/chat.json`, beside the
/// remembered vault (`main.rs`'s `last_vault_file`) and under the same `b2/`
/// vendor dir as the model cache. `None` only if the platform has no data dir,
/// in which case remembering is silently skipped — chat still works, it just
/// forgets the endpoint at quit.
pub fn prefs_file() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("b2").join("chat.json"))
}

/// The preferences this launch starts from: the endpoint and model out of the
/// state file, and the API key out of the [`KeyStore`]. Best-effort in every
/// direction — an unreadable or malformed file reads as "nothing configured", and
/// a store with nothing in it (or one that refuses) reads as "no key" — because
/// the fallback is a working local configuration.
pub fn read_prefs(keys: &dyn KeyStore) -> ChatPrefs {
    let file = prefs_file();
    read_prefs_from(file.as_deref(), keys)
}

/// [`read_prefs`] against an explicit path — the testable core (a tempfile stands in
/// for the real state file and `MemoryStore` for the Keychain, so tests touch neither
/// the user's data dir nor their keychain). `None` is the platform with no data dir:
/// no file to read, and a key that is still remembered, since the two live in
/// different places.
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
    // Reading is silent and prompts for nothing when no key was ever saved,
    // which is every user who has not configured a cloud model — so this costs
    // the common launch nothing (`keychain.rs`).
    prefs.api_key = keys.load();
    prefs.key_remembered = prefs.api_key.is_some();
    prefs
}

/// Apply a save's key field to the key in force, writing the [`KeyStore`] to
/// match — the one place the store is written, and the whole of the field's
/// three-state rule.
///
/// **The key is three-state, and has to be.** The Settings field paints empty
/// even when a key is set (a password field that echoes its secret back is a
/// worse idea), so "I didn't touch it" and "I cleared it" would otherwise be the
/// same input — and collapsing them either signs the user out on every save or
/// makes the key impossible to remove. The second is the dangerous one: with the
/// key un-removable, repointing `base_url` at a different provider would send the
/// *first* provider's bearer token to the second. So:
///
/// * `None` — **untouched**. Re-saving the endpoint must not sign you out, and
///   the store already agrees with what's in force.
/// * `Some(blank)` — **clear**, the UI's Remove button and the only way back to a
///   keyless configuration. Forgotten in the store as well as in memory, or it
///   would return at the next launch and make Remove a lie.
/// * `Some(key)` — **set**, and remembered for next time.
///
/// **Removal is all-or-nothing**, and that asymmetry with the *set* path is the
/// point. A refused save is survivable — the key still works, just for this run —
/// so it degrades. A refused *delete* is not: dropping the key from memory while
/// the store keeps it would show the user a keyless configuration and then hand
/// the credential back at the next launch. So a store that won't let go leaves
/// the key exactly where it was, and the panel goes on showing it, which is the
/// truth ([`KeyStore::clear`]).
///
/// The returned pair is `(key, remembered)` — [`ChatPrefs`]'s two fields. A store
/// that refuses is **not** an error: the key stays in force for this run and the
/// configuration reads [`ApiKeySource::Session`], which is exactly the behavior
/// that shipped before #176. The convenience is what degrades, never chat.
///
/// What clearing reaches is B2's own key. A `B2_LLM_API_KEY` in the environment
/// is the user's own configuration, still outranks this one, and Settings never
/// had the standing to unset it — the copy beside the button says so.
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
        // The store refused to let go. Keep the key precisely as it stood, so
        // what the panel reports and what the next launch will find stay the
        // same fact.
        "" => (prev.api_key.clone(), prev.key_remembered),
        key => (Some(key.to_string()), keys.save(key)),
    }
}

/// Remember the endpoint and model. **Best-effort host state**, like the last
/// opened vault: a write failure is logged and swallowed, never failing the
/// setting the user just made — the change is already live in memory either way.
pub fn persist_prefs(prefs: &ChatPrefs) {
    let Some(file) = prefs_file() else {
        eprintln!("[b2] could not remember chat settings: no platform data directory");
        return;
    };
    if let Err(e) = write_prefs_to(&file, prefs) {
        eprintln!("[b2] could not remember chat settings: {e}");
    }
}

/// [`persist_prefs`] against an explicit path — the testable core. Creates the
/// parent dir if needed. The API key is `#[serde(skip)]`, so what lands on disk
/// is the endpoint and the model and nothing else.
pub fn write_prefs_to(file: &Path, prefs: &ChatPrefs) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(prefs)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(file, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::MemoryStore;

    #[test]
    fn prefs_round_trip_without_the_key_in_the_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        // The file sits under a not-yet-created subdir — the write must `mkdir -p`.
        let file = tmp.path().join("state/b2/chat.json");
        let prefs = ChatPrefs {
            base_url: Some("http://localhost:1234/v1".into()),
            model: Some("qwen2.5".into()),
            api_key: Some("sk-live-must-not-persist".into()),
            key_remembered: true,
        };
        write_prefs_to(&file, &prefs).unwrap();

        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert!(
            !on_disk.contains("sk-live-must-not-persist"),
            "a bearer token must never reach the settings file: {on_disk}"
        );
        // Read back against an *empty* store: the endpoint and model come out of
        // the file, and the key does not, because the file never held it.
        let back = read_prefs_from(Some(&file), &MemoryStore::empty());
        assert_eq!(back.base_url.as_deref(), Some("http://localhost:1234/v1"));
        assert_eq!(back.model.as_deref(), Some("qwen2.5"));
        assert_eq!(
            back.api_key, None,
            "the settings file is structurally incapable of carrying a key"
        );
    }

    /// The other half of #176: the key *does* come back, from the store rather
    /// than the file — and it comes back marked as remembered, which is what the
    /// Settings copy reads.
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

    /// The two stores are independent: no settings file (a platform with no data
    /// dir, or a first run) still finds a remembered key, and a broken file does
    /// not take the key down with it.
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

    /// The field's three states, against the store they have to keep in step.
    #[test]
    fn applying_the_key_field_keeps_the_store_in_step() {
        let store = MemoryStore::empty();
        let none = ChatPrefs::default();

        // Set: in force, and remembered for next launch.
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

        // Untouched: nothing moves, in memory or in the store.
        let (key, remembered) = apply_key(&set, None, &store);
        assert_eq!(key.as_deref(), Some("sk-typed"));
        assert!(remembered);
        assert_eq!(store.peek().as_deref(), Some("sk-typed"));

        // Blank — the Remove button. Gone from *both*, or it would come back at
        // the next launch and make the button a lie.
        let (key, remembered) = apply_key(&set, Some("   "), &store);
        assert_eq!(key, None);
        assert!(!remembered);
        assert_eq!(store.peek(), None);
    }

    /// The failure this module most owes a test: a store that **won't let go**.
    ///
    /// Dropping the key from memory while the Keychain keeps it would show a
    /// keyless configuration and then resurrect the credential at the next
    /// launch. So removal is all-or-nothing — and the proof is the second half,
    /// where a fresh `read_prefs_from` over the same store finds exactly what the
    /// panel was still claiming.
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

        // The whole point, stated as the next launch: what the panel reports and
        // what the store will hand back are the same fact.
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

    /// The other side of it: a *session-only* key sits in no store, so removing
    /// it can't be refused — `clear` on an empty store is already the asked-for
    /// state (the real Keychain's `errSecItemNotFound`).
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

    /// A Keychain that says no must cost the convenience and nothing else: the
    /// key the user just typed is still the key in force, and the configuration
    /// is honest that it lasts only for this run (GH #176's fallback path).
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
        // And it is the key the next question is asked with. Stated as the
        // layering rather than as a literal, so it holds however the developer's
        // own environment happens to be set (`from_env` reads the process env).
        assert_eq!(
            prefs.config(),
            LlmConfig::from_env().with_api_key(Some("sk-typed"), ApiKeySource::Session)
        );
    }

    /// The disk guarantee above has a sibling the derive would have broken:
    /// `Debug` is how a secret reaches a *log*, and this host's log can be a file
    /// (`B2_LOG_FILE`). `LlmConfig` hand-writes its own impl for exactly this, and
    /// the type holding the session key owes the same.
    #[test]
    fn debug_never_prints_the_api_key() {
        let prefs = ChatPrefs {
            base_url: Some("https://api.example.com/v1".into()),
            model: Some("some-model".into()),
            api_key: Some("sk-live-do-not-log-me".into()),
            key_remembered: true,
        };
        let rendered = format!("{prefs:?}");
        assert!(
            !rendered.contains("sk-live-do-not-log-me"),
            "a bearer token must never reach a log: {rendered}"
        );
        assert!(rendered.contains("redacted"), "{rendered}");
        // Presence still shows — "is a key configured at all" is the question a
        // rejected cloud call actually needs answered — and now which key, which
        // is the follow-up when the answer is "yes, and it's being rejected".
        assert!(rendered.contains("api.example.com"), "{rendered}");
        assert!(rendered.contains("Stored"), "{rendered}");
        assert!(
            format!("{:?}", ChatPrefs::default()).contains("api_key: \"None\""),
            "a keyless configuration says so plainly"
        );
    }

    /// A file naming `api_key` must not be able to install one: the field is
    /// skipped in *both* directions, so a hand-edited (or synced) settings file
    /// can't quietly become a credential store. #176 moved where a key *does*
    /// live; it did not open this door.
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

    /// Empty preferences resolve to exactly what the CLI resolves with no flags —
    /// which is the point of laying them over `LlmConfig::from_env` rather than
    /// spelling the defaults here.
    #[test]
    fn empty_prefs_resolve_to_the_shared_default() {
        // `from_env` reads the process environment; this asserts the *layering*,
        // which holds however the environment happens to be set.
        assert_eq!(ChatPrefs::default().config(), LlmConfig::from_env());
        let pointed = ChatPrefs {
            base_url: Some("http://localhost:1234/v1".into()),
            ..ChatPrefs::default()
        };
        assert_eq!(pointed.config().base_url, "http://localhost:1234/v1");
        assert_eq!(pointed.config().model, LlmConfig::from_env().model);
    }
}
