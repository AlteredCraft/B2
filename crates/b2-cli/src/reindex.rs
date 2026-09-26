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
    // Global, per-machine setup — no vault involved.
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
        return emit(cli.json, &vault.plan_reindex(force)?, print_reindex_plan);
    }
    // Single-in-flight: take an advisory lock *before* the slow model load, so a second
    // `b2 reindex` refuses cleanly instead of two processes writing the same index.
    // Advisory, not a PID file: the OS frees it the instant the holder exits (crash, kill
    // or Ctrl-C included), so nothing stale is left behind. Held until this fn ends.
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
    // applies — nothing is written yet, so a hard stop there is safe.)
    install_cancel_on_sigint(false);
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
    emit(cli.json, &report, print_reindex_report)
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

pub fn cmd_status(cli: &Cli) -> Result<(), CliError> {
    // Read-only coverage report: how much of the vault is embedded (semantic
    // ranking live vs. keyword-only) and whether a background reindex is in
    // flight — the companion to backgrounding a slow reindex with `b2 reindex &`.
    // A pure model-free DB read (#26): open with the fake.
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
        // Name the process, not just the fact: `--cancel` is the supported stop,
        // and the pid keeps a plain `kill -INT` as the documented fallback.
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
    /// The running process's id — `null` when nothing is running (and on the sliver
    /// of a moment before a fresh holder stamps it).
    reindex_pid: Option<u32>,
}

/// `b2 reindex --cancel`'s result. `signalled`, not `cancelled`: the request landed;
/// the run stops at its next batch boundary and reports the partial work itself (honest
/// tense, like the dry-run's `would_*` keys).
#[derive(Debug, Serialize)]
struct CancelView {
    signalled: bool,
    pid: u32,
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
/// held**, which is what makes truncate-then-write safe (`open_reindex_lock` deliberately
/// does not truncate, since that would race a holder).
///
/// The pid outlives the run, harmlessly: [`reindex_holder`] reads it *only* when the lock
/// is contended, so the **lock**, never the file's contents, decides whether a run is in
/// flight.
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

/// `b2 reindex --cancel` (GH #55): stop a reindex on this vault by signalling the pid its
/// lock names. It adds **no cancellation machinery** — SIGINT is exactly what Ctrl-C
/// delivers, so the holder takes the shipped path to a consistent, re-runnable partial
/// index. That is the point: a run backgrounded with `b2 reindex &` has no controlling
/// terminal for Ctrl-C to reach.
///
/// One window is inherent and accepted: a holder still loading the model hasn't installed
/// the handler yet, so SIGINT terminates it outright — the same as Ctrl-C there today, and
/// safe for the same reason (nothing is written until embedding starts).
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
