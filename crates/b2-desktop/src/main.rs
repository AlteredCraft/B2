//! `b2-desktop`: the Tauri host, a dumb adapter over the [`Vault`](b2_core::vault::Vault)
//! façade and the GUI sibling of `b2-cli` (ADR-0012). See this crate's `CLAUDE.md`.
//!
//! This file owns vault root resolution (launch arg, else the last opened vault, else
//! `$B2_VAULT_PATH`; swappable at runtime by the picker) and embedder wiring, both as in the
//! CLI. Every command opens a fresh vault from the current root.

// This binary is desktop-only (no mobile entry point), so a plain `main` suffices.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod chat;
mod commands;
mod error;
mod keychain;
mod logging;
mod menu;
mod slot;
mod state_file;
mod stats;
mod watch;

use b2_core::vault::Vault;
use b2_embed::EmbedConfig;
use chat::ChatPrefs;
use error::CmdError;
use slot::Slot;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use watch::VaultWatcher;

/// The state file remembering the last opened vault.
const LAST_VAULT_FILE: &str = "last-vault";

/// The host's shared state: the active vault root (`None` when unconfigured), the reindex
/// and ask [`Slot`]s, and the chat preferences.
pub struct AppState {
    root: Mutex<Option<PathBuf>>,
    /// The vector-writing embed pass (`project` runs outside it). Cancelled by
    /// `cancel_reindex` and by a vault switch; checked at each batch boundary.
    pub reindex: Slot,
    /// The streaming answer (`ask` and `why_similar` share it), cancelled by `cancel_ask`.
    pub ask: Slot,
    /// The chat endpoint, model and key in force; never vault or index state (GH #151).
    chat: Mutex<ChatPrefs>,
    /// Held for a whole Settings save. A save spans the Keychain, `chat` and `chat.json`,
    /// and `chat` is unlocked between them (a Keychain prompt can block), so without this
    /// two saves could interleave.
    chat_saving: Mutex<()>,
}

impl AppState {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self::with_chat(root, ChatPrefs::default())
    }

    /// [`new`](Self::new) with explicit chat preferences.
    pub fn with_chat(root: Option<PathBuf>, chat: ChatPrefs) -> Self {
        Self {
            root: Mutex::new(root),
            reindex: Slot::default(),
            ask: Slot::default(),
            chat: Mutex::new(chat),
            chat_saving: Mutex::new(()),
        }
    }

    /// Claim the Settings save ([`chat_saving`](Self::chat_saving)). Blocks rather than
    /// refusing: a second Save should follow the first, not be dropped.
    pub fn begin_chat_save(&self) -> std::sync::MutexGuard<'_, ()> {
        lock_recover(&self.chat_saving)
    }

    /// The chat preferences in force, cloned so no lock is held over network I/O.
    pub fn chat_prefs(&self) -> ChatPrefs {
        lock_recover(&self.chat).clone()
    }

    /// Replace the chat preferences; every later ask resolves a fresh provider.
    pub fn set_chat_prefs(&self, prefs: ChatPrefs) {
        *lock_recover(&self.chat) = prefs;
    }

    /// The current vault root, cloned so no lock is held while a vault (and model) opens.
    pub fn current_root(&self) -> Option<PathBuf> {
        self.lock_root().clone()
    }

    /// Point the app at a new vault root (the vault switcher).
    pub fn set_root(&self, root: &Path) {
        *self.lock_root() = Some(root.to_path_buf());
    }

    /// See [`lock_recover`].
    fn lock_root(&self) -> std::sync::MutexGuard<'_, Option<PathBuf>> {
        lock_recover(&self.root)
    }
}

/// Lock a mutex, recovering the value if it is ever poisoned (the no-panic rule). Every
/// critical section here is a single clone, store or drop, so poisoning shouldn't happen.
pub(crate) fn lock_recover<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Open a fresh vault over the configured root with the right embedder, as the CLI's
/// `open_vault` does (`b2_embed::embedder_for`). Also returns the loaded model's id, which
/// `embed` attributes its time to.
pub fn open_vault(
    state: &AppState,
    needs_semantic: bool,
) -> Result<(Vault, Option<String>), CmdError> {
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    open_vault_at(&root, needs_semantic)
}

/// [`open_vault`] over an explicit root, so a command that also needs the root can't see
/// two sides of a vault switch.
pub fn open_vault_at(
    root: &Path,
    needs_semantic: bool,
) -> Result<(Vault, Option<String>), CmdError> {
    Ok(match b2_embed::embedder_for(needs_semantic)? {
        Some(embedder) => {
            let model = embedder.configured_model().to_string();
            (
                Vault::open_with_embedder(root, Box::new(embedder))?,
                Some(model),
            )
        }
        None => (Vault::open(root)?, None),
    })
}

/// A fresh vault over the fake embedder, for commands that never embed.
pub fn open_read(state: &AppState) -> Result<Vault, CmdError> {
    Ok(open_vault(state, false)?.0)
}

/// A fresh vault over the real embedder, for commands that embed.
pub fn open_semantic(state: &AppState) -> Result<Vault, CmdError> {
    Ok(open_vault(state, true)?.0)
}

/// Whether the real embedder is available (not under `B2_EMBEDDER=fake`, and provisioned),
/// so the UI never overstates the fake. A file probe, never a load (#133), since
/// `vault_info` calls it on every first paint; a corrupt model fails later, at `search`.
pub fn semantic_available() -> bool {
    if b2_embed::fake_requested() {
        return false;
    }
    EmbedConfig::load().is_ok_and(|c| c.is_model_provisioned(&c.model))
}

/// Resolve the vault root once at startup: launch arg, then the last picked vault, then
/// `$B2_VAULT_PATH`. The picker beats the env var because on a GUI it is how you choose. A
/// leading-`-` arg is ignored, so macOS's `-psn_…` is never taken for a path.
fn resolve_root() -> Option<PathBuf> {
    let arg = std::env::args()
        .nth(1)
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from);
    let env = std::env::var("B2_VAULT_PATH").ok().map(PathBuf::from);
    pick_root(arg, read_last_vault(), env)
}

/// The precedence rule behind [`resolve_root`].
fn pick_root(
    arg: Option<PathBuf>,
    remembered: Option<PathBuf>,
    env: Option<PathBuf>,
) -> Option<PathBuf> {
    arg.or(remembered).or(env)
}

/// The remembered vault root, or `None` if there is none or the directory no longer
/// exists, so startup falls back to the env default.
fn read_last_vault() -> Option<PathBuf> {
    read_last_vault_from(&state_file::path(LAST_VAULT_FILE)?)
}

/// [`read_last_vault`] against an explicit path. Strips only trailing newlines.
fn read_last_vault_from(file: &Path) -> Option<PathBuf> {
    let contents = std::fs::read_to_string(file).ok()?;
    let path = PathBuf::from(contents.trim_end_matches(['\n', '\r']));
    path.is_dir().then_some(path)
}

/// Remember `root` as the last opened vault. Best-effort; called only from `choose_vault`,
/// so tests never touch the real data dir.
fn persist_last_vault(root: &Path) {
    state_file::update(LAST_VAULT_FILE, "remember the last vault", |file| {
        persist_last_vault_to(file, root)
    });
}

/// [`persist_last_vault`] against an explicit path.
fn persist_last_vault_to(file: &Path, root: &Path) -> std::io::Result<()> {
    state_file::write(file, root.to_string_lossy().as_bytes())
}

fn main() {
    // Held for the whole run: dropping it flushes the log writer.
    let _guard = logging::init_logging();
    let state = AppState::with_chat(resolve_root(), chat::read_prefs(&keychain::Keychain));
    let app = tauri::Builder::default()
        // Declared, not `Menu::default()`, so its chords are enumerable (#119).
        .menu(menu::build)
        // B2's own items: forward the id; `ui/src/zoom.ts` decides what it means.
        .on_menu_event(|app, event| {
            let _ = app.emit(menu::MENU_COMMAND_EVENT, event.id().0.as_str());
        })
        // The dialog, opener and clipboard plugins are driven host-side only; the webview
        // gets none of their permissions (capabilities/default.json).
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(state)
        // Filesystem auto-reload (#14), re-pointed on a vault switch.
        .manage(VaultWatcher::default())
        .setup(|app| {
            // Watch the startup vault, if any; best-effort.
            if let Some(root) = app.state::<AppState>().current_root() {
                app.state::<VaultWatcher>().watch(app.handle(), &root);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault_info,
            commands::choose_vault,
            commands::read_note,
            commands::list_notes,
            commands::list_dirs,
            commands::list_resources,
            commands::explain_resource,
            commands::read_resource,
            commands::open_resource,
            commands::open_external,
            commands::clipboard_text,
            commands::write_note,
            commands::write_frontmatter,
            commands::create_note,
            commands::create_dir,
            commands::import_file,
            commands::import_path,
            commands::pick_import_files,
            commands::move_note,
            commands::move_resource,
            commands::move_dir,
            commands::delete_note,
            commands::delete_resource,
            commands::delete_dir,
            commands::similar,
            commands::explain_similar,
            commands::search,
            commands::explain,
            commands::link,
            commands::project,
            commands::embed,
            commands::cancel_reindex,
            commands::list_models,
            commands::set_model,
            commands::provision_model,
            commands::models_dir,
            commands::embed_device,
            commands::embed_stats,
            commands::menu_chords,
            commands::set_zoom,
            commands::ask,
            commands::why_similar,
            commands::cancel_ask,
            commands::chat_setup,
            commands::set_chat_config,
        ])
        .run(tauri::generate_context!());
    // The app itself didn't start: say so and exit non-zero, without panicking.
    if let Err(e) = app {
        eprintln!("[b2] the desktop app could not start: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    //! Vault-resolution tests, hermetic: the file helpers run against a tempdir.

    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> Option<PathBuf> {
        Some(PathBuf::from(s))
    }

    #[test]
    fn pick_root_precedence_arg_then_remembered_then_env() {
        // (arg, remembered, env) → the chosen root.
        let cases = [
            (p("/arg"), p("/mem"), p("/env"), p("/arg")),
            (None, p("/mem"), p("/env"), p("/mem")),
            (None, None, p("/env"), p("/env")),
            (p("/arg"), None, None, p("/arg")),
            (p("/arg"), None, p("/env"), p("/arg")),
            (None, p("/mem"), None, p("/mem")),
            (None, None, None, None),
        ];
        for (arg, remembered, env, want) in cases {
            assert_eq!(
                pick_root(arg.clone(), remembered.clone(), env.clone()),
                want,
                "pick_root({arg:?}, {remembered:?}, {env:?})"
            );
        }
    }

    #[test]
    fn persist_then_read_round_trips_an_existing_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join("my vault"); // a space proves we don't trim it
        std::fs::create_dir_all(&vault).unwrap();
        // The parent dir doesn't exist yet.
        let file = tmp.path().join("state/b2/last-vault");

        persist_last_vault_to(&file, &vault).unwrap();
        assert_eq!(read_last_vault_from(&file), Some(vault));
    }

    #[test]
    fn read_last_vault_ignores_a_stale_or_missing_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("last-vault");
        std::fs::write(&file, tmp.path().join("gone").to_string_lossy().as_bytes()).unwrap();
        assert_eq!(read_last_vault_from(&file), None);
    }

    #[test]
    fn read_last_vault_ignores_missing_or_empty_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(read_last_vault_from(&tmp.path().join("absent")), None);
        let empty = tmp.path().join("empty");
        std::fs::write(&empty, "\n").unwrap();
        assert_eq!(read_last_vault_from(&empty), None);
    }

    #[test]
    fn read_last_vault_strips_only_trailing_newlines() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let file = tmp.path().join("last-vault");
        // `persist` never writes one, but a hand-edit might.
        std::fs::write(&file, format!("{}\n", vault.display())).unwrap();
        assert_eq!(read_last_vault_from(&file), Some(vault));
    }
}
