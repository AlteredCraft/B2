//! Opening the index, the schema migration, and the projection helpers for the
//! Markdown-derived tiers: `notes`/`note_aliases`, `chunks` (+FTS5), the
//! `embeddings`/`note_centroids` vector tables, and the typed `edges` graph. Every
//! table here is a derived projection of Markdown — nothing is a source of truth
//! (ADR-0002).
//!
//! **A note is keyed by its vault-relative path** (ADR-0003): `notes.path` is the
//! primary key and every child references it `ON DELETE CASCADE ON UPDATE CASCADE`.
//! The update half is what makes a B2-performed move a **re-key** rather than a
//! rebuild — one `UPDATE notes SET path` carries every derived row with it. It needs
//! `PRAGMA foreign_keys = ON`, set on every connection alongside `WAL`.
//!
//! Vectors live in **plain tables**, scored in-process, content-addressed by the
//! blake3 of the chunk text (ADR-0006). The one bookkeeping cost is that a vector no
//! longer dies with its chunk — [`prune_orphan_vectors`] collects what nothing
//! references, on the same derived-data lifecycle as centroids.

use crate::chunk::Chunk;
use crate::embed::pack_f32;
use crate::error::{Error, Result};
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::{
    params, Connection, OptionalExtension, StatementStatus, Transaction, TransactionBehavior,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// The B2 index schema version stamped into `meta.schema_version`. Bumping it is the
/// migration gate: on a mismatch `migrate()` drops the derived tables and lets the
/// next `reindex` rebuild them — there are no migrations, by design (ADR-0002).
///
/// **2** dropped the suggestion machinery with the 2026-07-04 relator cut. **3**
/// replaced the `chunks_vec` vec0 virtual table with plain vector tables (ADR-0006);
/// a pre-3 index's orphaned `chunks_vec` entry stays inert in `sqlite_master`,
/// because its module is no longer linked to drop it. **4** added the `resources`
/// inventory and widened `edges` with resource targets. **5** switched `chunks_fts`
/// to `porter unicode61` (the GH #157 A/B's verdict). **6** re-keyed the whole index
/// on the vault-relative path and made `embeddings` content-addressed (GH #170).
pub const SCHEMA_VERSION: i64 = 6;

/// Statements at or over this take the slow-query WARN path (`B2_SLOW_QUERY_MS`
/// overrides; see [`slow_query_threshold`]).
const SLOW_QUERY_MS_DEFAULT: u64 = 100;

/// The duration at or above which a statement logs as a **slow query** (WARN instead
/// of DEBUG), read once from `B2_SLOW_QUERY_MS`. Observability config only — it never
/// changes what an operation computes.
fn slow_query_threshold() -> Duration {
    static THRESHOLD: OnceLock<Duration> = OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        let ms = std::env::var("B2_SLOW_QUERY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(SLOW_QUERY_MS_DEFAULT);
        Duration::from_millis(ms)
    })
}

/// Whether a finished statement is worth the string work in [`on_sqlite_profile`]:
/// true only when something would receive the event.
///
/// Split out because the `slow && warn` term is load-bearing: drop it (the tempting
/// "just check DEBUG" simplification) and slow-query WARNs silently disappear for a
/// WARN-only subscriber. The unit test below is the truth table.
fn should_emit(slow: bool, warn_enabled: bool, debug_enabled: bool) -> bool {
    (slow && warn_enabled) || debug_enabled
}

/// SQLite's own per-statement profiler (`sqlite3_trace_v2`), surfaced as structured
/// `tracing` events on target `b2::sqlite`: the SQL **template** (`?N` placeholders,
/// never bound values, so no note content lands in the log and events group by
/// statement), `duration_us`, and the `vm_steps`/`fullscan_steps` counters — the "why
/// was it slow" signal, since a high fullscan count means a missing index. At or over
/// [`slow_query_threshold`] it logs at WARN, otherwise DEBUG.
///
/// **`duration_us` precision is platform-bound** — some platforms (macOS observed)
/// quantize SQLite's profiler clock to ~1ms, so sub-millisecond statements read as
/// `0`. For fine-grained cost use `vm_steps`: VDBE opcodes, deterministic and
/// clock-independent.
///
/// A plain `fn` because `trace_v2` registers a function pointer.
fn on_sqlite_profile(event: TraceEvent<'_>) {
    let TraceEvent::Profile(stmt, elapsed) = event else {
        return; // only SQLITE_TRACE_PROFILE is masked in, but TraceEvent is non-exhaustive
    };
    let slow = elapsed >= slow_query_threshold();
    // Skip the string work when nobody is listening at the level this would emit at.
    if !should_emit(
        slow,
        tracing::enabled!(target: "b2::sqlite", tracing::Level::WARN),
        tracing::enabled!(target: "b2::sqlite", tracing::Level::DEBUG),
    ) {
        return;
    }
    // Collapse the multi-line SQL literals used in this file to one line, so each
    // event stays a single clean record with a stable, groupable `sql` key.
    let sql = stmt.sql().split_whitespace().collect::<Vec<_>>().join(" ");
    let duration_us = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
    let vm_steps = stmt.get_status(StatementStatus::VmStep);
    let fullscan_steps = stmt.get_status(StatementStatus::FullscanStep);
    if slow {
        tracing::warn!(
            target: "b2::sqlite",
            sql, duration_us, vm_steps, fullscan_steps, slow,
            "slow sqlite query"
        );
    } else {
        tracing::debug!(
            target: "b2::sqlite",
            sql, duration_us, vm_steps, fullscan_steps, slow,
            "sqlite query"
        );
    }
}

/// Open (creating if needed) the B2 index at `path` with the locked pragmas and an
/// idempotent migration. Safe to call on a fresh or an already-built index.
pub fn open(path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    // Profile every statement through SQLite's trace_v2 hook (`on_sqlite_profile`).
    conn.trace_v2(
        TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(on_sqlite_profile),
    );
    // execute_batch tolerates the row PRAGMA mmap_size returns.
    // busy_timeout: WAL allows one writer at a time, and two short-statement writers
    // can legitimately race (a save during the background embed); a modest wait turns
    // that into a few-ms stall instead of SQLITE_BUSY. Set explicitly rather than
    // leaned on — rusqlite arms the same 5 s by default, but that is its contract.
    // mmap_size + cache_size: whole-space vector scans stream ~100+ MB of blob rows
    // per call on a real vault, which under the 2 MB default cache was syscall-bound
    // (the bulk of `b2 similar`'s ~4.4 s, #38). mmap_size is a *cap*, not an
    // allocation; cache_size is KiB when negative (32 MiB).
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;
         PRAGMA mmap_size = 1073741824;
         PRAGMA cache_size = -32768;",
    )?;
    // Separate, and retried, because the busy timeout above does not cover it (#111).
    enter_wal_mode(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// How many times [`enter_wal_mode`] attempts the flip before surfacing the busy error.
/// Large, because these retries *stand in for* `busy_timeout` — the one statement it does
/// not cover (ADR-0021) — so the whole ≈4 s wait budget is here.
const WAL_FLIP_ATTEMPTS: u32 = 16;

/// How many times the DDL rebuilds re-attempt their `BEGIN IMMEDIATE`. Small where the
/// flip's is large, and for the opposite reason: these sit *on top of* `busy_timeout`, so
/// each attempt already waits its full 5 s. Past ≈15 s it is a stuck writer (ADR-0021).
const REBUILD_ATTEMPTS: u32 = 3;

/// The pause schedule [`retry_while_locked`] uses between attempts.
const LOCK_RETRY_BACKOFF_START: Duration = Duration::from_millis(2);
const LOCK_RETRY_BACKOFF_MAX: Duration = Duration::from_millis(500);

/// Run `op`, retrying while SQLite reports the lock it wants is held by someone else
/// — the one failure in this module that is a *race* rather than a fault. `what` names
/// the contended step for the log line only.
///
/// The backoff sleeps but reads no clock and decides nothing from one, so the core's
/// determinism is untouched. Both callers hand it an operation that is idempotent and
/// self-checking: a retry re-reads the state it is about to change, so the attempt
/// after a lost race finds the work already done.
fn retry_while_locked<T>(
    what: &str,
    attempts: u32,
    mut op: impl FnMut() -> Result<T>,
) -> Result<T> {
    let mut backoff = LOCK_RETRY_BACKOFF_START;
    let mut attempt = 1;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            // Out of budget, or not contention at all — surface it.
            Err(e) if attempt >= attempts || !is_locked(&e) => return Err(e),
            Err(_) => {
                tracing::debug!(
                    target: "b2::sqlite",
                    what, attempt, backoff_ms = backoff.as_millis() as u64,
                    "contended by a concurrent opener; retrying"
                );
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(LOCK_RETRY_BACKOFF_MAX);
                attempt += 1;
            }
        }
    }
}

/// Put the connection in WAL mode, waiting out a concurrent opener (ADR-0021).
///
/// `journal_mode = WAL` is the one statement in [`open`] that takes a write lock, and only
/// when it actually *changes* the mode — so once per vault, ever — and it is the one
/// `busy_timeout` cannot cover, so the retry is ours ([`retry_while_locked`]). It converges
/// fast: the next attempt either takes the lock or finds the database already in WAL.
///
/// **The mode is read back, not assumed.** A filesystem with no shared-memory support
/// declines the flip with `SQLITE_OK` and the *old* mode, which a row-discarding
/// `execute_batch` would report as success. A decline is not an error — B2 is correct in
/// rollback-journal mode — but it is said out loud, and not retried: nothing holds a lock,
/// so waiting changes nothing.
fn enter_wal_mode(conn: &Connection) -> Result<()> {
    // A *declined* flip comes back as `Ok` carrying the old mode, so it leaves the
    // retry immediately.
    let mode = retry_while_locked("journal_mode=WAL", WAL_FLIP_ATTEMPTS, || {
        Ok(conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))?)
    })?;
    if !mode.eq_ignore_ascii_case("wal") {
        tracing::warn!(
            target: "b2::sqlite",
            journal_mode = %mode,
            "this filesystem does not support WAL; the index stays in rollback-journal mode"
        );
    }
    Ok(())
}

/// Whether an error is SQLite lock contention — the retryable kind. `SQLITE_LOCKED`
/// is matched alongside `SQLITE_BUSY` because the desktop host opens the index from
/// more than one thread.
fn is_locked(err: &Error) -> bool {
    matches!(err, Error::Sqlite(e) if matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ))
}

/// The tables [`apply_schema`] creates — the structural half of [`schema_is_current`].
///
/// Tables only, and that is sufficient rather than lazy: this guards against a
/// concurrent `DROP TABLE` (#114), and dropping a table takes its indexes and triggers
/// with it, so a missing table is the visible edge of every partial rebuild. The unit
/// test at the foot of this file pins the list to what the DDL actually creates.
const SCHEMA_TABLES: [&str; 7] = [
    "meta",
    "notes",
    "note_aliases",
    "chunks",
    "chunks_fts",
    "resources",
    "edges",
];

/// Bring the index to [`SCHEMA_VERSION`] — **atomically, and serialized against every other
/// opener** on SQLite's own write lock (ADR-0021, which carries the measured races and why
/// an advisory lock file was rejected).
///
/// Three properties, each load-bearing. **Serialized:** `migrate` reads, decides, then
/// writes, so two openers that both read a stale version both rebuild and their ~30 DDL
/// statements interleave. **Atomic:** the drop-and-rebuild runs in one transaction, so the
/// stamp and the tables it vouches for commit together, which is what lets
/// [`schema_is_current`] trust it. **The fast path takes no write lock at all**, which is
/// what keeps C1's "a reader is never refused" true: an already-current index costs two
/// reads instead of the ~30 `IF NOT EXISTS` statements it used to re-run on every open.
fn migrate(conn: &mut Connection) -> Result<()> {
    if schema_is_current(conn)? {
        return Ok(());
    }
    retry_while_locked("schema migration", REBUILD_ATTEMPTS, || {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Re-checked *inside* the write lock, which is the whole point of taking it:
        // if we lost the race we waited on the winner's transaction, and this is where
        // we find its work already committed.
        if !schema_is_current(&tx)? {
            apply_schema(&tx)?;
        }
        tx.commit()?;
        Ok(())
    })
}

/// Whether this connection sees a **complete** schema at the current
/// [`SCHEMA_VERSION`]: every table in [`SCHEMA_TABLES`] present *and* the stamp
/// current. Both halves are load-bearing — a current stamp over an incomplete schema
/// is precisely what #114 could leave behind, so an index damaged by an older `b2` is
/// detected here rather than far from its cause on the next `search`.
fn schema_is_current(conn: &Connection) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
    let present: HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    if !SCHEMA_TABLES.iter().all(|t| present.contains(*t)) {
        return Ok(false);
    }
    Ok(stamped_version(conn)? == Some(SCHEMA_VERSION))
}

/// The value stored in `meta` under `key`, or `None` when unset. Callers must
/// know `meta` exists — every caller reads it past a check that implies it
/// (a table-presence check, or the embed pass having ensured the space).
fn meta_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

/// The `schema_version` recorded in `meta`, or `None` on an index that has never been
/// stamped — or whose stamp was lost, which [`apply_schema`] treats the same way.
fn stamped_version(conn: &Connection) -> Result<Option<i64>> {
    Ok(meta_value(conn, "schema_version")?.and_then(|s| s.parse().ok()))
}

/// Create the schema and stamp `schema_version`, dropping whatever was there first.
/// The DDL mirrors `index-engine.md`; the vector tables are created at embed time
/// instead (ADR-0006, [`ensure_embedding_space`]).
///
/// **One outcome, whatever it finds:** an empty schema at the current version.
/// Reaching here means [`schema_is_current`] said no, and every way it can say no —
/// wrong shape, structurally incomplete (#114), or present but unstamped — is an index
/// of no known version. So the drop is unconditional: guarding it on "was there a prior
/// stamp?" reads like an optimization, but `DROP TABLE IF EXISTS` over an empty catalog
/// is already a no-op, and all the guard would do is wave through the
/// unstamped-but-populated index. Dropping is safe because the index is disposable
/// (ADR-0002), and rows surviving in tables of unknown shape are worse than none: an
/// incremental reindex skips notes whose `body_hash` still matches, so it would leave
/// the recreated tables empty forever.
///
/// **Called only from inside [`migrate`]'s transaction**, which is what makes
/// drop-then-create safe; it is not a standalone entry point.
fn apply_schema(conn: &Connection) -> Result<()> {
    // `meta` must exist before the batch below can clear it.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    // Children first (FKs); dropping `chunks` takes its FTS triggers with it. The
    // legacy vec0 `chunks_vec` (schema <= 2) is deliberately absent: its module is no
    // longer linked, so SQLite cannot DROP it. `DELETE FROM meta` clears the recorded
    // embedder, so the next embed pass recreates the vector tables from nothing.
    conn.execute_batch(
        "DROP TABLE IF EXISTS edge_provenance;
         DROP TABLE IF EXISTS edges;
         DROP TABLE IF EXISTS resources;
         DROP TABLE IF EXISTS note_centroids;
         DROP TABLE IF EXISTS embeddings;
         DROP TABLE IF EXISTS chunks_fts;
         DROP TABLE IF EXISTS chunks;
         DROP TABLE IF EXISTS note_aliases;
         DROP TABLE IF EXISTS notes;
         DELETE FROM meta;",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
           path        TEXT PRIMARY KEY,
           type        TEXT NOT NULL,
           title       TEXT,
           description TEXT,
           created     TEXT,
           updated     TEXT,
           body_hash   TEXT NOT NULL,
           mtime       INTEGER,
           indexed_at  TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS notes_type_idx ON notes(type);

         CREATE TABLE IF NOT EXISTS note_aliases (
           note_path TEXT NOT NULL
                       REFERENCES notes(path) ON DELETE CASCADE ON UPDATE CASCADE,
           alias     TEXT NOT NULL,
           PRIMARY KEY (note_path, alias)
         );
         CREATE INDEX IF NOT EXISTS note_aliases_alias_idx ON note_aliases(alias);

         CREATE TABLE IF NOT EXISTS chunks (
           id           INTEGER PRIMARY KEY,
           note_path    TEXT NOT NULL
                          REFERENCES notes(path) ON DELETE CASCADE ON UPDATE CASCADE,
           seq          INTEGER NOT NULL,
           char_start   INTEGER NOT NULL,
           char_end     INTEGER NOT NULL,
           token_count  INTEGER NOT NULL,
           heading_path TEXT,
           text         TEXT NOT NULL,
           text_hash    TEXT NOT NULL,
           UNIQUE (note_path, seq)
         );
         CREATE INDEX IF NOT EXISTS chunks_note_idx ON chunks(note_path);
         CREATE INDEX IF NOT EXISTS chunks_text_hash_idx ON chunks(text_hash);

         CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
           text,
           content       = 'chunks',
           content_rowid = 'id',
           tokenize      = 'porter unicode61'
         );
         CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
           INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
           INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
         END;
         CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
           INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
           INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
         END;

         CREATE TABLE IF NOT EXISTS resources (
           path         TEXT PRIMARY KEY,
           class        TEXT NOT NULL CHECK (class IN
                          ('text','html','pdf','image','media','binary')),
           size         INTEGER NOT NULL,
           mtime        INTEGER,
           content_hash TEXT NOT NULL,
           indexed_at   TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS resources_class_idx ON resources(class);

         CREATE TABLE IF NOT EXISTS edges (
           id                TEXT PRIMARY KEY,
           src_path          TEXT NOT NULL
                               REFERENCES notes(path) ON DELETE CASCADE ON UPDATE CASCADE,
           dst_path          TEXT,
           dst_resource_path TEXT REFERENCES resources(path) ON DELETE SET NULL,
           dst_path_raw      TEXT NOT NULL,
           type              TEXT NOT NULL,
           origin            TEXT NOT NULL CHECK (origin IN ('inline','frontmatter')),
           explanation       TEXT,
           embed             INTEGER NOT NULL DEFAULT 0,
           caption           TEXT,
           occurrence_index  INTEGER NOT NULL DEFAULT 0,
           UNIQUE (src_path, dst_path, type, occurrence_index)
         );
         CREATE INDEX IF NOT EXISTS edges_src_idx      ON edges(src_path);
         CREATE INDEX IF NOT EXISTS edges_dst_type_idx ON edges(dst_path, type);
         CREATE INDEX IF NOT EXISTS edges_dst_resource_idx ON edges(dst_resource_path)
           WHERE dst_resource_path IS NOT NULL;
         CREATE UNIQUE INDEX IF NOT EXISTS edges_resource_unique_idx
           ON edges(src_path, dst_resource_path, type, occurrence_index)
           WHERE dst_resource_path IS NOT NULL;
         CREATE INDEX IF NOT EXISTS edges_dangling_idx ON edges(dst_path_raw)
           WHERE dst_path IS NULL AND dst_resource_path IS NULL;",
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// notes + aliases
// ---------------------------------------------------------------------------

/// One note's projection into `notes` (+ its `aliases`). Borrowed view so callers
/// pass slices of an already-parsed note without extra allocation.
pub struct NoteRow<'a> {
    pub path: &'a str,
    pub r#type: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub created: Option<&'a str>,
    pub updated: Option<&'a str>,
    pub body_hash: &'a str,
    pub mtime: Option<i64>,
    pub aliases: &'a [String],
}

/// Upsert a note keyed by its vault-relative `path` and replace its aliases.
/// `indexed_at` is set by SQLite so the projection needs no wall-clock from Rust.
///
/// `ON CONFLICT(path)` is the *whole* of path reconciliation, which is the point of
/// keying on the path (ADR-0003): the filesystem already guarantees one file per path,
/// so a note deleted and recreated there is simply that path's note now.
pub fn upsert_note(conn: &Connection, row: &NoteRow) -> Result<()> {
    conn.execute(
        "INSERT INTO notes
           (path, type, title, description, created, updated, body_hash, mtime, indexed_at)
         VALUES
           (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(path) DO UPDATE SET
           type        = excluded.type,
           title       = excluded.title,
           description = excluded.description,
           created     = excluded.created,
           updated     = excluded.updated,
           body_hash   = excluded.body_hash,
           mtime       = excluded.mtime,
           indexed_at  = excluded.indexed_at",
        params![
            row.path,
            row.r#type,
            row.title,
            row.description,
            row.created,
            row.updated,
            row.body_hash,
            row.mtime,
        ],
    )?;
    conn.execute("DELETE FROM note_aliases WHERE note_path = ?1", [row.path])?;
    for alias in row.aliases {
        conn.execute(
            "INSERT OR IGNORE INTO note_aliases(note_path, alias) VALUES (?1, ?2)",
            params![row.path, alias],
        )?;
    }
    Ok(())
}

/// Delete every `notes` row whose path is not in `seen` — the paths the whole-vault
/// walk actually met, including ones skipped as unreadable (the walk *saw* that file,
/// so evicting it would lie). Returns how many were pruned: the note half of #31,
/// without which a file deleted outside `b2` leaves a ghost row that listings, search,
/// `similar` and the graph keep serving, so an incremental reindex diverges from a
/// from-scratch rebuild (S3).
///
/// Aliases, chunks (FTS in lockstep via the `chunks_ad` trigger), centroid and
/// **outgoing** edges cascade with the row. Vectors no longer do — they are
/// content-addressed and may be shared, so [`prune_orphan_vectors`] collects them.
/// **Inbound** edges are the caller's concern: `edges.dst_path` carries no FK (it must
/// be free to be NULL — the dangling case), so this must run *before* edge derivation,
/// which then re-dangles the links that pointed here.
pub fn prune_notes_except(conn: &Connection, seen: &HashSet<&str>) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT path FROM notes")?;
    let stored = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut pruned = 0;
    for path in stored {
        if !seen.contains(path.as_str()) {
            pruned += conn.execute("DELETE FROM notes WHERE path = ?1", [&path])?;
        }
    }
    Ok(pruned)
}

// ---------------------------------------------------------------------------
// resources (file-type support slice 1 — data-model.md §10)
// ---------------------------------------------------------------------------

/// One resource's projection into `resources`. Borrowed view like [`NoteRow`] —
/// passed straight from the walk, never stored.
pub struct ResourceRow<'a> {
    pub path: &'a str,
    pub class: &'a str,
    pub size: i64,
    pub mtime: Option<i64>,
    pub content_hash: &'a str,
}

/// Upsert a resource keyed by its vault-relative path. `indexed_at` is set by
/// SQLite, like [`upsert_note`]'s — the projection needs no wall-clock from Rust.
pub fn upsert_resource(conn: &Connection, row: &ResourceRow) -> Result<()> {
    conn.execute(
        "INSERT INTO resources (path, class, size, mtime, content_hash, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(path) DO UPDATE SET
           class        = excluded.class,
           size         = excluded.size,
           mtime        = excluded.mtime,
           content_hash = excluded.content_hash,
           indexed_at   = excluded.indexed_at",
        params![row.path, row.class, row.size, row.mtime, row.content_hash],
    )?;
    Ok(())
}

/// The stored `(size, mtime)` for an inventoried resource — the change-detection
/// short-circuit: a matching stat means the bytes are not re-read or re-hashed
/// (hashing is the only byte-read the inventory pass performs).
pub fn resource_stat(conn: &Connection, path: &str) -> Result<Option<(i64, Option<i64>)>> {
    Ok(conn
        .query_row(
            "SELECT size, mtime FROM resources WHERE path = ?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// One `list_resources` row — a resource's identity + stat for the file tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceListing {
    pub path: String,
    pub class: String,
    pub size: i64,
    pub mtime: Option<i64>,
}

/// One resource's full inventory row (`resource_detail`) — the fallback card's
/// metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDetail {
    pub class: String,
    pub size: i64,
    pub mtime: Option<i64>,
    pub content_hash: String,
}

/// Every inventoried resource — [`ResourceListing`] rows, path-ordered — the
/// file tree's resource half (`Vault::list_resources`, research §9b #10).
pub fn list_resources(conn: &Connection) -> Result<Vec<ResourceListing>> {
    let mut stmt = conn.prepare("SELECT path, class, size, mtime FROM resources ORDER BY path")?;
    let rows = stmt.query_map([], |r| {
        Ok(ResourceListing {
            path: r.get(0)?,
            class: r.get(1)?,
            size: r.get(2)?,
            mtime: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One resource's full inventory row — [`ResourceDetail`] — the fallback card's
/// metadata. `None` when the path is not inventoried.
pub fn resource_detail(conn: &Connection, path: &str) -> Result<Option<ResourceDetail>> {
    Ok(conn
        .query_row(
            "SELECT class, size, mtime, content_hash FROM resources WHERE path = ?1",
            [path],
            |r| {
                Ok(ResourceDetail {
                    class: r.get(0)?,
                    size: r.get(1)?,
                    mtime: r.get(2)?,
                    content_hash: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// One edge pointing *at* a resource, resolved with its source note's display
/// fields — a row of the fallback card's backlinks panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBacklinkRow {
    pub note_path: String,
    pub note_title: Option<String>,
    pub r#type: String,
    pub caption: Option<String>,
    pub embed: bool,
}

/// Every active edge pointing *at* the resource: the source note's identity plus
/// the edge's `type`/`caption`/`embed` — the fallback card's backlinks panel,
/// straight off the materialized graph. Ordered for deterministic display.
pub fn inbound_resource_edges(conn: &Connection, path: &str) -> Result<Vec<ResourceBacklinkRow>> {
    let mut stmt = conn.prepare(
        "SELECT n.path, n.title, e.type, e.caption, e.embed
         FROM edges e JOIN notes n ON n.path = e.src_path
         WHERE e.dst_resource_path = ?1
         ORDER BY n.path, e.occurrence_index",
    )?;
    let rows = stmt.query_map([path], |r| {
        Ok(ResourceBacklinkRow {
            note_path: r.get(0)?,
            note_title: r.get(1)?,
            r#type: r.get(2)?,
            caption: r.get(3)?,
            embed: r.get::<_, i64>(4)? != 0,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One edge a note points *at a resource*, joined with the inventory's `class`
/// (the display glyph) — a row of `explain`'s file-links panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEdgeRow {
    pub path: String,
    pub class: String,
    pub r#type: String,
    pub origin: String,
    pub caption: Option<String>,
    pub embed: bool,
    pub explanation: Option<String>,
}

/// Every active edge a note points *at a resource* — the outbound complement of
/// [`inbound_resource_edges`], so `explain` can present all three target kinds a
/// note authors (note / resource / dangling — GH #22) instead of silently hiding
/// its file links. Ordered for deterministic display.
pub fn outbound_resource_edges(conn: &Connection, note_path: &str) -> Result<Vec<ResourceEdgeRow>> {
    let mut stmt = conn.prepare(
        "SELECT e.dst_resource_path, r.class, e.type, e.origin, e.caption, e.embed, e.explanation
         FROM edges e JOIN resources r ON r.path = e.dst_resource_path
         WHERE e.src_path = ?1 AND e.dst_resource_path IS NOT NULL
         ORDER BY e.dst_resource_path, e.type, e.occurrence_index",
    )?;
    let rows = stmt.query_map([note_path], |r| {
        Ok(ResourceEdgeRow {
            path: r.get(0)?,
            class: r.get(1)?,
            r#type: r.get(2)?,
            origin: r.get(3)?,
            caption: r.get(4)?,
            embed: r.get::<_, i64>(5)? != 0,
            explanation: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The bounded inbound set a **resource move** must rewrite — [`InboundEdge`]
/// rows. The resource sibling of [`inbound_edge_targets`]; ordered for
/// deterministic rewriting.
pub fn inbound_resource_edge_targets(conn: &Connection, path: &str) -> Result<Vec<InboundEdge>> {
    let mut stmt = conn.prepare(
        "SELECT e.src_path, e.dst_path_raw
         FROM edges e
         WHERE e.dst_resource_path = ?1
         ORDER BY e.src_path, e.dst_path_raw",
    )?;
    let rows = stmt.query_map([path], |r| {
        Ok(InboundEdge {
            src_path: r.get(0)?,
            dst_raw: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Delete every `resources` row whose path is not in `seen` (the walk's survivors)
/// and return how many were pruned. Inbound edges **re-dangle** automatically —
/// `edges.dst_resource_path` is `ON DELETE SET NULL`, `dst_path_raw` retained —
/// so a stale inventory row never outlives its file (the resource half of #31).
pub fn prune_resources_except(conn: &Connection, seen: &HashSet<String>) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT path FROM resources")?;
    let stored = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut pruned = 0;
    for path in stored {
        if !seen.contains(&path) {
            pruned += conn.execute("DELETE FROM resources WHERE path = ?1", [&path])?;
        }
    }
    Ok(pruned)
}

// ---------------------------------------------------------------------------
// chunks (FTS kept in lockstep by the triggers in migrate())
// ---------------------------------------------------------------------------

/// The content address of one chunk's embed input: blake3 of the chunk text. The text
/// stored on the row *is* what the embedder is handed (`chunk.rs` folds the heading
/// breadcrumb in before it lands here), so this hash keys the vector store exactly
/// (ADR-0006) — two chunks with byte-identical text must have the same vector, which is
/// a correctness statement before it is a saving.
///
/// The model identity is deliberately *not* mixed in: a model swap drops the whole
/// `embeddings` table (ADR-0007), so the space is per-model by construction and a wider
/// key would only make that drop look optional.
pub fn text_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

/// Replace a note's chunks (delete + reinsert) and return the new chunk ids in `seq`
/// order; the FTS triggers emit the `'delete'` sentinel for the removed rows.
///
/// Stored vectors do **not** cascade — they are content-addressed and may be shared,
/// so they outlive any one chunk row and the whole-vault pass collects what nothing
/// references (ADR-0006, [`prune_orphan_vectors`]). That is exactly what makes a
/// re-chunk (or a move) re-embed nothing when the text is unchanged. The note's
/// centroid summarizes the *old* chunk set, so it is dropped here and the next embed
/// pass recomputes it. The caller embeds the returned ids.
pub fn replace_chunks(conn: &Connection, note_path: &str, chunks: &[Chunk]) -> Result<Vec<i64>> {
    // Guarded on existence so the model-free projection pass still never *creates*
    // the embedding space (index-engine.md).
    if embedding_space_exists(conn)? {
        conn.execute(
            "DELETE FROM note_centroids WHERE note_path = ?1",
            [note_path],
        )?;
    }
    conn.execute("DELETE FROM chunks WHERE note_path = ?1", [note_path])?;

    let mut new_ids = Vec::with_capacity(chunks.len());
    for c in chunks {
        conn.execute(
            "INSERT INTO chunks
               (note_path, seq, char_start, char_end, token_count, heading_path, text, text_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                note_path,
                c.seq as i64,
                c.char_start as i64,
                c.char_end as i64,
                c.token_count as i64,
                c.heading_path,
                c.text,
                text_hash(&c.text),
            ],
        )?;
        new_ids.push(conn.last_insert_rowid());
    }
    Ok(new_ids)
}

/// The closed set of tokenizers `chunks_fts` can be rebuilt with — an enum, so the
/// string spliced into [`rebuild_fts`]'s DDL is never caller-supplied text.
/// `PorterUnicode61` is the shipped default (the GH #157 verdict); `Unicode61` is the
/// unstemmed ablation arm the eval harness keeps measurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtsTokenizer {
    Unicode61,
    PorterUnicode61,
}

impl FtsTokenizer {
    /// The FTS5 `tokenize =` value this variant names — public so the eval's
    /// recorded rows spell the tokenizer exactly as the schema does.
    pub fn sql(self) -> &'static str {
        match self {
            FtsTokenizer::Unicode61 => "unicode61",
            FtsTokenizer::PorterUnicode61 => "porter unicode61",
        }
    }
}

/// The tokenizer `chunks_fts` was actually created with, read back from the schema
/// rather than assumed — because it *moves*: [`rebuild_fts`] swaps it under the GH #157
/// ablation, so a caller that must tokenize a query the way the index does (see
/// [`search::lexical_evidence`](crate::search::lexical_evidence)) has to ask.
///
/// The recorded value is matched against the closed [`FtsTokenizer`] set, never spliced
/// onward as text. An unreadable or unrecognised schema degrades to the shipped
/// default, which is the one `migrate` creates.
pub fn index_tokenizer(conn: &Connection) -> Result<FtsTokenizer> {
    const DEFAULT: FtsTokenizer = FtsTokenizer::PorterUnicode61;
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'chunks_fts'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let Some(sql) = sql else {
        return Ok(DEFAULT);
    };
    // Longest spelling first: "unicode61" is a substring of "porter unicode61",
    // so a shortest-first scan would read every stemmed index as unstemmed.
    let mut known = [FtsTokenizer::Unicode61, FtsTokenizer::PorterUnicode61];
    known.sort_by_key(|t| std::cmp::Reverse(t.sql().len()));
    Ok(known
        .into_iter()
        .find(|t| sql.contains(t.sql()))
        .unwrap_or(DEFAULT))
}

/// Drop and recreate `chunks_fts` with `tokenizer`, repopulated from the untouched
/// `chunks` content table (FTS5's external-content `'rebuild'`). Chunk rows, vectors
/// and centroids are untouched — the tokenizer only changes how the lexical half
/// indexes the same text, which is what makes the GH #157 stemmer A/B runnable without
/// re-chunking or re-embedding.
///
/// A drop-and-rebuild like the migration, so it takes the same write-lock discipline;
/// the op is idempotent, so an attempt after a lost race lands on the same end state.
/// The `chunks_*` triggers live on `chunks` and reference this table by name, so they
/// survive the swap.
pub fn rebuild_fts(conn: &Connection, tokenizer: FtsTokenizer) -> Result<()> {
    retry_while_locked("chunks_fts rebuild", REBUILD_ATTEMPTS, || {
        // `new_unchecked` for the same reason as `ensure_embedding_space`: this takes
        // `&Connection`, and nothing in this crate can already be inside a transaction.
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        tx.execute_batch(&format!(
            "DROP TABLE IF EXISTS chunks_fts;
             CREATE VIRTUAL TABLE chunks_fts USING fts5(
               text,
               content       = 'chunks',
               content_rowid = 'id',
               tokenize      = '{}'
             );
             INSERT INTO chunks_fts(chunks_fts) VALUES ('rebuild');",
            tokenizer.sql()
        ))?;
        tx.commit()?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// embeddings — the vector tables are created at embed time, not in migrate():
// their *existence* is the "this vault has an embedding space" signal the
// projected-but-unembedded fallbacks key on (ADR-0006).
// ---------------------------------------------------------------------------

/// Whether the embedding space (the `embeddings` table) currently exists.
pub fn embedding_space_exists(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'embeddings'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Ensure the vector tables exist, recording `(embed_model_id, embed_dim)` in `meta`. If
/// either differs from what is recorded — a model swap — the tables are dropped and
/// recreated empty, so a full re-embed follows: `meta` is the only place a swap is
/// detectable, so vectors never go silently stale (ADR-0007). (`dim` is bookkeeping only now
/// that vectors are plain BLOBs, but it still gates the swap and the read-time fail-fast.)
///
/// **Serialized and atomic on the same terms as [`migrate`]** (ADR-0021): two embed passes
/// genuinely overlap — the desktop's reindex task and a `b2 reindex`, which the CLI's own
/// advisory lock does not cover — and B's `DROP` after A's `CREATE` leaves A inserting into
/// a table that no longer exists.
pub fn ensure_embedding_space(conn: &Connection, model_id: &str, dim: usize) -> Result<()> {
    if embedding_space_matches(conn, model_id, dim)? {
        return Ok(());
    }
    retry_while_locked("embedding-space rebuild", REBUILD_ATTEMPTS, || {
        // `new_unchecked` because this takes `&Connection`, not `&mut` — the embed
        // pass threads a shared connection through. Sound because `b2-core` opens no
        // other transaction on it.
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        if !embedding_space_matches(&tx, model_id, dim)? {
            tx.execute_batch(
                "DROP TABLE IF EXISTS note_centroids;
                 DROP TABLE IF EXISTS embeddings;
                 CREATE TABLE embeddings (
                   text_hash TEXT PRIMARY KEY,
                   vector    BLOB NOT NULL
                 );
                 CREATE TABLE note_centroids (
                   note_path TEXT PRIMARY KEY
                               REFERENCES notes(path) ON DELETE CASCADE ON UPDATE CASCADE,
                   centroid  BLOB NOT NULL
                 );",
            )?;
            upsert_meta(&tx, "embed_model_id", model_id)?;
            upsert_meta(&tx, "embed_dim", &dim.to_string())?;
        }
        tx.commit()?;
        Ok(())
    })
}

/// Whether the vector tables exist *and* were built by this exact embedder identity —
/// the "nothing to do" test [`ensure_embedding_space`] runs twice, once cheaply and
/// once under the write lock. Identity first: a differing model settles it without the
/// `sqlite_master` lookup.
fn embedding_space_matches(conn: &Connection, model_id: &str, dim: usize) -> Result<bool> {
    let unchanged = meta_value(conn, "embed_model_id")?.as_deref() == Some(model_id)
        && meta_value(conn, "embed_dim")?.as_deref() == Some(dim.to_string().as_str());
    Ok(unchanged && embedding_space_exists(conn)?)
}

/// The `(embed_model_id, embed_dim)` a prior ingest recorded, if any; `None` means the
/// vault has never been embedded. The only place a model swap is detectable, so a read
/// compares it to the active embedder and fails fast on a mismatch (ADR-0007).
pub fn recorded_embedder(conn: &Connection) -> Result<Option<(String, usize)>> {
    let model = meta_value(conn, "embed_model_id")?;
    let dim = meta_value(conn, "embed_dim")?;
    match (model, dim) {
        (Some(m), Some(d)) => Ok(Some((m, d.parse().unwrap_or(0)))),
        _ => Ok(None),
    }
}

fn upsert_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Store one embedding under the content address of the text it came from. `OR IGNORE`
/// because the store is shared: two notes with byte-identical chunk text address the
/// same row, and the second writer is agreeing with the first. That also makes a
/// resumed or overlapping embed pass idempotent.
pub fn set_vector(conn: &Connection, text_hash: &str, embedding: &[f32]) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO embeddings(text_hash, vector) VALUES (?1, ?2)",
        params![text_hash, pack_f32(embedding)],
    )?;
    Ok(())
}

/// Delete every stored vector no chunk references — the collection pass
/// content-addressing costs us, run by the whole-vault projection once this run's chunk
/// set is final.
///
/// A vector cannot be dropped with its chunk precisely because it may be shared, and
/// that sharing is the point: a moved note's chunks are deleted and re-inserted under a
/// new path, and their vectors have to survive the gap (ADR-0006). So the lifecycle is
/// the centroids' — derived data reconciled by the pass that knows the whole picture,
/// never by a per-row rule that cannot. Requires the embedding space to exist.
pub fn prune_orphan_vectors(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM embeddings
         WHERE text_hash NOT IN (SELECT text_hash FROM chunks)",
        [],
    )?)
}

/// The note a chunk belongs to (the search-hit → note resolution).
pub fn note_for_chunk(conn: &Connection, chunk_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT note_path FROM chunks WHERE id = ?1",
            [chunk_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// The whole `chunk_id -> note_path` map in one scan — the bulk form of
/// [`note_for_chunk`] for loops that resolve *many* hits (graph-filtered search walks
/// the full ranked space, where a per-hit lookup is the N+1 shape that once made
/// `b2 similar` a ~130s stall, #37).
pub fn chunk_note_map(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut stmt = conn.prepare("SELECT id, note_path FROM chunks")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}

/// A chunk's text (None if the chunk id is unknown) — the search-hit → snippet
/// resolution the CLI shows.
pub fn chunk_text(conn: &Connection, chunk_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT text FROM chunks WHERE id = ?1", [chunk_id], |r| {
            r.get(0)
        })
        .optional()?)
}

/// A chunk's heading breadcrumb + text in one read (None if the chunk id is
/// unknown) — the chunk-level hit resolution (`Vault::search_chunks`).
pub fn chunk_detail(conn: &Connection, chunk_id: i64) -> Result<Option<(Option<String>, String)>> {
    Ok(conn
        .query_row(
            "SELECT heading_path, text FROM chunks WHERE id = ?1",
            [chunk_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// A note's stored body hash (None if the note isn't indexed yet). Read **before**
/// re-upserting so an incremental reindex can tell whether the body actually
/// changed and skip re-embedding an unchanged note.
pub fn note_body_hash(conn: &Connection, note_path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT body_hash FROM notes WHERE path = ?1",
            [note_path],
            |r| r.get(0),
        )
        .optional()?)
}

/// Whether every chunk of `note_path` already has a stored vector (and it has at least
/// one). False after a model swap emptied the vector tables, so an unchanged-body note
/// is still re-embedded then. Requires the embedding space to exist.
pub fn note_fully_embedded(conn: &Connection, note_path: &str) -> Result<bool> {
    let (n_chunks, n_missing): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE v.text_hash IS NULL)
         FROM chunks c LEFT JOIN embeddings v ON v.text_hash = c.text_hash
         WHERE c.note_path = ?1",
        [note_path],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(n_chunks > 0 && n_missing == 0)
}

/// The vault's embedding coverage as `(notes_embedded, notes_total)` — the honest
/// "N/M embedded" signal (#26). `notes_embedded` counts notes whose every chunk has a
/// stored vector; it is `0` before any embed, so this reads cleanly on a
/// projected-but-unembedded vault. **Model-free:** a pure count over the projection.
pub fn embed_progress(conn: &Connection) -> Result<(usize, usize)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
    // No embeddings table -> nothing is embedded, and the join below would reference a
    // missing table.
    if !embedding_space_exists(conn)? {
        return Ok((0, total as usize));
    }
    // A note counts as embedded iff it has >=1 chunk and none lack a vector — the same
    // predicate as `note_fully_embedded`, aggregated.
    let embedded: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notes n
         WHERE EXISTS (SELECT 1 FROM chunks c WHERE c.note_path = n.path)
           AND NOT EXISTS (
             SELECT 1 FROM chunks c
             LEFT JOIN embeddings v ON v.text_hash = c.text_hash
             WHERE c.note_path = n.path AND v.text_hash IS NULL
           )",
        [],
        |r| r.get(0),
    )?;
    Ok((embedded as usize, total as usize))
}

/// One chunk still lacking a stored vector — a row of the DB-derived pending set
/// ([`chunks_missing_vectors`]) the embed pass fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChunk {
    pub note_path: String,
    pub text: String,
    /// The content address the embed pass stores this vector under — carried on the
    /// row so the pass never re-hashes text SQLite already hashed at chunk time.
    pub text_hash: String,
}

/// Every chunk still lacking a stored vector, in `(path, seq)` order — the
/// **DB-derived pending set** the embed pass fills. Deriving it here is what decouples
/// projection from embedding: nothing is handed between the passes in memory, so any
/// stop point (a cancelled embed, a crash between passes) heals on the next embed. The
/// ordering reproduces the fused reindex's per-note batching and progress. Requires the
/// embedding space to exist.
///
/// A row is a *chunk*, not a distinct vector: two chunks sharing text appear twice,
/// because the caller needs to know which notes are waiting on work. Deduplicating the
/// embedder calls is [`crate::ingest::embed_vault`]'s job — it is the only layer that
/// knows the order the notes will be worked in.
pub fn chunks_missing_vectors(conn: &Connection) -> Result<Vec<PendingChunk>> {
    let mut stmt = conn.prepare(
        "SELECT c.note_path, c.text, c.text_hash
         FROM chunks c
         LEFT JOIN embeddings v ON v.text_hash = c.text_hash
         WHERE v.text_hash IS NULL
         ORDER BY c.note_path, c.seq",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(PendingChunk {
            note_path: r.get(0)?,
            text: r.get(1)?,
            text_hash: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// [`chunks_missing_vectors`] for **one** note, in `seq` order — what the inline
/// ingest path (`add`/`link`/`mv`) embeds after re-projecting a single file. Its own
/// indexed query rather than a filter over the whole-vault set: a directory move
/// re-projects every note it moved, and filtering there would be O(vault × moved).
pub fn note_chunks_missing_vectors(
    conn: &Connection,
    note_path: &str,
) -> Result<Vec<PendingChunk>> {
    let mut stmt = conn.prepare_cached(
        "SELECT c.note_path, c.text, c.text_hash
         FROM chunks c
         LEFT JOIN embeddings v ON v.text_hash = c.text_hash
         WHERE c.note_path = ?1 AND v.text_hash IS NULL
         ORDER BY c.seq",
    )?;
    let rows = stmt.query_map([note_path], |r| {
        Ok(PendingChunk {
            note_path: r.get(0)?,
            text: r.get(1)?,
            text_hash: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A note's `title` (None if the note is absent or has no title).
pub fn note_title(conn: &Connection, note_path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT title FROM notes WHERE path = ?1",
            [note_path],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// A note's `created` date (`None` if absent or unset), resolved from the
/// projection (GH #22): a neighbor is dated for display without an adapter ever
/// re-reading the file just for a date.
pub fn note_created(conn: &Connection, note_path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT created FROM notes WHERE path = ?1",
            [note_path],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// Every indexed note's `(path, title)`, ordered by `path` — the flat listing
/// the desktop UI's file tree is built from (`Vault::list_notes`). Path order means
/// the adapter can assemble the folder tree in one pass without re-sorting.
pub fn all_notes(conn: &Connection) -> Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare("SELECT path, title FROM notes ORDER BY path")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// A note's stored chunk vectors as `(chunk_id, vector)` in `seq` order — one indexed
/// join, not a per-chunk round-trip. Reading a note's own vectors back is what lets
/// discovery search from them without re-embedding (passage-to-passage, no
/// `embed_query`); it is also discovery's rescore unit and the input to a centroid
/// refresh. Call only when the embedding space exists. `prepare_cached` because
/// discovery calls this once per shortlisted note.
pub fn note_chunk_vectors(conn: &Connection, note_path: &str) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT c.id, e.vector FROM chunks c
         JOIN embeddings e ON e.text_hash = c.text_hash
         WHERE c.note_path = ?1 ORDER BY c.seq",
    )?;
    let rows = stmt.query_map([note_path], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            crate::embed::unpack_f32(&r.get::<_, Vec<u8>>(1)?),
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Recompute and store `note_path`'s centroid from its currently stored chunk vectors
/// (the row is deleted when it has none). The embed pass calls this after finishing a
/// note, so a centroid row exists exactly for embedded notes and always summarizes
/// their *current* vectors — derived data with no separate invalidation (ADR-0006).
pub fn refresh_note_centroid(conn: &Connection, note_path: &str) -> Result<()> {
    let vectors: Vec<Vec<f32>> = note_chunk_vectors(conn, note_path)?
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    match crate::embed::centroid_of(&vectors) {
        Some(c) => {
            conn.execute(
                "INSERT INTO note_centroids(note_path, centroid) VALUES (?1, ?2)
                 ON CONFLICT(note_path) DO UPDATE SET centroid = excluded.centroid",
                params![note_path, pack_f32(&c)],
            )?;
        }
        None => {
            conn.execute(
                "DELETE FROM note_centroids WHERE note_path = ?1",
                [note_path],
            )?;
        }
    }
    Ok(())
}

/// Every fully-embedded note with **no** centroid row, path-ordered — the second half
/// of the embed pass's work list (GH #170).
///
/// It exists because content-addressing broke a coupling the old design leaned on.
/// `replace_chunks` drops a note's centroid and used to drop its vectors too, so a
/// re-chunked note always had pending chunks and refreshing the centroid inside the
/// embed loop sufficed. Vectors now survive a re-chunk — that is the whole point — so a
/// note can reach the embed pass with a complete vector set, no centroid, and nothing
/// pending to bring it back. Silently: discovery's coarse stage scans centroids only,
/// so such a note would stop being discoverable while looking perfectly indexed
/// (S3, which is how the property suite caught it).
///
/// "Fully embedded" is the gate, not "has any vector": a centroid over a partial set
/// would be wrong rather than merely stale.
pub fn notes_missing_centroids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT c.note_path FROM chunks c
         WHERE NOT EXISTS (
                 SELECT 1 FROM note_centroids nc WHERE nc.note_path = c.note_path)
           AND NOT EXISTS (
                 SELECT 1 FROM chunks c2
                 LEFT JOIN embeddings e ON e.text_hash = c2.text_hash
                 WHERE c2.note_path = c.note_path AND e.text_hash IS NULL)
         ORDER BY c.note_path",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Stream every stored `(note_path, centroid_blob)` through `f`, one row at a time —
/// discovery's first-stage coarse scan. O(notes), the whole point of the two-stage
/// shape (#38): the O(chunks) work happens only for the shortlisted notes. The blob
/// is *borrowed* for the callback (`get_ref`), so scoring adds no per-row allocation.
pub fn for_each_note_centroid(conn: &Connection, mut f: impl FnMut(&str, &[u8])) -> Result<()> {
    let mut stmt = conn.prepare("SELECT note_path, centroid FROM note_centroids")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        // Match the ValueRefs rather than `.as_str()?` — their `FromSqlError` isn't in
        // our error enum, and the column types are fixed by our own DDL, so a
        // mismatched row is skipped rather than an error.
        let rusqlite::types::ValueRef::Text(text) = row.get_ref(0)? else {
            continue;
        };
        let Ok(note) = std::str::from_utf8(text) else {
            continue;
        };
        if let rusqlite::types::ValueRef::Blob(blob) = row.get_ref(1)? {
            f(note, blob);
        }
    }
    Ok(())
}

/// Stream every embedded chunk's `(chunk_id, vector_blob)` through `f`, one row at a
/// time — the scan behind every vector read, which never materializes the whole space
/// at once (ADR-0006). The blob is *borrowed* for the callback, so scoring adds no
/// per-row allocation.
///
/// Ranking is per **chunk**, so content-addressing has to be undone here: the join
/// hands each vector to every chunk that addresses it, which is why two notes with
/// identical text still get one rank each. A shared vector is read once and scored
/// twice rather than stored twice.
pub fn for_each_stored_vector(conn: &Connection, mut f: impl FnMut(i64, &[u8])) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT c.id, e.vector FROM embeddings e JOIN chunks c ON c.text_hash = e.text_hash",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let chunk_id: i64 = row.get(0)?;
        // Match the ValueRef rather than `as_blob()?` — its `FromSqlError` isn't in our
        // error enum, and a stored vector is always a Blob.
        if let rusqlite::types::ValueRef::Blob(blob) = row.get_ref(1)? {
            f(chunk_id, blob);
        }
    }
    Ok(())
}

/// Every chunk's squared-L2 distance to `query`, sorted nearest first (ties broken by
/// `chunk_id` for determinism) — the shared scan behind [`vector_search`] /
/// [`vector_search_all`], computed in-process over the [`for_each_stored_vector`]
/// stream: one sequential statement, one reused decode buffer (ADR-0006).
fn scan_vector_distances(conn: &Connection, query: &[f32]) -> Result<Vec<(i64, f32)>> {
    let mut out: Vec<(i64, f32)> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    for_each_stored_vector(conn, |chunk_id, blob| {
        crate::embed::unpack_f32_into(blob, &mut scratch);
        out.push((chunk_id, crate::embed::l2_sq(query, &scratch)));
    })?;
    out.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    Ok(out)
}

/// Brute-force nearest-neighbour search: the `k` nearest chunk ids to `query` with
/// their L2 distances, nearest first. A full linear scan — exact, no silent truncation
/// at any `k` — which is what ADR-0006 specs as comfortable at vault scale. L2 over the
/// stored embeddings ranks by cosine (b2-embed L2-normalizes); the `sqrt` is applied
/// once per *returned* hit, since ranking is monotonic in the squared distance.
pub fn vector_search(conn: &Connection, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
    let mut hits = scan_vector_distances(conn, query)?;
    hits.truncate(k);
    Ok(hits.into_iter().map(|(id, d)| (id, d.sqrt())).collect())
}

/// [`vector_search`] without the `k` bound: **every** chunk's distance to `query`,
/// nearest first (same scan, same `chunk_id` tie-break). The whole-space caller —
/// graph-filtered search — ranks the entire vault, so it takes this rather than
/// pass a sentinel `k`.
pub fn vector_search_all(conn: &Connection, query: &[f32]) -> Result<Vec<(i64, f32)>> {
    let hits = scan_vector_distances(conn, query)?;
    Ok(hits.into_iter().map(|(id, d)| (id, d.sqrt())).collect())
}

// ---------------------------------------------------------------------------
// edges
// ---------------------------------------------------------------------------

/// One authored edge row, ready to project. Owns its data (built from resolved
/// links during ingest).
pub struct EdgeRow {
    pub id: String,
    /// The authoring note's vault-relative path.
    pub src_path: String,
    /// The resolved **note** target (a vault-relative path into `notes`); `None`
    /// when the authored link named no note.
    pub dst_path: Option<String>,
    /// The resolved **resource** target (vault-relative path into `resources`),
    /// when the link names a non-`.md` file — mutually exclusive with `dst_path`
    /// in practice (a target resolves as a note or a resource, never both).
    pub dst_resource_path: Option<String>,
    pub dst_path_raw: String,
    pub r#type: String,
    pub origin: String,
    pub explanation: Option<String>,
    /// An embed form (`![alt](…)` / `![[…]]`) — display nicety, not a verb.
    pub embed: bool,
    /// The authored alt/link/alias text — an image's index text (slice 3).
    pub caption: Option<String>,
    pub occurrence_index: i64,
}

/// Replace a note's edges. Every edge is authored (body links ∪ frontmatter
/// `b2_relations:`), so this deletes the note's edges and re-inserts them from the
/// current Markdown (Flow ①) — the whole graph is a projection of Markdown, with no
/// suggestion rows to preserve.
pub fn replace_authored_edges(conn: &Connection, src_path: &str, edges: &[EdgeRow]) -> Result<()> {
    conn.execute("DELETE FROM edges WHERE src_path = ?1", [src_path])?;
    for e in edges {
        conn.execute(
            "INSERT INTO edges
               (id, src_path, dst_path, dst_resource_path, dst_path_raw, type, origin,
                explanation, embed, caption, occurrence_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                e.id,
                e.src_path,
                e.dst_path,
                e.dst_resource_path,
                e.dst_path_raw,
                e.r#type,
                e.origin,
                e.explanation,
                e.embed,
                e.caption,
                e.occurrence_index,
            ],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// resolver: the authored `[[path]]` → the note it names
// ---------------------------------------------------------------------------

/// Whether `path` names an indexed note — the existence check that replaced the
/// two-way `b2id ⇄ path` resolver (GH #170): with the path *being* the identity,
/// resolution is one membership test rather than a translation.
pub fn note_exists(conn: &Connection, path: &str) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM notes WHERE path = ?1", [path], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

/// One inbound edge a move/delete must act on: the source note's vault-relative
/// path and the exact authored link text (`dst_path_raw`) written there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundEdge {
    pub src_path: String,
    pub dst_raw: String,
}

/// Every active authored edge pointing *at* the note `dst_path`, as [`InboundEdge`]
/// rows. This is the bounded set a move must rewrite — the
/// materialized graph names the files to touch, so a move never scans the vault
/// (index-engine.md §8). Ordered for deterministic rewriting.
pub fn inbound_edge_targets(conn: &Connection, dst_path: &str) -> Result<Vec<InboundEdge>> {
    let mut stmt = conn.prepare(
        "SELECT e.src_path, e.dst_path_raw
         FROM edges e
         WHERE e.dst_path = ?1
         ORDER BY e.src_path, e.dst_path_raw",
    )?;
    let rows = stmt.query_map([dst_path], |r| {
        Ok(InboundEdge {
            src_path: r.get(0)?,
            dst_raw: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every indexed note path under the directory `dir` (vault-relative, no trailing
/// slash), path-ordered — the moved set a **directory move** operates on.
/// Prefix-matched with `substr` (not `LIKE`) so a dir name containing `%`/`_` never
/// wildcards.
pub fn notes_under_dir(conn: &Connection, dir: &str) -> Result<Vec<String>> {
    let prefix = format!("{dir}/");
    let mut stmt = conn.prepare(
        "SELECT path FROM notes
         WHERE substr(path, 1, length(?1)) = ?1
         ORDER BY path",
    )?;
    let rows = stmt.query_map([&prefix], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every inventoried resource path under the directory `dir` — the resource half
/// of [`notes_under_dir`], same prefix semantics, path-ordered.
pub fn resources_under_dir(conn: &Connection, dir: &str) -> Result<Vec<String>> {
    let prefix = format!("{dir}/");
    let mut stmt = conn.prepare(
        "SELECT path FROM resources
         WHERE substr(path, 1, length(?1)) = ?1
         ORDER BY path",
    )?;
    let rows = stmt.query_map([&prefix], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Re-key a note from `old_path` to `new_path` — **the** index-side move (ADR-0003).
///
/// One statement, and the FK graph does the rest: `note_aliases`, `chunks`,
/// `note_centroids` and `edges.src_path` all declare `ON UPDATE CASCADE`, so every
/// derived row travels with the note atomically and its chunk *vectors* are never
/// touched at all — they are content-addressed, so they belong to the text, not the
/// path (ADR-0006). What does **not** cascade is `edges.dst_path`: it carries no FK,
/// because it must be free to be NULL for a dangling link, so callers re-project every
/// inbound source afterwards.
///
/// A **directory move** runs this for every moved note *before* re-projecting any file,
/// so link resolution stays independent of re-projection order. Requires
/// `PRAGMA foreign_keys = ON`; without it the cascades silently do not fire.
pub fn repoint_note_path(conn: &Connection, old_path: &str, new_path: &str) -> Result<()> {
    conn.execute(
        "UPDATE notes SET path = ?1 WHERE path = ?2",
        params![new_path, old_path],
    )?;
    Ok(())
}

/// Resolve a wikilink target (`dst_path_raw`, written without the `.md`
/// extension in Obsidian) to the note path it names. Tries the literal path, then
/// with `.md` appended. `None` means the link is dangling.
pub fn resolve_link_target(conn: &Connection, link_path: &str) -> Result<Option<String>> {
    if note_exists(conn, link_path)? {
        return Ok(Some(link_path.to_string()));
    }
    let with_ext = format!("{link_path}.md");
    Ok(note_exists(conn, &with_ext)?.then_some(with_ext))
}

/// Resolve a link target against the **resource inventory** — an exact
/// vault-relative path match (extension-only dispatch decided the target is a
/// resource before calling this; slice-1 spec §3). Returns the stored path, or
/// `None` for dangling.
pub fn resolve_resource_target(conn: &Connection, path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT path FROM resources WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?)
}

// ---------------------------------------------------------------------------
// edge existence — used by `b2 link` to stay idempotent
// ---------------------------------------------------------------------------

/// Whether the directed edge `(src_path, dst_path, type)` already exists. `b2 link`
/// uses this to avoid appending a duplicate frontmatter relation for a connection
/// that is already recorded (data-model.md §4).
pub fn edge_exists(
    conn: &Connection,
    src_path: &str,
    dst_path: &str,
    edge_type: &str,
) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM edges WHERE src_path = ?1 AND dst_path = ?2 AND type = ?3 LIMIT 1",
            params![src_path, dst_path, edge_type],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

#[cfg(test)]
mod tests {
    use super::{apply_schema, should_emit, SCHEMA_TABLES};
    use rusqlite::Connection;
    use std::collections::HashSet;

    /// [`SCHEMA_TABLES`] is what a completed migration is *checked* against, so a table
    /// added to the DDL and not to the list would narrow that check in silence. This
    /// pins the list to what the DDL actually creates, in both directions.
    #[test]
    fn schema_tables_lists_exactly_what_the_ddl_creates() {
        let conn = Connection::open_in_memory().unwrap();
        apply_schema(&conn).unwrap();

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap();
        let created: HashSet<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|n| n.unwrap())
            // FTS5 keeps its own shadow tables (`chunks_fts_data`, `_idx`, …) alongside
            // the virtual table; they are SQLite's bookkeeping, created and dropped with
            // it, never ours to list. `sqlite_%` is reserved for the same reason.
            .filter(|n| !n.starts_with("chunks_fts_") && !n.starts_with("sqlite_"))
            .collect();

        let listed: HashSet<String> = SCHEMA_TABLES.iter().map(|t| t.to_string()).collect();
        assert_eq!(
            created, listed,
            "SCHEMA_TABLES must match the tables the migration DDL creates"
        );
    }

    /// The full truth table for the profiler's emit guard (all 8 combinations).
    /// Both listening paths have to survive independently: a slow statement logs at
    /// WARN, so it must emit with DEBUG **off**, and DEBUG on emits regardless of speed.
    #[test]
    fn emits_only_when_some_level_would_receive_the_event() {
        // Something is listening at the level this event would use.
        assert!(
            should_emit(true, true, false),
            "slow + WARN on: the slow-query log, and the case a naive simplification drops"
        );
        assert!(should_emit(true, true, true));
        assert!(
            should_emit(true, false, true),
            "DEBUG on catches it even with WARN off"
        );
        assert!(should_emit(false, true, true));
        assert!(
            should_emit(false, false, true),
            "DEBUG on: every statement logs"
        );

        // Nothing would receive it — skip the string work.
        assert!(
            !should_emit(false, true, false),
            "fast statement, only WARN on: nothing to say"
        );
        assert!(!should_emit(true, false, false), "slow, but no WARN sink");
        assert!(!should_emit(false, false, false), "nobody listening at all");
    }
}
