//! `b2-desktop` — the Tauri host, B2's **second dumb adapter** over the
//! [`Vault`](b2_core::vault::Vault) façade (the GUI sibling of `b2-cli`). It holds
//! **no engine logic**: each `#[tauri::command]` deserializes its args, calls one
//! façade method, and serializes the result (specs/desktop-ui-mvp.md §3). The rules
//! that keep it a *dumb* adapter live in this crate's charter, `CLAUDE.md`.
//!
//! Two things this file owns, both mirroring the CLI:
//!   * **Vault root resolution** — a launch arg or `$B2_VAULT_PATH` (the CLI's
//!     positional / `-C`). Resolved once at startup into [`AppState`]; every command
//!     opens a *fresh* vault from it, exactly as the one-process-per-command CLI does.
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
use std::path::PathBuf;

/// The host's shared state: the vault root, resolved once at startup. It is
/// immutable, so no lock is needed — every command opens its own short-lived
/// [`Vault`] over this root (SQLite WAL permits concurrent readers + one writer),
/// the faithful mirror of the CLI opening a fresh vault per invocation. `None` means
/// no vault was configured; commands then return an actionable [`CmdError::VaultRequired`].
pub struct AppState {
    pub root: Option<PathBuf>,
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
    let root = state.root.as_deref().ok_or(CmdError::VaultRequired)?;
    if needs_semantic && !use_fake_embedder() {
        let config = EmbedConfig::load()?;
        let embedder = LocalEmbedder::load(&config)?;
        let vault = Vault::open_with_embedder(root, Box::new(embedder) as Box<dyn Embedder>)?;
        Ok((vault, true))
    } else {
        Ok((Vault::open(root)?, false))
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
    let state = AppState {
        root: resolve_root(),
    };
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::vault_info,
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
