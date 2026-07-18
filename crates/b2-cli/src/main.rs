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
use b2_core::resource::{doc_kind, DocKind};
use b2_core::vault::Vault;
use b2_embed::{provision, EmbedConfig, EmbedError, LocalEmbedder};
use clap::{Parser, Subcommand};
use std::fs::{File, OpenOptions};
use std::io::{IsTerminal, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the Ctrl-C handler installed for a foreground `reindex`. The embed loop
/// reads it at each batch boundary (through the [`ControlFlow`] the progress closure
/// returns — the shipped cancel seam) and stops *after* the current batch: a
/// consistent, re-runnable partial index, never a torn write (async-indexing.md §3/§8).
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Map the Ctrl-C flag onto the reindex embed loop's cooperative-cancel signal —
/// [`ControlFlow::Break`] once a cancel has been requested (stop after the current
/// batch), else [`ControlFlow::Continue`]. Shared by `reindex`'s two progress closures.
fn cancel_flow() -> ControlFlow<()> {
    if CANCEL.load(Ordering::SeqCst) {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}

#[derive(Parser)]
#[command(
    name = "b2",
    version,
    about = "B2 — explore a Markdown vault's typed graph and search from the terminal"
)]
struct Cli {
    /// Vault root (the folder of Markdown). The index lives in `<vault>/.b2/`.
    /// Set it with `-C <path>` or `$B2_VAULT_PATH` (the flag wins). Read-only commands
    /// fall back to the current dir; commands that write (`reindex`/`add`/`mv`/`link`)
    /// require it explicitly, so they can never silently touch the wrong directory.
    #[arg(short = 'C', long = "vault", global = true, env = "B2_VAULT_PATH")]
    vault: Option<PathBuf>,

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
    /// Incremental by default: notes whose content is unchanged keep their vectors.
    Reindex {
        /// Vault root (overrides --vault / $B2_VAULT_PATH). Required — no cwd default.
        vault: Option<PathBuf>,
        /// Re-embed every note, even unchanged ones (a full rebuild in place).
        #[arg(long)]
        force: bool,
        /// Preview what a reindex would do without writing anything — no b2id
        /// stamped, no index or log change, no embedding.
        #[arg(long)]
        dry_run: bool,
    },
    /// Report embedding coverage — how many notes are embedded (so semantic ranking
    /// is live vs. keyword-only) and whether a reindex is currently running. A pure,
    /// model-free read; handy after kicking off a slow reindex in the background with
    /// `b2 reindex &`.
    Status,
    /// Create a new note and project it into the index (it's immediately in the
    /// graph + searchable). PATH is vault-relative (the `.md` extension is optional).
    Add {
        /// Where the new note goes: a vault-relative path (`.md` optional).
        path: String,
        /// The note's human title (frontmatter `title:`).
        #[arg(long)]
        title: Option<String>,
        /// Initial body content (Markdown). Omit for an empty note to fill in later.
        #[arg(long)]
        content: Option<String>,
    },
    /// Show a note's typed neighbors. NOTE is a vault-relative path or a b2id.
    Neighbors { note: String },
    /// Explain a note's connections — every typed edge and its "why". NOTE is a
    /// vault-relative path or a b2id.
    Explain { note: String },
    /// Move/rename a note and rewrite every inbound link to point at its new path.
    Mv {
        /// The note to move: a vault-relative path or a b2id.
        from: String,
        /// The new vault-relative path (the `.md` extension is optional).
        to: String,
    },
    /// Hybrid keyword+semantic+graph search across the vault.
    Search {
        query: String,
        /// Maximum number of notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Surface the notes most semantically similar to NOTE that you haven't linked
    /// yet — connection discovery. NOTE is a vault-relative path or a b2id. A local
    /// read over stored vectors (run `b2 reindex` with the real model first).
    Similar {
        /// The note to find similar notes for: a vault-relative path or a b2id.
        note: String,
        /// Maximum number of similar notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Commit a typed connection SRC → DST into SRC's frontmatter `relations:`.
    /// SRC and DST are each a vault-relative path or a b2id.
    Link {
        /// The source note (the edge points *from* it): path or b2id.
        src: String,
        /// The target note (the edge points *to* it): path or b2id.
        dst: String,
        /// The relation verb (a core verb, e.g. elaborates/supports/supersedes).
        #[arg(long = "type", default_value = "references")]
        edge_type: String,
        /// Optional explanation — trailing text shown after the link.
        #[arg(long)]
        explanation: Option<String>,
    },
}

impl Cli {
    /// The vault root for **read-only** commands (`search`, `neighbors`, `explain`,
    /// `similar`): the `-C`/`$B2_VAULT_PATH` value if given, else the current directory.
    /// A pure read can't pollute anything, so the cwd convenience is safe here.
    fn vault_or_cwd(&self) -> PathBuf {
        self.vault.clone().unwrap_or_else(|| PathBuf::from("."))
    }

    /// The vault root for commands that **write** to the vault (`reindex`, `add`, `mv`,
    /// `link`): `positional` (only `reindex` has one) wins, then `-C`/`$B2_VAULT_PATH`;
    /// with none, error rather than silently building or mutating in the current
    /// directory — a stale binary or a mistyped var would otherwise pollute the wrong
    /// place (and leave a stray `.b2/`). This is the write-side counterpart to
    /// [`vault_or_cwd`](Self::vault_or_cwd).
    fn require_vault<'a>(&'a self, positional: Option<&'a Path>) -> Result<&'a Path, CliError> {
        positional
            .or(self.vault.as_deref())
            .ok_or(CliError::VaultRequired)
    }
}

fn main() -> ExitCode {
    init_logging();
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", user_message(&e));
            ExitCode::FAILURE
        }
    }
}

/// Opt-in structured debug logging: the kernel's `tracing` events — per-statement
/// SQLite timings from SQLite's own profiler (`b2::sqlite`, with `duration_us` and
/// `slow=true` on anything at/over `B2_SLOW_QUERY_MS`, default 100), façade-op
/// spans (`b2::vault`), and flow milestones (`b2::search`/`b2::ingest`) — rendered
/// as **JSON Lines**, one flat object per line, so a run's log pipes straight into
/// jq/DuckDB/pandas for reporting and plotting while `--json` stdout stays pure data.
///
/// The sink is stderr by default; `B2_LOG_FILE=<path>` writes the log there instead
/// (**append** mode, so successive runs accumulate into one reportable dataset —
/// every event carries its own timestamp). A file is also the guaranteed-pure
/// capture: stderr can interleave human notices (progress lines, skipped-file
/// lists) with the JSONL in non-`--json` runs.
///
/// `B2_LOG` holds a tracing filter directive (e.g. `debug`, `b2::sqlite=debug`,
/// `warn` for slow queries only); setting `B2_DEBUG` (which already opts into error
/// detail) or `B2_LOG_FILE` without `B2_LOG` implies `debug`. With none of the
/// three set, no subscriber is installed and the kernel's instrumentation stays
/// inert.
fn init_logging() {
    let log_file = std::env::var_os("B2_LOG_FILE");
    let directive = match std::env::var("B2_LOG") {
        Ok(v) if !v.trim().is_empty() => v,
        _ if std::env::var_os("B2_DEBUG").is_some() || log_file.is_some() => "debug".to_string(),
        _ => return,
    };
    let filter = match tracing_subscriber::EnvFilter::try_new(&directive) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("warning: invalid B2_LOG filter '{directive}' ({e}); using 'debug'");
            tracing_subscriber::EnvFilter::new("debug")
        }
    };
    let builder = tracing_subscriber::fmt()
        .json()
        // Event fields at the top level of each object (not nested under "fields")
        // — what makes `jq '.duration_us'`-style reporting one-liners work.
        .flatten_event(true)
        // Close events give each façade-op span its measured duration; the clock
        // lives here in the adapter, keeping b2-core itself wall-clock-free.
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .with_current_span(true)
        .with_span_list(false)
        .with_ansi(false)
        .with_env_filter(filter);
    // A CLI run is short-lived and single-threaded at the log site, so a plain
    // `Mutex<File>` writer suffices — no async appender needed.
    match log_file.map(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::path::Path::new(&p))
            .map_err(|e| (p, e))
    }) {
        Some(Ok(file)) => builder.with_writer(std::sync::Mutex::new(file)).init(),
        Some(Err((path, e))) => {
            eprintln!(
                "warning: cannot open B2_LOG_FILE '{}' ({e}); logging to stderr",
                path.to_string_lossy()
            );
            builder.with_writer(std::io::stderr).init();
        }
        None => builder.with_writer(std::io::stderr).init(),
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
        Command::Reindex {
            vault,
            force,
            dry_run,
        } => {
            // Reindex writes an index → require an explicit vault (positional wins),
            // never a silent cwd fallback. See `Cli::require_vault`.
            let root = cli.require_vault(vault.as_deref())?;
            if *dry_run {
                // A dry-run neither embeds nor stamps → no model needed (open with
                // the fake, like `neighbors`); it's a pure read, so there's no slow
                // embed phase to show progress for.
                let (vault, _semantic) = open_vault(root, false)?;
                let plan = vault.plan_reindex(*force)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&plan)?);
                } else {
                    println!(
                        "Dry run: would index {} note(s) — {} to embed, {} to stamp. No changes made.",
                        plan.would_index, plan.would_embed, plan.would_stamp
                    );
                }
                return Ok(());
            }
            // Single-in-flight: take an advisory lock *before* the (slow) model load so
            // a second `b2 reindex` — e.g. a foreground run racing one you backgrounded
            // with `b2 reindex &` — refuses cleanly instead of two processes writing the
            // same index. Advisory, not a PID file: the OS frees it the instant the holder
            // exits (crash, kill, or Ctrl-C included), so nothing stale is ever left behind.
            // `lock` is held until this arm ends; dropping it releases the lock.
            let lock = open_reindex_lock(root)?;
            match lock.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => return Err(CliError::ReindexRunning),
                Err(std::fs::TryLockError::Error(e)) => return Err(CliError::Io(e)),
            }
            // Reindex embeds every changed chunk → it needs the real model.
            let (vault, _semantic) = open_vault(root, true)?;
            // Wire Ctrl-C to the cooperative-cancel flag now that the model is loaded and
            // real embedding is next. (During the model load the default SIGINT still
            // applies — nothing is written yet, so a hard stop there is safe.) Best-effort:
            // if the handler can't be installed, Ctrl-C keeps its default (terminate), which
            // still leaves a consistent index since edges + FTS land before any vectors.
            let _ = ctrlc::set_handler(|| CANCEL.store(true, Ordering::SeqCst));
            // Embedding a large vault on CPU is slow; show a live progress line so it
            // never looks frozen. Only on an interactive stderr (never in --json, and
            // never when piped/captured) so machine output and tests stay clean.
            let report = if cli.json || !std::io::stderr().is_terminal() {
                vault.reindex_with_progress(*force, &mut |_| cancel_flow())?
            } else {
                // Name the vault being indexed up front, then a live line that counts
                // the notes actually (re)embedded — not every note, most of which an
                // incremental run reuses untouched — with the current file + its chunks.
                let shown = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
                eprintln!("Indexing {}", shown.display());
                let mut progressed = false;
                let mut on_progress = |p: b2_core::ingest::ReindexProgress| {
                    progressed = true;
                    // \x1b[K clears any tail of a previous, longer line (paths vary in
                    // length); safe here because this branch only runs on a real terminal.
                    eprint!(
                        "\r  embedding {}/{} · {} ({} chunk{})\x1b[K",
                        p.notes_embedded,
                        p.notes_to_embed,
                        p.note_path,
                        p.note_chunks,
                        if p.note_chunks == 1 { "" } else { "s" },
                    );
                    let _ = std::io::stderr().flush();
                    // Stop after this batch if Ctrl-C was pressed (async-indexing.md §3/§8),
                    // else carry on. The batch is already written above, so a cancel here
                    // never tears a write.
                    cancel_flow()
                };
                let report = vault.reindex_with_progress(*force, &mut on_progress)?;
                if progressed {
                    eprintln!(); // close the progress line
                }
                report
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Indexed {} notes ({} embedded, {} stamped{}) and {} resources{}",
                    report.indexed,
                    report.embedded,
                    report.stamped,
                    if report.notes_pruned > 0 {
                        format!(", {} pruned", report.notes_pruned)
                    } else {
                        String::new()
                    },
                    report.resources_indexed,
                    if report.resources_pruned > 0 {
                        format!(" ({} pruned)", report.resources_pruned)
                    } else {
                        String::new()
                    }
                );
                // One unreadable file no longer aborts the reindex — it is skipped and
                // named here (to stderr, so it never pollutes the machine-readable stdout
                // line above) with a short, file-level reason.
                if !report.skipped.is_empty() {
                    eprintln!("Skipped {} unreadable file(s):", report.skipped.len());
                    for s in &report.skipped {
                        eprintln!("  - {} ({})", s.path, s.reason);
                    }
                }
                // The counts above already report the partial work truthfully; add the
                // one line that tells the user it was interrupted and is safe to resume.
                if report.cancelled {
                    eprintln!(
                        "Cancelled — the index is consistent but only partly embedded. Re-run `b2 reindex` to finish the rest."
                    );
                }
            }
        }
        Command::Status => {
            // Read-only coverage report: how much of the vault is embedded (semantic
            // ranking live vs. keyword-only) and whether a background reindex is in
            // flight — the companion to backgrounding a slow reindex with `b2 reindex &`
            // (async-indexing.md §8). A pure model-free DB read (#26): open with the fake.
            let root = cli.vault_or_cwd();
            let (vault, _semantic) = open_vault(&root, false)?;
            let status = vault.embed_status()?;
            let running = reindex_in_flight(&root);
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "embedded": status.embedded,
                        "total": status.total,
                        "reindex_running": running,
                    }))?
                );
            } else {
                if status.total == 0 {
                    println!("No notes indexed yet. Run `b2 reindex` to build the index.");
                } else if status.embedded == 0 {
                    println!(
                        "Embedded 0/{} notes — keyword-only. Run `b2 reindex` for semantic ranking.",
                        status.total
                    );
                } else if status.embedded < status.total {
                    println!(
                        "Embedded {}/{} notes — semantic ranking partial ({} still keyword-only).",
                        status.embedded,
                        status.total,
                        status.total - status.embedded
                    );
                } else {
                    println!(
                        "Embedded {}/{} notes — semantic ranking fully live.",
                        status.embedded, status.total
                    );
                }
                if running {
                    println!("A reindex is currently running.");
                }
            }
        }
        Command::Add {
            path,
            title,
            content,
        } => {
            // Add writes a new note (and embeds its body) → require an explicit vault
            // (no silent cwd), and it needs the real model like `reindex`/`mv`/`link`.
            let (vault, _semantic) = open_vault(cli.require_vault(None)?, true)?;
            let report = vault.add_note(path, title.as_deref(), content.as_deref())?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Created {} (b2id {}).", report.path, report.b2id);
            }
        }
        Command::Neighbors { note } => {
            // Neighbors is a pure graph query — it never embeds, so don't require
            // the model (no needless `b2 init` just to explore the graph).
            let (vault, _semantic) = open_vault(&cli.vault_or_cwd(), false)?;
            let neighbors = vault.neighbors(note)?;
            // Dangling outbound links (a `[[folder]]` or a typo) that resolve to no
            // note or resource — surfaced, not dropped (GH #12). `--json` keeps its
            // resolved-neighbors array contract; the full structured picture,
            // including these, is `b2 explain --json`.
            let unresolved = vault.unresolved_links(note)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&neighbors)?);
            } else if neighbors.is_empty() && unresolved.is_empty() {
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
                for u in &unresolved {
                    println!(
                        "⚠ {}  [[{}]] — unresolved (no matching note or file)",
                        u.relation, u.target
                    );
                }
            }
        }
        Command::Explain { note } => {
            // Explain is a pure graph read (edges + their explanations), no embed —
            // like `neighbors`, it opens with the fake and needs no `b2 init`.
            let (vault, _semantic) = open_vault(&cli.vault_or_cwd(), false)?;
            // Kind dispatch by the argument's own shape (core's one rule, §9b #8):
            // a resource arg gets the fallback card's view — metadata + backlinks.
            if doc_kind(note) == DocKind::Resource {
                let view = vault.explain_resource(note)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&view)?);
                } else {
                    println!("{} ({}, {} bytes)", view.path, view.class, view.size);
                    if view.backlinks.is_empty() {
                        println!("No backlinks yet.");
                    } else {
                        println!("Backlinks:");
                        for b in &view.backlinks {
                            let name = b.title.as_deref().unwrap_or(&b.path);
                            let mut line = format!("  ← {name} ({})  {}", b.path, b.r#type);
                            if b.embed {
                                line.push_str(" (embed)");
                            }
                            if let Some(c) = &b.caption {
                                line.push_str(&format!(" — \"{c}\""));
                            }
                            println!("{line}");
                        }
                    }
                }
                return Ok(());
            }
            let view = vault.explain(note)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else {
                let name = view.title.as_deref().unwrap_or(&view.path);
                println!("{name} ({})  [b2id {}]", view.path, view.b2id);
                if view.connections.is_empty() && view.unresolved.is_empty() {
                    // Zero connections at all — nothing links to it and it links to
                    // nothing (an orphan; the kernel only surfaces, never archives).
                    println!("No connections yet.");
                } else if !view.connections.is_empty() {
                    println!("Connections:");
                    for c in &view.connections {
                        let arrow = if c.direction == "outbound" {
                            "→"
                        } else {
                            "←"
                        };
                        let target = c.title.as_deref().unwrap_or(&c.path);
                        println!(
                            "  {arrow} {}  {target} ({})  [{}]",
                            c.label, c.path, c.origin
                        );
                        if let Some(why) = &c.explanation {
                            println!("      why: {why}");
                        }
                    }
                    // If nothing points *at* the note, it's an orphan — surfaced, not
                    // acted on (user-stories.md Story 2; files are only touched when asked).
                    if !view.connections.iter().any(|c| c.direction == "inbound") {
                        println!("No inbound links — this note is an orphan.");
                    }
                }
                // Dangling outbound links (a `[[folder]]` or a typo): a note is one
                // `.md` file, so these resolve to nothing — shown as broken rather
                // than silently dropped (GH #12).
                if !view.unresolved.is_empty() {
                    println!("Unresolved links:");
                    for u in &view.unresolved {
                        println!(
                            "  ⚠ {}  [[{}]]  (no matching note or file)  [{}]",
                            u.relation, u.target, u.origin
                        );
                        if let Some(why) = &u.explanation {
                            println!("      why: {why}");
                        }
                    }
                }
            }
        }
        Command::Mv { from, to } => {
            // A move rewrites files (and re-embeds them on re-projection) → require an
            // explicit vault (no silent cwd), and it needs the real model the index was
            // built with, like `reindex`/`add`/`link`.
            let (vault, _semantic) = open_vault(cli.require_vault(None)?, true)?;
            // Kind dispatch (§9b #8): the two arms differ only in the report type
            // (a resource has no b2id to carry); the human line is the same shape.
            let (from_path, to_path, links_rewritten, rewrote_files, json) =
                if doc_kind(from) == DocKind::Resource {
                    let report = vault.move_resource(from, to)?;
                    let json = serde_json::to_string_pretty(&report)?;
                    (
                        report.from,
                        report.to,
                        report.links_rewritten,
                        report.rewrote.len(),
                        json,
                    )
                } else {
                    let report = vault.move_note(from, to)?;
                    let json = serde_json::to_string_pretty(&report)?;
                    (
                        report.from,
                        report.to,
                        report.links_rewritten,
                        report.rewrote.len(),
                        json,
                    )
                };
            if cli.json {
                println!("{json}");
            } else {
                println!("Moved {from_path} → {to_path}");
                if links_rewritten > 0 {
                    println!(
                        "Rewrote {links_rewritten} inbound link(s) across {rewrote_files} file(s)."
                    );
                } else {
                    println!("No inbound links to rewrite.");
                }
            }
        }
        Command::Search { query, limit } => {
            // Search embeds the query for the vector half → it needs the real model.
            let (vault, semantic) = open_vault(&cli.vault_or_cwd(), true)?;
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
        Command::Similar { note, limit } => {
            // Candidate generation reads the *stored* vectors (no query embedding), so
            // like `neighbors` it needs no live model — a prior `reindex` supplies them.
            // Open with the fake; it's a pure, instant local read.
            let (vault, _semantic) = open_vault(&cli.vault_or_cwd(), false)?;
            let results = vault.similar(note, *limit)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else if results.is_empty() {
                println!(
                    "No similar notes. (If you haven't yet, run `b2 init` then `b2 reindex` so similarity is semantic.)"
                );
            } else {
                for r in results.iter() {
                    let name = r.title.as_deref().unwrap_or(&r.path);
                    println!("{:.4}  {name} ({})", r.score, r.path);
                    if !r.evidence.is_empty() {
                        println!("    {}", r.evidence);
                    }
                }
                // Nudge toward the commit step, on stderr so stdout stays pure results.
                eprintln!("Commit one with:  b2 link {note} <note> --type <verb>");
            }
        }
        Command::Link {
            src,
            dst,
            edge_type,
            explanation,
        } => {
            // Link writes the source note's frontmatter and re-projects it → require an
            // explicit vault (no silent cwd), opening with the same real model the index
            // was built with (like `add`/`mv`); a frontmatter-only edit won't re-embed.
            let (vault, _semantic) = open_vault(cli.require_vault(None)?, true)?;
            let report = vault.link(src, dst, edge_type, explanation.as_deref())?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if report.created {
                println!(
                    "Linked {} —{}→ {}. Wrote the relation into the source note's frontmatter.",
                    report.src_path, report.relation, report.dst_path
                );
            } else {
                println!(
                    "Already linked {} —{}→ {}. Nothing changed.",
                    report.src_path, report.relation, report.dst_path
                );
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

/// Path to the single-in-flight advisory lock for `reindex`, under the disposable
/// index dir (`<vault>/.b2/reindex.lock`, gitignored along with the rest of `.b2/`).
fn reindex_lock_path(root: &Path) -> PathBuf {
    root.join(".b2").join("reindex.lock")
}

/// Open (creating if absent) the reindex lock file, ensuring `.b2/` exists first so
/// the lock can be taken *before* the index does on a first-ever reindex. The caller
/// takes the lock with [`File::try_lock`] and holds the returned handle for the run.
fn open_reindex_lock(root: &Path) -> Result<File, CliError> {
    std::fs::create_dir_all(root.join(".b2"))?;
    Ok(OpenOptions::new()
        .create(true)
        // A lock file carries no contents we care about — never truncate it (that's a
        // needless write, and could race a holder). Presence + the advisory lock is all.
        .truncate(false)
        .read(true)
        .write(true)
        .open(reindex_lock_path(root))?)
}

/// Whether a reindex currently holds the lock — a best-effort peek for `b2 status`:
/// open the *existing* lock file and try to take it; a contended lock means a run is
/// in flight. Never creates the file (a read-only command), so a missing lock file
/// simply means no reindex has run; any I/O hiccup degrades to "not running".
fn reindex_in_flight(root: &Path) -> bool {
    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .open(reindex_lock_path(root))
    else {
        return false;
    };
    // If we *can* take it, nobody holds it; `file` drops here and releases at once.
    matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
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
        CliError::Core(b2_core::Error::MoveTargetExists(p)) => format!(
            "Can't move: a file already exists at '{p}'. Choose a different destination."
        ),
        CliError::Core(b2_core::Error::MoveDestination(_)) => {
            "That move destination isn't valid. Give a vault-relative path like `notes/new-name.md`.".to_string()
        }
        CliError::Core(b2_core::Error::AddTargetExists(p)) => format!(
            "A note already exists at '{p}'. Choose a different path, or edit that note."
        ),
        CliError::Core(b2_core::Error::AddDestination(_)) => {
            "That note path isn't valid. Give a vault-relative path like `notes/new-name.md`.".to_string()
        }
        CliError::Core(b2_core::Error::InvalidRelation(v)) => format!(
            "'{v}' isn't a known relation type. Use one of: references, relates, elaborates, supports, refutes, contradicts, example-of, part-of, supersedes, derived-from."
        ),
        CliError::Core(b2_core::Error::WriteConflict(_)) => {
            "This note changed on disk since it was opened. Reload the note, then reapply your edit.".to_string()
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
            "A reindex is already running on this vault. Wait for it to finish (check `b2 status`), or stop the other run.".to_string()
        }
        _ => "Something went wrong. Please check the vault path and try again.".to_string(),
    };
    if std::env::var_os("B2_DEBUG").is_some() {
        let detail = match err {
            CliError::Core(e) => e.to_string(),
            CliError::Embed(e) => e.to_string(),
            CliError::Serde(e) => e.to_string(),
            CliError::Io(e) => e.to_string(),
            CliError::VaultRequired => err.to_string(),
            CliError::ReindexRunning => err.to_string(),
        };
        format!("{msg}\n(debug: {detail})")
    } else {
        msg
    }
}
