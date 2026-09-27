//! Structured debug logging: SQLite's profiler emits a `b2::sqlite` event per statement
//! (the SQL template, never bound values, plus `duration_us`), and façade ops run in named
//! spans. Checked through a JSON subscriber like the CLI's `B2_LOG` sink.

mod common;

use common::golden_vault_copy;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::MakeWriter;

/// Captures everything the subscriber renders.
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

/// Run `f` under a JSON subscriber configured like the CLI's `B2_LOG` sink.
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
    // Thread-local: SQLite's trace callback fires synchronously on this thread.
    tracing::subscriber::with_default(subscriber, f);
    let bytes = capture.0.lock().expect("capture lock").clone();
    String::from_utf8(bytes).expect("log output is UTF-8")
}

/// Two phases in one test: as separate tests on parallel threads, tracing's global
/// callsite-interest cache would race and drop events. A harness artifact only.
#[test]
fn sqlite_queries_emit_parseable_timing_events() {
    // Phase 1: inert without a subscriber.
    {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_vault_copy(&root);
        let vault = b2_core::vault::Vault::open(&root).unwrap();
        let report = vault.reindex().unwrap();
        assert_eq!(report.indexed, 2);
        assert!(!vault.search("memory", 5).unwrap().is_empty());
    }

    // Phase 2: under a JSON subscriber.
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
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON log line ({e}): {line}"));

        if v["target"] == "b2::sqlite" {
            sqlite_events += 1;
            assert!(v["duration_us"].is_u64(), "duration_us not a u64: {line}");
            assert!(v["vm_steps"].is_number(), "vm_steps missing: {line}");
            assert!(v["slow"].is_boolean(), "slow flag missing: {line}");
            let sql = v["sql"].as_str().expect("sql is a string");
            assert!(!sql.contains('\n'), "sql not collapsed to one line: {line}");
        }

        if v["target"] == "b2::vault" && v["span"]["name"] == "search" {
            saw_vault_span_close = true;
        }
    }

    assert!(
        sqlite_events > 10,
        "expected many b2::sqlite events, got {sqlite_events}"
    );
    assert!(saw_vault_span_close, "no b2::vault search span event seen");
}
