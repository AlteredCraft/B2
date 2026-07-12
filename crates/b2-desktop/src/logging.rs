//! Opt-in structured debug logging for the desktop host — the **GUI mirror** of the
//! CLI's `init_logging` (`b2-cli/src/main.rs`). Same knobs, same on-the-wire shape, so
//! `b2` and `b2-desktop` write into **one reportable JSONL dataset**: the kernel's
//! `tracing` events (per-statement SQLite timings `b2::sqlite`, façade-op spans
//! `b2::vault`, flow milestones `b2::search`/`b2::ingest`) as JSON Lines, one flat
//! object per line for jq/DuckDB/pandas.
//!
//! Installing the subscriber is legitimate **host** work, not engine logic: the core
//! only *emits*; the subscriber and its wall-clock live in the adapter, keeping
//! `b2-core` clock-free (root `CLAUDE.md`, "The core only emits").
//!
//! Knobs (as the CLI): the sink is stderr by default; `B2_LOG_FILE=<path>` writes there
//! in **append** mode instead. `B2_LOG` is a tracing filter directive (`debug`,
//! `b2::sqlite=debug`, `warn`, …), honored verbatim. With none of `B2_LOG` / `B2_DEBUG`
//! / `B2_LOG_FILE` set, no subscriber is installed and the kernel's instrumentation
//! stays inert. Relative `B2_LOG_FILE` paths resolve against the process CWD — under
//! `just app` (`cargo tauri dev`) that is `crates/b2-desktop/`, not the repo root;
//! prefer an absolute path.
//!
//! **One knob differs from the CLI, deliberately:** when `B2_DEBUG`/`B2_LOG_FILE` imply
//! a default filter (no explicit `B2_LOG`), the desktop scopes it to **`b2=debug`**, not
//! the CLI's bare `debug`. The CLI process is quiet — clap/rusqlite emit no `tracing`, so
//! bare `debug` is already just the kernel's `b2::*` events. This process embeds Tauri +
//! wry + hyper + reqwest, all noisy `tracing` emitters, and their records have a foreign
//! shape (`log.line`, `log.module_path`, no `sql`/`duration_us`) that would pollute the
//! one reportable JSONL dataset. `b2=debug` keeps the *default* file byte-compatible with
//! the CLI's; opt into the firehose with an explicit `B2_LOG=debug`.
//!
//! **One difference from the CLI, deliberate:** the CLI is a short-lived, single-shot
//! process, so it writes through a plain `Mutex<File>`. The desktop app is long-lived
//! and multi-threaded — the background embed pass alone can emit a burst of `b2::sqlite`
//! events off the UI thread — so blocking those threads on file I/O would both stutter
//! the GUI and pollute the throughput we capture these logs to measure. Here the sink is
//! a `tracing-appender` **non-blocking** writer: events cross a channel to one dedicated
//! writer thread. Its [`WorkerGuard`] flushes the channel on drop, so `main` must hold
//! the returned guard for the whole run (dropping it early silently stops logging).

use std::fs::OpenOptions;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

/// Install the JSONL debug-logging subscriber if any of `B2_LOG` / `B2_DEBUG` /
/// `B2_LOG_FILE` is set; otherwise a no-op returning `None`. The returned
/// [`WorkerGuard`] owns the background writer thread's flush-on-drop — the caller
/// (`main`) must keep it alive for the process's lifetime, or buffered events are lost.
pub fn init_logging() -> Option<WorkerGuard> {
    let log_file = std::env::var_os("B2_LOG_FILE");
    let directive = match std::env::var("B2_LOG") {
        Ok(v) if !v.trim().is_empty() => v,
        // Implied default scoped to the kernel's targets (`b2::sqlite`/`vault`/`ingest`/
        // `search`) — not the CLI's bare `debug` — so Tauri/wry/hyper tracing doesn't
        // pollute the file. See the module doc for why this one knob diverges.
        _ if std::env::var_os("B2_DEBUG").is_some() || log_file.is_some() => "b2=debug".to_string(),
        _ => return None,
    };
    let filter = match EnvFilter::try_new(&directive) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("warning: invalid B2_LOG filter '{directive}' ({e}); using 'debug'");
            EnvFilter::new("debug")
        }
    };
    // Pick the sink, then wrap it non-blocking. Both arms yield the same `NonBlocking`
    // writer type (the inner writer is moved onto the worker thread), so the builder
    // chain below is written once regardless of stderr-vs-file.
    let (writer, guard) = match log_file {
        Some(path) => match OpenOptions::new()
            .create(true)
            .append(true)
            .open(Path::new(&path))
        {
            Ok(file) => tracing_appender::non_blocking(file),
            Err(e) => {
                eprintln!(
                    "warning: cannot open B2_LOG_FILE '{}' ({e}); logging to stderr",
                    Path::new(&path).display()
                );
                tracing_appender::non_blocking(std::io::stderr())
            }
        },
        None => tracing_appender::non_blocking(std::io::stderr()),
    };
    // Field-for-field identical to the CLI's builder so both adapters emit the same
    // record shape (b2-core/tests/logging.rs pins this contract): flat event fields for
    // `jq '.duration_us'`-style reporting, CLOSE span events so each façade-op span
    // carries its measured duration, current-span name but no ancestor list, no ANSI.
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_current_span(true)
        .with_span_list(false)
        .with_ansi(false)
        .with_env_filter(filter)
        .with_writer(writer)
        .init();
    Some(guard)
}
