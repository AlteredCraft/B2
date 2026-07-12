//! The kernel's structured debug logging: SQLite's own profiler
//! (`sqlite3_trace_v2` + `SQLITE_TRACE_PROFILE`, wired in `db::open`) must emit a
//! `b2::sqlite` tracing event per statement — the SQL **template** (never bound
//! values) plus a numeric `duration_us` — and the façade ops must run inside named
//! spans. Asserted through a real JSON subscriber, so what is checked is the exact
//! contract the CLI's `B2_LOG` sink exposes for reporting/plotting: every line is
//! one parseable JSON object with flat, typed fields.

mod common;

use common::golden_vault_copy;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::MakeWriter;

/// A `MakeWriter` capturing everything the subscriber renders, so the test can
/// parse the produced JSONL back.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// Run `f` under a JSON subscriber configured like the CLI's `B2_LOG` sink
/// (flattened events, span-close events) and return the captured log text.
fn capture_logs(f: impl FnOnce()) -> String {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_current_span(true)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    // Thread-local subscriber: SQLite's trace callback fires synchronously on this
    // thread, so its events land here without touching other tests' threads.
    tracing::subscriber::with_default(subscriber, f);
    let bytes = capture.0.lock().expect("capture lock").clone();
    String::from_utf8(bytes).expect("log output is UTF-8")
}

/// One test, two phases run in sequence. They must not be separate `#[test]`s:
/// the harness would run them on parallel threads, and tracing's **global**
/// callsite-interest cache races when one thread evaluates callsites with no
/// subscriber while the other holds a scoped one — events are then dropped
/// nondeterministically. (Real processes install at most one subscriber, so the
/// race is a test-harness artifact, not a kernel one.)
#[test]
fn sqlite_queries_emit_parseable_timing_events() {
    // Phase 1 — inert without a subscriber: same results, no output path, no panic.
    {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_vault_copy(&root);
        let vault = b2_core::vault::Vault::open(&root).unwrap();
        let report = vault.reindex().unwrap();
        assert_eq!(report.indexed, 2);
        assert!(!vault.search("memory", 5).unwrap().is_empty());
    }

    // Phase 2 — under a JSON subscriber, the same flow emits the reporting contract.
    let text = capture_logs(|| {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_vault_copy(&root);
        let vault = b2_core::vault::Vault::open(&root).unwrap();
        vault.reindex().unwrap();
        vault.search("memory", 5).unwrap();
    });

    let mut sqlite_events = 0usize;
    let mut saw_vault_span_close = false;
    for line in text.lines() {
        // The reporting contract: every emitted line is one JSON object.
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON log line ({e}): {line}"));

        if v["target"] == "b2::sqlite" {
            sqlite_events += 1;
            // Numeric duration (µs) straight from SQLite's profiler — plottable as-is.
            assert!(v["duration_us"].is_u64(), "duration_us not a u64: {line}");
            assert!(v["vm_steps"].is_number(), "vm_steps missing: {line}");
            assert!(v["slow"].is_boolean(), "slow flag missing: {line}");
            // The SQL template, single-line; bound values are never expanded into it.
            let sql = v["sql"].as_str().expect("sql is a string");
            assert!(!sql.contains('\n'), "sql not collapsed to one line: {line}");
        }

        // Façade ops are spans; their close events carry the op's own timing.
        if v["target"] == "b2::vault" && v["span"]["name"] == "search" {
            saw_vault_span_close = true;
        }
    }

    // A reindex + search runs many statements (migration, upserts, FTS, KNN…).
    assert!(
        sqlite_events > 10,
        "expected many b2::sqlite events, got {sqlite_events}"
    );
    assert!(saw_vault_span_close, "no b2::vault search span event seen");
}
