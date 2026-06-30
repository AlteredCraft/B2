//! Step 2 — chunks are projected and the FTS5 index tracks them (the minimal
//! paragraph chunker; the qmd heuristic lands at step 5, see the build spec).

mod common;

use b2_core::event::NullSink;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use common::{golden_vault_copy, SRS_ID};

#[test]
fn chunks_are_projected_for_each_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &NullSink).unwrap();

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert!(total >= 2, "at least one chunk per golden note");

    // spaced-repetition splits into two paragraphs (prose + the Relations list).
    let srs_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE note_b2id = ?1",
            [SRS_ID],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(srs_chunks, 2);

    // char offsets must address the slice that produced the chunk text.
    let (start, end, text): (i64, i64, String) = conn
        .query_row(
            "SELECT char_start, char_end, text FROM chunks WHERE note_b2id = ?1 AND seq = 0",
            [SRS_ID],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(end > start);
    assert!(text.starts_with("Spaced repetition exploits"));
}

#[test]
fn fts_index_tracks_chunks_and_matches_body_text() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &NullSink).unwrap();

    // 'forgetting' appears only in spaced-repetition's Relations paragraph.
    let note: String = conn
        .query_row(
            "SELECT c.note_b2id FROM chunks_fts f
             JOIN chunks c ON c.id = f.rowid
             WHERE chunks_fts MATCH 'forgetting'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, SRS_ID);
}

#[test]
fn reindexing_a_note_does_not_leave_stale_fts_rows() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    ingest_vault(&conn, &vault, &UlidGen, &NullSink).unwrap();
    let fts_count = |c: &rusqlite::Connection| -> i64 {
        c.query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
            .unwrap()
    };
    let before = fts_count(&conn);

    // Re-ingesting must replace, not accumulate (delete sentinel + reinsert).
    ingest_vault(&conn, &vault, &UlidGen, &NullSink).unwrap();
    assert_eq!(
        before,
        fts_count(&conn),
        "FTS rows must not accumulate on reindex"
    );
}
