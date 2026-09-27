//! Building the index: `init` (the model it embeds with), `reindex` with its lock and
//! cross-process cancel, and `status`.

use crate::args::Cli;
use crate::cancel::{cancel_flow, install_cancel_on_sigint};
use crate::emit;
use crate::error::CliError;
use crate::wiring::open_vault;
use b2_embed::{provision, EmbedConfig};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub fn cmd_init(json: bool) -> Result<(), CliError> {
    // Per-machine setup: no vault involved.
    let config = EmbedConfig::load()?;
    let report = provision(&config, |line| eprintln!("{line}"))?;
    emit(json, &report, |report| {
        if report.already_present {
            println!("Model '{}' is already installed.", report.model);
        } else {
            println!(
                "Installed '{}' ({} dims). Run `b2 reindex` to embed your vault.",
                report.model, report.dim
            );
        }
    })
}

pub fn cmd_reindex(
    cli: &Cli,
    vault: Option<&Path>,
    force: bool,
    dry_run: bool,
    cancel: bool,
) -> Result<(), CliError> {
    // Writes, so an explicit vault is required.
    let root = cli.require_vault(vault)?;
    if cancel {
        // Signals another process; never opens the vault.
        return cancel_reindex(root, cli.json);
    }
    if dry_run {
        // A pure read: no model and no progress line.
        let vault = open_vault(root, false)?;
        return emit(cli.json, &vault.plan_reindex(force)?, print_reindex_plan);
    }
    // Single-in-flight, taken before the slow model load. An advisory lock, not a PID
    // file: the OS frees it when the holder exits, however it exits.
    let lock = open_reindex_lock(root)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Err(CliError::ReindexRunning),
        Err(std::fs::TryLockError::Error(e)) => return Err(CliError::Io(e)),
    }
    // Stamp our pid for `--cancel` and `b2 status` (GH #55). Best-effort: a failed write
    // costs the cancel affordance, not the reindex.
    let _ = record_reindex_pid(&lock);
    let vault = open_vault(root, true)?;
    // Only after the model load: until then nothing is written, so the default SIGINT
    // is safe.
    install_cancel_on_sigint(false);
    // A live progress line, only on an interactive stderr and never in --json.
    let report = if cli.json || !std::io::stderr().is_terminal() {
        vault.reindex_with_progress(force, &mut |_| cancel_flow())?
    } else {
        // Counts the notes actually (re)embedded, not every note.
        let shown = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        eprintln!("Indexing {}", shown.display());
        let mut progressed = false;
        let mut on_progress = |p: b2_core::ingest::ReindexProgress| {
            progressed = true;
            // \x1b[K clears the tail of a longer previous line; this is a real terminal.
            eprint!(
                "\r  embedding {}/{} · {} ({} chunk{})\x1b[K",
                p.notes_embedded,
                p.notes_to_embed,
                p.note_path,
                p.note_chunks,
                if p.note_chunks == 1 { "" } else { "s" },
            );
            let _ = std::io::stderr().flush();
            // The batch is already written, so a cancel here never tears a write.
            cancel_flow()
        };
        let report = vault.reindex_with_progress(force, &mut on_progress)?;
        if progressed {
            eprintln!();
        }
        report
    };
    emit(cli.json, &report, print_reindex_report)
}

/// The human-readable `reindex --dry-run` preview.
fn print_reindex_plan(plan: &b2_core::vault::ReindexPlan) {
    println!(
        "Dry run: would index {} note(s) — {} to embed. Nothing in your vault is written either way.",
        plan.would_index, plan.would_embed
    );
}

/// The human-readable reindex summary: one stdout line, then stderr notices.
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
    if !report.skipped.is_empty() {
        eprintln!("Skipped {} unreadable file(s):", report.skipped.len());
        for s in &report.skipped {
            eprintln!("  - {} ({})", s.path, s.reason);
        }
    }
    if report.cancelled {
        eprintln!(
            "Cancelled — the index is consistent but only partly embedded. Re-run `b2 reindex` to finish the rest."
        );
    }
}

pub fn cmd_status(cli: &Cli) -> Result<(), CliError> {
    // A model-free read (GH #26): embedding coverage, and whether a reindex is running.
    let root = cli.vault_or_cwd();
    let vault = open_vault(root, false)?;
    let status = vault.embed_status()?;
    let holder = reindex_holder(root);
    let view = StatusView {
        embedded: status.embedded,
        total: status.total,
        reindex_running: holder.is_some(),
        reindex_pid: holder.and_then(|h| h.pid),
    };
    emit(cli.json, &view, |status| {
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
        // The pid keeps `kill -INT` as the documented fallback to `--cancel`.
        match (status.reindex_running, status.reindex_pid) {
            (true, Some(pid)) => println!(
                "A reindex is currently running (pid {pid}). Stop it with `b2 reindex --cancel` (or `kill -INT {pid}`)."
            ),
            (true, None) => {
                println!("A reindex is currently running. Stop it with `b2 reindex --cancel`.")
            }
            (false, _) => {}
        }
    })
}

/// `b2 status`: embedding coverage, and whether a reindex holds the lock. The `--json`
/// keys are a contract agents read (`tests/cli.rs` pins them).
#[derive(Debug, Serialize)]
struct StatusView {
    embedded: usize,
    total: usize,
    reindex_running: bool,
    /// `null` when nothing is running, or before a fresh holder stamps it.
    reindex_pid: Option<u32>,
}

/// `b2 reindex --cancel`'s result. `signalled`, not `cancelled`: the run stops at its next
/// batch boundary and reports the partial work itself.
#[derive(Debug, Serialize)]
struct CancelView {
    signalled: bool,
    pid: u32,
}

/// `<vault>/.b2/reindex.lock`, the single-in-flight advisory lock.
fn reindex_lock_path(root: &Path) -> PathBuf {
    root.join(".b2").join("reindex.lock")
}

/// Open or create the reindex lock file, creating `.b2/` so the lock precedes the index.
fn open_reindex_lock(root: &Path) -> Result<File, CliError> {
    std::fs::create_dir_all(root.join(".b2"))?;
    Ok(OpenOptions::new()
        .create(true)
        // Truncating here could race a holder.
        .truncate(false)
        .read(true)
        .write(true)
        .open(reindex_lock_path(root))?)
}

/// Stamp this process's id into the reindex lock. Only called with the lock held, which
/// makes truncate-then-write safe. The stale pid left after the run is harmless: the lock,
/// not the contents, decides whether a run is in flight.
fn record_reindex_pid(lock: &File) -> std::io::Result<()> {
    let mut handle = lock;
    handle.set_len(0)?;
    handle.seek(SeekFrom::Start(0))?;
    writeln!(handle, "{}", std::process::id())?;
    handle.flush()
}

/// A reindex holding the lock, seen from another process.
#[derive(Debug)]
struct ReindexHolder {
    /// `None` when the lock is held but no readable pid is there yet (or ever).
    pid: Option<u32>,
}

/// Peek at the reindex lock for `b2 status` and `--cancel`: a contended lock means a run
/// is in flight. Never creates the file; any I/O hiccup degrades to "not running".
fn reindex_holder(root: &Path) -> Option<ReindexHolder> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(reindex_lock_path(root))
        .ok()?;
    // If we can take it, nobody holds it; dropping `file` releases it.
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

/// `b2 reindex --cancel` (GH #55): send SIGINT to the pid the lock names, so a backgrounded
/// run takes the Ctrl-C path. A holder still loading the model dies outright, which is safe:
/// nothing is written until embedding starts.
fn cancel_reindex(root: &Path, json: bool) -> Result<(), CliError> {
    let Some(holder) = reindex_holder(root) else {
        return Err(CliError::NoReindexRunning);
    };
    let Some(pid) = holder.pid else {
        return Err(CliError::ReindexPidUnknown);
    };
    signal_reindex(pid)?;
    let view = CancelView {
        signalled: true,
        pid,
    };
    emit(json, &view, |view| {
        println!(
            "Cancelling the reindex on this vault (pid {}). It stops after the current batch, leaving a consistent index — re-run `b2 reindex` to finish.",
            view.pid
        );
    })
}

/// SIGINT, the signal Ctrl-C raises, so there is one cancel path.
fn signal_reindex(pid: u32) -> Result<(), CliError> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    // A pid that doesn't fit `pid_t` was never ours: don't signal a truncation.
    let raw = i32::try_from(pid).map_err(|_| CliError::NoReindexRunning)?;
    match kill(Pid::from_raw(raw), Signal::SIGINT) {
        Ok(()) => Ok(()),
        // The holder exited between the peek and the signal.
        Err(nix::errno::Errno::ESRCH) => Err(CliError::NoReindexRunning),
        Err(e) => Err(CliError::Io(std::io::Error::from_raw_os_error(e as i32))),
    }
}
