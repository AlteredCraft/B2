//! Opening the index, the schema migration, and the projection helpers for `notes`,
//! `chunks` (+FTS5), the vector tables and the typed `edges` graph. Every table is a
//! derived projection of Markdown (ADR-0002).
//!
//! A note is keyed by its vault-relative path (ADR-0003); children reference it
//! `ON DELETE CASCADE ON UPDATE CASCADE`, so a B2-performed move is one `UPDATE notes
//! SET path` (needs `PRAGMA foreign_keys = ON`).
//!
//! Vectors live in plain tables, content-addressed by the blake3 of the chunk text
//! (ADR-0006), so they outlive their chunk; [`prune_orphan_vectors`] collects them.

use crate::chunk::Chunk;
use crate::embed::pack_f32;
use crate::error::{Error, Result};
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::{
    params, Connection, OptionalExtension, StatementStatus, Transaction, TransactionBehavior,
};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// The index schema version stamped into `meta.schema_version`. A stale or unknown
/// stamp drops the derived tables for the next `reindex` to rebuild; there are no
/// migrations (ADR-0002). A stamp above this is refused untouched ([`refuse_if_newer`]).
///
/// 3: plain vector tables (ADR-0006). 4: `resources`. 5: `porter unicode61` FTS (GH #157).
/// 6: path-keyed, content-addressed `embeddings` (GH #170). 7: unread columns and
/// `note_aliases` dropped.
pub const SCHEMA_VERSION: i64 = 7;

/// Statements at or over this take the slow-query WARN path (`B2_SLOW_QUERY_MS`
/// overrides; see [`slow_query_threshold`]).
const SLOW_QUERY_MS_DEFAULT: u64 = 100;

/// The duration at which a statement logs as a slow query (WARN instead of DEBUG), read
/// once from `B2_SLOW_QUERY_MS`.
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

/// Whether anything would receive a finished statement's event. The `slow && warn`
/// term keeps slow-query WARNs for a WARN-only subscriber; don't reduce it to DEBUG.
fn should_emit(slow: bool, warn_enabled: bool, debug_enabled: bool) -> bool {
    (slow && warn_enabled) || debug_enabled
}

/// SQLite's per-statement profiler as `tracing` events on `b2::sqlite`: the SQL template
/// (never bound values, so no note content is logged), `duration_us`, and
/// `vm_steps`/`fullscan_steps` (a high fullscan count means a missing index).
///
/// Some platforms (macOS) quantize `duration_us` to ~1ms; `vm_steps` is the
/// deterministic cost measure.
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
    // One line per event, so `sql` is a stable, groupable key.
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
    conn.trace_v2(
        TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(on_sqlite_profile),
    );
    // execute_batch tolerates the row PRAGMA mmap_size returns.
    // busy_timeout: two writers can race (a save during the background embed); set
    // explicitly rather than relying on rusqlite's default.
    // mmap_size + cache_size: whole-space vector scans stream 100+ MB per call, which
    // the 2 MB default cache made syscall-bound (#38). mmap_size is a cap, not an
    // allocation; negative cache_size is KiB (32 MiB).
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

/// Attempts at the WAL flip. Large, because these retries stand in for `busy_timeout`,
/// which does not cover it: the whole ≈4 s budget is here (ADR-0021).
const WAL_FLIP_ATTEMPTS: u32 = 16;

/// Attempts at a DDL rebuild's `BEGIN IMMEDIATE`. Small, because each already waits the
/// full 5 s `busy_timeout`; past ≈15 s it is a stuck writer (ADR-0021).
const REBUILD_ATTEMPTS: u32 = 3;

/// The pause schedule [`retry_while_locked`] uses between attempts.
const LOCK_RETRY_BACKOFF_START: Duration = Duration::from_millis(2);
const LOCK_RETRY_BACKOFF_MAX: Duration = Duration::from_millis(500);

/// Run `op`, retrying while SQLite reports lock contention. `what` names the step for
/// the log. The backoff sleeps but reads no clock. `op` must be idempotent and
/// self-checking, so a retry after a lost race finds the work already done.
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

/// Put the connection in WAL mode, waiting out a concurrent opener (ADR-0021). The flip
/// takes a write lock that `busy_timeout` does not cover, so the retry is ours.
///
/// The mode is read back: a filesystem without shared memory declines with `SQLITE_OK`
/// and the old mode. That is logged, not retried; B2 works in rollback-journal mode.
fn enter_wal_mode(conn: &Connection) -> Result<()> {
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

/// Whether an error is lock contention. `SQLITE_LOCKED` too, because the desktop opens
/// the index from more than one thread.
fn is_locked(err: &Error) -> bool {
    matches!(err, Error::Sqlite(e) if matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ))
}

/// The tables [`apply_schema`] creates, checked by [`schema_is_current`]. Tables suffice:
/// a dropped table takes its indexes and triggers with it (#114). A unit test pins the
/// list to the DDL.
const SCHEMA_TABLES: [&str; 6] = [
    "meta",
    "notes",
    "chunks",
    "chunks_fts",
    "resources",
    "edges",
];

/// Bring the index to [`SCHEMA_VERSION`], atomically and serialized against other openers
/// on SQLite's write lock (ADR-0021). The stamp commits with the tables it vouches for, and
/// a current index takes no write lock at all, so a reader is never refused (C1).
fn migrate(conn: &mut Connection) -> Result<()> {
    if schema_is_current(conn)? {
        return Ok(());
    }
    // Also checked here so a newer index is refused without taking a write lock.
    refuse_if_newer(conn)?;
    retry_while_locked("schema migration", REBUILD_ATTEMPTS, || {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Re-checked inside the lock: a lost race finds the winner's work committed.
        if !schema_is_current(&tx)? {
            // The opener we waited on may have been a newer b2.
            refuse_if_newer(&tx)?;
            apply_schema(&tx)?;
        }
        tx.commit()?;
        Ok(())
    })
}

/// Refuse an index stamped above [`SCHEMA_VERSION`] ([`Error::IndexTooNew`]) rather than
/// let [`apply_schema`] drop it: an older `b2` on `PATH` must not wipe a newer index.
///
/// The stamp alone decides. It commits with its tables, and a future schema may rename
/// any of [`SCHEMA_TABLES`], so their absence is not damage.
fn refuse_if_newer(conn: &Connection) -> Result<()> {
    // No `meta`, no stamp to compare.
    if !table_exists(conn, "meta")? {
        return Ok(());
    }
    match stamped_version(conn)? {
        Some(found) if found > SCHEMA_VERSION => Err(Error::IndexTooNew {
            found,
            supported: SCHEMA_VERSION,
        }),
        _ => Ok(()),
    }
}

/// Whether `name` is a table in this database.
fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Whether every table in [`SCHEMA_TABLES`] is present and the stamp is current. Both
/// halves matter: #114 could leave a current stamp over an incomplete schema.
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

/// The value stored in `meta` under `key`, or `None` when unset. Callers must know
/// `meta` exists.
fn meta_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

/// The `schema_version` recorded in `meta`, or `None` when unstamped.
fn stamped_version(conn: &Connection) -> Result<Option<i64>> {
    Ok(meta_value(conn, "schema_version")?.and_then(|s| s.parse().ok()))
}

/// Drop whatever is there, create the schema and stamp `schema_version`. The DDL mirrors
/// `index-engine.md`; vector tables are created at embed time ([`ensure_embedding_space`]).
///
/// The drop is unconditional: anything reaching here is of no known version, and
/// surviving rows would be skipped by an incremental reindex (matching `body_hash`),
/// leaving the tables empty forever. Call only inside [`migrate`]'s transaction.
fn apply_schema(conn: &Connection) -> Result<()> {
    // `meta` must exist before the batch below can clear it.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    // Children first (FKs). Legacy vec0 `chunks_vec` is absent: its module is no longer
    // linked, so SQLite cannot drop it. Clearing `meta` makes the next embed pass
    // recreate the vector tables.
    conn.execute_batch(
        "DROP TABLE IF EXISTS edge_provenance;
         DROP TABLE IF EXISTS edges;
         DROP TABLE IF EXISTS resources;
         DROP TABLE IF EXISTS note_centroids;
         DROP TABLE IF EXISTS embeddings;
         DROP TABLE IF EXISTS chunks_fts;
         DROP TABLE IF EXISTS chunks;
         DROP TABLE IF EXISTS note_aliases; -- schema <= 6
         DROP TABLE IF EXISTS notes;
         DELETE FROM meta;",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
           path        TEXT PRIMARY KEY,
           title       TEXT,
           created     TEXT,
           body_hash   TEXT NOT NULL,
           mtime       INTEGER,
           indexed_at  TEXT NOT NULL
         );

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
// notes
// ---------------------------------------------------------------------------

/// One note's projection into `notes`. Borrowed view so callers
/// pass slices of an already-parsed note without extra allocation.
#[derive(Debug)]
pub struct NoteRow<'a> {
    pub path: &'a str,
    pub title: Option<&'a str>,
    pub created: Option<&'a str>,
    pub body_hash: &'a str,
    pub mtime: Option<i64>,
}

/// Upsert a note keyed by its vault-relative `path` (ADR-0003). SQLite sets `indexed_at`,
/// so no wall clock is needed from Rust.
pub fn upsert_note(conn: &Connection, row: &NoteRow) -> Result<()> {
    conn.execute(
        "INSERT INTO notes (path, title, created, body_hash, mtime, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(path) DO UPDATE SET
           title       = excluded.title,
           created     = excluded.created,
           body_hash   = excluded.body_hash,
           mtime       = excluded.mtime,
           indexed_at  = excluded.indexed_at",
        params![row.path, row.title, row.created, row.body_hash, row.mtime],
    )?;
    Ok(())
}

/// Delete every `notes` row whose path is not in `seen` (every path the walk met,
/// unreadable ones included) and return how many went, so a file deleted outside `b2`
/// leaves no ghost row (#31, S3).
///
/// Chunks, centroid and outgoing edges cascade; vectors are shared, so
/// [`prune_orphan_vectors`] collects them. `edges.dst_path` has no FK, so run this before
/// edge derivation, which re-dangles inbound links.
pub fn prune_notes_except(conn: &Connection, seen: &HashSet<&str>) -> Result<usize> {
    prune_members_except(conn, Members::Notes, |path| seen.contains(path))
}

/// Drop the `notes` row at `path` and return how many rows went (0 or 1). Cascades as
/// [`prune_notes_except`]; inbound edges are the caller's to re-project.
pub fn delete_note_row(conn: &Connection, path: &str) -> Result<usize> {
    delete_member(conn, Members::Notes, path)
}

/// The two path-keyed member tables: a vault path names a note or a resource (L1, L3).
#[derive(Debug, Clone, Copy)]
enum Members {
    Notes,
    Resources,
}

impl Members {
    fn table(self) -> &'static str {
        match self {
            Members::Notes => "notes",
            Members::Resources => "resources",
        }
    }
}

/// Delete one member row by path; returns the rows deleted (0 or 1).
fn delete_member(conn: &Connection, members: Members, path: &str) -> Result<usize> {
    let sql = format!("DELETE FROM {} WHERE path = ?1", members.table());
    Ok(conn.execute(&sql, [path])?)
}

/// Delete every member row whose path `keep` rejects; returns how many went.
fn prune_members_except(
    conn: &Connection,
    members: Members,
    keep: impl Fn(&str) -> bool,
) -> Result<usize> {
    let mut stmt = conn.prepare(&format!("SELECT path FROM {}", members.table()))?;
    let stored = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut pruned = 0;
    for path in stored.iter().filter(|p| !keep(p)) {
        pruned += delete_member(conn, members, path)?;
    }
    Ok(pruned)
}

/// Every member path under the folder `dir` (no trailing slash), path-ordered. Uses
/// `substr`, not `LIKE`, so `%`/`_` in a folder name never wildcard.
fn members_under_dir(conn: &Connection, members: Members, dir: &str) -> Result<Vec<String>> {
    let prefix = format!("{dir}/");
    let mut stmt = conn.prepare(&format!(
        "SELECT path FROM {}
         WHERE substr(path, 1, length(?1)) = ?1
         ORDER BY path",
        members.table()
    ))?;
    let rows = stmt.query_map([&prefix], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// resources (data-model.md §10)
// ---------------------------------------------------------------------------

/// One resource's projection into `resources`. A borrowed view like [`NoteRow`].
#[derive(Debug)]
pub struct ResourceRow<'a> {
    pub path: &'a str,
    pub class: &'a str,
    pub size: i64,
    pub mtime: Option<i64>,
    pub content_hash: &'a str,
}

/// Upsert a resource keyed by its vault-relative path; SQLite sets `indexed_at`.
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

/// The stored `(size, mtime)` for a resource. A matching stat skips re-hashing.
pub fn resource_stat(conn: &Connection, path: &str) -> Result<Option<(i64, Option<i64>)>> {
    Ok(conn
        .query_row(
            "SELECT size, mtime FROM resources WHERE path = ?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// One `list_resources` row: a resource's identity and stat for the file tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceListing {
    pub path: String,
    pub class: String,
    pub size: i64,
    pub mtime: Option<i64>,
}

/// One resource's full inventory row: the fallback card's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDetail {
    pub class: String,
    pub size: i64,
    pub mtime: Option<i64>,
    pub content_hash: String,
}

/// Every inventoried resource, path-ordered.
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

/// One resource's [`ResourceDetail`], or `None` when not inventoried.
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

/// One edge pointing at a resource, with its source note's display fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBacklinkRow {
    pub note_path: String,
    pub note_title: Option<String>,
    pub r#type: String,
    pub caption: Option<String>,
    pub embed: bool,
}

/// Every edge pointing at the resource, for the fallback card's backlinks. Ordered.
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

/// One edge from a note to a resource, with the resource's `class` (the display glyph).
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

/// Every edge from a note to a resource, so `explain` shows file links alongside note
/// and dangling targets (GH #22). Ordered.
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

/// The inbound edges a resource move must rewrite; the resource sibling of
/// [`inbound_edge_targets`].
pub fn inbound_resource_edge_targets(conn: &Connection, path: &str) -> Result<Vec<InboundEdge>> {
    inbound_edges_on(conn, "dst_resource_path", path)
}

/// Delete every `resources` row whose path is not in `seen` and return how many went
/// (#31). Inbound edges re-dangle via `ON DELETE SET NULL`.
pub fn prune_resources_except(conn: &Connection, seen: &HashSet<String>) -> Result<usize> {
    prune_members_except(conn, Members::Resources, |path| seen.contains(path))
}

/// Drop the inventory row at `path` and return how many rows went (0 or 1). Inbound
/// edges re-dangle.
pub fn delete_resource_row(conn: &Connection, path: &str) -> Result<usize> {
    delete_member(conn, Members::Resources, path)
}

/// Re-key the inventory row at `old_path` to `new_path`, keeping `size` and
/// `content_hash`. Returns whether `old_path` was inventoried.
///
/// Upsert then delete, not `UPDATE`: `edges.dst_resource_path` has no `ON UPDATE
/// CASCADE`, so inbound edges re-dangle until the caller re-projects their sources.
pub fn repoint_resource(
    conn: &Connection,
    old_path: &str,
    new_path: &str,
    class: &'static str,
    mtime: Option<i64>,
) -> Result<bool> {
    let Some(detail) = resource_detail(conn, old_path)? else {
        return Ok(false);
    };
    upsert_resource(
        conn,
        &ResourceRow {
            path: new_path,
            class,
            size: detail.size,
            mtime,
            content_hash: &detail.content_hash,
        },
    )?;
    delete_resource_row(conn, old_path)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// chunks (FTS kept in lockstep by the triggers in apply_schema())
// ---------------------------------------------------------------------------

/// The content address of a chunk's embed input: blake3 of the stored text, which is
/// exactly what the embedder sees (ADR-0006). No model id: a model swap drops the whole
/// table (ADR-0007).
pub fn text_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

/// Replace a note's chunks and return the new chunk ids in `seq` order.
///
/// Vectors are shared, so they survive (ADR-0006, [`prune_orphan_vectors`]) and a re-chunk
/// of unchanged text re-embeds nothing. The centroid is stale, so it is dropped.
pub fn replace_chunks(conn: &Connection, note_path: &str, chunks: &[Chunk]) -> Result<Vec<i64>> {
    // The model-free projection must never create the embedding space (index-engine.md).
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

/// The tokenizers `chunks_fts` can be rebuilt with; an enum so [`rebuild_fts`]'s DDL never
/// splices caller text. `PorterUnicode61` ships (GH #157); `Unicode61` is the eval's
/// unstemmed arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtsTokenizer {
    Unicode61,
    PorterUnicode61,
}

impl FtsTokenizer {
    /// The FTS5 `tokenize =` value, spelled as the schema does.
    pub fn sql(self) -> &'static str {
        match self {
            FtsTokenizer::Unicode61 => "unicode61",
            FtsTokenizer::PorterUnicode61 => "porter unicode61",
        }
    }
}

/// The tokenizer `chunks_fts` was created with, read back because [`rebuild_fts`] can
/// swap it (see [`search::lexical_evidence`](crate::search::lexical_evidence)). An
/// unrecognised schema degrades to the shipped default.
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

/// Drop and recreate `chunks_fts` with `tokenizer`, repopulated from `chunks`, without
/// re-chunking or re-embedding (GH #157). Same write-lock discipline as the migration;
/// the `chunks_*` triggers reference this table by name, so they survive.
pub fn rebuild_fts(conn: &Connection, tokenizer: FtsTokenizer) -> Result<()> {
    retry_while_locked("chunks_fts rebuild", REBUILD_ATTEMPTS, || {
        // `new_unchecked`: see `ensure_embedding_space`.
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
// embeddings: created at embed time, not in apply_schema(); their existence is the
// "this vault has an embedding space" signal (ADR-0006).
// ---------------------------------------------------------------------------

/// Whether the embedding space (the `embeddings` table) currently exists.
pub fn embedding_space_exists(conn: &Connection) -> Result<bool> {
    table_exists(conn, "embeddings")
}

/// Ensure the vector tables exist for `(model_id, dim)`, recorded in `meta`. A model swap
/// drops and recreates them empty, so vectors never go silently stale (ADR-0007).
///
/// Serialized and atomic like [`migrate`] (ADR-0021): the desktop's reindex and a
/// `b2 reindex` can overlap, and one's `DROP` after the other's `CREATE` breaks it.
pub fn ensure_embedding_space(conn: &Connection, model_id: &str, dim: usize) -> Result<()> {
    if embedding_space_matches(conn, model_id, dim)? {
        return Ok(());
    }
    retry_while_locked("embedding-space rebuild", REBUILD_ATTEMPTS, || {
        // `new_unchecked` because the embed pass shares a `&Connection`. Sound because
        // `b2-core` opens no other transaction on it.
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

/// Whether the vector tables exist and were built by this exact embedder.
fn embedding_space_matches(conn: &Connection, model_id: &str, dim: usize) -> Result<bool> {
    let unchanged = matches!(
        recorded_embedder(conn)?,
        Some((m, d)) if m == model_id && d == dim
    );
    Ok(unchanged && embedding_space_exists(conn)?)
}

/// The `(embed_model_id, embed_dim)` a prior ingest recorded; `None` if never embedded.
/// Reads compare it to the active embedder and fail fast on a mismatch (ADR-0007).
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

/// Store one embedding under its text's content address. `OR IGNORE`: identical text
/// addresses the same row, which also makes overlapping embed passes idempotent.
pub fn set_vector(conn: &Connection, text_hash: &str, embedding: &[f32]) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO embeddings(text_hash, vector) VALUES (?1, ?2)",
        params![text_hash, pack_f32(embedding)],
    )?;
    Ok(())
}

/// Delete every stored vector no chunk references, once the whole-vault pass's chunk set
/// is final. Not per-row: a moved note's vectors must survive re-insertion (ADR-0006).
/// Requires the embedding space to exist.
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

/// A ranked chunk resolved for display. One statement, so the note and chunk come from
/// one snapshot: a hit is whole or `None` (GH #137).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkHit {
    pub note_path: String,
    pub title: Option<String>,
    pub heading_path: Option<String>,
    pub text: String,
}

/// [`ChunkHit`] for `chunk_id`, `None` if the chunk or its note is gone.
pub fn chunk_hit(conn: &Connection, chunk_id: i64) -> Result<Option<ChunkHit>> {
    Ok(conn
        .query_row(
            "SELECT c.note_path, n.title, c.heading_path, c.text
             FROM chunks c JOIN notes n ON n.path = c.note_path
             WHERE c.id = ?1",
            [chunk_id],
            |r| {
                Ok(ChunkHit {
                    note_path: r.get(0)?,
                    title: r.get(1)?,
                    heading_path: r.get(2)?,
                    text: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// A chunk's text (None if the chunk id is unknown) — a similar card's evidence passage.
pub fn chunk_text(conn: &Connection, chunk_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT text FROM chunks WHERE id = ?1", [chunk_id], |r| {
            r.get(0)
        })
        .optional()?)
}

/// A chunk's heading breadcrumb and text (None if the chunk id is unknown).
pub fn chunk_detail(conn: &Connection, chunk_id: i64) -> Result<Option<(Option<String>, String)>> {
    Ok(conn
        .query_row(
            "SELECT heading_path, text FROM chunks WHERE id = ?1",
            [chunk_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// A note's stored body hash (None if not indexed). Read before re-upserting, so an
/// incremental reindex can skip an unchanged note.
pub fn note_body_hash(conn: &Connection, note_path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT body_hash FROM notes WHERE path = ?1",
            [note_path],
            |r| r.get(0),
        )
        .optional()?)
}

/// Whether `note_path` has no chunk awaiting a vector (the per-note [`embed_progress`]
/// predicate). A chunkless note is vacuously embedded. Requires the embedding space.
pub fn note_fully_embedded(conn: &Connection, note_path: &str) -> Result<bool> {
    let n_missing: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM chunks c LEFT JOIN embeddings v ON v.text_hash = c.text_hash
         WHERE c.note_path = ?1 AND v.text_hash IS NULL",
        [note_path],
        |r| r.get(0),
    )?;
    Ok(n_missing == 0)
}

/// Embedding coverage as `(notes_embedded, notes_total)`: notes with no chunk awaiting a
/// vector (#26). Model-free.
///
/// A chunkless note counts as embedded. Consumers treat `embedded < total` as pending
/// work, so otherwise one empty note would schedule no-op embed passes forever.
pub fn embed_progress(conn: &Connection) -> Result<(usize, usize)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
    // No embeddings table yet: only chunkless notes are embedded.
    if !embedding_space_exists(conn)? {
        let chunkless: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notes n
             WHERE NOT EXISTS (SELECT 1 FROM chunks c WHERE c.note_path = n.path)",
            [],
            |r| r.get(0),
        )?;
        return Ok((chunkless as usize, total as usize));
    }
    // `note_fully_embedded`, aggregated.
    let embedded: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notes n
         WHERE NOT EXISTS (
             SELECT 1 FROM chunks c
             LEFT JOIN embeddings v ON v.text_hash = c.text_hash
             WHERE c.note_path = n.path AND v.text_hash IS NULL
           )",
        [],
        |r| r.get(0),
    )?;
    Ok((embedded as usize, total as usize))
}

/// One chunk still lacking a stored vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChunk {
    pub note_path: String,
    pub text: String,
    /// The content address to store the vector under, so the pass never re-hashes.
    pub text_hash: String,
}

/// The row mapper both pending-set queries share: `SELECT c.note_path, c.text, c.text_hash`.
fn pending_chunk(r: &rusqlite::Row) -> rusqlite::Result<PendingChunk> {
    Ok(PendingChunk {
        note_path: r.get(0)?,
        text: r.get(1)?,
        text_hash: r.get(2)?,
    })
}

/// Every chunk still lacking a stored vector, in `(path, seq)` order: the pending set the
/// embed pass fills. Derived from the DB, so any interrupted pass heals on the next.
/// Requires the embedding space.
///
/// Chunks sharing text appear twice; [`crate::ingest::embed_vault`] dedups embedder calls.
pub fn chunks_missing_vectors(conn: &Connection) -> Result<Vec<PendingChunk>> {
    let mut stmt = conn.prepare(
        "SELECT c.note_path, c.text, c.text_hash
         FROM chunks c
         LEFT JOIN embeddings v ON v.text_hash = c.text_hash
         WHERE v.text_hash IS NULL
         ORDER BY c.note_path, c.seq",
    )?;
    let rows = stmt.query_map([], pending_chunk)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// [`chunks_missing_vectors`] for one note, for the inline ingest path. Its own indexed
/// query, because a directory move would make filtering O(vault × moved).
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
    let rows = stmt.query_map([note_path], pending_chunk)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A note's chunk ids in `seq` order, embedded or not. Empty for an unknown or empty note.
pub fn note_chunk_ids(conn: &Connection, note_path: &str) -> Result<Vec<i64>> {
    let mut stmt =
        conn.prepare_cached("SELECT id FROM chunks WHERE note_path = ?1 ORDER BY seq")?;
    let rows = stmt.query_map([note_path], |r| r.get::<_, i64>(0))?;
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

/// A note's `(title, created)` from the projection (both `None` if absent), so adapters
/// never re-read the file (GH #22).
pub fn note_header(conn: &Connection, note_path: &str) -> Result<(Option<String>, Option<String>)> {
    Ok(conn
        .query_row(
            "SELECT title, created FROM notes WHERE path = ?1",
            [note_path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or_default())
}

/// Every indexed note's `(path, title)`, path-ordered so the file tree builds in one pass.
pub fn all_notes(conn: &Connection) -> Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare("SELECT path, title FROM notes ORDER BY path")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// A note's stored chunk vectors as `(chunk_id, vector)` in `seq` order, so discovery
/// searches from them without re-embedding. Requires the embedding space. Cached:
/// discovery calls it once per shortlisted note.
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

/// Recompute `note_path`'s centroid from its stored chunk vectors, deleting it when there
/// are none (ADR-0006).
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

/// Every fully-embedded note with no centroid row, path-ordered (GH #170). A re-chunk
/// keeps vectors but drops the centroid, leaving nothing pending; without this the note
/// silently leaves discovery's coarse stage (S3). Fully embedded only: a partial
/// centroid would be wrong.
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

/// Stream every `(note_path, centroid_blob)` through `f`: discovery's O(notes) coarse
/// scan (#38). The blob is borrowed, so scoring allocates nothing per row.
pub fn for_each_note_centroid(conn: &Connection, mut f: impl FnMut(&str, &[u8])) -> Result<()> {
    let mut stmt = conn.prepare("SELECT note_path, centroid FROM note_centroids")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        // Match ValueRefs: `FromSqlError` isn't in our error enum, and our DDL fixes the
        // types, so a mismatched row is skipped.
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

/// Stream every embedded chunk's `(chunk_id, vector_blob)` through `f` without
/// materializing the space (ADR-0006). The join hands a shared vector to each chunk that
/// addresses it, since ranking is per chunk.
pub fn for_each_stored_vector(conn: &Connection, mut f: impl FnMut(i64, &[u8])) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT c.id, e.vector FROM embeddings e JOIN chunks c ON c.text_hash = e.text_hash",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let chunk_id: i64 = row.get(0)?;
        // Match the ValueRef: `FromSqlError` isn't in our error enum.
        if let rusqlite::types::ValueRef::Blob(blob) = row.get_ref(1)? {
            f(chunk_id, blob);
        }
    }
    Ok(())
}

/// Every chunk's squared-L2 distance to `query`, nearest first, ties by `chunk_id`
/// (ADR-0006).
fn scan_vector_distances(conn: &Connection, query: &[f32]) -> Result<Vec<(i64, f32)>> {
    let mut out: Vec<(i64, f32)> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    for_each_stored_vector(conn, |chunk_id, blob| {
        crate::embed::unpack_f32_into(blob, &mut scratch);
        out.push((chunk_id, crate::embed::l2_sq(query, &scratch)));
    })?;
    out.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    Ok(out)
}

/// Exact brute-force search: the `k` nearest chunk ids with L2 distances (ADR-0006).
/// Vectors are normalized, so this ranks by cosine.
pub fn vector_search(conn: &Connection, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
    let mut hits = scan_vector_distances(conn, query)?;
    hits.truncate(k);
    Ok(hits.into_iter().map(|(id, d)| (id, d.sqrt())).collect())
}

// ---------------------------------------------------------------------------
// edges
// ---------------------------------------------------------------------------

/// One authored edge row, ready to project.
#[derive(Debug)]
pub struct EdgeRow {
    pub id: String,
    /// The authoring note's vault-relative path.
    pub src_path: String,
    /// The resolved note target; `None` when the link named no note.
    pub dst_path: Option<String>,
    /// The resolved resource target (a non-`.md` file); exclusive with `dst_path`.
    pub dst_resource_path: Option<String>,
    pub dst_path_raw: String,
    pub r#type: String,
    pub origin: String,
    pub explanation: Option<String>,
    /// An embed form (`![alt](…)` / `![[…]]`): display only, not a verb.
    pub embed: bool,
    /// The authored alt/link/alias text.
    pub caption: Option<String>,
    pub occurrence_index: i64,
}

/// Replace a note's edges from its current Markdown (Flow ①, G1).
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

/// Whether `path` names an indexed note; the path is the identity (GH #170).
pub fn note_exists(conn: &Connection, path: &str) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM notes WHERE path = ?1", [path], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

/// One inbound edge a move or delete must act on: the source note and the authored link
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundEdge {
    pub src_path: String,
    pub dst_raw: String,
}

/// Every edge pointing at the note `dst_path`: the set a move rewrites without scanning
/// the vault (index-engine.md §8). Ordered.
pub fn inbound_edge_targets(conn: &Connection, dst_path: &str) -> Result<Vec<InboundEdge>> {
    inbound_edges_on(conn, "dst_path", dst_path)
}

/// Which member an [`inbound_edges_of`] edge points at, as an index into its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundTarget {
    Note(usize),
    Resource(usize),
}

/// Every edge pointing at one of `notes` or `resources`, tagged with its target; notes
/// first. The graph read a move or delete starts from (index-engine.md §8).
pub fn inbound_edges_of(
    conn: &Connection,
    notes: &[&str],
    resources: &[&str],
) -> Result<Vec<(InboundTarget, InboundEdge)>> {
    let mut out = Vec::new();
    for (i, path) in notes.iter().enumerate() {
        let edges = inbound_edge_targets(conn, path)?;
        out.extend(edges.into_iter().map(|e| (InboundTarget::Note(i), e)));
    }
    for (i, path) in resources.iter().enumerate() {
        let edges = inbound_resource_edge_targets(conn, path)?;
        out.extend(edges.into_iter().map(|e| (InboundTarget::Resource(i), e)));
    }
    Ok(out)
}

/// The notes linking into a set of members, sorted and deduped.
pub fn inbound_sources(
    conn: &Connection,
    notes: &[&str],
    resources: &[&str],
) -> Result<BTreeSet<String>> {
    Ok(inbound_edges_of(conn, notes, resources)?
        .into_iter()
        .map(|(_, e)| e.src_path)
        .collect())
}

/// The edges whose `column` (`dst_path` or `dst_resource_path`) names `path`, as
/// [`InboundEdge`] rows ordered by source then authored text.
fn inbound_edges_on(
    conn: &Connection,
    column: &'static str,
    path: &str,
) -> Result<Vec<InboundEdge>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT e.src_path, e.dst_path_raw
         FROM edges e
         WHERE e.{column} = ?1
         ORDER BY e.src_path, e.dst_path_raw"
    ))?;
    let rows = stmt.query_map([path], |r| {
        Ok(InboundEdge {
            src_path: r.get(0)?,
            dst_raw: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every indexed note path under `dir` (no trailing slash), path-ordered.
pub fn notes_under_dir(conn: &Connection, dir: &str) -> Result<Vec<String>> {
    members_under_dir(conn, Members::Notes, dir)
}

/// Every inventoried resource path under `dir`, as [`notes_under_dir`].
pub fn resources_under_dir(conn: &Connection, dir: &str) -> Result<Vec<String>> {
    members_under_dir(conn, Members::Resources, dir)
}

/// Re-key a note from `old_path` to `new_path`: the index-side move (ADR-0003). Child rows
/// cascade; vectors belong to the text (ADR-0006). `edges.dst_path` has no FK, so callers
/// re-project inbound sources afterwards.
///
/// A directory move re-keys every note before re-projecting any file, so resolution is
/// order-independent. Requires `PRAGMA foreign_keys = ON`.
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

/// Resolve a link target against the resource inventory by exact path; `None` for
/// dangling.
pub fn resolve_resource_target(conn: &Connection, path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT path FROM resources WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?)
}

// ---------------------------------------------------------------------------
// edge existence (keeps `b2 link` idempotent)
// ---------------------------------------------------------------------------

/// Whether the directed edge `(src_path, dst_path, type)` already exists
/// (data-model.md §4).
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

    /// A table added to the DDL but not the list would silently narrow the check.
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
            // FTS5 shadow tables and `sqlite_%` are SQLite's own.
            .filter(|n| !n.starts_with("chunks_fts_") && !n.starts_with("sqlite_"))
            .collect();

        let listed: HashSet<String> = SCHEMA_TABLES.iter().map(|t| t.to_string()).collect();
        assert_eq!(
            created, listed,
            "SCHEMA_TABLES must match the tables the migration DDL creates"
        );
    }

    /// All 8 combinations: slow emits with only WARN on; DEBUG emits regardless.
    #[test]
    fn emits_only_when_some_level_would_receive_the_event() {
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

        assert!(
            !should_emit(false, true, false),
            "fast statement, only WARN on: nothing to say"
        );
        assert!(!should_emit(true, false, false), "slow, but no WARN sink");
        assert!(!should_emit(false, false, false), "nobody listening at all");
    }
}
