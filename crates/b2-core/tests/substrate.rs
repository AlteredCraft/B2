//! Step 0 — DB skeleton & the substrate bet.
//!
//! Green-scenario assertions for build-plan step 0
//! (index-engine.md):
//!   - FTS5 is compiled in (the `bundled` SQLite). *(Vectors need no substrate proof
//!     since schema v3, #38: they are plain BLOB tables scored in-process — the
//!     `sqlite-vec` half of the original bet was retired with the dependency.)*
//!   - open→reopen is stable; `WAL` + `foreign_keys=ON` hold; the #38 scan pragmas
//!     (`mmap_size`/`cache_size`) are applied; `schema_version` seeded.
//!   - the **first** open of a vault's index survives contention (#111): the one
//!     pragma `busy_timeout` cannot cover is retried until it lands.

use b2_core::{open, SCHEMA_VERSION};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

/// The load-bearing bet: BM25 full-text search in the statically-linked bundled
/// SQLite, no runtime `load_extension`.
#[test]
fn fts5_works_in_the_bundled_connection() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    // FTS5 present, BM25 ranking works.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE docs_fts USING fts5(text);
         INSERT INTO docs_fts(rowid, text) VALUES (1, 'spaced repetition and human memory');
         INSERT INTO docs_fts(rowid, text) VALUES (2, 'an unrelated cooking recipe');",
    )
    .unwrap();
    let hit: i64 = conn
        .query_row(
            "SELECT rowid FROM docs_fts WHERE docs_fts MATCH 'memory' ORDER BY rank LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hit, 1, "BM25 should rank the memory note first");
}

/// The locked pragmas and the `meta` bookkeeping survive a close/reopen, and the
/// `schema_version` gate is seeded exactly once (idempotent migration).
#[test]
fn pragmas_and_schema_version_persist_across_reopen() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "wal", "WAL must be engaged");
        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1, "foreign_keys must be ON");
        // The #38 read-path pragmas: whole-space vector scans must stream through
        // the OS page cache (mmap), not a pread-per-page under the 2 MB default.
        let mmap_size: i64 = conn
            .query_row("PRAGMA mmap_size", [], |r| r.get(0))
            .unwrap();
        assert!(mmap_size > 0, "mmap_size must be engaged, got {mmap_size}");
        let cache_size: i64 = conn
            .query_row("PRAGMA cache_size", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cache_size, -32768, "cache_size must be raised (KiB units)");
    } // connection dropped → file closed

    // Reopen: schema_version is stable and not duplicated.
    let conn = open(&db_path).unwrap();
    let version: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION.to_string());

    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "migration must be idempotent across reopen");
}

/// The **first** open of a vault's index is the one that can lose a lock race, and it
/// must wait it out rather than fail (#111).
///
/// `journal_mode = WAL` is the only statement in [`open`] that takes a write lock, and
/// only when it actually *changes* the mode — so exactly once per vault, on the
/// first-ever open. `busy_timeout` does not cover that flip (SQLite skips the busy
/// handler for a write lock upgraded from an open read transaction), so a second
/// opener took an immediate `SQLITE_BUSY` — "database is locked" — and the documented
/// cold-vault flow `b2 reindex & ; b2 status` failed ~40% of the time, usually killing
/// the reindex.
///
/// Made deterministic by holding the contended lock outright rather than racing for
/// it: a rollback-journal connection sits in `BEGIN IMMEDIATE` and releases it well
/// inside the retry budget. Unfixed, `open` gives up ~200 µs in, long before the
/// holder lets go.
///
/// **`IMMEDIATE`, not `EXCLUSIVE`** — the distinction is the whole test. `IMMEDIATE`
/// holds `RESERVED`: readers still get in, writers don't. That is precisely the lock
/// the flip trips over, because it fails on the *write* half after its read
/// transaction is already open — the half SQLite skips the busy handler for. Swap in
/// `EXCLUSIVE` and the flip blocks on the *read* half instead, which the busy handler
/// does cover, so it waits happily with or without the fix and the test asserts
/// nothing.
#[test]
fn first_open_waits_out_a_held_lock() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    // A real (rollback-journal) database with its write lock held — what a racing
    // first `open` finds when another process got to the mode flip first.
    let holder = rusqlite::Connection::open(&db_path).unwrap();
    holder
        .execute_batch(
            "PRAGMA journal_mode = DELETE; CREATE TABLE placeholder (x); BEGIN IMMEDIATE",
        )
        .unwrap();

    // Released only once `open` is under way, so the wait window can't be spent on
    // thread startup and let the test pass vacuously.
    let (started_tx, started_rx) = mpsc::channel();
    let releaser = std::thread::spawn(move || {
        started_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(250));
        holder.execute_batch("ROLLBACK").unwrap();
    });

    started_tx.send(()).unwrap();
    let conn = open(&db_path).expect("a contended first open must wait, not fail");
    releaser.join().unwrap();

    // And the wait bought the real thing: the mode flip applied, not skipped.
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal", "WAL must be engaged");
}

/// The reported shape of #111: several openers reaching a brand-new index at once —
/// `b2 reindex &` racing a `b2 status`, or the desktop app launching against a vault a
/// CLI reindex is building. Every one of them must come back with a connection.
///
/// Threads rather than processes because the contention is SQLite's, not the OS's:
/// locking is per-*connection*, so same-process openers race the mode flip exactly as
/// separate `b2` invocations do. The barrier is what makes it bite — released one by
/// one, eight opens finish before they can overlap, and the bug never shows.
#[test]
fn concurrent_first_opens_all_succeed() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    let start = Arc::new(Barrier::new(8));
    let openers: Vec<_> = (0..8)
        .map(|_| {
            let db_path = db_path.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                open(&db_path).map(|_| ())
            })
        })
        .collect();

    for opener in openers {
        opener
            .join()
            .unwrap()
            .expect("a concurrent first open must not fail");
    }
}
