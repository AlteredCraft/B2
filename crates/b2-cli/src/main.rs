//! `b2` — the first adapter over the `b2-core` typed API (invariants.md,
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
use std::io::{IsTerminal, Read, Seek, SeekFrom, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the SIGINT handler installed for a `reindex` — by Ctrl-C in the foreground,
/// or by another process's `b2 reindex --cancel` (GH #55), which raises the *same*
/// signal so both reach here. The embed loop reads it at each batch boundary (through
/// the [`ControlFlow`] the progress closure returns — the shipped cancel seam) and
/// stops *after* the current batch: a consistent, re-runnable partial index, never a
/// torn write (index-engine.md).
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
    /// fall back to the current dir; commands that write (`reindex`/`add`/`mv`/`rm`/
    /// `link`) require it explicitly, so they can never silently touch the wrong directory.
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
    /// Re-project every note under the vault into the index. Reads your vault and
    /// writes only to `.b2/`. Incremental by default: notes whose content is
    /// unchanged keep their vectors.
    Reindex {
        /// Vault root (overrides --vault / $B2_VAULT_PATH). Required — no cwd default.
        vault: Option<PathBuf>,
        /// Re-embed every note, even unchanged ones (a full rebuild in place).
        #[arg(long)]
        force: bool,
        /// Preview what a reindex would do without touching the index — how many
        /// notes it would project and embed.
        #[arg(long)]
        dry_run: bool,
        /// Stop a reindex already running on this vault — the way to reach one
        /// backgrounded with `b2 reindex &`, which has no controlling terminal for
        /// Ctrl-C. It stops after the current batch, leaving the same consistent,
        /// re-runnable partial index a foreground Ctrl-C does.
        #[arg(long, conflicts_with_all = ["force", "dry_run"])]
        cancel: bool,
    },
    /// Report embedding coverage — how many notes are embedded (so semantic ranking
    /// is live vs. keyword-only) and whether a reindex is currently running, with the
    /// process id to stop it by. A pure, model-free read; handy after kicking off a
    /// slow reindex in the background with `b2 reindex &`.
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
    /// Replace a note's **body** with Markdown piped on stdin — the CLI/agent editing
    /// surface (the counterpart to the desktop editor's save). NOTE is a vault-relative
    /// path; the note must already exist (`b2 add` creates one). Pipe the body
    /// **alone** — the frontmatter (including B2's managed `b2_relations:`) is
    /// left untouched, so no `---` block. The note is re-projected (chunks + FTS + graph)
    /// so the index stays live; a later `b2 reindex` fills the changed body's vectors.
    Write {
        /// The note whose body to overwrite: a vault-relative path.
        note: String,
    },
    /// Show a note's typed neighbors. NOTE is a vault-relative path.
    Neighbors { note: String },
    /// Explain a note's connections — every typed edge and its "why". NOTE is a
    /// vault-relative path.
    Explain { note: String },
    /// Move/rename a note, file, or folder and rewrite every inbound link to
    /// point at the new path. An existing directory moves as a whole folder
    /// (everything under it, one rename).
    Mv {
        /// What to move: a vault-relative path (note, file, or folder).
        from: String,
        /// The new vault-relative path (for a note, the `.md` extension is optional).
        to: String,
    },
    /// Delete a note, file, or folder from the vault *and* the disk. Inbound links
    /// are never rewritten — they dangle and surface as unresolved (`b2 explain`).
    /// An existing directory deletes as a whole folder and requires --recursive.
    Rm {
        /// What to delete: a vault-relative path (note, file, or folder).
        target: String,
        /// Required to delete a folder (and everything inside it, unindexed files too).
        #[arg(short = 'r', long)]
        recursive: bool,
    },
    /// Hybrid keyword+semantic+graph search across the vault.
    Search {
        query: String,
        /// Maximum number of notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Surface the notes most semantically similar to NOTE that you haven't linked
    /// yet — connection discovery. NOTE is a vault-relative path. A local
    /// read over stored vectors (run `b2 reindex` with the real model first).
    Similar {
        /// The note to find similar notes for: a vault-relative path.
        note: String,
        /// Maximum number of similar notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Show the raw nearest notes without the discovery quality floor —
        /// every candidate up to --limit, however weak.
        #[arg(long)]
        no_floor: bool,
    },
    /// Commit a typed connection SRC → DST into SRC's frontmatter `b2_relations:`.
    /// SRC and DST are each a vault-relative path.
    Link {
        /// The source note (the edge points *from* it): a vault-relative path.
        src: String,
        /// The target note (the edge points *to* it): a vault-relative path.
        dst: String,
        /// The relation verb (a core verb: references/supports/contradicts).
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
    fn vault_or_cwd(&self) -> &Path {
        self.vault.as_deref().unwrap_or_else(|| Path::new("."))
    }

    /// The vault root for commands that **write** to the vault (`reindex`, `add`, `mv`,
    /// `rm`, `link`): `positional` (only `reindex` has one) wins, then `-C`/`$B2_VAULT_PATH`;
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
    match log_file {
        Some(p) => match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::path::Path::new(&p))
        {
            Ok(file) => builder.with_writer(std::sync::Mutex::new(file)).init(),
            Err(e) => {
                eprintln!(
                    "warning: cannot open B2_LOG_FILE '{}' ({e}); logging to stderr",
                    p.to_string_lossy()
                );
                builder.with_writer(std::io::stderr).init();
            }
        },
        None => builder.with_writer(std::io::stderr).init(),
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
        Command::Search { query, limit } => cmd_search(cli, query, *limit),
        Command::Similar {
            note,
            limit,
            no_floor,
        } => cmd_similar(cli, note, *limit, *no_floor),
        Command::Link {
            src,
            dst,
            edge_type,
            explanation,
        } => cmd_link(cli, src, dst, edge_type, explanation.as_deref()),
    }
}

/// Print `value` as pretty JSON on stdout — the one `--json` output path, shared by
/// every subcommand.
fn print_json<T: serde::Serialize + ?Sized>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn cmd_init(json: bool) -> Result<(), CliError> {
    // Global, per-machine setup — no vault involved.
    let config = EmbedConfig::load()?;
    let report = provision(&config, |line| eprintln!("{line}"))?;
    if json {
        print_json(&report)?;
    } else if report.already_present {
        println!("Model '{}' is already installed.", report.model);
    } else {
        println!(
            "Installed '{}' ({} dims). Run `b2 reindex` to embed your vault.",
            report.model, report.dim
        );
    }
    Ok(())
}

fn cmd_reindex(
    cli: &Cli,
    vault: Option<&Path>,
    force: bool,
    dry_run: bool,
    cancel: bool,
) -> Result<(), CliError> {
    // Reindex writes an index → require an explicit vault (positional wins),
    // never a silent cwd fallback. See `Cli::require_vault`.
    let root = cli.require_vault(vault)?;
    if cancel {
        // Signals another process and returns; it never opens the vault (no
        // model load, no index read) — the run being cancelled owns all of that.
        return cancel_reindex(root, cli.json);
    }
    if dry_run {
        // A dry-run neither embeds nor stamps → no model needed (open with
        // the fake, like `neighbors`); it's a pure read, so there's no slow
        // embed phase to show progress for.
        let vault = open_vault(root, false)?;
        let plan = vault.plan_reindex(force)?;
        if cli.json {
            print_json(&plan)?;
        } else {
            print_reindex_plan(&plan);
        }
        return Ok(());
    }
    // Single-in-flight: take an advisory lock *before* the (slow) model load so
    // a second `b2 reindex` — e.g. a foreground run racing one you backgrounded
    // with `b2 reindex &` — refuses cleanly instead of two processes writing the
    // same index. Advisory, not a PID file: the OS frees it the instant the holder
    // exits (crash, kill, or Ctrl-C included), so nothing stale is ever left behind.
    // `lock` is held until this command fn ends; dropping it releases the lock.
    let lock = open_reindex_lock(root)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Err(CliError::ReindexRunning),
        Err(std::fs::TryLockError::Error(e)) => return Err(CliError::Io(e)),
    }
    // Now that the lock is ours, stamp who holds it: the address `b2 reindex
    // --cancel` signals, and what `b2 status` prints so a manual `kill` stays
    // available (GH #55). Best-effort — a failed write costs the cancel
    // affordance, not the reindex.
    let _ = record_reindex_pid(&lock);
    // Reindex embeds every changed chunk → it needs the real model.
    let vault = open_vault(root, true)?;
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
        vault.reindex_with_progress(force, &mut |_| cancel_flow())?
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
            // Stop after this batch if Ctrl-C was pressed,
            // else carry on. The batch is already written above, so a cancel here
            // never tears a write.
            cancel_flow()
        };
        let report = vault.reindex_with_progress(force, &mut on_progress)?;
        if progressed {
            eprintln!(); // close the progress line
        }
        report
    };
    if cli.json {
        print_json(&report)?;
    } else {
        print_reindex_report(&report);
    }
    Ok(())
}

/// The human-readable `reindex --dry-run` preview (the `--json` sibling prints the
/// plan itself).
fn print_reindex_plan(plan: &b2_core::vault::ReindexPlan) {
    println!(
        "Dry run: would index {} note(s) — {} to embed. Nothing in your vault is written either way.",
        plan.would_index, plan.would_embed
    );
}

/// The human-readable reindex summary: the one stdout line, then the stderr
/// notices — skipped files and the cancelled line.
fn print_reindex_report(report: &b2_core::vault::ReindexReport) {
    println!(
        "Indexed {} notes ({} embedded{}) and {} resources{}",
        report.indexed,
        report.embedded,
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

fn cmd_status(cli: &Cli) -> Result<(), CliError> {
    // Read-only coverage report: how much of the vault is embedded (semantic
    // ranking live vs. keyword-only) and whether a background reindex is in
    // flight — the companion to backgrounding a slow reindex with `b2 reindex &`.
    // A pure model-free DB read (#26): open with the fake.
    let root = cli.vault_or_cwd();
    let vault = open_vault(root, false)?;
    let status = vault.embed_status()?;
    let holder = reindex_holder(root);
    if cli.json {
        print_json(&serde_json::json!({
            "embedded": status.embedded,
            "total": status.total,
            "reindex_running": holder.is_some(),
            // The running process's id — `null` when nothing is running (and
            // on the sliver of a moment before a fresh holder stamps it).
            "reindex_pid": holder.as_ref().and_then(|h| h.pid),
        }))?;
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
        // Name the process, not just the fact: `--cancel` is the supported stop,
        // and the pid keeps a plain `kill -INT` as the documented fallback.
        match holder.as_ref().map(|h| h.pid) {
            Some(Some(pid)) => println!(
                "A reindex is currently running (pid {pid}). Stop it with `b2 reindex --cancel` (or `kill -INT {pid}`)."
            ),
            Some(None) => {
                println!("A reindex is currently running. Stop it with `b2 reindex --cancel`.")
            }
            None => {}
        }
    }
    Ok(())
}

fn cmd_add(
    cli: &Cli,
    path: &str,
    title: Option<&str>,
    content: Option<&str>,
) -> Result<(), CliError> {
    // Add writes a new note (and embeds its body) → require an explicit vault
    // (no silent cwd), and it needs the real model like `reindex`/`mv`/`link`.
    let vault = open_vault(cli.require_vault(None)?, true)?;
    let report = vault.add_note(path, title, content)?;
    if cli.json {
        print_json(&report)?;
    } else {
        println!("Created {}.", report.path);
    }
    Ok(())
}

fn cmd_write(cli: &Cli, note: &str) -> Result<(), CliError> {
    // A body splice + re-projection: **model-free** (like the desktop's save and
    // `rm`), still an explicit vault like every write. Refuse an interactive
    // terminal up front so the command never silently hangs waiting for
    // hand-typed input — the new body is always *piped* (an agent, `cat file |`, …).
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(CliError::StdinRequired);
    }
    let vault = open_vault(cli.require_vault(None)?, false)?;
    let mut body = String::new();
    stdin.lock().read_to_string(&mut body)?;
    // Stateless one-shot: read the current on-disk revision and chain the write on
    // it. A CLI holds no long-lived buffer, so there's no external-edit window to
    // guard here — the content-hash guard exists for the desktop's in-memory
    // buffer; for a one-shot, the contract is simply "the body becomes exactly
    // this" (`Vault::write` still keeps the frontmatter bytes untouched).
    let current = vault.read(note)?;
    let report = vault.write(note, &body, &current.revision)?;
    if cli.json {
        print_json(&report)?;
    } else {
        println!("Wrote {} ({} bytes).", report.path, body.len());
    }
    Ok(())
}

fn cmd_neighbors(cli: &Cli, note: &str) -> Result<(), CliError> {
    // Neighbors is a pure graph query — it never embeds, so don't require
    // the model (no needless `b2 init` just to explore the graph).
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    let neighbors = vault.neighbors(note)?;
    // Dangling outbound links (a `[[folder]]` or a typo) that resolve to no
    // note or resource — surfaced, not dropped (GH #12). `--json` keeps its
    // resolved-neighbors array contract; the full structured picture,
    // including these, is `b2 explain --json`.
    let unresolved = vault.unresolved_links(note)?;
    if cli.json {
        print_json(&neighbors)?;
    } else if neighbors.is_empty() && unresolved.is_empty() {
        println!("No neighbors.");
    } else {
        for n in &neighbors {
            let arrow = arrow(&n.direction);
            let name = display_name(n.title.as_deref(), &n.path);
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
    Ok(())
}

fn cmd_explain(cli: &Cli, note: &str) -> Result<(), CliError> {
    // Explain is a pure graph read (edges + their explanations), no embed —
    // like `neighbors`, it opens with the fake and needs no `b2 init`.
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    // Kind dispatch by the argument's own shape (core's one rule, §9b #8):
    // a resource arg gets the fallback card's view — metadata + backlinks.
    if doc_kind(note) == DocKind::Resource {
        let view = vault.explain_resource(note)?;
        if cli.json {
            print_json(&view)?;
        } else {
            println!("{} ({}, {} bytes)", view.path, view.class, view.size);
            if view.backlinks.is_empty() {
                println!("No backlinks yet.");
            } else {
                println!("Backlinks:");
                for b in &view.backlinks {
                    let name = display_name(b.title.as_deref(), &b.path);
                    let mut line = format!("  ← {name} ({})  {}", b.path, b.r#type);
                    decorate(&mut line, b.embed, b.caption.as_deref());
                    println!("{line}");
                }
            }
        }
        return Ok(());
    }
    let view = vault.explain(note)?;
    if cli.json {
        print_json(&view)?;
    } else {
        let name = display_name(view.title.as_deref(), &view.path);
        println!("{name} ({})", view.path);
        if view.connections.is_empty() && view.resources.is_empty() && view.unresolved.is_empty() {
            // Zero connections at all — nothing links to it and it links to
            // nothing (an orphan; the kernel only surfaces, never archives).
            println!("No connections yet.");
        } else if !view.connections.is_empty() {
            println!("Connections:");
            for c in &view.connections {
                let arrow = arrow(&c.direction);
                let target = display_name(c.title.as_deref(), &c.path);
                println!(
                    "  {arrow} {}  {target} ({})  [{}]",
                    c.label, c.path, c.origin
                );
                if let Some(why) = &c.explanation {
                    println!("      why: {why}");
                }
            }
            // If nothing points *at* the note, it's an orphan — surfaced, not
            // acted on (invariants.md; files are only touched when asked).
            if !view.connections.iter().any(|c| c.direction == "inbound") {
                println!("No inbound links — this note is an orphan.");
            }
        }
        // Outbound links at resources (images, PDFs, …) — the third target
        // kind an edge can have, shown from the note's side (GH #22).
        if !view.resources.is_empty() {
            println!("Resource links:");
            for r in &view.resources {
                let mut line = format!(
                    "  → {}  {} ({})  [{}]",
                    r.relation, r.path, r.class, r.origin
                );
                decorate(&mut line, r.embed, r.caption.as_deref());
                println!("{line}");
                if let Some(why) = &r.explanation {
                    println!("      why: {why}");
                }
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
    Ok(())
}

fn cmd_mv(cli: &Cli, from: &str, to: &str) -> Result<(), CliError> {
    // A move rewrites files (and re-embeds them on re-projection) → require an
    // explicit vault (no silent cwd), and it needs the real model the index was
    // built with, like `reindex`/`add`/`link`.
    let root = cli.require_vault(None)?;
    let vault = open_vault(root, true)?;
    // Kind dispatch (§9b #8): an existing directory moves as a folder
    // (every file under it, one rename); otherwise the two file arms
    // differ only in the report type they print.
    // The human "Moved" line differs per arm, the rewrite tally is shared.
    if is_dir_arg(root, from) {
        let report = vault.move_dir(from, to)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!(
                "Moved {}/ → {}/ ({} note(s), {} file(s))",
                report.from, report.to, report.moved_notes, report.moved_resources
            );
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        }
    } else if doc_kind(from) == DocKind::Resource {
        let report = vault.move_resource(from, to)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Moved {} → {}", report.from, report.to);
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        }
    } else {
        let report = vault.move_note(from, to)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Moved {} → {}", report.from, report.to);
            print_rewrite_tally(report.links_rewritten, report.rewrote.len());
        }
    }
    Ok(())
}

fn cmd_rm(cli: &Cli, target: &str, recursive: bool) -> Result<(), CliError> {
    // A delete removes files and index rows but never rewrites a body
    // (inbound links dangle, they aren't repaired) → **model-free**, like
    // the desktop's delete; still an explicit vault, like every write.
    let root = cli.require_vault(None)?;
    let vault = open_vault(root, false)?;
    // Kind dispatch (§9b #8), mirroring `mv`: an existing directory deletes
    // as a folder — gated on --recursive, the CLI's stand-in for the
    // desktop's confirm dialog — else the extension picks the file arm.
    if is_dir_arg(root, target) {
        if !recursive {
            return Err(CliError::RecursiveRequired(target.to_string()));
        }
        let report = vault.delete_dir(target)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!(
                "Deleted {}/ ({} note(s), {} file(s))",
                report.dir, report.deleted_notes, report.deleted_resources
            );
            print_dangled(&report.dangled);
        }
    } else if doc_kind(target) == DocKind::Resource {
        let report = vault.delete_resource(target)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Deleted {}", report.path);
            print_dangled(&report.dangled);
        }
    } else {
        let report = vault.delete_note(target)?;
        if cli.json {
            print_json(&report)?;
        } else {
            println!("Deleted {}", report.path);
            print_dangled(&report.dangled);
        }
    }
    Ok(())
}

fn cmd_search(cli: &Cli, query: &str, limit: usize) -> Result<(), CliError> {
    // Search embeds the query for the vector half → it needs the real model.
    let vault = open_vault(cli.vault_or_cwd(), true)?;
    let results = vault.search(query, limit)?;
    if cli.json {
        print_json(&results)?;
    } else {
        if results.is_empty() {
            println!("No results.");
        } else {
            for r in &results {
                let name = display_name(r.title.as_deref(), &r.path);
                println!("{:.4}  {name} ({})", r.score, r.path);
                if !r.snippet.is_empty() {
                    println!("    {}", r.snippet);
                }
            }
        }
        // Honesty (never overstate): with the fake embedder the vector half
        // isn't semantic. Under the real model it is, so no caveat. Kept on
        // stderr so stdout stays pure results.
        if use_fake_embedder() {
            eprintln!(
                "note: keyword (BM25) ranking is live; semantic ranking is off (fake embedder)."
            );
        }
    }
    Ok(())
}

fn cmd_similar(cli: &Cli, note: &str, limit: usize, no_floor: bool) -> Result<(), CliError> {
    // Candidate generation reads the *stored* vectors (no query embedding), so
    // like `neighbors` it needs no live model — a prior `reindex` supplies them.
    // Open with the fake; it's a pure, instant local read. (The discovery floor
    // keys on the vault's RECORDED model id, so opening with the fake for reading
    // never disables it — only --no-floor, the explicit raw-nearest ask, does.)
    let vault = open_vault(cli.vault_or_cwd(), false)?;
    let results = if no_floor {
        vault.similar_raw(note, limit)?
    } else {
        vault.similar(note, limit)?
    };
    if cli.json {
        print_json(&results)?;
    } else if results.is_empty() && no_floor {
        // Raw mode: no floor could have filtered anything, so an empty list means
        // there was nothing to compare at all — and suggesting --no-floor here
        // would recommend the flag already in effect.
        println!("Nothing unlinked has stored vectors to compare.");
    } else if results.is_empty() {
        // Two honest empty states (GH #150): a vault with vectors that has nothing
        // strong enough to show, vs a vault whose similarity isn't semantic yet.
        let status = vault.embed_status()?;
        if status.embedded > 0 {
            println!(
                "No strong candidates — nothing unlinked stands out as related to this note. \
                 (`--no-floor` shows the raw nearest anyway.)"
            );
        } else {
            println!(
                "No similar notes. (If you haven't yet, run `b2 init` then `b2 reindex` so similarity is semantic.)"
            );
        }
    } else {
        for r in &results {
            let name = display_name(r.title.as_deref(), &r.path);
            println!("{:.4}  {name} ({})", r.score, r.path);
            if !r.evidence.is_empty() {
                println!("    {}", r.evidence);
            }
        }
        // Nudge toward the commit step, on stderr so stdout stays pure results.
        eprintln!("Commit one with:  b2 link {note} <note> --type <verb>");
    }
    Ok(())
}

fn cmd_link(
    cli: &Cli,
    src: &str,
    dst: &str,
    edge_type: &str,
    explanation: Option<&str>,
) -> Result<(), CliError> {
    // Link writes the source note's frontmatter and re-projects it → require an
    // explicit vault (no silent cwd), opening with the same real model the index
    // was built with (like `add`/`mv`); a frontmatter-only edit won't re-embed.
    let vault = open_vault(cli.require_vault(None)?, true)?;
    let report = vault.link(src, dst, edge_type, explanation)?;
    if cli.json {
        print_json(&report)?;
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
    Ok(())
}

/// Open a vault with the appropriate embedder.
///
/// `needs_semantic` commands (`reindex`, `search`) load the real [`LocalEmbedder`]
/// from the shared cache and **fail fast** with "run `b2 init`" if it's absent.
/// Pure-graph commands pass `false` and use the fake — no model required just to
/// explore the graph. `B2_EMBEDDER=fake` forces the fake everywhere (offline/dev
/// mode, and what the test suite runs under).
fn open_vault(root: &Path, needs_semantic: bool) -> Result<Vault, CliError> {
    if needs_semantic && !use_fake_embedder() {
        let config = EmbedConfig::load()?;
        let embedder = LocalEmbedder::load(&config)?;
        Ok(Vault::open_with_embedder(
            root,
            Box::new(embedder) as Box<dyn Embedder>,
        )?)
    } else {
        Ok(Vault::open(root)?)
    }
}

fn use_fake_embedder() -> bool {
    std::env::var_os("B2_EMBEDDER").is_some_and(|v| v == "fake")
}

/// The presentation rule for naming a note: its title when it has one, else its
/// vault-relative path.
fn display_name<'a>(title: Option<&'a str>, path: &'a str) -> &'a str {
    title.unwrap_or(path)
}

/// The direction glyph: `→` for an outbound edge (this note → other), `←` inbound.
fn arrow(direction: &str) -> &'static str {
    if direction == "outbound" {
        "→"
    } else {
        "←"
    }
}

/// Append a resource line's decorations: the `(embed)` marker and the quoted caption.
fn decorate(line: &mut String, embed: bool, caption: Option<&str>) {
    use std::fmt::Write as _;
    if embed {
        line.push_str(" (embed)");
    }
    if let Some(c) = caption {
        let _ = write!(line, " — \"{c}\"");
    }
}

/// Whether a `mv`/`rm` argument names an existing directory under `root` — the
/// kind-dispatch test that routes to the folder arm (a trailing `/` is tolerated).
fn is_dir_arg(root: &Path, arg: &str) -> bool {
    root.join(arg.trim_end_matches('/')).is_dir()
}

/// The `mv` tally, shared by its three arms: how many inbound link targets were
/// rewritten across how many files — or that nothing linked to the moved item.
fn print_rewrite_tally(links_rewritten: usize, rewrote_files: usize) {
    if links_rewritten > 0 {
        println!("Rewrote {links_rewritten} inbound link(s) across {rewrote_files} file(s).");
    } else {
        println!("No inbound links to rewrite.");
    }
}

/// The `rm` tally, shared by its three arms: which surviving files' links now
/// dangle — or that none were affected.
fn print_dangled(dangled: &[String]) {
    if dangled.is_empty() {
        println!("No inbound links affected.");
    } else {
        println!(
            "Links in {} file(s) now unresolved: {}",
            dangled.len(),
            dangled.join(", ")
        );
    }
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

/// Stamp this process's id into the reindex lock — **only ever called with the lock
/// held**, which is what makes truncate-then-write safe here (we are its sole writer;
/// `open_reindex_lock` deliberately doesn't truncate, since that would race a holder).
///
/// The pid outlives the run — nothing clears it on exit — and that is harmless by
/// construction: [`reindex_holder`] reads it *only* when the lock is contended, so the
/// **lock**, never the file's contents, decides whether a run is in flight. A leftover
/// pid from a finished run is therefore never mistaken for a live one.
fn record_reindex_pid(lock: &File) -> std::io::Result<()> {
    let mut handle = lock;
    handle.set_len(0)?;
    handle.seek(SeekFrom::Start(0))?;
    // A trailing newline so the file reads sanely under `cat`; parsing trims it.
    writeln!(handle, "{}", std::process::id())?;
    handle.flush()
}

/// A reindex holding the lock, seen from another process.
#[derive(Debug)]
struct ReindexHolder {
    /// The pid it stamped into the lock. `None` when the lock is held but no readable
    /// pid is there — a holder that took the lock microseconds ago and hasn't written
    /// yet, an older `b2` that stamped none, or an unreadable file. "Running" is the
    /// lock's answer; the pid is the extra affordance that may be missing.
    pid: Option<u32>,
}

/// Peek at the reindex lock — the best-effort read behind `b2 status` and `b2 reindex
/// --cancel`: open the *existing* lock file and try to take it; a contended lock means
/// a run is in flight, and its pid is read from the file we could not lock. Never
/// creates the file (both callers are read-only on the vault), so a missing lock file
/// simply means no reindex has run; any I/O hiccup degrades to "not running".
fn reindex_holder(root: &Path) -> Option<ReindexHolder> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(reindex_lock_path(root))
        .ok()?;
    // If we *can* take it, nobody holds it; `file` drops here and releases at once.
    if !matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)) {
        return None;
    }
    let mut buf = String::new();
    let pid = file
        .read_to_string(&mut buf)
        .ok()
        .and_then(|_| buf.trim().parse().ok());
    Some(ReindexHolder { pid })
}

/// `b2 reindex --cancel` (GH #55): stop a reindex running on this vault by signalling
/// the pid its lock names. It adds **no cancellation machinery** — SIGINT is exactly
/// what Ctrl-C delivers, so the holder takes the shipped path (the handler installed in
/// the `reindex` arm → the [`CANCEL`] flag → [`ControlFlow::Break`] at the next batch
/// boundary → a consistent, re-runnable partial index). That's the whole point: a run
/// backgrounded with `b2 reindex &` has no controlling terminal for Ctrl-C to reach.
///
/// One window is inherent and accepted: a holder still loading the model hasn't
/// installed the handler yet, so SIGINT terminates it outright — the same as Ctrl-C
/// there today, and safe for the same reason (nothing is written until embedding starts).
fn cancel_reindex(root: &Path, json: bool) -> Result<(), CliError> {
    let Some(holder) = reindex_holder(root) else {
        return Err(CliError::NoReindexRunning);
    };
    let Some(pid) = holder.pid else {
        return Err(CliError::ReindexPidUnknown);
    };
    signal_reindex(pid)?;
    if json {
        // `signalled`, not `cancelled`: the request landed; the run stops at its next
        // batch boundary and reports the partial work itself (honest tense, like the
        // dry-run's `would_*` keys).
        print_json(&serde_json::json!({
            "signalled": true,
            "pid": pid,
        }))?;
    } else {
        println!(
            "Cancelling the reindex on this vault (pid {pid}). It stops after the current batch, leaving a consistent index — re-run `b2 reindex` to finish."
        );
    }
    Ok(())
}

/// Send the cancel signal to a reindex holder. SIGINT, deliberately: the identical
/// signal a foreground Ctrl-C raises, so there is one cancel path, not two.
fn signal_reindex(pid: u32) -> Result<(), CliError> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    // A pid that doesn't fit `pid_t` was never one we wrote — treat a garbled lock as
    // nothing to cancel rather than signalling whatever the truncation would name.
    let raw = i32::try_from(pid).map_err(|_| CliError::NoReindexRunning)?;
    match kill(Pid::from_raw(raw), Signal::SIGINT) {
        Ok(()) => Ok(()),
        // The holder exited between the lock peek and the signal: there is nothing left
        // to cancel — the outcome the user wanted, reported as such rather than as I/O.
        Err(nix::errno::Errno::ESRCH) => Err(CliError::NoReindexRunning),
        Err(e) => Err(CliError::Io(std::io::Error::from_raw_os_error(e as i32))),
    }
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

/// Translate an internal error into a generic, actionable, user-facing message —
/// never leaking sqlite/io/serde internals. Set `B2_DEBUG` to also print the detail.
fn user_message(err: &CliError) -> String {
    let msg = match err {
        CliError::Core(b2_core::Error::NoteNotFound(r)) => format!(
            "Note not found: '{r}'. Check the path, and run `b2 reindex` first."
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
