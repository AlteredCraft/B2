//! The CLI's error type and its one translation into a user-facing sentence.

use b2_embed::{EmbedConfig, EmbedError};
use b2_llm::{model_missing_message, refusal_message, unreachable_message, LlmError};

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
    /// A command that writes (`reindex`, `add`, `write`, `mv`, `rm`, `link`) was run with
    /// no vault at all (no positional, no `-C`, no `$B2_VAULT_PATH`) — refuse rather than
    /// silently write into the current directory.
    #[error("no vault specified")]
    VaultRequired,
    /// Another `reindex` already holds the single-in-flight lock on this vault.
    #[error("a reindex is already running")]
    ReindexRunning,
    /// `reindex --cancel` found no run in flight on this vault — nothing to signal.
    #[error("no reindex is running")]
    NoReindexRunning,
    /// `reindex --cancel` found a run in flight whose lock names no readable pid, so
    /// there is no address to signal (a holder that hasn't stamped its pid yet).
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

/// The core relation verbs, as a message lists them — read from `b2_core::relation::CORE`
/// so a verb added there is offered here without an edit.
fn core_verbs() -> String {
    b2_core::relation::CORE
        .iter()
        .map(|c| c.verb)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The embedder config file, as a message names it.
fn config_file() -> String {
    EmbedConfig::config_path()
        .map_or_else(|| "config.toml".to_string(), |p| p.display().to_string())
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
        CliError::Embed(EmbedError::Load(_)) => {
            "The embedding model's files failed to load. Run `b2 init` to fetch them again.".to_string()
        }
        // `config.toml` didn't parse, or its `source` names a folder missing a model file.
        // Either way the fix is in that one file, so name it.
        CliError::Embed(EmbedError::Config(_)) => format!(
            "B2 couldn't use its embedder settings. Check the [embedder] table in {}, then try again.",
            config_file()
        ),
        CliError::Embed(EmbedError::Io(_)) => {
            "B2 couldn't read or write the embedding model's files. Check that the model cache is readable and writable, then run `b2 init` again.".to_string()
        }
        // Only a settings picker names a model to switch to; the CLI never does, so this
        // arm exists for exhaustiveness and says what the fix would be.
        CliError::Embed(EmbedError::UnknownModel(m)) => format!(
            "'{m}' isn't an embedding model B2 offers. Set `model` in {} to one of: {}.",
            config_file(),
            b2_embed::AVAILABLE_MODELS
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
                .join(", ")
        ),
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
            "'{v}' isn't a known relation type. Use one of: {}.",
            core_verbs()
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
        // The E4 case chat is most likely to hit: nothing is serving the endpoint. The
        // sentence is b2-llm's, advised by the endpoint the user actually configured
        // (`ollama serve` only when it is Ollama's); the CLI adds only its own way of
        // pointing somewhere else.
        CliError::Llm(LlmError::Unreachable { endpoint, .. }) => format!(
            "{} Or point --llm-url (or B2_LLM_URL) at a different one.",
            unreachable_message(endpoint)
        ),
        // Something answered the probe and refused it — a *different* mistake from
        // nothing listening, and one this used to report as success. The sentence is
        // b2-llm's (`refusal_message`), so the CLI and the app say the same thing; the
        // Ollama-root hint is the app's alone, since only its card has asked the daemon.
        CliError::Llm(LlmError::Refused { endpoint, status, message }) => {
            refusal_message(endpoint, *status, message, None)
        }
        // b2-llm's sentence again, plus the CLI's flag — and the server's own list, since
        // "pick one it already serves" is only advice when it names them.
        CliError::Llm(LlmError::ModelMissing { model, endpoint, available }) => {
            let msg = model_missing_message(model, endpoint);
            match model_hint(available) {
                Some(list) => format!("{msg} Or point --llm-model at one it already serves ({list})."),
                None => msg,
            }
        }
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
        // Everything else in the composed crates is an internal (sqlite/io/serde/…) this
        // message must never show. Spelled out rather than `_`, so a new `CliError`
        // variant fails to compile here instead of silently landing in the catch-all.
        CliError::Core(_) | CliError::Serde(_) | CliError::Io(_) => {
            "Something went wrong. Please check the vault path and try again.".to_string()
        }
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
    fn an_invalid_relation_lists_every_core_verb() {
        let msg = user_message(&CliError::Core(b2_core::Error::InvalidRelation(
            "refutes".into(),
        )));
        assert!(msg.contains("'refutes'"), "{msg}");
        for verb in b2_core::relation::CORE {
            assert!(msg.contains(verb.verb), "{msg}");
        }
    }

    /// `b2 init` meets these two, and they used to fall through to "check the vault path"
    /// — advice about a vault the command never opened.
    #[test]
    fn embedder_setup_failures_say_what_to_fix_not_to_check_the_vault() {
        for err in [
            EmbedError::Load("weights: truncated".into()),
            EmbedError::Config("config.toml: expected `=`".into()),
        ] {
            let msg = user_message(&CliError::Embed(err));
            assert!(!msg.contains("vault path"), "{msg}");
            assert!(
                !msg.contains("truncated") && !msg.contains("expected"),
                "the detail stays internal: {msg}"
            );
        }
        let load = user_message(&CliError::Embed(EmbedError::Load(String::new())));
        assert!(load.contains("b2 init"), "{load}");
        let config = user_message(&CliError::Embed(EmbedError::Config(String::new())));
        assert!(config.contains("[embedder]"), "{config}");
    }

    /// The sentences are b2-llm's, shared with the desktop; the CLI adds only its flags.
    #[test]
    fn chat_setup_failures_use_the_shared_sentence_plus_the_cli_flag() {
        let endpoint = "http://localhost:1234/v1";
        let unreachable = user_message(&CliError::Llm(LlmError::Unreachable {
            endpoint: endpoint.into(),
            detail: "connection refused".into(),
        }));
        assert!(
            unreachable.starts_with(&unreachable_message(endpoint)),
            "{unreachable}"
        );
        assert!(unreachable.contains("--llm-url"), "{unreachable}");

        let missing = |available: Vec<String>| {
            user_message(&CliError::Llm(LlmError::ModelMissing {
                model: "llama3.2".into(),
                endpoint: "http://localhost:11434/v1".into(),
                available,
            }))
        };
        let bare = missing(Vec::new());
        assert_eq!(
            bare,
            model_missing_message("llama3.2", "http://localhost:11434/v1")
        );
        assert!(bare.contains("ollama pull llama3.2"), "{bare}");
        let listed = missing(vec!["qwen2.5:latest".into()]);
        assert!(
            listed.contains("--llm-model") && listed.contains("qwen2.5:latest"),
            "{listed}"
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
