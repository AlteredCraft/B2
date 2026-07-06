//! `b2-desktop` — the Tauri host, B2's **second dumb adapter** over the
//! [`Vault`](b2_core::vault::Vault) façade (the GUI sibling of `b2-cli`). It holds
//! **no engine logic**: each `#[tauri::command]` deserializes its args, calls one
//! façade method, and serializes the result (specs/desktop-ui-mvp.md §3). The rules
//! that keep it a *dumb* adapter live in this crate's charter, `CLAUDE.md`.
//!
//! Two things this file owns, both mirroring the CLI:
//!   * **Vault root resolution** — a launch arg or `$B2_VAULT_PATH` (the CLI's
//!     positional / `-C`), seeded once at startup into [`AppState`] and thereafter
//!     **swappable at runtime** by the in-app vault picker (`choose_vault`). Every
//!     command opens a *fresh* vault from the current root, exactly as the
//!     one-process-per-command CLI does.
//!   * **Embedder wiring** — pure reads open with the deterministic fake; anything
//!     that embeds a query or re-projects (`search` / `link` / `reindex`) opens the
//!     real [`LocalEmbedder`] and **fails fast** with "run `b2 init`" if it's absent.
//!     `B2_EMBEDDER=fake` forces the fake everywhere (offline/dev mode).

// This binary is desktop-only (no mobile entry point), so a plain `main` suffices.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;

use b2_core::embed::Embedder;
use b2_core::vault::Vault;
use b2_embed::{EmbedConfig, LocalEmbedder};
use error::CmdError;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The host's shared state: the active vault root. Resolved once at startup, then
/// **swappable at runtime** by the in-app vault picker (`choose_vault`) — so the root
/// sits behind a [`Mutex`]. Every command still opens its own short-lived [`Vault`]
/// over the *current* root (SQLite WAL permits concurrent readers + one writer), the
/// faithful mirror of the CLI opening a fresh vault per invocation. `None` means no
/// vault is configured; commands then return an actionable [`CmdError::VaultRequired`].
pub struct AppState {
    root: Mutex<Option<PathBuf>>,
}

impl AppState {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root: Mutex::new(root),
        }
    }

    /// The current vault root, cloned out so the lock is **not** held while a command
    /// opens a vault (which may load the model — slow). `None` when unconfigured.
    pub fn current_root(&self) -> Option<PathBuf> {
        self.lock_root().clone()
    }

    /// Point the app at a new vault root (the vault switcher). Takes effect for every
    /// subsequent command, since each opens a fresh vault over `current_root`.
    pub fn set_root(&self, root: &Path) {
        *self.lock_root() = Some(root.to_path_buf());
    }

    /// The critical sections here are a single clone or store — neither can panic —
    /// so the lock can never be poisoned; recover the inner value rather than unwrap
    /// (the no-panic rule) if a poison ever somehow occurs.
    fn lock_root(&self) -> std::sync::MutexGuard<'_, Option<PathBuf>> {
        self.root
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Whether the deterministic fake embedder is forced (`B2_EMBEDDER=fake`) — the CLI's
/// offline/dev switch, honored identically so the two adapters behave the same.
fn use_fake_embedder() -> bool {
    matches!(std::env::var("B2_EMBEDDER").ok().as_deref(), Some("fake"))
}

/// Open a fresh vault over the configured root with the right embedder — the desktop
/// mirror of the CLI's `open_vault`. `needs_semantic` commands (`search`/`link`/
/// `reindex`) load the real [`LocalEmbedder`] (fail-fast "run `b2 init`" if absent);
/// pure reads use the fake. Returns the vault and whether its embedder is semantic
/// (used only for honest UI). Errors with [`CmdError::VaultRequired`] if no vault is set.
pub fn open_vault(state: &AppState, needs_semantic: bool) -> Result<(Vault, bool), CmdError> {
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    if needs_semantic && !use_fake_embedder() {
        let config = EmbedConfig::load()?;
        let embedder = LocalEmbedder::load(&config)?;
        let vault = Vault::open_with_embedder(&root, Box::new(embedder) as Box<dyn Embedder>)?;
        Ok((vault, true))
    } else {
        Ok((Vault::open(&root)?, false))
    }
}

/// Whether the real (semantic) embedder is available right now — mirrors the CLI:
/// false under `B2_EMBEDDER=fake`, or if the model isn't provisioned yet. Used by
/// `vault_info` to tell the UI whether semantic ranking is live, so the app can be
/// honest (never overstate the fake), exactly as `b2 search` is.
pub fn semantic_available() -> bool {
    if use_fake_embedder() {
        return false;
    }
    match EmbedConfig::load() {
        Ok(config) => LocalEmbedder::load(&config).is_ok(),
        Err(_) => false,
    }
}

/// Resolve the vault root once at startup: the first launch argument wins (the CLI's
/// positional), then `$B2_VAULT_PATH` (the CLI's `-C` / env). A leading-`-` first arg
/// is ignored so a macOS `-psn_…` Finder argument is never mistaken for a path.
fn resolve_root() -> Option<PathBuf> {
    std::env::args()
        .nth(1)
        .filter(|a| !a.starts_with('-'))
        .or_else(|| std::env::var("B2_VAULT_PATH").ok())
        .map(PathBuf::from)
}

fn main() {
    let state = AppState::new(resolve_root());
    tauri::Builder::default()
        // The dialog plugin backs the native folder picker for `choose_vault`. It is
        // driven host-side only; the webview gets no dialog permission (capabilities/
        // default.json), so it can never open a dialog itself.
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::vault_info,
            commands::choose_vault,
            commands::read_note,
            commands::list_notes,
            commands::similar,
            commands::search,
            commands::neighbors,
            commands::explain,
            commands::link,
            commands::reindex,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the B2 desktop app");
}
