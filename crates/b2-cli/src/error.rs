//! The CLI's error type and its one translation into a user-facing sentence.

use b2_embed::{EmbedConfig, EmbedError};
use b2_llm::{model_missing_message, refusal_message, unreachable_message, LlmError};

/// The CLI's error, composing the crates it drives. Kept internal: `user_message` turns it
/// into a generic, actionable line, and `Display` surfaces only under `B2_DEBUG`.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] b2_core::Error),
    #[error(transparent)]
    Embed(#[from] EmbedError),
    /// A setup-time chat failure the adapter can act on. Call-time failures arrive as
    /// [`b2_core::Error::Llm`].
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    /// A filesystem error creating `.b2/` or opening the reindex lock file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A writing command got no vault; refuse rather than write into the cwd.
    #[error("no vault specified")]
    VaultRequired,
    /// Another `reindex` already holds the single-in-flight lock on this vault.
    #[error("a reindex is already running")]
    ReindexRunning,
    /// `reindex --cancel` found no run in flight.
    #[error("no reindex is running")]
    NoReindexRunning,
    /// `reindex --cancel` found a run whose lock holds no readable pid yet.
    #[error("the running reindex recorded no pid")]
    ReindexPidUnknown,
    /// `rm` on a folder without `--recursive`: the CLI's stand-in for the desktop's
    /// confirm dialog.
    #[error("folder delete requires --recursive: {0}")]
    RecursiveRequired(String),
    /// `write` with stdin on a terminal: refuse rather than hang waiting for input.
    #[error("no body piped on stdin")]
    StdinRequired,
}

/// The head of a server's model list, bounded because a cloud endpoint lists hundreds.
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

/// The core relation verbs, read from `b2_core::relation::CORE`.
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

/// A generic, actionable message that never leaks internals; `B2_DEBUG` adds the detail.
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
        // Either way the fix is in that one file, so name it.
        CliError::Embed(EmbedError::Config(_)) => format!(
            "B2 couldn't use its embedder settings. Check the [embedder] table in {}, then try again.",
            config_file()
        ),
        CliError::Embed(EmbedError::Io(_)) => {
            "B2 couldn't read or write the embedding model's files. Check that the model cache is readable and writable, then run `b2 init` again.".to_string()
        }
        // Only a settings picker names a model to switch to; here for exhaustiveness.
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
        // E4. The sentence is b2-llm's; the CLI adds only its own flag.
        CliError::Llm(LlmError::Unreachable { endpoint, .. }) => format!(
            "{} Or point --llm-url (or B2_LLM_URL) at a different one.",
            unreachable_message(endpoint)
        ),
        // Something answered and refused, a different mistake from nothing listening. The
        // Ollama-root hint is the app's alone: only its card has asked the daemon.
        CliError::Llm(LlmError::Refused { endpoint, status, message }) => {
            refusal_message(endpoint, *status, message, None)
        }
        // Plus the server's own list: "pick one it serves" is only advice if it names them.
        CliError::Llm(LlmError::ModelMissing { model, endpoint, available }) => {
            let msg = model_missing_message(model, endpoint);
            match model_hint(available) {
                Some(list) => format!("{msg} Or point --llm-model at one it already serves ({list})."),
                None => msg,
            }
        }
        // The server is up and the model is there, so the generic advice would mislead.
        CliError::Core(b2_core::Error::ToolCallLimit { limit }) => format!(
            "The chat model asked for more than {limit} tool calls in one reply, so b2 stopped it. Try again or use another model (--llm-model). If this model really needs more, raise {}.",
            b2_llm::ENV_MAX_TOOL_CALLS
        ),
        CliError::Llm(_) | CliError::Core(b2_core::Error::Llm(_)) => {
            "The model server couldn't answer. Check that it's running and that the model is installed (`ollama list`), then try again.".to_string()
        }
        // Internals this message must never show. Spelled out rather than `_`, so a new
        // `CliError` variant fails to compile instead of landing in the catch-all.
        CliError::Core(_) | CliError::Serde(_) | CliError::Io(_) => {
            "Something went wrong. Please check the vault path and try again.".to_string()
        }
    };
    if std::env::var_os("B2_DEBUG").is_some() {
        // The enum's own `Display` is the per-variant detail.
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

    /// `b2 init` never opens a vault, so "check the vault path" would mislead.
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
