//! Regression: `discover::candidates` must resolve chunk→note in **one bulk query**,
//! not a per-hit `note_for_chunk` round-trip. The full-space scan visits every vault
//! chunk for *each* anchor chunk, so a per-hit lookup was O(anchor_chunks × vault_chunks)
//! — on a real ~38k-chunk vault a 12-chunk anchor fired ~463k of these, turning
//! `b2 similar` (the desktop's note-open discovery) into a ~130s stall and a 277 MB
//! `B2_LOG` file. This locks the fix: per-hit note resolution must not scale with the
//! product.
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
/// The whole-space scan `db::for_each_stored_vector` emits — must run **once** for the
/// whole call, not once per anchor chunk (the old per-anchor KNN scan storm).
const SPACE_SCAN_SQL: &str = "SELECT chunk_id, embedding FROM chunks_vec";

#[test]
fn candidates_resolves_notes_without_a_per_hit_query() {
    // A multi-chunk anchor plus several multi-chunk notes: a per-hit resolution would
    // fire anchor_chunks × total_chunks times (a scaled-down mirror of the real vault).
    const NOTES: usize = 12;
    const PARAS: usize = 5; // blank-line-separated paragraphs → ~1 chunk each

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut ids = Vec::new();
    for n in 0..NOTES {
        let b2id = format!("01JN{n:022}"); // ULID-shaped, unique, all-valid
        let body = (0..PARAS)
            .map(|p| format!("note {n} paragraph {p}: shared topic alpha beta gamma"))
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
        anchor_chunks > 1 && total_chunks > anchor_chunks,
        "the N+1 only shows with a multi-chunk anchor over a larger vault \
         (anchor={anchor_chunks}, total={total_chunks})"
    );

    // Run candidates under a DEBUG JSON subscriber so SQLite's per-statement profiler
    // (target b2::sqlite) is captured; count the per-hit resolution template.
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

    // (1) chunk→note resolution is one bulk load, not a per-hit query.
    let per_hit = text.matches(PER_HIT_SQL).count();
    let n_plus_1 = (anchor_chunks * total_chunks) as usize;
    assert!(
        per_hit <= 1,
        "chunk→note resolution must be one bulk query, not per hit: saw {per_hit} \
         `note_for_chunk` statements (an N+1 would be anchor×vault = \
         {anchor_chunks}×{total_chunks} = {n_plus_1})"
    );

    // (2) the vector space is scanned exactly once, not once per anchor chunk — the
    // old per-anchor KNN scan reread the whole ~100k-vector space `anchor_chunks` times.
    let space_scans = text.matches(SPACE_SCAN_SQL).count();
    assert_eq!(
        space_scans, 1,
        "the whole-space scan must run once for the call, not once per anchor chunk \
         (anchor_chunks = {anchor_chunks})"
    );
}
