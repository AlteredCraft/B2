//! Opt-in structured debug logging for the desktop host — the **GUI mirror** of the CLI's
//! `init_logging`. Same knobs, same wire shape, so both adapters write into one reportable
//! JSONL dataset of the kernel's `tracing` events.
//!
//! Installing the subscriber is legitimate **host** work: the core only *emits*; the
//! subscriber and its wall-clock live in the adapter, keeping `b2-core` clock-free.
//!
//! Knobs, as the CLI: stderr by default, `B2_LOG_FILE=<path>` in **append** mode instead,
//! `B2_LOG` a tracing filter honored verbatim. With none of the three set, no subscriber is
//! installed. Relative `B2_LOG_FILE` paths resolve against the process CWD — under
//! `just app` that is `crates/b2-desktop/`, so prefer an absolute path.
//!
//! **The implied default is scoped, and both adapters scope it the same way:** with no
//! explicit `B2_LOG`, `B2_DEBUG`/`B2_LOG_FILE` imply **`b2=debug`**, never a bare `debug`.
//! This process embeds Tauri + wry + hyper + reqwest, all noisy emitters whose records have
//! a foreign shape that would pollute the dataset. Opt into the firehose explicitly.
//!
//! **One difference from the CLI, deliberate:** the CLI is short-lived and writes through a
//! plain `Mutex<File>`. This app is long-lived and multi-threaded — the background embed
//! pass alone bursts `b2::sqlite` events off the UI thread — so blocking those threads on
//! file I/O would stutter the GUI and pollute the very throughput these logs measure. The
//! sink is a `tracing-appender` **non-blocking** writer, whose [`WorkerGuard`] flushes on
//! drop, so `main` must hold the returned guard for the whole run.

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
