//! The host's error type + the generic, actionable, no-internals-leaked mapping to a
//! user-facing string — the desktop mirror of the CLI's `user_message`
//! (crates/b2-desktop/CLAUDE.md; the repo-wide logging policy in the parent CLAUDE.md).
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
    /// A command ran with no vault configured (no launch arg, no remembered pick, no
    /// `$B2_VAULT_PATH`) — refuse rather than guess a directory, and tell the user how to
    /// point B2 at one.
    #[error("no vault specified")]
    VaultRequired,
    /// A `reindex` was requested while one was already running (single-in-flight).
    /// The UI disables the button, so this is a belt-and-
    /// suspenders refusal that reaches the webview only in a race.
    #[error("a reindex is already running")]
    ReindexInFlight,
    /// The OS refused to open a resource in its default app (`open_resource`, the
    /// fallback card's one action). The message is the opener plugin's detail —
    /// logged in full server-side, generic to the webview like everything else.
    #[error("open in system default failed: {0}")]
    OpenFailed(String),
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
        CmdError::Embed(EmbedError::UnknownModel(_)) => {
            "That isn't a model B2 offers. Pick one from the list in Settings.".to_string()
        }
        CmdError::Embed(EmbedError::Load(_)) => {
            "The embedding model failed to load. Try downloading it again, or pick a different model in Settings."
                .to_string()
        }
        CmdError::Core(b2_core::Error::InvalidRelation(v)) => format!(
            "'{v}' isn't a known relation type. Use one of: references, supports, contradicts."
        ),
        CmdError::Core(b2_core::Error::MoveTargetExists(p)) => format!(
            "Can't move: something already exists at '{p}'. Choose a different name or destination."
        ),
        CmdError::Core(b2_core::Error::MoveDestination(_)) => {
            "That destination isn't valid. Give a vault-relative name like `notes/new-name`."
                .to_string()
        }
        CmdError::Core(b2_core::Error::DirNotFound(p)) => {
            format!("Folder not found: '{p}'. It may have been moved — reindex and try again.")
        }
        CmdError::Core(b2_core::Error::AddTargetExists(p)) => format!(
            "A note already exists at '{p}'. Choose a different name, or open that note."
        ),
        CmdError::Core(b2_core::Error::AddDestination(_)) => {
            "That note name isn't valid. Give a vault-relative name like `notes/new-idea`."
                .to_string()
        }
        CmdError::Core(b2_core::Error::DirTargetExists(p)) => format!(
            "Something already exists at '{p}'. Choose a different folder name."
        ),
        CmdError::Core(b2_core::Error::DirDestination(_)) => {
            "That folder name isn't valid. Give a vault-relative name like `projects/2026`."
                .to_string()
        }
        CmdError::Core(b2_core::Error::WriteConflict(_)) => {
            "This note changed on disk since it was opened. Reload the note, then reapply your edit."
                .to_string()
        }
        CmdError::Core(b2_core::Error::ResourceNotFound(r)) => {
            format!("File not found in the vault: '{r}'. Check the path, and reindex first.")
        }
        CmdError::Core(b2_core::Error::ResourceUnsupported(_)) => {
            "Similarity for non-Markdown files isn't available yet — it arrives in a later release."
                .to_string()
        }
        CmdError::OpenFailed(_) => {
            "Couldn't open the file in its default app. Try opening it from your file manager."
                .to_string()
        }
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
            CmdError::VaultRequired | CmdError::ReindexInFlight | CmdError::OpenFailed(_) => {
                err.to_string()
            }
        };
        format!("{msg}\n(debug: {detail})")
    } else {
        msg
    }
}

/// Log an error's **full internal detail** to stderr — the sqlite/io/serde specifics
/// the webview must never see (repo logging policy: the server log carries everything,
/// the client only the generic message). The desktop mirror of the CLI writing detail
/// to stderr; under `tauri dev` it lands in the terminal running the host, so a failing
/// command is diagnosable without a rebuild or `B2_DEBUG`. Called from the one place
/// every command error crosses to the webview — [`CmdError`]'s `Serialize` impl — so
/// logging stays uniform and out of the dumb command handlers.
fn log_internal(err: &CmdError) {
    let detail = match err {
        CmdError::Core(e) => e.to_string(),
        CmdError::Embed(e) => e.to_string(),
        CmdError::VaultRequired | CmdError::ReindexInFlight | CmdError::OpenFailed(_) => {
            err.to_string()
        }
    };
    eprintln!("[b2] command failed: {detail}");
}

/// Serialize the error **as its user-facing message** — the whole point of the type:
/// the webview receives a generic, actionable string, never an internal. This is also
/// the single boundary where an error reaches the client, so it is where the full
/// detail is logged server-side ([`log_internal`]) before the generic string goes out.
impl Serialize for CmdError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        log_internal(self);
        serializer.serialize_str(&user_message(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializing an error — the one path by which a command failure reaches the
    /// webview — yields the **generic** user-facing string, never the internal detail.
    /// (It also runs `log_internal`, which writes the full detail to stderr; the
    /// harness captures that, so this exercises the server-log boundary too.)
    #[test]
    fn serializes_to_the_generic_message_and_hides_internals() {
        // An unmapped Core error is the exact "Something went wrong" case the reindex
        // bug hit — the client sees the generic message, not the io/utf-8 detail.
        let err = CmdError::Core(b2_core::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )));
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            json,
            "\"Something went wrong. Please check the vault and try again.\""
        );
        assert!(!json.to_lowercase().contains("utf-8"), "no internal leaks");
    }

    /// The move family maps to specific, actionable messages (not the catch-all),
    /// mirroring the CLI's `user_message` arms.
    #[test]
    fn move_errors_map_to_actionable_messages() {
        let exists = CmdError::Core(b2_core::Error::MoveTargetExists("a/b.md".into()));
        assert!(user_message(&exists).contains("already exists at 'a/b.md'"));

        let dest = CmdError::Core(b2_core::Error::MoveDestination("../up".into()));
        let msg = user_message(&dest);
        assert!(msg.contains("isn't valid"));
        assert!(
            !msg.contains("../up"),
            "the raw destination detail stays server-side"
        );

        let dir = CmdError::Core(b2_core::Error::DirNotFound("old-folder".into()));
        assert!(user_message(&dir).contains("Folder not found: 'old-folder'"));
    }
}
