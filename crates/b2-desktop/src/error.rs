//! The host's error type and its generic, actionable user-facing message, the desktop
//! mirror of the CLI's `user_message`. [`CmdError`] serializes to that message, so the
//! webview never sees an internal. `B2_DEBUG` appends the raw detail.

use b2_embed::{EmbedConfig, EmbedError};
use serde::{Serialize, Serializer};

/// The host's error, composing the crates it drives. Reaches the webview only through
/// [`user_message`].
#[derive(Debug, thiserror::Error)]
pub enum CmdError {
    #[error(transparent)]
    Core(#[from] b2_core::Error),
    #[error(transparent)]
    Embed(#[from] EmbedError),
    /// No vault configured (no launch arg, remembered pick or `$B2_VAULT_PATH`).
    #[error("no vault specified")]
    VaultRequired,
    /// A `reindex` while one is running; the UI prevents it, so only in a race.
    #[error("a reindex is already running")]
    ReindexInFlight,
    /// An `ask` while an answer is streaming (GH #155); only in a race.
    #[error("an answer is already streaming")]
    AskInFlight,
    /// The OS refused to open a resource in its default app (`open_resource`).
    #[error("open in system default failed: {0}")]
    OpenFailed(String),
    /// A note link that isn't `http`/`https`/`mailto` (`open_external`). Notes are
    /// untrusted (E5), and an arbitrary scheme launches whatever app claims it. The URL is
    /// for the log only.
    #[error("refused to open a non-web link: {0}")]
    UnsupportedLink(String),
    /// The OS refused the clipboard read behind ⌘⇧V (`clipboard_text`).
    #[error("clipboard read failed: {0}")]
    ClipboardFailed(String),
    /// The webview refused a page-zoom change (`set_zoom`); not expected in practice.
    #[error("could not change the window size: {0}")]
    ZoomFailed(String),
    /// `import_file` got a payload that isn't base64: a transport fault, not the user's.
    #[error("import payload was not valid base64: {0}")]
    ImportPayload(String),
}

/// A generic, actionable message for `err`, mirroring the CLI's `user_message`.
/// `B2_DEBUG` appends the raw detail.
pub fn user_message(err: &CmdError) -> String {
    let msg = match err {
        CmdError::Core(b2_core::Error::NoteNotFound(r)) => {
            format!("Note not found: '{r}'. Check the path, and reindex first.")
        }
        CmdError::Core(b2_core::Error::IndexTooNew { .. }) => {
            "This vault's index was built by a newer version of B2 than this app. Open the newer B2 (or update this one) — the index was left untouched, so nothing needs rebuilding."
                .to_string()
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
        // The fix is in `config.toml`, so name it.
        CmdError::Embed(EmbedError::Config(_)) => format!(
            "B2 couldn't use its embedder settings. Check the [embedder] table in {}, then try again.",
            EmbedConfig::config_path()
                .map_or_else(|| "config.toml".to_string(), |p| p.display().to_string())
        ),
        CmdError::Core(b2_core::Error::InvalidRelation(v)) => format!(
            "'{v}' isn't a known relation type. Use one of: {}.",
            b2_core::relation::CORE
                .iter()
                .map(|c| c.verb)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CmdError::Core(b2_core::Error::MoveTargetExists(p)) => format!(
            "Can't move: something already exists at '{p}'. Choose a different name or destination."
        ),
        CmdError::Core(b2_core::Error::MoveDestination(_)) => {
            "That destination isn't valid. Give a vault-relative name like `notes/new-name`."
                .to_string()
        }
        CmdError::Core(b2_core::Error::MoveIncomplete { paths, .. }) => format!(
            "The move failed, and B2 could not undo its link changes in: {}. Check the links in those files, then reindex.",
            paths.join(", ")
        ),
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
        // The import family says "file", never "note": what arrived may be a PDF.
        CmdError::Core(b2_core::Error::ImportTargetExists(p)) => format!(
            "A file already exists at '{p}'. Rename it, or drop this one into a different folder."
        ),
        CmdError::Core(b2_core::Error::ImportDestination(_)) => {
            "That file can't be imported under its own name. Rename it and try again."
                .to_string()
        }
        CmdError::ImportPayload(_) => {
            "That file couldn't be read for import. Try again, or copy it into the vault folder in Finder."
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
        CmdError::Core(b2_core::Error::Frontmatter(d)) => {
            // The detail is a domain message, never an internal.
            format!("Can't save this frontmatter: {d}.")
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
        CmdError::UnsupportedLink(_) => {
            "B2 only opens web links — http, https, and mailto — outside the app. This link is none of those, so it wasn't opened."
                .to_string()
        }
        CmdError::ClipboardFailed(_) => {
            "Couldn't read the clipboard. Copy the text again, then paste.".to_string()
        }
        CmdError::ZoomFailed(_) => {
            "Couldn't change the text size. Try again, or press ⌘0 to go back to the default."
                .to_string()
        }
        CmdError::VaultRequired => {
            "No vault open. Launch B2 with a vault path, or set B2_VAULT_PATH to your vault folder."
                .to_string()
        }
        CmdError::ReindexInFlight => {
            "A reindex is already in progress. Please wait for it to finish.".to_string()
        }
        CmdError::AskInFlight => {
            "B2 is still answering. Wait for it to finish, or press Esc to stop it.".to_string()
        }
        // Not the generic chat failure below: the server and model are fine, and the cap
        // is the only fix on this side of the wire (GH #154/#155).
        CmdError::Core(b2_core::Error::ToolCallLimit { limit }) => format!(
            "The chat model asked for more than {limit} tool calls in one reply, so B2 stopped it. Try again or pick another model in Settings → Chat. If this model really needs more, relaunch with {} set higher.",
            b2_llm::ENV_MAX_TOOL_CALLS
        ),
        CmdError::Core(b2_core::Error::Llm(_)) => {
            "The model server couldn't answer. Check that it's running and that the model is installed, then try again."
                .to_string()
        }
        // Internals the webview must never see. Not `_`, so a new CmdError variant fails
        // to compile here.
        CmdError::Core(_) | CmdError::Embed(EmbedError::Io(_)) => {
            "Something went wrong. Please check the vault and try again.".to_string()
        }
    };
    if std::env::var_os("B2_DEBUG").is_some() {
        let detail = err.to_string();
        format!("{msg}\n(debug: {detail})")
    } else {
        msg
    }
}

/// Log an error's full internal detail to stderr, so a failure is diagnosable without
/// `B2_DEBUG`. Called from `Serialize`, the one place every command error crosses.
fn log_internal(err: &CmdError) {
    // `Core`/`Embed` are transparent, so this displays the source for every variant.
    let detail = err.to_string();
    eprintln!("[b2] command failed: {detail}");
}

/// Serialize as the user-facing message, logging the full detail first.
impl Serialize for CmdError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        log_internal(self);
        serializer.serialize_str(&user_message(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blown_tool_call_cap_names_the_limit_and_the_setting_that_raises_it() {
        let msg = user_message(&CmdError::Core(b2_core::Error::ToolCallLimit { limit: 64 }));
        assert!(msg.contains("64"), "{msg}");
        assert!(msg.contains("B2_LLM_MAX_TOOL_CALLS"), "{msg}");
        assert!(
            !msg.contains("Check that it's running"),
            "the server is fine — the generic chat advice would mislead: {msg}"
        );
    }

    #[test]
    fn serializes_to_the_generic_message_and_hides_internals() {
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

    /// The verb list is `b2_core::relation::CORE`'s, so a verb added there is offered here.
    #[test]
    fn an_invalid_relation_lists_every_core_verb() {
        let msg = user_message(&CmdError::Core(b2_core::Error::InvalidRelation(
            "refutes".into(),
        )));
        for verb in b2_core::relation::CORE {
            assert!(msg.contains(verb.verb), "{msg}");
        }
    }

    #[test]
    fn a_broken_embedder_config_names_the_settings_not_the_vault() {
        let msg = user_message(&CmdError::Embed(EmbedError::Config(
            "config.toml: expected `=`".into(),
        )));
        assert!(msg.contains("[embedder]"), "{msg}");
        assert!(
            !msg.contains("check the vault") && !msg.contains("expected"),
            "{msg}"
        );
    }

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

        let incomplete = CmdError::Core(b2_core::Error::MoveIncomplete {
            paths: vec!["b.md".into(), "notes/c.md".into()],
            source: Box::new(b2_core::Error::Io(std::io::Error::other("disk full"))),
        });
        let msg = user_message(&incomplete);
        assert!(msg.contains("in: b.md, notes/c.md."));
        assert!(!msg.contains("disk full"), "the cause stays internal");
        assert!(
            incomplete.to_string().contains("disk full"),
            "the internal log (`log_internal`) keeps the cause"
        );
    }
}
