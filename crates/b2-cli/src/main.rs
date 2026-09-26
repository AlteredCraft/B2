//! `b2` — one of the two dumb adapters over the `b2-core` typed API (ADR-0012),
//! headless-first: "the CLI is the UI before the UI". It holds **no engine logic** — it
//! parses args, injects the embedder and chat provider, calls the [`Vault`] façade, and
//! prints (human-readable, or `--json` for agents).
//!
//! The embedder is the real candle-backed [`LocalEmbedder`] by default. It is **not
//! bundled**: `b2 init` downloads it into a shared XDG cache, and `reindex`/`search` fail
//! fast with "run `b2 init`" if it is absent, never a surprise mid-command download
//! (ADR-0020). `B2_EMBEDDER=fake` forces the deterministic fake — an offline/dev mode,
//! and what the CLI suite runs under.

mod args;
mod cancel;
mod chat;
mod error;
mod logging;
mod read;
mod reindex;
mod wiring;
mod write;

use args::{Cli, Command};
use chat::{cmd_ask, cmd_chat, cmd_why};
use clap::Parser;
use error::{user_message, CliError};
use read::{cmd_explain, cmd_explain_similar, cmd_neighbors, cmd_search, cmd_similar};
use reindex::{cmd_init, cmd_reindex, cmd_status};
use std::process::ExitCode;
use write::{cmd_add, cmd_link, cmd_mv, cmd_rm, cmd_write};

fn main() -> ExitCode {
    logging::init_logging();
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", user_message(&e));
            ExitCode::FAILURE
        }
    }
}

/// The thin router: each subcommand's whole behavior lives in its `cmd_*` fn below;
/// this match only destructures the parsed args and forwards them.
fn dispatch(cli: &Cli) -> Result<(), CliError> {
    match &cli.command {
        Command::Init => cmd_init(cli.json),
        Command::Reindex {
            vault,
            force,
            dry_run,
            cancel,
        } => cmd_reindex(cli, vault.as_deref(), *force, *dry_run, *cancel),
        Command::Status => cmd_status(cli),
        Command::Add {
            path,
            title,
            content,
        } => cmd_add(cli, path, title.as_deref(), content.as_deref()),
        Command::Write { note } => cmd_write(cli, note),
        Command::Neighbors { note } => cmd_neighbors(cli, note),
        Command::Explain { note } => cmd_explain(cli, note),
        Command::Mv { from, to } => cmd_mv(cli, from, to),
        Command::Rm { target, recursive } => cmd_rm(cli, target, *recursive),
        Command::Search {
            query,
            limit,
            exclude,
        } => cmd_search(cli, query, *limit, exclude),
        Command::Similar {
            note,
            limit,
            explain: Some(other),
        } => cmd_explain_similar(cli, note, other, *limit),
        Command::Similar {
            note,
            limit,
            explain: None,
        } => cmd_similar(cli, note, *limit),
        Command::Link {
            src,
            dst,
            edge_type,
            explanation,
        } => cmd_link(cli, src, dst, edge_type, explanation.as_deref()),
        Command::Ask { question, llm } => cmd_ask(cli, question, llm),
        Command::Why {
            note,
            candidate,
            limit,
            llm,
        } => cmd_why(cli, note, candidate, *limit, llm),
        Command::Chat { llm } => cmd_chat(cli, llm),
    }
}

/// Print `value` as pretty JSON on stdout — the one `--json` output path, shared by
/// every subcommand.
pub fn print_json<T: serde::Serialize + ?Sized>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
