//! Opt-in structured debug logging for the CLI process.

/// The kernel's `tracing` events as JSON Lines, one flat object per line, on stderr or
/// appended to `B2_LOG_FILE` (the pure capture: stderr can interleave notices).
///
/// `B2_LOG` is a filter directive; `B2_DEBUG` or `B2_LOG_FILE` alone implies `b2=debug`,
/// kernel targets only, so `ureq`'s foreign-shaped `log` output stays out. With none set,
/// no subscriber is installed.
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
        // Top-level fields, so `jq '.duration_us'` works.
        .flatten_event(true)
        // Close events time each façade-op span; the clock lives in the adapter, not b2-core.
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .with_current_span(true)
        .with_span_list(false)
        .with_ansi(false)
        .with_env_filter(filter);
    // A short-lived run: a plain `Mutex<File>` writer suffices.
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
