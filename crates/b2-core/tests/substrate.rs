//! The index's SQLite substrate: FTS5 in the bundled build, the locked pragmas, a first
//! open that survives contention (GH #111), and a migration that is concurrency-safe and
//! never refuses a reader (GH #114, C1; the vector tables' side is in `embed.rs`).

use b2_core::{open, SCHEMA_VERSION};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

/// The tables `db::migrate` must leave behind, restated because integration tests can't
/// see the engine's list. A table forgotten here only weakens this file's assertions.
const SCHEMA_TABLES: [&str; 6] = [
    "meta",
    "notes",
    "chunks",
    "chunks_fts",
    "resources",
    "edges",
];

/// Every table in [`SCHEMA_TABLES`] present in this index, named in the panic if not.
fn assert_schema_complete(db_path: &std::path::Path, context: &str) {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    for table in SCHEMA_TABLES {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "{context}: `{table}` missing from the index");
    }
}

/// BM25 full-text search in the bundled SQLite, no runtime `load_extension`.
#[test]
fn fts5_works_in_the_bundled_connection() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

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
        // GH #38: vector scans stream through mmap, not pread-per-page.
        let mmap_size: i64 = conn
            .query_row("PRAGMA mmap_size", [], |r| r.get(0))
            .unwrap();
        assert!(mmap_size > 0, "mmap_size must be engaged, got {mmap_size}");
        let cache_size: i64 = conn
            .query_row("PRAGMA cache_size", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cache_size, -32768, "cache_size must be raised (KiB units)");
    }

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

/// The first open of an index can lose a lock race and must wait it out (ADR-0021). Made
/// deterministic by holding the lock outright. `IMMEDIATE`, not `EXCLUSIVE`: `EXCLUSIVE`
/// blocks the read half, which the busy handler covers, so it would pass without the fix.
#[test]
fn first_open_waits_out_a_held_lock() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    // What a racing first `open` finds when another process reached the mode flip first.
    let holder = rusqlite::Connection::open(&db_path).unwrap();
    holder
        .execute_batch(
            "PRAGMA journal_mode = DELETE; CREATE TABLE placeholder (x); BEGIN IMMEDIATE",
        )
        .unwrap();

    // Released only once `open` is under way, so the test can't pass vacuously.
    let (started_tx, started_rx) = mpsc::channel();
    let releaser = std::thread::spawn(move || {
        started_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(250));
        holder.execute_batch("ROLLBACK").unwrap();
    });

    started_tx.send(()).unwrap();
    let conn = open(&db_path).expect("a contended first open must wait, not fail");
    releaser.join().unwrap();

    // The mode flip applied, not skipped.
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal", "WAL must be engaged");
}

/// A coexistence smoke test, not the GH #111 gate (it caught the unfixed bug only 3 times in
/// 25; [`first_open_waits_out_a_held_lock`] is the gate). It proves N concurrent openers all
/// finish. Threads suffice: SQLite locks per connection.
#[test]
fn concurrent_openers_of_a_fresh_index_coexist() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    // One binding for both counts: if they drift, the barrier never releases and CI hangs.
    const OPENERS: usize = 8;
    let start = Arc::new(Barrier::new(OPENERS));
    let openers: Vec<_> = (0..OPENERS)
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

/// An index at the previous schema version, as after an upgrade: every opener takes the
/// drop-and-rebuild branch.
fn stale_index(db_path: &std::path::Path) {
    let conn = open(db_path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
        [(SCHEMA_VERSION - 1).to_string()],
    )
    .unwrap();
}

/// Concurrent openers of a stale index leave one complete schema and none fails (ADR-0021,
/// C1). The quiet failure is missing tables behind every `Ok`. A strong probe (7 of 8 runs
/// caught the unfixed bug), not a certainty; the deterministic gates are below.
#[test]
fn concurrent_opens_of_a_stale_index_leave_a_complete_schema() {
    const ROUNDS: usize = 20;
    const OPENERS: usize = 8;

    for round in 0..ROUNDS {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("b2.sqlite");
        stale_index(&db_path);

        let start = Arc::new(Barrier::new(OPENERS));
        let openers: Vec<_> = (0..OPENERS)
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
                .unwrap_or_else(|e| panic!("round {round}: a concurrent open failed: {e}"));
        }

        assert_schema_complete(&db_path, &format!("round {round}"));
    }
}

/// An index stamped current but missing tables is rebuilt, not trusted (GH #114, C1). The
/// rebuild must drop surviving `notes` rows: an incremental reindex skips a matching
/// `body_hash`, so patched tables would stay empty forever (S3).
#[test]
fn an_index_stamped_current_but_missing_a_table_is_rebuilt() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        // A row that must not outlive the repair.
        conn.execute(
            "INSERT INTO notes(path, body_hash, indexed_at)
             VALUES ('kept.md', 'hash', '2026-07-26T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute_batch("DROP TABLE chunks_fts; DROP TABLE chunks;")
            .unwrap();
    }

    let conn = open(&db_path).expect("an incomplete index must be repaired, not refused");
    assert_schema_complete(&db_path, "after reopening an incomplete index");

    let surviving: i64 = conn
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        surviving, 0,
        "a repaired index must be rebuilt from empty, or an incremental reindex \
         will never refill the tables it just recreated"
    );
}

/// An index with all tables but no stamp is rebuilt from empty too (GH #114): tables of
/// unknown shape must not be re-stamped current (S3).
#[test]
fn an_index_with_its_schema_stamp_missing_is_rebuilt() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO notes(path, body_hash, indexed_at)
             VALUES ('kept.md', 'hash', '2026-07-26T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM meta WHERE key = 'schema_version'", [])
            .unwrap();
    }

    let conn = open(&db_path).expect("an unstamped index must be repaired, not refused");
    assert_schema_complete(&db_path, "after reopening an unstamped index");

    let surviving: i64 = conn
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        surviving, 0,
        "an index of no known version must be rebuilt from empty, not adopted as current"
    );
}

/// Opening a current index is a read, so a writer can't refuse it (GH #114, C1). The
/// writer's lock is proven held by a probe, or the test would pass vacuously.
#[test]
fn an_open_of_a_current_index_is_not_refused_by_a_writer() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");
    drop(open(&db_path).unwrap()); // an index at the current schema

    let writer = rusqlite::Connection::open(&db_path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();

    let probe = rusqlite::Connection::open(&db_path).unwrap();
    probe.execute_batch("PRAGMA busy_timeout = 0").unwrap();
    assert!(
        probe.execute_batch("BEGIN IMMEDIATE").is_err(),
        "the writer must actually hold the write lock, or this test asserts nothing"
    );

    let conn = open(&db_path).expect("a reader must never be refused by a writer (C1)");

    let notes: i64 = conn
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(notes, 0);

    writer.execute_batch("ROLLBACK").unwrap();
}

/// An index stamped newer than this binary is refused and left untouched, so an old `b2` on
/// `PATH` can't destroy it. The structural check doesn't gate this: a future schema may drop
/// any table in our list.
#[test]
fn an_index_from_a_newer_b2_is_refused_not_rebuilt() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO notes(path, body_hash, indexed_at)
             VALUES ('kept.md', 'hash', '2026-08-28T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
            [(SCHEMA_VERSION + 1).to_string()],
        )
        .unwrap();
    }

    let err = open(&db_path).expect_err("an index from a newer b2 must be refused");
    assert!(
        matches!(
            err,
            b2_core::Error::IndexTooNew { found, supported }
                if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION
        ),
        "the refusal must name both versions so the adapters can say which b2 to run: {err}"
    );

    // The refusal cost the index nothing.
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let surviving: i64 = conn
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        surviving, 1,
        "a refused open must leave the newer index exactly as it found it"
    );
    let stamp: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        stamp,
        (SCHEMA_VERSION + 1).to_string(),
        "a refused open must not restamp the index it declined to read"
    );
}
