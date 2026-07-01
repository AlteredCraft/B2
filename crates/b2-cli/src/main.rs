//! `b2` — the first adapter over the `b2-core` typed API (vision-and-scope,
//! headless-first: "the CLI is the UI before the UI"). It holds **no engine logic**:
//! it parses args, picks + injects the embedder, calls the [`Vault`] façade, and
//! prints — human-readable by default, or `--json` for agents.
//!
//! The embedder is the real, candle-backed [`LocalEmbedder`] by default (`search`'s
//! vector half is genuinely semantic). It is **not bundled**: `b2 init` downloads it
//! into a shared XDG cache, and `reindex`/`search` **fail fast** with "run `b2 init`"
//! if it is absent — never a surprise mid-command download (index-engine.md §6).
//! `B2_EMBEDDER=fake` forces the deterministic fake embedder — an offline/dev mode
//! that needs no model, and what the CLI test suite uses to stay fast and model-free.

use b2_core::embed::Embedder;
use b2_core::vault::Vault;
use b2_embed::{provision, EmbedConfig, EmbedError, LocalEmbedder};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
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
    /// Download + verify the embedding model into the shared cache (one-time setup).
    Init,
    /// Re-project every note under the vault into the index (stamps missing b2ids).
    Reindex {
        /// Vault root (overrides --vault); defaults to --vault / the current dir.
        vault: Option<PathBuf>,
    },
    /// Show a note's typed neighbors. NOTE is a vault-relative path or a b2id.
    Neighbors { note: String },
    /// Hybrid keyword+semantic+graph search across the vault.
    Search {
        query: String,
        /// Maximum number of notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Generate connection suggestions (idempotent) and list the pending queue.
    Suggest {
        /// Candidates considered per note — the breadth of discovery.
        #[arg(long, default_value_t = 5)]
        top: usize,
    },
    /// Accept a pending suggestion by ID: write its typed link into the source note.
    Accept { id: String },
    /// Reject a pending suggestion by ID: it will never be proposed again.
    Reject { id: String },
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

fn dispatch(cli: &Cli) -> Result<(), CliError> {
    match &cli.command {
        Command::Init => {
            // Global, per-machine setup — no vault involved.
            let config = EmbedConfig::load()?;
            let report = provision(&config, |line| eprintln!("{line}"))?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if report.already_present {
                println!("Model '{}' is already installed.", report.model);
            } else {
                println!(
                    "Installed '{}' ({} dims). Run `b2 reindex` to embed your vault.",
                    report.model, report.dim
                );
            }
        }
        Command::Reindex { vault } => {
            // Reindex's positional vault wins over the global flag.
            let root = vault.as_ref().unwrap_or(&cli.vault);
            // Reindex embeds every chunk → it needs the real model.
            let (vault, _semantic) = open_vault(root, true)?;
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
            // Neighbors is a pure graph query — it never embeds, so don't require
            // the model (no needless `b2 init` just to explore the graph).
            let (vault, _semantic) = open_vault(&cli.vault, false)?;
            let neighbors = vault.neighbors(note)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&neighbors)?);
            } else if neighbors.is_empty() {
                println!("No neighbors.");
            } else {
                for n in &neighbors {
                    let arrow = if n.direction == "outbound" {
                        "→"
                    } else {
                        "←"
                    };
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
            // Search embeds the query for the vector half → it needs the real model.
            let (vault, semantic) = open_vault(&cli.vault, true)?;
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
                // Honesty (never overstate): with the fake embedder the vector half
                // isn't semantic. Under the real model it is, so no caveat. Kept on
                // stderr so stdout stays pure results.
                if !semantic {
                    eprintln!(
                        "note: keyword (BM25) ranking is live; semantic ranking is off (fake embedder)."
                    );
                }
            }
        }
        Command::Suggest { top } => {
            // Candidate generation reads the stored vectors (no re-embed) and the
            // relator is a deterministic stub, so `suggest` needs no model — like
            // `neighbors`. A prior `reindex` supplies the vectors it reads.
            let (vault, _semantic) = open_vault(&cli.vault, false)?;
            let tally = vault.generate_suggestions(*top)?;
            let queue = vault.list_suggestions()?;
            if cli.json {
                // Pure data on stdout: the pending queue (agents act on `edge_id`).
                println!("{}", serde_json::to_string_pretty(&queue)?);
            } else {
                if queue.is_empty() {
                    println!("No suggestions.");
                } else {
                    for s in &queue {
                        let src = s.src_title.as_deref().unwrap_or(&s.src_path);
                        let dst = s.dst_title.as_deref().unwrap_or(&s.dst_path);
                        let conf = s
                            .confidence
                            .map(|c| format!("  ({c:.2})"))
                            .unwrap_or_default();
                        println!("[{}]  {src}  {}→  {dst}{conf}", s.edge_id, s.relation);
                        if let Some(e) = &s.explanation {
                            println!("    {e}");
                        }
                    }
                }
                // Feedback + honesty on stderr, so stdout stays pure results.
                eprintln!("Generated {} new suggestion(s).", tally.generated);
                eprintln!(
                    "note: suggestions come from a stub relator (no judgment model yet) — treat them as placeholders, not real connections."
                );
            }
        }
        Command::Accept { id } => {
            // Accept re-projects (re-embeds) the source note, so it must use the same
            // embedder the index was built with → load the real model, like reindex.
            let (vault, _semantic) = open_vault(&cli.vault, true)?;
            if !vault.accept_suggestion(id)? {
                return Err(CliError::SuggestionNotFound(id.clone()));
            }
            if cli.json {
                let out = serde_json::json!({ "accepted": true, "edge_id": id });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Accepted. Wrote the typed link into the source note's frontmatter.");
            }
        }
        Command::Reject { id } => {
            // Reject only appends a tombstone event + flips status — no embedding.
            let (vault, _semantic) = open_vault(&cli.vault, false)?;
            if !vault.reject_suggestion(id)? {
                return Err(CliError::SuggestionNotFound(id.clone()));
            }
            if cli.json {
                let out = serde_json::json!({ "rejected": true, "edge_id": id });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Rejected. This pair won't be suggested again.");
            }
        }
    }
    Ok(())
}

/// Open a vault with the appropriate embedder. Returns the vault and whether its
/// embedder is semantic (real model) — the caller uses that only for honest output.
///
/// `needs_semantic` commands (`reindex`, `search`) load the real [`LocalEmbedder`]
/// from the shared cache and **fail fast** with "run `b2 init`" if it's absent.
/// Pure-graph commands pass `false` and use the fake — no model required just to
/// explore the graph. `B2_EMBEDDER=fake` forces the fake everywhere (offline/dev
/// mode, and what the test suite runs under).
fn open_vault(root: &Path, needs_semantic: bool) -> Result<(Vault, bool), CliError> {
    if needs_semantic && !use_fake_embedder() {
        let config = EmbedConfig::load()?;
        let embedder = LocalEmbedder::load(&config)?;
        Ok((
            Vault::open_with_embedder(root, Box::new(embedder) as Box<dyn Embedder>)?,
            true,
        ))
    } else {
        Ok((Vault::open(root)?, false))
    }
}

fn use_fake_embedder() -> bool {
    matches!(std::env::var("B2_EMBEDDER").ok().as_deref(), Some("fake"))
}

/// The CLI's error, composing the two crates it drives. Kept internal; `user_message`
/// turns it into a generic, actionable, no-internals-leaked line (logging policy).
/// `#[from]` supplies the `?` conversions; `transparent` defers `Display` to the
/// inner error (only ever surfaced under `B2_DEBUG`).
#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Core(#[from] b2_core::Error),
    #[error(transparent)]
    Embed(#[from] EmbedError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    /// A CLI-level domain error: `accept`/`reject` were given an id that is not a
    /// pending suggestion. Owned here (not in `b2-core`) because it is purely about
    /// the command's UX — the façade just returns `false`.
    #[error("suggestion not found: {0}")]
    SuggestionNotFound(String),
}

/// Translate an internal error into a generic, actionable, user-facing message —
/// never leaking sqlite/io/serde internals. Set `B2_DEBUG` to also print the detail.
fn user_message(err: &CliError) -> String {
    let msg = match err {
        CliError::Core(b2_core::Error::NoteNotFound(r)) => format!(
            "Note not found: '{r}'. Check the path or b2id, and run `b2 reindex` first."
        ),
        CliError::Core(b2_core::Error::ModelMismatch { .. }) => {
            "This vault's index was built with a different embedding model. Run `b2 reindex` to rebuild it.".to_string()
        }
        CliError::Embed(EmbedError::NotProvisioned { model, .. }) => format!(
            "Embedding model '{model}' is not installed. Run `b2 init` to download it (or set B2_EMBEDDER=fake for an offline, non-semantic mode)."
        ),
        CliError::Embed(EmbedError::Download(_)) => {
            "Could not download the embedding model. Check your network and try `b2 init` again.".to_string()
        }
        CliError::SuggestionNotFound(id) => format!(
            "No pending suggestion with id '{id}'. Run `b2 suggest` to see the current queue and its ids."
        ),
        _ => "Something went wrong. Please check the vault path and try again.".to_string(),
    };
    if std::env::var_os("B2_DEBUG").is_some() {
        let detail = match err {
            CliError::Core(e) => e.to_string(),
            CliError::Embed(e) => e.to_string(),
            CliError::Serde(e) => e.to_string(),
            CliError::SuggestionNotFound(id) => format!("suggestion not found: {id}"),
        };
        format!("{msg}\n(debug: {detail})")
    } else {
        msg
    }
}
