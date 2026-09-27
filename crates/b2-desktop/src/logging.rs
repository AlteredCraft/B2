//! Opt-in structured debug logging for the desktop host, the GUI mirror of the CLI's
//! `init_logging`: same knobs (`B2_LOG`, `B2_DEBUG`, `B2_LOG_FILE` in append mode), same
//! JSONL shape. A relative `B2_LOG_FILE` resolves against the CWD, which under `make app`
//! is `crates/b2-desktop/`.
//!
//! Unlike the CLI's `Mutex<File>`, the sink is non-blocking, so file I/O never stalls the
//! GUI or embed threads.

use std::fs::OpenOptions;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

/// Install the JSONL subscriber if any of `B2_LOG` / `B2_DEBUG` / `B2_LOG_FILE` is set.
/// The caller must hold the returned [`WorkerGuard`] for the whole run, or buffered events
/// are lost.
pub fn init_logging() -> Option<WorkerGuard> {
    let log_file = std::env::var_os("B2_LOG_FILE");
    let directive = match std::env::var("B2_LOG") {
        Ok(v) if !v.trim().is_empty() => v,
        // Scoped to B2's own targets, as in the CLI, so Tauri/wry/hyper records stay out.
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
    // Identical to the CLI's builder so both emit one record shape
    // (b2-core/tests/logging.rs). CLOSE span events carry each façade op's duration.
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
