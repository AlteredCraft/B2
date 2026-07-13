//! Regression: `discover::candidates` must not issue O(chunks) SQL — neither a
//! per-hit `note_for_chunk` round-trip (#37's N+1: ~463k statements, a ~130s
//! `b2 similar` on a real vault) nor a whole-space chunk-vector scan per open
//! (#38: exact brute force over every stored vector — ~38.6k rows read, and under
//! the old vec0 store ~38.6k shadow-probe log lines — per note-open). This locks the
//! two-stage shape: **one** O(notes) centroid scan, then one bounded per-note vector
//! fetch per shortlisted note.
//!
//! Sole test in its binary on purpose: it installs a scoped tracing subscriber to read
//! SQLite's per-statement profiler back, and tracing's global callsite-interest cache
//! races when a sibling test evaluates the same callsites on another thread with no
//! subscriber (see the note in `tests/logging.rs`).

use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::{discover, open};
use std::fs;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

/// A `MakeWriter` that captures everything the subscriber renders, so the test can
/// count the SQL templates SQLite's profiler emitted.
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

/// The exact template `db::note_for_chunk` emits — a per-hit chunk→note resolution.
const PER_HIT_SQL: &str = "SELECT note_b2id FROM chunks WHERE id = ?1";
/// The stage-1 coarse scan `db::for_each_note_centroid` emits — must run **once**
/// per call: it is the only whole-space read discovery is allowed.
const CENTROID_SCAN_SQL: &str = "SELECT note_b2id, centroid FROM note_centroids";
/// The whole-space chunk-vector scan (`db::for_each_stored_vector`) — search's
/// primitive, which discovery must **never** run: reading every stored vector per
/// note-open is exactly the O(vault) cost #38 removed.
const SPACE_SCAN_SQL: &str = "SELECT chunk_id, vector FROM embeddings";
/// The per-note vector fetch (`db::note_chunk_vectors`) — stage 2's unit, allowed
/// once for the anchor plus once per *shortlisted note*, never per chunk.
const PER_NOTE_SQL: &str = "SELECT c.id, e.vector FROM chunks c JOIN embeddings e";

#[test]
fn candidates_issues_bounded_sql_never_o_chunks() {
    // Multi-chunk notes so any per-chunk statement pattern would visibly exceed the
    // note count (a scaled-down mirror of the real vault). Each paragraph is sized
    // well past the qmd ~450-token target, so every note splits into several chunks.
    const NOTES: usize = 12;
    const PARAS: usize = 6;

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut ids = Vec::new();
    for n in 0..NOTES {
        let b2id = format!("01JN{n:022}"); // ULID-shaped, unique, all-valid
        let body = (0..PARAS)
            .map(|p| format!("note {n} paragraph {p}: shared topic alpha beta gamma. ").repeat(40))
            .collect::<Vec<_>>()
            .join("\n\n");
        fs::write(
            vault.join(format!("n{n}.md")),
            format!("---\nb2id: {b2id}\ntype: note\ntitle: N{n}\n---\n{body}\n"),
        )
        .unwrap();
        ids.push(b2id);
    }
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();

    let total_chunks: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    let anchor_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE note_b2id = ?1",
            [&ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        anchor_chunks > 1 && total_chunks > NOTES as i64,
        "O(chunks) patterns only show with multi-chunk notes \
         (anchor={anchor_chunks}, total={total_chunks})"
    );

    // Run candidates under a DEBUG JSON subscriber so SQLite's per-statement profiler
    // (target b2::sqlite) is captured; count the statement templates.
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let cands = tracing::subscriber::with_default(subscriber, || {
        discover::candidates(&conn, &ids[0], 10).unwrap()
    });
    assert!(!cands.is_empty(), "unlinked notes are all candidates");

    let text = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();

    // (1) chunk→note resolution never happens per hit (stage 2 works note-by-note,
    // so it needs no resolution at all).
    let per_hit = text.matches(PER_HIT_SQL).count();
    assert_eq!(
        per_hit, 0,
        "discovery must not resolve chunk→note per hit (saw {per_hit})"
    );

    // (2) exactly one whole-space read, and it is the O(notes) centroid scan.
    let centroid_scans = text.matches(CENTROID_SCAN_SQL).count();
    assert_eq!(
        centroid_scans, 1,
        "the coarse centroid scan must run exactly once per call"
    );

    // (3) the O(chunks) whole-space vector scan never runs on the note-open path.
    let space_scans = text.matches(SPACE_SCAN_SQL).count();
    assert_eq!(
        space_scans, 0,
        "discovery must not scan every stored chunk vector (#38): saw {space_scans}"
    );

    // (4) per-note vector fetches are bounded by the shortlist (≤ one per note in
    // this small vault: the anchor + every candidate), never by the chunk count.
    let per_note = text.matches(PER_NOTE_SQL).count();
    assert!(
        (1..=NOTES).contains(&per_note),
        "stage-2 fetches must be one per shortlisted note (≤ {NOTES}), got {per_note} \
         (an O(chunks) pattern would approach {total_chunks})"
    );
}
