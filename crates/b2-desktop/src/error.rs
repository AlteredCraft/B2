//! The host's error type + the generic, actionable, no-internals-leaked mapping to a
//! user-facing string — the desktop mirror of the CLI's `user_message`
//! (specs/desktop-ui-mvp.md §3; the repo-wide logging policy in the parent CLAUDE.md).
//!
//! [`CmdError`] **serializes to that string**, so a `#[tauri::command]` returning
//! `Result<T, CmdError>` hands the webview a safe, actionable message and never a
//! sqlite/io/serde internal. `#[from]` supplies the `?` conversions from the two
//! crates the host drives; `B2_DEBUG` opts into the raw detail for the developer,
//! exactly as the CLI does.

use b2_embed::EmbedError;
use serde::{Serialize, Serializer};

/// The host's error, composing the crates it drives. Kept internal; it is only ever
/// surfaced to the webview through [`user_message`] (via its [`Serialize`] impl).
#[derive(Debug, thiserror::Error)]
pub enum CmdError {
    #[error(transparent)]
    Core(#[from] b2_core::Error),
    #[error(transparent)]
    Embed(#[from] EmbedError),
    /// A command ran with no vault configured (no launch arg, no `$B2_VAULT_PATH`) —
    /// refuse rather than guess a directory, and tell the user how to point B2 at one.
    #[error("no vault specified")]
    VaultRequired,
    /// A `reindex` was requested while one was already running (single-in-flight,
    /// async-indexing.md §4). The UI disables the button, so this is a belt-and-
    /// suspenders refusal that reaches the webview only in a race.
    #[error("a reindex is already running")]
    ReindexInFlight,
}

/// Translate an internal error into a generic, actionable, user-facing message —
/// never leaking sqlite/io/serde internals. Mirrors the CLI's `user_message` so the
/// two adapters speak the same language; `B2_DEBUG` also appends the raw detail.
pub fn user_message(err: &CmdError) -> String {
    let msg = match err {
        CmdError::Core(b2_core::Error::NoteNotFound(r)) => {
            format!("Note not found: '{r}'. Check the path or b2id, and reindex first.")
        }
        CmdError::Core(b2_core::Error::ModelMismatch { .. }) => {
            "This vault's index was built with a different embedding model. Reindex to rebuild it."
                .to_string()
        }
        CmdError::Embed(EmbedError::NotProvisioned { model, .. }) => format!(
            "Embedding model '{model}' is not installed. Run `b2 init` in a terminal to download it (or set B2_EMBEDDER=fake for an offline, non-semantic mode)."
        ),
        CmdError::Embed(EmbedError::Download(_)) => {
            "Could not download the embedding model. Check your network and run `b2 init` again."
                .to_string()
        }
        CmdError::Core(b2_core::Error::InvalidRelation(v)) => format!(
            "'{v}' isn't a known relation type. Use one of: references, relates, elaborates, supports, refutes, contradicts, example-of, part-of, supersedes, derived-from."
        ),
        CmdError::VaultRequired => {
            "No vault open. Launch B2 with a vault path, or set B2_VAULT_PATH to your vault folder."
                .to_string()
        }
        CmdError::ReindexInFlight => {
            "A reindex is already in progress. Please wait for it to finish.".to_string()
        }
        _ => "Something went wrong. Please check the vault and try again.".to_string(),
    };
    if std::env::var_os("B2_DEBUG").is_some() {
        let detail = match err {
            CmdError::Core(e) => e.to_string(),
            CmdError::Embed(e) => e.to_string(),
            CmdError::VaultRequired | CmdError::ReindexInFlight => err.to_string(),
        };
        format!("{msg}\n(debug: {detail})")
    } else {
        msg
    }
}

/// Serialize the error **as its user-facing message** — the whole point of the type:
/// the webview receives a generic, actionable string, never an internal.
impl Serialize for CmdError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&user_message(self))
    }
}
