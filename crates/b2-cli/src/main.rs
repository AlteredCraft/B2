//! `b2` — the first adapter over the `b2-core` typed API (vision-and-scope,
//! headless-first: "the CLI is the UI before the UI"). It holds **no logic**: it
//! parses args, calls the [`Vault`] façade, and prints — human-readable by default,
//! or `--json` for agents (the agent drives the CLI). All behavior lives in the
//! core; this file is deliberately dumb.
//!
//! Deterministic today: the shipped embedder is the fake one, so `search`'s BM25
//! (keyword) half is real while the vector half is **not yet semantic**. The human
//! `search` output says so; do not overstate it.

use b2_core::vault::Vault;
use b2_core::Error;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "b2",
    version,
    about = "B2 — explore a Markdown vault's typed graph and search from the terminal"
)]
struct Cli {
    /// Vault root (the folder of Markdown). The index + log live in `<vault>/.b2/`.
    #[arg(short = 'C', long = "vault", global = true, default_value = ".")]
    vault: PathBuf,

    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Re-project every note under the vault into the index (stamps missing b2ids).
    Reindex {
        /// Vault root (overrides --vault); defaults to --vault / the current dir.
        vault: Option<PathBuf>,
    },
    /// Show a note's typed neighbors. NOTE is a vault-relative path or a b2id.
    Neighbors { note: String },
    /// Hybrid keyword+graph search across the vault.
    Search {
        query: String,
        /// Maximum number of notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", user_message(&e));
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: &Cli) -> b2_core::Result<()> {
    // Reindex's positional vault wins over the global flag; other commands use it.
    let root = match &cli.command {
        Command::Reindex { vault: Some(v) } => v.clone(),
        _ => cli.vault.clone(),
    };
    let vault = Vault::open(&root)?;

    match &cli.command {
        Command::Reindex { .. } => {
            let report = vault.reindex()?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Indexed {} notes ({} stamped)",
                    report.indexed, report.stamped
                );
            }
        }
        Command::Neighbors { note } => {
            let neighbors = vault.neighbors(note)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&neighbors)?);
            } else if neighbors.is_empty() {
                println!("No neighbors.");
            } else {
                for n in &neighbors {
                    let arrow = if n.direction == "outbound" { "→" } else { "←" };
                    let name = n.title.as_deref().unwrap_or(&n.path);
                    let explanation = n
                        .explanation
                        .as_deref()
                        .map(|e| format!(" — {e}"))
                        .unwrap_or_default();
                    println!("{arrow} {}  {name} ({}){explanation}", n.label, n.path);
                }
            }
        }
        Command::Search { query, limit } => {
            let results = vault.search(query, *limit)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                if results.is_empty() {
                    println!("No results.");
                } else {
                    for r in &results {
                        let name = r.title.as_deref().unwrap_or(&r.path);
                        println!("{:.4}  {name} ({})", r.score, r.path);
                        if !r.snippet.is_empty() {
                            println!("    {}", r.snippet);
                        }
                    }
                }
                // Honesty (never overstate): keyword ranking is live; semantic is not
                // until the real embedder lands. Kept on stderr so stdout is results.
                eprintln!(
                    "note: keyword (BM25) ranking is live; semantic ranking is not yet enabled."
                );
            }
        }
    }
    Ok(())
}

/// Translate an internal error into a generic, actionable, user-facing message —
/// never leaking sqlite/io/serde internals (project logging policy). Set `B2_DEBUG`
/// to also print the underlying detail for troubleshooting.
fn user_message(err: &Error) -> String {
    let msg = match err {
        Error::NoteNotFound(r) => format!(
            "Note not found: '{r}'. Check the path or b2id, and run `b2 reindex` first."
        ),
        _ => "Something went wrong. Please check the vault path and try again.".to_string(),
    };
    if std::env::var_os("B2_DEBUG").is_some() {
        format!("{msg}\n(debug: {err})")
    } else {
        msg
    }
}
