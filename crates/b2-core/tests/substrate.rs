//! Step 0 — DB skeleton & the substrate bet.
//!
//! Green-scenario assertions for build-plan step 0
//! (planning/specs/completed/index-engine-build.md §4):
//!   - FTS5 is compiled in (the `bundled` SQLite). *(Vectors need no substrate proof
//!     since schema v3, #38: they are plain BLOB tables scored in-process — the
//!     `sqlite-vec` half of the original bet was retired with the dependency.)*
//!   - open→reopen is stable; `WAL` + `foreign_keys=ON` hold; the #38 scan pragmas
//!     (`mmap_size`/`cache_size`) are applied; `schema_version` seeded.

use b2_core::{open, SCHEMA_VERSION};

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
