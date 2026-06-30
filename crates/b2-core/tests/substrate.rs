//! Step 0 — DB skeleton & the substrate bet.
//!
//! Green-scenario assertions for build-plan step 0
//! (planning/specs/index-engine-build.md §4):
//!   - `sqlite-vec` statically links; FTS5 is compiled in (the `bundled` SQLite).
//!   - open→reopen is stable; `WAL` + `foreign_keys=ON` hold; `schema_version` seeded.
//!
//! This is the riskiest integration in the whole design (index-engine.md §3): if
//! FTS5 (BM25) and `sqlite-vec` (KNN) can't live in one connection, the
//! single-store premise is wrong. So it is the first thing we prove.

use b2_core::{open, SCHEMA_VERSION};

/// The load-bearing bet: BM25 full-text search AND brute-force vector KNN, in the
/// *same* statically-linked connection, no runtime `load_extension`.
#[test]
fn fts5_and_sqlite_vec_coexist_in_one_connection() {
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

    // sqlite-vec present, brute-force KNN over a vec0 virtual table works.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE v USING vec0(embedding FLOAT[3]);
         INSERT INTO v(rowid, embedding) VALUES (1, '[0.10, 0.20, 0.30]');
         INSERT INTO v(rowid, embedding) VALUES (2, '[0.90, 0.80, 0.70]');",
    )
    .unwrap();
    let nearest: i64 = conn
        .query_row(
            "SELECT rowid FROM v WHERE embedding MATCH '[0.11, 0.19, 0.31]' ORDER BY distance LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nearest, 1, "KNN should return the nearest vector");
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
