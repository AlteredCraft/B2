//! Chat wiring — the desktop's half of the second AI seam (GH #151, cut as
//! GH #155), and the sibling of `main.rs`'s embedder wiring.
//!
//! What lives here is *which provider a chat command talks to*, resolved the way
//! the CLI resolves it and layered the same way the vault root is: **flag beats
//! environment beats default**, with the app's Settings standing in for the
//! flags. Resolution itself is `b2_llm::LlmConfig::from_env`'s — one place, so
//! the two adapters cannot drift (root CLAUDE.md).
//!
//! Two things are deliberate and worth reading before changing them:
//!
//! * **Chat config is adapter state, never vault or index state** (GH #151).
//!   Nothing here is recorded in the vault, in `meta`, or anywhere the index can
//!   see it — which is what makes "change models at any time" true by
//!   construction: a chat model swap costs no reindex (contrast M2). The
//!   endpoint and the model id persist beside the remembered vault, in the app's
//!   own data dir, exactly like `last-vault`.
//! * **The API key is never written down.** A **Cloud models** configuration
//!   (M5 — the only way note content leaves the machine) needs a bearer token,
//!   and this host holds one **for the session only**: it lives in memory, it is
//!   never serialized to disk, and it never crosses back to the webview (the
//!   status view carries `has_api_key`, not the key). `B2_LLM_API_KEY` is how a
//!   user makes one persist, which is the CLI's posture unchanged — a secret
//!   B2 stores is a secret B2 is responsible for, and a plaintext file in
//!   Application Support is not a place to take that responsibility.

use b2_core::llm::{FakeLlm, LlmProvider};
use b2_llm::{LlmConfig, OpenAiCompatProvider};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The desktop's chat preferences: the Settings section's two persisted fields,
/// plus the session-only key. `None` means "whatever the environment and the
/// defaults say" — so a user who has never opened the section is exactly the CLI
/// with no flags, and clearing a field returns to that rather than to `""`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPrefs {
    /// The OpenAI-compatible base URL the user typed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The chat model id the user typed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The bearer token for a cloud endpoint — **session only**.
    ///
    /// `skip` on both halves, not merely `skip_serializing_if`: this type is
    /// what [`write_prefs_to`] writes, so the field must be structurally
    /// incapable of reaching disk, and a file that somehow named it must not be
    /// able to put one *back*. The module header says why.
    #[serde(skip)]
    pub api_key: Option<String>,
}

impl ChatPrefs {
    /// The configuration these preferences resolve to: the shared env resolution
    /// with this host's explicit choices laid over it (the `B2_VAULT_PATH`
    /// convention — an explicit setting wins, the environment seeds it).
    pub fn config(&self) -> LlmConfig {
        LlmConfig::from_env()
            .with_overrides(self.base_url.as_deref(), self.model.as_deref())
            .with_api_key(self.api_key.as_deref())
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

/// The persisted preferences, or the defaults when there are none. Best-effort in
/// every direction: an unreadable or malformed file reads as "nothing configured"
/// rather than failing the launch, because the fallback (env + defaults) is a
/// working local configuration.
pub fn read_prefs() -> ChatPrefs {
    prefs_file()
        .map(|f| read_prefs_from(&f))
        .unwrap_or_default()
}

/// [`read_prefs`] against an explicit path — the testable core (a tempfile stands
/// in for the real state file, so tests never touch the user's data dir).
pub fn read_prefs_from(file: &Path) -> ChatPrefs {
    let Ok(text) = std::fs::read_to_string(file) else {
        return ChatPrefs::default();
    };
    match serde_json::from_str::<ChatPrefs>(&text) {
        Ok(prefs) => prefs,
        Err(e) => {
            eprintln!("[b2] ignoring unreadable chat settings: {e}");
            ChatPrefs::default()
        }
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

    #[test]
    fn prefs_round_trip_without_the_key() {
        let tmp = tempfile::TempDir::new().unwrap();
        // The file sits under a not-yet-created subdir — the write must `mkdir -p`.
        let file = tmp.path().join("state/b2/chat.json");
        let prefs = ChatPrefs {
            base_url: Some("http://localhost:1234/v1".into()),
            model: Some("qwen2.5".into()),
            api_key: Some("sk-live-must-not-persist".into()),
        };
        write_prefs_to(&file, &prefs).unwrap();

        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert!(
            !on_disk.contains("sk-live-must-not-persist"),
            "a bearer token must never reach disk: {on_disk}"
        );
        let back = read_prefs_from(&file);
        assert_eq!(back.base_url.as_deref(), Some("http://localhost:1234/v1"));
        assert_eq!(back.model.as_deref(), Some("qwen2.5"));
        assert_eq!(
            back.api_key, None,
            "the key is session-only, by construction"
        );
    }

    /// A file naming `api_key` must not be able to install one: the field is
    /// skipped in *both* directions, so a hand-edited (or synced) settings file
    /// can't quietly become a credential store.
    #[test]
    fn a_hand_written_key_in_the_file_is_ignored() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("chat.json");
        std::fs::write(
            &file,
            r#"{"base_url":"http://x/v1","api_key":"sk-smuggled"}"#,
        )
        .unwrap();
        let prefs = read_prefs_from(&file);
        assert_eq!(prefs.base_url.as_deref(), Some("http://x/v1"));
        assert_eq!(prefs.api_key, None);
    }

    #[test]
    fn a_missing_or_broken_file_reads_as_nothing_configured() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(
            read_prefs_from(&tmp.path().join("absent")),
            ChatPrefs::default()
        );
        let broken = tmp.path().join("broken.json");
        std::fs::write(&broken, "{not json").unwrap();
        assert_eq!(read_prefs_from(&broken), ChatPrefs::default());
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
            model: None,
            api_key: None,
        };
        assert_eq!(pointed.config().base_url, "http://localhost:1234/v1");
        assert_eq!(pointed.config().model, LlmConfig::from_env().model);
    }
}
