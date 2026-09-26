//! The CLI's error type and its one translation into a user-facing sentence.

use b2_embed::EmbedError;
use b2_llm::{is_ollama, refusal_message, LlmError, OLLAMA_INSTALL_URL};

/// The CLI's error, composing the two crates it drives. Kept internal; `user_message`
/// turns it into a generic, actionable, no-internals-leaked line (logging policy).
/// `#[from]` supplies the `?` conversions; `transparent` defers `Display` to the
/// inner error (only ever surfaced under `B2_DEBUG`).
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] b2_core::Error),
    #[error(transparent)]
    Embed(#[from] EmbedError),
    /// A setup-time chat failure the adapter can act on — an unreachable model
    /// server, an un-pulled model. Call-time failures arrive as
    /// [`b2_core::Error::Llm`] instead (the seam collapses them to a message).
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    /// A filesystem error creating `.b2/` or opening the reindex lock file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// `reindex` was run with no vault at all (no positional, no `-C`, no
    /// `$B2_VAULT_PATH`) — refuse rather than silently index the current directory.
    #[error("no vault specified")]
    VaultRequired,
    /// Another `reindex` already holds the single-in-flight lock on this vault.
    #[error("a reindex is already running")]
    ReindexRunning,
    /// `reindex --cancel` found no run in flight on this vault — nothing to signal.
    #[error("no reindex is running")]
    NoReindexRunning,
    /// `reindex --cancel` found a run in flight whose lock names no readable pid, so
    /// there is no address to signal (see [`ReindexHolder::pid`]).
    #[error("the running reindex recorded no pid")]
    ReindexPidUnknown,
    /// `rm` was pointed at a folder without `--recursive` — refuse rather than
    /// silently remove a whole subtree (the CLI's stand-in for the desktop's
    /// confirm dialog; there is no interactive prompt to give an agent).
    #[error("folder delete requires --recursive: {0}")]
    RecursiveRequired(String),
    /// `write` was invoked with stdin attached to a terminal (nothing piped) — refuse
    /// rather than hang waiting for hand-typed input; the new body must be piped in.
    #[error("no body piped on stdin")]
    StdinRequired,
}

// `is_ollama` — "does this endpoint look like the Ollama daemon" — decides whether
// Ollama's own commands belong in an error message (`--llm-url` also points at LM Studio,
// llama.cpp, vLLM and cloud endpoints, where "run `ollama serve`" is advice about the
// wrong program). The rule lives in `b2-llm` (GH #155), where guided setup needs the same
// answer for a bigger decision, so one rule with two callers cannot drift.

/// The head of a model server's own model list, for "…or pick one it already
/// serves". Bounded: a local runtime holds a handful, but a cloud endpoint lists
/// hundreds, and a hundred-model line is not a hint.
fn model_hint(available: &[String]) -> Option<String> {
    const SHOWN: usize = 6;
    if available.is_empty() {
        return None;
    }
    let head = available.iter().take(SHOWN).cloned().collect::<Vec<_>>();
    Some(if available.len() > SHOWN {
        format!("{}, …", head.join(", "))
    } else {
        head.join(", ")
    })
}

/// Translate an internal error into a generic, actionable, user-facing message —
/// never leaking sqlite/io/serde internals. Set `B2_DEBUG` to also print the detail.
pub fn user_message(err: &CliError) -> String {
    let msg = match err {
        CliError::Core(b2_core::Error::NoteNotFound(r)) => format!(
            "Note not found: '{r}'. Check the path, and run `b2 reindex` first."
        ),
        CliError::Core(b2_core::Error::IndexTooNew { .. }) => {
            "This vault's index was built by a newer version of B2 than this `b2`. Run the newer B2 (or upgrade this one) — the index was left untouched, so nothing needs rebuilding.".to_string()
        }
        CliError::Core(b2_core::Error::ModelMismatch { .. }) => {
            "This vault's index was built with a different embedding model. Run `b2 reindex` to rebuild it.".to_string()
        }
        CliError::Embed(EmbedError::NotProvisioned { model, .. }) => format!(
            "Embedding model '{model}' is not installed. Run `b2 init` to download it (or set B2_EMBEDDER=fake for an offline, non-semantic mode)."
        ),
        CliError::Embed(EmbedError::Download(_)) => {
            "Could not download the embedding model. Check your network and try `b2 init` again.".to_string()
        }
        CliError::Core(b2_core::Error::MoveTargetExists(p)) => format!(
            "Can't move: a file already exists at '{p}'. Choose a different destination."
        ),
        CliError::Core(b2_core::Error::MoveDestination(_)) => {
            "That move destination isn't valid. Give a vault-relative path like `notes/new-name.md`.".to_string()
        }
        CliError::Core(b2_core::Error::MoveIncomplete { paths, .. }) => format!(
            "The move failed, and B2 could not undo its link changes in: {}. Check the links in those files; `b2 reindex` then shows any that no longer resolve.",
            paths.join(", ")
        ),
        CliError::Core(b2_core::Error::DirNotFound(p)) => format!(
            "Folder not found: '{p}'. Check the path (folders are vault-relative, like `notes/archive`)."
        ),
        CliError::Core(b2_core::Error::AddTargetExists(p)) => format!(
            "A note already exists at '{p}'. Choose a different path, or edit that note."
        ),
        CliError::Core(b2_core::Error::AddDestination(_)) => {
            "That note path isn't valid. Give a vault-relative path like `notes/new-name.md`.".to_string()
        }
        CliError::Core(b2_core::Error::InvalidRelation(v)) => format!(
            "'{v}' isn't a known relation type. Use one of: references, supports, contradicts."
        ),
        CliError::Core(b2_core::Error::WriteConflict(_)) => {
            "This note changed on disk since it was opened. Reload the note, then reapply your edit.".to_string()
        }
        CliError::Core(b2_core::Error::Frontmatter(d)) => {
            format!("Can't edit that frontmatter: {d}.")
        }
        CliError::Core(b2_core::Error::ResourceNotFound(r)) => format!(
            "File not found in the vault: '{r}'. Check the path, and run `b2 reindex` first."
        ),
        CliError::Core(b2_core::Error::ResourceUnsupported(_)) => {
            "Similarity for non-Markdown files isn't available yet — it arrives in a later release. `b2 explain <file>` shows its backlinks today.".to_string()
        }
        CliError::VaultRequired => {
            "No vault specified. Point B2 at your vault with `-C <path>`, or set B2_VAULT_PATH.".to_string()
        }
        CliError::ReindexRunning => {
            "A reindex is already running on this vault. Wait for it to finish (check `b2 status`), or stop it with `b2 reindex --cancel`.".to_string()
        }
        CliError::NoReindexRunning => {
            "No reindex is running on this vault — nothing to cancel. Check `b2 status`.".to_string()
        }
        CliError::ReindexPidUnknown => {
            "A reindex is running on this vault, but it recorded no process id to signal. Try `b2 reindex --cancel` again in a moment, or stop the run from the shell it was started in.".to_string()
        }
        CliError::RecursiveRequired(p) => format!(
            "'{p}' is a folder. Re-run with -r/--recursive to delete it and everything inside it."
        ),
        CliError::StdinRequired => {
            "No body piped on stdin. Pipe the new note body in, e.g. `cat new-body.md | b2 write notes/foo`.".to_string()
        }
        // The E4 case chat is most likely to hit: nothing is serving the endpoint.
        // Named by the endpoint the user actually configured — and *advised* by it
        // too: `ollama serve` is the fix for Ollama and no help at all for LM
        // Studio, llama.cpp, vLLM, or a cloud provider, all of which `--llm-url`
        // supports.
        CliError::Llm(LlmError::Unreachable { endpoint, .. }) if is_ollama(endpoint) => format!(
            "Can't reach the model server at {endpoint} — is Ollama running? (`ollama serve`, or install: {OLLAMA_INSTALL_URL})"
        ),
        CliError::Llm(LlmError::Unreachable { endpoint, .. }) => format!(
            "Can't reach the model server at {endpoint}. Start it, or point --llm-url (or B2_LLM_URL) at one that's running."
        ),
        // Something answered the probe and refused it — a *different* mistake from
        // nothing listening, and one this used to report as success. The sentence is
        // b2-llm's (`refusal_message`), so the CLI and the app say the same thing; the
        // Ollama-root hint is the app's alone, since only its card has asked the daemon.
        CliError::Llm(LlmError::Refused { endpoint, status, message }) => {
            refusal_message(endpoint, *status, message, None)
        }
        CliError::Llm(LlmError::ModelMissing { model, endpoint, available }) => format!(
            "Model '{model}' isn't available at {endpoint}. {}{}",
            if is_ollama(endpoint) {
                format!("Pull it with `ollama pull {model}`")
            } else {
                "Load it there".to_string()
            },
            match model_hint(available) {
                Some(list) => format!(", or point --llm-model at one it already serves ({list})."),
                None => ".".to_string(),
            }
        ),
        // Not the generic chat failure below: the server is up and the model is there,
        // so that advice would mislead. The cap is the one fix on this side of the wire.
        CliError::Core(b2_core::Error::ToolCallLimit { limit }) => format!(
            "The chat model asked for more than {limit} tool calls in one reply, so b2 stopped it. Try again or use another model (--llm-model). If this model really needs more, raise {}.",
            b2_llm::ENV_MAX_TOOL_CALLS
        ),
        // Every remaining chat failure — an HTTP refusal at probe time, a malformed
        // stream — is one sentence with one fix, and the detail is a `B2_DEBUG` away.
        CliError::Llm(_) | CliError::Core(b2_core::Error::Llm(_)) => {
            "The model server couldn't answer. Check that it's running and that the model is installed (`ollama list`), then try again.".to_string()
        }
        _ => "Something went wrong. Please check the vault path and try again.".to_string(),
    };
    if std::env::var_os("B2_DEBUG").is_some() {
        // Every wrapper variant is `#[error(transparent)]` and every local variant
        // carries its own `#[error("…")]` line, so the enum's own `Display` *is* the
        // per-variant detail — no match needed.
        let detail = err.to_string();
        format!("{msg}\n(debug: {detail})")
    } else {
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blown_tool_call_cap_names_the_limit_and_the_variable_that_raises_it() {
        let msg = user_message(&CliError::Core(b2_core::Error::ToolCallLimit { limit: 64 }));
        assert!(msg.contains("64"), "{msg}");
        assert!(msg.contains("B2_LLM_MAX_TOOL_CALLS"), "{msg}");
        assert!(
            !msg.contains("Check that it's running"),
            "the server is fine — the generic chat advice would mislead: {msg}"
        );
    }

    #[test]
    fn a_move_that_could_not_be_undone_names_the_files_to_check() {
        let msg = user_message(&CliError::Core(b2_core::Error::MoveIncomplete {
            paths: vec!["b.md".into(), "notes/c.md".into()],
            source: Box::new(b2_core::Error::Io(std::io::Error::other("disk full"))),
        }));
        assert!(msg.contains("in: b.md, notes/c.md."), "{msg}");
        assert!(
            !msg.contains("disk full"),
            "the cause stays internal: {msg}"
        );
    }
}
