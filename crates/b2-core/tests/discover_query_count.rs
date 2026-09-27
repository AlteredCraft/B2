//! `discover::candidates` must not issue O(chunks) SQL (GH #37's N+1, GH #38's whole-space
//! scan): one O(notes) centroid scan, then one fetch per shortlisted note.
//!
//! Sole test in its binary: tracing's global callsite-interest cache races when a sibling
//! test hits the same callsites on another thread with no subscriber.

use b2_core::embed::FakeEmbedder;
use b2_core::ingest::ingest_vault;
use b2_core::{discover, open};
use std::fs;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

/// Captures everything the subscriber renders, to count SQL templates.
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

/// `db::note_for_chunk`'s template: a per-hit chunk-to-note resolution.
const PER_HIT_SQL: &str = "SELECT note_path FROM chunks WHERE id = ?1";
/// The stage-1 centroid scan, the only whole-space read discovery may make, once.
const CENTROID_SCAN_SQL: &str = "SELECT note_path, centroid FROM note_centroids";
/// Search's whole-space vector scan, which discovery must never run (GH #38).
const SPACE_SCAN_SQL: &str = "SELECT chunk_id, vector FROM embeddings";
/// Stage 2's per-note fetch: once per shortlisted note, never per chunk.
const PER_NOTE_SQL: &str = "SELECT c.id, e.vector FROM chunks c JOIN embeddings e";

#[test]
fn candidates_issues_bounded_sql_never_o_chunks() {
    // Multi-chunk notes, so a per-chunk statement pattern would exceed the note count.
    const NOTES: usize = 12;
    const PARAS: usize = 6;

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut ids = Vec::new();
    for n in 0..NOTES {
        let body = (0..PARAS)
            .map(|p| format!("note {n} paragraph {p}: shared topic alpha beta gamma. ").repeat(40))
            .collect::<Vec<_>>()
            .join("\n\n");
        let name = format!("n{n}.md");
        fs::write(
            vault.join(&name),
            format!("---\ntype: note\ntitle: N{n}\n---\n{body}\n"),
        )
        .unwrap();
        ids.push(name);
    }
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &FakeEmbedder::new(64)).unwrap();

    let total_chunks: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    let anchor_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE note_path = ?1",
            [&ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        anchor_chunks > 1 && total_chunks > NOTES as i64,
        "O(chunks) patterns only show with multi-chunk notes \
         (anchor={anchor_chunks}, total={total_chunks})"
    );

    // A DEBUG subscriber captures SQLite's per-statement profiler (b2::sqlite).
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let cands = tracing::subscriber::with_default(subscriber, || {
        discover::candidates(&conn, &ids[0], 10, false).unwrap()
    });
    assert!(!cands.is_empty(), "unlinked notes are all candidates");

    let text = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();

    // Stage 2 works note by note, so it needs no resolution.
    let per_hit = text.matches(PER_HIT_SQL).count();
    assert_eq!(
        per_hit, 0,
        "discovery must not resolve chunk→note per hit (saw {per_hit})"
    );

    let centroid_scans = text.matches(CENTROID_SCAN_SQL).count();
    assert_eq!(
        centroid_scans, 1,
        "the coarse centroid scan must run exactly once per call"
    );

    let space_scans = text.matches(SPACE_SCAN_SQL).count();
    assert_eq!(
        space_scans, 0,
        "discovery must not scan every stored chunk vector (#38): saw {space_scans}"
    );

    let per_note = text.matches(PER_NOTE_SQL).count();
    assert!(
        (1..=NOTES).contains(&per_note),
        "stage-2 fetches must be one per shortlisted note (≤ {NOTES}), got {per_note} \
         (an O(chunks) pattern would approach {total_chunks})"
    );
}
