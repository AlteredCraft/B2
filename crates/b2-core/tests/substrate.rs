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
//!     pragma `busy_timeout` cannot cover is retried until it lands, and concurrent
//!     openers of one fresh index coexist.
//!   - `open` is concurrency-safe *through* its `schema_version` migration (#114,
//!     invariant C1): a stale-schema rebuild is atomic and serialized, an incomplete
//!     schema is rebuilt rather than trusted, and a reader is never refused. (C1's other
//!     drop-and-rebuild — the vector tables, created at embed time — is covered where it
//!     lives, in `embed.rs`.)

use b2_core::{open, SCHEMA_VERSION};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

/// The tables `db::migrate` must leave behind — restated here because an integration
/// test sees only the public API, never the engine's own `SCHEMA_TABLES`.
///
/// The engine's list is pinned to the DDL by a unit test in `db.rs`; this copy is not,
/// and doesn't need to be. A table added there and forgotten here only makes the
/// assertions in *this* file weaker, never wrong — the completeness check that actually
/// guards the index reads the pinned list, not this one.
const SCHEMA_TABLES: [&str; 7] = [
    "meta",
    "notes",
    "note_aliases",
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

/// Eight openers reaching a brand-new index at once all come back with a connection —
/// the user-visible shape of #111 (`b2 reindex &` racing a `b2 status`, or the desktop
/// app launching against a vault a CLI reindex is building).
///
/// **This is a coexistence smoke test, not the #111 gate.** Say so plainly, because the
/// name would otherwise promise a regression it does not catch: the flip's race window
/// is ~200 µs wide, so eight barrier-released threads land inside it only sometimes —
/// measured at **3 failures in 25 runs** against the unfixed `open`, i.e. green ~88% of
/// the time on the very bug it appears to name. [`first_open_waits_out_a_held_lock`] is
/// the gate; it holds the contended lock outright instead of racing for it, and fails
/// 100% of the time when the retry is removed. What this test *does* buy is the property
/// no deterministic single-lock test can state: that N concurrent openers finish at all
/// — no deadlock, no starved thread, no error escaping the retry — which is what would
/// break if `open` ever took a lock it holds rather than one it waits out.
///
/// Threads rather than processes because the contention is SQLite's, not the OS's:
/// locking is per-*connection*, so same-process openers race the mode flip exactly as
/// separate `b2` invocations do. The barrier is what gives it any chance of biting —
/// without it the eight opens are released one by one and finish before they can
/// overlap, and the race never even starts.
#[test]
fn concurrent_openers_of_a_fresh_index_coexist() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    // One binding for both the barrier's count and the thread count: if those two ever
    // drift, the barrier simply never releases and the test *hangs* — a CI timeout with
    // no failing assertion to read, which is a far worse way to learn about a typo.
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

/// Build an index at the *previous* schema version, so every opener of it takes the
/// drop-and-rebuild branch of the migration — the state a vault is in on the first `b2`
/// run after an upgrade that bumps `SCHEMA_VERSION`.
fn stale_index(db_path: &std::path::Path) {
    let conn = open(db_path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
        [(SCHEMA_VERSION - 1).to_string()],
    )
    .unwrap();
}

/// Eight openers reaching a **stale-schema** index at once leave one complete schema
/// behind, and none of them fails (#114, invariant C1).
///
/// The migration is a read-then-decide-then-write sequence, and its rebuild was ~30
/// separately-committed DDL statements: two openers that both read the stale version
/// both rebuilt, and their statements interleaved — opener B's `DROP TABLE resources`
/// landing after opener A's `CREATE TABLE resources`, leaving A to stamp the current
/// version over a schema B had partly demolished. `busy_timeout` never applied, because
/// nothing was contending for a lock; every statement succeeded, in the wrong order.
///
/// **Both assertions are the test, and the second is the one that matters.** Failing
/// opens (`no such table: main.resources`) are the loud half — bad, but self-announcing.
/// The quiet half is an index left missing tables while *every* opener returned `Ok`,
/// which surfaces later as a broken `search` or `reindex`, arbitrarily far from the
/// cause. Measured against the unfixed engine at 20 rounds × 8 openers: 2–12 failed
/// opens per run and up to 8 missing-table observations, caught in **7 of 8** runs.
///
/// So: a strong probe, not a certainty — the same honest caveat
/// [`concurrent_openers_of_a_fresh_index_coexist`] carries, and for the same reason (a
/// race only bites when the threads actually interleave). The deterministic gates for
/// this fix are its two siblings below; this is the one that reproduces the bug as
/// reported. Rounds are capped at 20 because detection flattens out past that while the
/// wall-clock does not — the correlation is per-run machine mood, not per-round luck.
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

        // Every opener returned Ok, so each believes the index is migrated. Is it?
        assert_schema_complete(&db_path, &format!("round {round}"));
    }
}

/// An index whose stamp says "current" but whose tables say otherwise is **rebuilt**,
/// not trusted (#114, invariant C1).
///
/// This is the wreckage the bug leaves in the field: a stamped-but-incomplete index,
/// written by a `b2` old enough to have raced itself. The fix makes new ones impossible,
/// and this is the other half — the existing ones have to heal, or the fix only helps
/// vaults that were never bitten.
///
/// **Dropping the surviving rows is the assertion with teeth**, and the reason the
/// repair is a full rebuild rather than a patch: recreating just the missing tables
/// would leave `notes` rows claiming to be indexed while their chunks are gone, and an
/// incremental reindex skips a note whose `body_hash` still matches — so the recreated
/// tables would stay empty forever, quietly breaking S3 (`full-reindex ≡
/// incremental-update`). The stamp is honest only if the whole projection is.
#[test]
fn an_index_stamped_current_but_missing_a_table_is_rebuilt() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        // A note the index believes is projected — the row that must not outlive the repair.
        conn.execute(
            "INSERT INTO notes(path, type, body_hash, indexed_at)
             VALUES ('kept.md', 'note', 'hash', '2026-07-26T00:00:00Z')",
            [],
        )
        .unwrap();
        // What a lost race left behind: the chunk tables gone, the stamp still current.
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

/// An index whose tables are all present but whose **stamp is gone** is rebuilt from
/// empty too — the other way `schema_is_current` can say no (#114).
///
/// The sibling above loses a table and keeps the stamp; this one keeps every table and
/// loses the stamp, which is the shape a guard on "was there a prior stamp?" waves
/// through: nothing to compare, so nothing dropped, and the `CREATE … IF NOT EXISTS`
/// batch settles over surviving tables of an unknown shape and re-stamps them current.
/// Whatever wrote that state — a rebuild killed between clearing `meta` and re-stamping
/// it, a half-restored backup, a hand-edited index — the tables are of no known version,
/// and the rows in them are exactly the ones an incremental reindex would decline to
/// refresh (S3). So the rebuild is unconditional: `apply_schema` has one outcome, an
/// empty schema at the current version, whatever it finds.
#[test]
fn an_index_with_its_schema_stamp_missing_is_rebuilt() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO notes(path, type, body_hash, indexed_at)
             VALUES ('kept.md', 'note', 'hash', '2026-07-26T00:00:00Z')",
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

/// Opening an index at the current schema is a **read**, so a writer holding the index
/// cannot refuse it (#114, invariant C1: concurrent readers are unrestricted).
///
/// The migration used to re-run its ~30 `IF NOT EXISTS` DDL statements and re-stamp
/// `meta` on *every* open, current schema or not — a write, on the one path that must
/// never need one. Against a held write lock that stamp waited out the full 5 s
/// `busy_timeout` and then failed the open outright: `b2 search` refused, in a vault
/// whose only sin was having a reindex running. Now the common path reads two rows and
/// writes nothing, so there is no lock to lose.
///
/// The writer here is a reindex mid-batch as a second process sees it. Its lock is
/// proven held rather than assumed — a `busy_timeout = 0` probe must bounce off it —
/// because a `BEGIN IMMEDIATE` that quietly took nothing would make this test pass
/// against the very code it exists to catch.
#[test]
fn an_open_of_a_current_index_is_not_refused_by_a_writer() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");
    drop(open(&db_path).unwrap()); // an index at the current schema

    let writer = rusqlite::Connection::open(&db_path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();

    // Non-vacuity: the write lock is genuinely held right now.
    let probe = rusqlite::Connection::open(&db_path).unwrap();
    probe.execute_batch("PRAGMA busy_timeout = 0").unwrap();
    assert!(
        probe.execute_batch("BEGIN IMMEDIATE").is_err(),
        "the writer must actually hold the write lock, or this test asserts nothing"
    );

    let conn = open(&db_path).expect("a reader must never be refused by a writer (C1)");

    // And it is a working connection over the real schema, not a hollow one.
    let notes: i64 = conn
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(notes, 0);

    writer.execute_batch("ROLLBACK").unwrap();
}
