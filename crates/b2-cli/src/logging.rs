//! Opt-in structured debug logging for the CLI process.

/// Opt-in structured debug logging: the kernel's `tracing` events — per-statement SQLite
/// timings, façade-op spans, flow milestones — rendered as **JSON Lines**, one flat object
/// per line, so a run's log pipes straight into jq/DuckDB while `--json` stdout stays pure
/// data.
///
/// The sink is stderr by default; `B2_LOG_FILE=<path>` writes there instead, in **append**
/// mode so successive runs accumulate into one dataset. A file is also the
/// guaranteed-pure capture: stderr can interleave human notices with the JSONL.
///
/// `B2_LOG` holds a tracing filter directive; `B2_DEBUG` or `B2_LOG_FILE` without it
/// implies **`b2=debug`** — the kernel's own targets only. That scoping is what keeps the
/// dataset reportable now that a chat command links an HTTP client: `ureq` logs through
/// the `log` bridge in a foreign shape, and a bare `debug` would fold it into the same
/// file. Opt into the firehose with an explicit `B2_LOG=debug`. With none of the three
/// set, no subscriber is installed and the instrumentation stays inert.
pub fn init_logging() {
    let log_file = std::env::var_os("B2_LOG_FILE");
    let directive = match std::env::var("B2_LOG") {
        Ok(v) if !v.trim().is_empty() => v,
        _ if std::env::var_os("B2_DEBUG").is_some() || log_file.is_some() => "b2=debug".to_string(),
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
