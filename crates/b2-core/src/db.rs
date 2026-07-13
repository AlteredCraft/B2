//! Opening the index, the schema migration, and the projection helpers for the
//! Markdown-derived tiers: `notes`/`note_aliases`, `chunks` (+FTS5), the
//! `embeddings`/`note_centroids` vector tables, and the typed `edges` graph, plus
//! the `b2id ⇄ path` resolver.
//!
//! Every connection is opened `WAL` + `foreign_keys=ON` per
//! planning/specs/completed/index-engine-build.md §0. Every table here is a derived
//! projection of `Markdown` — nothing is a source of truth.
//!
//! Vectors live in **plain tables** and every distance is computed in-process
//! (schema v3, #38). The previous store — `sqlite-vec`'s `chunks_vec` `vec0`
//! virtual table — charged a per-row shadow-table probe on every scan (~38.6k
//! internal statements per `b2 similar` on a real vault) while its only shipped
//! search was the same brute force we compute ourselves; a plain-table scan is one
//! sequential statement.

use crate::chunk::Chunk;
use crate::embed::pack_f32;
use crate::error::Result;
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::{params, Connection, OptionalExtension, StatementStatus};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// The B2 index schema version stamped into `meta.schema_version`. Bumping it is
/// the migration gate: on a mismatch `migrate()` drops the derived tables and lets
/// the next `reindex` rebuild them (the index is disposable). **2** dropped the
/// suggestion machinery — the `status` column, the `origin='suggested'` value, and
/// the `edge_provenance` table — with the 2026-07-04 relator cut (data-model.md §4).
/// **3** replaced the `chunks_vec` vec0 virtual table with the plain `embeddings` +
/// `note_centroids` tables and dropped the `sqlite-vec` dependency (#38); a pre-3
/// index's orphaned `chunks_vec` entry is left inert in `sqlite_master` (its module
/// is no longer linked, so it can't be dropped) — delete `.b2/b2.sqlite` for a
/// byte-clean slate; either way the next `reindex` rebuilds everything queried.
/// **4** added the `resources` inventory and widened `edges` with resource targets
/// (`dst_resource_path`/`embed`/`caption`) — file-type support slice 1
/// (planning/specs/resources-inventory-graph.md §1).
pub const SCHEMA_VERSION: i64 = 4;

/// Statements at or over this take the slow-query WARN path (`B2_SLOW_QUERY_MS`
/// overrides; see [`slow_query_threshold`]).
const SLOW_QUERY_MS_DEFAULT: u64 = 100;

/// The duration at or above which a statement logs as a **slow query** (WARN
/// instead of DEBUG). Read once from `B2_SLOW_QUERY_MS` (milliseconds), defaulting
/// to [`SLOW_QUERY_MS_DEFAULT`]. Observability config only — it never changes what
/// any operation computes, so the core's determinism guarantee is untouched.
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

/// SQLite's own per-statement profiler, surfaced as structured `tracing` events:
/// `sqlite3_trace_v2(SQLITE_TRACE_PROFILE)` fires this when a statement finishes,
/// with the statement and its execution time measured by SQLite itself. Each event
/// (target `b2::sqlite`) carries the SQL **template** (`?N` placeholders, never the
/// bound values — so no note content or embedding blobs land in the log, and events
/// group cleanly by statement for reporting), a numeric `duration_us`, and the
/// statement's `vm_steps`/`fullscan_steps` counters (the "why was it slow" signal —
/// a high fullscan count means a missing index). Statements at or over
/// [`slow_query_threshold`] log at WARN with `slow=true`; the rest at DEBUG.
///
/// **`duration_us` precision is platform-bound.** SQLite's profiler clock is only as
/// fine as the host's; on some platforms (macOS observed) it quantizes to ~1ms, so
/// `duration_us` comes back as a multiple of 1000 and sub-millisecond statements read
/// as `0`. Treat it as coarse wall-clock. For fine-grained per-statement cost use
/// `vm_steps` — it counts VDBE opcodes, so it's deterministic and clock-independent.
///
/// A plain `fn` because `trace_v2` registers a function pointer (no captured
/// state). Emitting through `tracing` costs nothing until an adapter installs a
/// subscriber (the CLI's is opt-in via `B2_LOG`/`B2_DEBUG`).
fn on_sqlite_profile(event: TraceEvent<'_>) {
    let TraceEvent::Profile(stmt, elapsed) = event else {
        return; // only SQLITE_TRACE_PROFILE is masked in, but TraceEvent is non-exhaustive
    };
    let slow = elapsed >= slow_query_threshold();
    // Skip the string work when nobody is listening at the level this would emit at.
    if !(slow && tracing::enabled!(target: "b2::sqlite", tracing::Level::WARN))
        && !tracing::enabled!(target: "b2::sqlite", tracing::Level::DEBUG)
    {
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
    let conn = Connection::open(path)?;
    // Profile every statement on this connection through SQLite's trace_v2 hook —
    // the source of the `b2::sqlite` query-timing events (see `on_sqlite_profile`).
    conn.trace_v2(
        TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(on_sqlite_profile),
    );
    // execute_batch tolerates the rows PRAGMA journal_mode / mmap_size return.
    // busy_timeout: WAL allows one writer at a time, and two short-statement
    // writers can now legitimately race (a save during the background embed —
    // desktop-editing.md §4). A modest wait turns that contention into a few-ms
    // stall instead of an immediate SQLITE_BUSY error.
    // mmap_size + cache_size: the whole-space vector scans stream ~100+ MB of blob
    // rows per call on a real vault; under the 2 MB default cache with no mmap that
    // read path was pread/syscall-bound — the bulk of `b2 similar`'s ~4.4 s (#38).
    // mmap_size is a *cap*, not an allocation (the OS page cache does the work — the
    // one cache B2 is happy to lean on); cache_size is in KiB when negative (32 MiB).
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA mmap_size = 1073741824;
         PRAGMA cache_size = -32768;",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

/// Create the schema and stamp `schema_version`. `IF NOT EXISTS` keeps the CREATEs a
/// no-op on reopen; a `schema_version` mismatch drops the derived tables first so the
/// next `reindex` rebuilds them (the index is disposable). The DDL mirrors
/// planning/specs/completed/index-engine-build.md §1 (the vector tables are created at
/// embed time — see [`ensure_embedding_space`]).
fn migrate(conn: &Connection) -> Result<()> {
    // `meta` must exist before we can read the schema version the index was built at.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    // Schema-version gate: a mismatch means the derived tables have the wrong shape.
    // The index is disposable (vision-and-scope, "volatile vault over a disposable
    // index"), so drop the stale derived tables and clear `meta` — the next `reindex`
    // rebuilds everything under the new schema. Children first (FKs); dropping `chunks`
    // takes its FTS triggers with it.
    let prior: Option<i64> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|s| s.parse().ok());
    // The legacy vec0 `chunks_vec` (schema ≤ 2) is deliberately absent from this
    // list: its module is no longer linked, so SQLite cannot DROP it — any orphaned
    // entry stays inert in `sqlite_master` and nothing ever queries it (delete the
    // index file for a byte-clean slate). `DELETE FROM meta` clears the recorded
    // embedder, so the next embed pass recreates the vector tables from nothing.
    if prior.is_some_and(|v| v != SCHEMA_VERSION) {
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
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
           b2id        TEXT PRIMARY KEY,
           path        TEXT NOT NULL UNIQUE,
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
           note_b2id TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,
           alias     TEXT NOT NULL,
           PRIMARY KEY (note_b2id, alias)
         );
         CREATE INDEX IF NOT EXISTS note_aliases_alias_idx ON note_aliases(alias);

         CREATE TABLE IF NOT EXISTS chunks (
           id           INTEGER PRIMARY KEY,
           note_b2id    TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,
           seq          INTEGER NOT NULL,
           char_start   INTEGER NOT NULL,
           char_end     INTEGER NOT NULL,
           token_count  INTEGER NOT NULL,
           heading_path TEXT,
           text         TEXT NOT NULL,
           UNIQUE (note_b2id, seq)
         );
         CREATE INDEX IF NOT EXISTS chunks_note_idx ON chunks(note_b2id);

         CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
           text,
           content       = 'chunks',
           content_rowid = 'id',
           tokenize      = 'unicode61'
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
           src_id            TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,
           dst_id            TEXT,
           dst_resource_path TEXT REFERENCES resources(path) ON DELETE SET NULL,
           dst_path_raw      TEXT NOT NULL,
           type              TEXT NOT NULL,
           origin            TEXT NOT NULL CHECK (origin IN ('inline','frontmatter')),
           explanation       TEXT,
           embed             INTEGER NOT NULL DEFAULT 0,
           caption           TEXT,
           occurrence_index  INTEGER NOT NULL DEFAULT 0,
           UNIQUE (src_id, dst_id, type, occurrence_index)
         );
         CREATE INDEX IF NOT EXISTS edges_src_idx      ON edges(src_id);
         CREATE INDEX IF NOT EXISTS edges_dst_type_idx ON edges(dst_id, type);
         CREATE INDEX IF NOT EXISTS edges_dst_resource_idx ON edges(dst_resource_path)
           WHERE dst_resource_path IS NOT NULL;
         CREATE UNIQUE INDEX IF NOT EXISTS edges_resource_unique_idx
           ON edges(src_id, dst_resource_path, type, occurrence_index)
           WHERE dst_resource_path IS NOT NULL;
         CREATE INDEX IF NOT EXISTS edges_dangling_idx ON edges(dst_path_raw)
           WHERE dst_id IS NULL AND dst_resource_path IS NULL;",
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
    pub b2id: &'a str,
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

/// Upsert a note keyed by `b2id` and replace its aliases. `indexed_at` is set by
/// SQLite so the projection needs no wall-clock from Rust.
///
/// A note's `path` is `UNIQUE`, and the Markdown is the source of truth: if a
/// *different* `b2id` currently holds this path in the index, that row is **stale** —
/// the note there was deleted then recreated, or files were renamed/swapped outside
/// `b2 mv` (all normal for a local-first vault edited in other tools). We drop the stale
/// holder first (its chunks/edges cascade) so the upsert can't hit the `notes.path`
/// UNIQUE constraint. Without this, one such collision raises a raw SQLite error that
/// aborts the entire reindex, and an incremental reindex diverges from a from-scratch
/// rebuild — violating the core "incremental ≡ full rebuild" invariant (index-engine.md).
pub fn upsert_note(conn: &Connection, row: &NoteRow) -> Result<()> {
    conn.execute(
        "DELETE FROM notes WHERE path = ?1 AND b2id <> ?2",
        params![row.path, row.b2id],
    )?;
    conn.execute(
        "INSERT INTO notes
           (b2id, path, type, title, description, created, updated, body_hash, mtime, indexed_at)
         VALUES
           (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(b2id) DO UPDATE SET
           path        = excluded.path,
           type        = excluded.type,
           title       = excluded.title,
           description = excluded.description,
           created     = excluded.created,
           updated     = excluded.updated,
           body_hash   = excluded.body_hash,
           mtime       = excluded.mtime,
           indexed_at  = excluded.indexed_at",
        params![
            row.b2id,
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
    conn.execute("DELETE FROM note_aliases WHERE note_b2id = ?1", [row.b2id])?;
    for alias in row.aliases {
        conn.execute(
            "INSERT OR IGNORE INTO note_aliases(note_b2id, alias) VALUES (?1, ?2)",
            params![row.b2id, alias],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// resources (file-type support slice 1 — planning/specs/resources-inventory-graph.md §2)
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

/// Replace a note's chunks (delete + reinsert) and return the new chunk ids in
/// `seq` order. The FTS triggers emit the `'delete'` sentinel for the removed rows,
/// and any stored vectors cascade with them (`embeddings.chunk_id` is an
/// `ON DELETE CASCADE` FK). The note's centroid summarizes the *old* chunk set, so
/// it is dropped here too — the next embed pass recomputes it. Together this is
/// what makes an incremental re-index equal a full rebuild. The caller embeds the
/// returned ids (Flow ①).
pub fn replace_chunks(conn: &Connection, note_b2id: &str, chunks: &[Chunk]) -> Result<Vec<i64>> {
    // Guarded on existence so the model-free projection pass still never *creates*
    // the embedding space (projection-embedding-split.md §4).
    if embedding_space_exists(conn)? {
        conn.execute(
            "DELETE FROM note_centroids WHERE note_b2id = ?1",
            [note_b2id],
        )?;
    }
    conn.execute("DELETE FROM chunks WHERE note_b2id = ?1", [note_b2id])?;

    let mut new_ids = Vec::with_capacity(chunks.len());
    for c in chunks {
        conn.execute(
            "INSERT INTO chunks(note_b2id, seq, char_start, char_end, token_count, heading_path, text)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
            params![
                note_b2id,
                c.seq as i64,
                c.char_start as i64,
                c.char_end as i64,
                c.token_count as i64,
                c.text,
            ],
        )?;
        new_ids.push(conn.last_insert_rowid());
    }
    Ok(new_ids)
}

// ---------------------------------------------------------------------------
// embeddings — the vector tables are created at embed time (not in migrate()):
// their *existence* is the "this vault has an embedding space" signal the
// projected-but-unembedded fallbacks key on (projection-embedding-split.md §5).
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

/// Ensure the vector tables (`embeddings` + `note_centroids`) exist, recording
/// `(embed_model_id, embed_dim)` in `meta`. If either differs from what is recorded
/// — a model swap — the tables are dropped and recreated empty, so a full re-embed
/// follows (index-engine.md §8). `meta` is the only place a swap can be detected,
/// so vectors never go silently stale. (`dim` is bookkeeping only now — a plain
/// BLOB column needs no `FLOAT[N]` DDL literal — but it still gates the swap and
/// the read-time fail-fast.)
pub fn ensure_embedding_space(conn: &Connection, model_id: &str, dim: usize) -> Result<()> {
    let cur_model: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'embed_model_id'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let cur_dim: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'embed_dim'", [], |r| {
            r.get(0)
        })
        .optional()?;

    let unchanged = cur_model.as_deref() == Some(model_id)
        && cur_dim.as_deref() == Some(dim.to_string().as_str());
    if unchanged && embedding_space_exists(conn)? {
        return Ok(());
    }

    conn.execute_batch(
        "DROP TABLE IF EXISTS note_centroids;
         DROP TABLE IF EXISTS embeddings;
         CREATE TABLE embeddings (
           chunk_id INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
           vector   BLOB NOT NULL
         );
         CREATE TABLE note_centroids (
           note_b2id TEXT PRIMARY KEY REFERENCES notes(b2id) ON DELETE CASCADE,
           centroid  BLOB NOT NULL
         );",
    )?;
    upsert_meta(conn, "embed_model_id", model_id)?;
    upsert_meta(conn, "embed_dim", &dim.to_string())?;
    Ok(())
}

/// The `(embed_model_id, embed_dim)` a prior ingest recorded in `meta`, if any.
/// `None` means the vault has never been embedded (no vector tables yet). This is
/// the only place a model swap is detectable, so a read compares it to the active
/// embedder and fails fast on a mismatch (index-engine.md §8).
pub fn recorded_embedder(conn: &Connection) -> Result<Option<(String, usize)>> {
    let model: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'embed_model_id'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let dim: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'embed_dim'", [], |r| {
            r.get(0)
        })
        .optional()?;
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

/// Store a chunk's embedding (chunk ids are fresh on each reindex, so a plain
/// insert never conflicts).
pub fn set_chunk_vector(conn: &Connection, chunk_id: i64, embedding: &[f32]) -> Result<()> {
    conn.execute(
        "INSERT INTO embeddings(chunk_id, vector) VALUES (?1, ?2)",
        params![chunk_id, pack_f32(embedding)],
    )?;
    Ok(())
}

/// The note a chunk belongs to (the search-hit → note resolution).
pub fn note_for_chunk(conn: &Connection, chunk_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT note_b2id FROM chunks WHERE id = ?1",
            [chunk_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// The whole `chunk_id → note_b2id` map in one scan — the bulk form of
/// [`note_for_chunk`] for hot loops that resolve *many* hits to their notes
/// (graph-filtered search walks the full ranked space; a per-hit `note_for_chunk`
/// there is an O(vault) round-trip storm in the worst case — the same N+1 shape
/// that once made `b2 similar` a ~130s stall, #37). One map load turns the inner
/// loop into a pointer chase.
pub fn chunk_note_map(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut stmt = conn.prepare("SELECT id, note_b2id FROM chunks")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, note) = row?;
        map.insert(id, note);
    }
    Ok(map)
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

/// A note's stored body hash (None if the note isn't indexed yet). Read **before**
/// re-upserting so an incremental reindex can tell whether the body actually
/// changed and skip re-embedding an unchanged note.
pub fn note_body_hash(conn: &Connection, b2id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT body_hash FROM notes WHERE b2id = ?1", [b2id], |r| {
            r.get(0)
        })
        .optional()?)
}

/// Whether every chunk of `b2id` already has a stored vector (and it has at least
/// one chunk). False after a model swap emptied the vector tables, so an
/// unchanged-body note is still re-embedded then. Requires the embedding space to
/// exist — callers ensure it first. A plain indexed anti-join — the vec0 version
/// paid a virtual-table shadow probe per chunk here (#36).
pub fn note_fully_embedded(conn: &Connection, b2id: &str) -> Result<bool> {
    let (n_chunks, n_missing): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE v.chunk_id IS NULL)
         FROM chunks c LEFT JOIN embeddings v ON v.chunk_id = c.id
         WHERE c.note_b2id = ?1",
        [b2id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(n_chunks > 0 && n_missing == 0)
}

/// Every chunk still lacking a stored vector, as `(note_b2id, path, chunk_id, text)`
/// in `(path, seq)` order — the **DB-derived pending set** the embed pass fills
/// (projection-embedding-split.md §2). Deriving it here is what decouples projection
/// from embedding: nothing is handed between the two passes in memory, so any stop
/// point (a cancelled embed, a crash between the passes) heals on the next embed.
/// The ordering reproduces the fused reindex's per-note batching + progress.
/// Generalizes [`note_fully_embedded`]; like it, requires the embedding space to
/// exist — callers ensure it first.
pub fn chunks_missing_vectors(conn: &Connection) -> Result<Vec<(String, String, i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT c.note_b2id, n.path, c.id, c.text
         FROM chunks c
         JOIN notes n ON n.b2id = c.note_b2id
         LEFT JOIN embeddings v ON v.chunk_id = c.id
         WHERE v.chunk_id IS NULL
         ORDER BY n.path, c.seq",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A note's `title` (None if the note is absent or has no title) — the alias for a
/// `[[path|title]]` link written by `b2 link`.
pub fn note_title(conn: &Connection, b2id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT title FROM notes WHERE b2id = ?1", [b2id], |r| {
            r.get::<_, Option<String>>(0)
        })
        .optional()?
        .flatten())
}

/// Every indexed note's `(b2id, path, title)`, ordered by `path` — the flat listing
/// the desktop UI's file tree is built from (`Vault::list_notes`). Path order means
/// the adapter can assemble the folder tree in one pass without re-sorting.
pub fn all_notes(conn: &Connection) -> Result<Vec<(String, String, Option<String>)>> {
    let mut stmt = conn.prepare("SELECT b2id, path, title FROM notes ORDER BY path")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// A note's stored chunk vectors as `(chunk_id, vector)` in `seq` order — one
/// indexed join, not a per-chunk round-trip. Reading a note's own vectors back is
/// what lets discovery search from them without re-embedding — passage↔passage, no
/// `embed_query` (tasks.md ①); it is also discovery's second-stage rescore unit and
/// the input to a centroid refresh. Call only when the embedding space exists
/// (`embedding_space_exists`), else the read hits a missing table. `prepare_cached`
/// because discovery calls this once per shortlisted note.
pub fn note_chunk_vectors(conn: &Connection, note_b2id: &str) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT c.id, e.vector FROM chunks c
         JOIN embeddings e ON e.chunk_id = c.id
         WHERE c.note_b2id = ?1 ORDER BY c.seq",
    )?;
    let rows = stmt.query_map([note_b2id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            crate::embed::unpack_f32(&r.get::<_, Vec<u8>>(1)?),
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Recompute and store `note_b2id`'s centroid from its currently stored chunk
/// vectors (the row is deleted when it has none). The embed pass calls this after
/// finishing a note, so a centroid row exists exactly for embedded notes and always
/// summarizes their *current* vectors — the derived-projection discipline, no
/// separate invalidation. Requires the embedding space to exist.
pub fn refresh_note_centroid(conn: &Connection, note_b2id: &str) -> Result<()> {
    let vectors: Vec<Vec<f32>> = note_chunk_vectors(conn, note_b2id)?
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    match crate::embed::centroid_of(&vectors) {
        Some(c) => {
            conn.execute(
                "INSERT INTO note_centroids(note_b2id, centroid) VALUES (?1, ?2)
                 ON CONFLICT(note_b2id) DO UPDATE SET centroid = excluded.centroid",
                params![note_b2id, pack_f32(&c)],
            )?;
        }
        None => {
            conn.execute(
                "DELETE FROM note_centroids WHERE note_b2id = ?1",
                [note_b2id],
            )?;
        }
    }
    Ok(())
}

/// Stream every stored `(note_b2id, centroid_blob)` through `f`, one row at a time —
/// discovery's first-stage coarse scan. O(notes), the whole point of the two-stage
/// shape (#38): the O(chunks) work happens only for the shortlisted notes. The blob
/// is *borrowed* for the callback (`get_ref`), so scoring adds no per-row allocation.
pub fn for_each_note_centroid(conn: &Connection, mut f: impl FnMut(&str, &[u8])) -> Result<()> {
    let mut stmt = conn.prepare("SELECT note_b2id, centroid FROM note_centroids")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        // Match the ValueRefs rather than `.as_str()?`/`.as_blob()?` — their
        // `FromSqlError` isn't in our error enum, and the column types are fixed by
        // our own DDL, so a mismatched row is skipped rather than an error.
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

/// Stream every stored `(chunk_id, vector_blob)` through `f`, one row at a time — a
/// single sequential scan of the plain `embeddings` table that never materializes
/// the whole vector space at once. One SQL statement for the whole space: the vec0
/// version of this scan cost a shadow-table probe per row (~38.6k internal
/// statements — and O(vault) log lines — per call on a real vault, #38). The blob is
/// *borrowed* for the callback (`get_ref`), so scoring it adds no per-row allocation.
pub fn for_each_stored_vector(conn: &Connection, mut f: impl FnMut(i64, &[u8])) -> Result<()> {
    let mut stmt = conn.prepare("SELECT chunk_id, vector FROM embeddings")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let chunk_id: i64 = row.get(0)?;
        // Match the ValueRef rather than `as_blob()?` — its `FromSqlError` isn't in our
        // error enum, and a stored vector is always a Blob, so a non-blob is skipped.
        if let rusqlite::types::ValueRef::Blob(blob) = row.get_ref(1)? {
            f(chunk_id, blob);
        }
    }
    Ok(())
}

/// Every chunk's squared-L2 distance to `query`, sorted nearest first (ties broken
/// by `chunk_id` for determinism) — the shared scan behind [`vector_search`] /
/// [`vector_search_all`]. Distances are computed **in-process** over the
/// [`for_each_stored_vector`] stream: one sequential statement, one reused decode
/// buffer, the unrolled [`l2_sq`](crate::embed::l2_sq) — the #38 read-path shape.
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

/// Brute-force nearest-neighbour search: the `k` nearest chunk ids to `query`, with
/// their L2 distances, nearest first (ties broken by `chunk_id` for determinism).
/// A full linear scan — exact, no silent truncation at any `k` — which is the
/// brute force index-engine.md §4 specs as comfortable at vault scale. L2 over the
/// stored embeddings ranks by cosine (b2-embed L2-normalizes). The `sqrt` is applied
/// once per *returned* hit; ranking happens on the squared distance (monotonic).
/// [`vector_search_all`] is the same scan without the `k` bound.
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
    pub src_id: String,
    pub dst_id: Option<String>,
    /// The resolved **resource** target (vault-relative path into `resources`),
    /// when the link names a non-`.md` file — mutually exclusive with `dst_id`
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
/// `relations:`), so this deletes the note's edges and re-inserts them from the
/// current Markdown (Flow ①) — the whole graph is a projection of Markdown, with no
/// suggestion rows to preserve.
pub fn replace_authored_edges(conn: &Connection, src_id: &str, edges: &[EdgeRow]) -> Result<()> {
    conn.execute("DELETE FROM edges WHERE src_id = ?1", [src_id])?;
    for e in edges {
        conn.execute(
            "INSERT INTO edges
               (id, src_id, dst_id, dst_resource_path, dst_path_raw, type, origin,
                explanation, embed, caption, occurrence_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                e.id,
                e.src_id,
                e.dst_id,
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
// resolver: b2id ⇄ path  (the resolver *is* notes(b2id PK, path UNIQUE))
// ---------------------------------------------------------------------------

/// `path → b2id` (data-model.md §1).
pub fn resolve_path_to_b2id(conn: &Connection, path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT b2id FROM notes WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?)
}

/// `b2id → path`.
pub fn resolve_b2id_to_path(conn: &Connection, b2id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT path FROM notes WHERE b2id = ?1", [b2id], |r| {
            r.get(0)
        })
        .optional()?)
}

/// Every active authored edge pointing *at* `dst_b2id`: the source note's `b2id`,
/// its vault-relative `path`, and the exact `dst_path_raw` text the inbound link
/// was written with. This is the bounded set a move must rewrite — the
/// materialized graph names the files to touch, so a move never scans the vault
/// (index-engine.md §8). Ordered for deterministic rewriting.
pub fn inbound_edge_targets(
    conn: &Connection,
    dst_b2id: &str,
) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT e.src_id, n.path, e.dst_path_raw
         FROM edges e JOIN notes n ON n.b2id = e.src_id
         WHERE e.dst_id = ?1
         ORDER BY n.path, e.dst_path_raw",
    )?;
    let rows = stmt.query_map([dst_b2id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Resolve a wikilink target (`dst_path_raw`, written without the `.md`
/// extension in Obsidian) to a `b2id`. Tries the literal path, then with `.md`
/// appended. `None` means the link is dangling.
pub fn resolve_link_target(conn: &Connection, link_path: &str) -> Result<Option<String>> {
    if let Some(id) = resolve_path_to_b2id(conn, link_path)? {
        return Ok(Some(id));
    }
    resolve_path_to_b2id(conn, &format!("{link_path}.md"))
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

/// Whether the directed edge `(src_id, dst_id, type)` already exists. `b2 link` uses
/// this to avoid appending a duplicate frontmatter relation for a connection that is
/// already recorded (data-model.md §4).
pub fn edge_exists(conn: &Connection, src_id: &str, dst_id: &str, edge_type: &str) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM edges WHERE src_id = ?1 AND dst_id = ?2 AND type = ?3 LIMIT 1",
            params![src_id, dst_id, edge_type],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}
