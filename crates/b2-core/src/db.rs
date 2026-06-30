//! Opening the index, the schema migration, and the `notes`/`note_aliases`
//! projection + the `b2id ⇄ path` resolver.
//!
//! `sqlite-vec` is registered as a SQLite *auto-extension* (statically linked, no
//! runtime `load_extension`), and every connection is opened `WAL` +
//! `foreign_keys=ON` per planning/specs/index-engine-build.md §0. Every table here
//! is a derived projection of `(Markdown ∪ log)` — nothing is a source of truth.

use crate::error::Result;
use rusqlite::{ffi, params, Connection, OptionalExtension};
use sqlite_vec::sqlite3_vec_init;
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::sync::Once;

/// The B2 index schema version stamped into `meta.schema_version`. Bumping it is
/// the migration gate — which B2 schema built a given `b2.sqlite` (§1.0).
pub const SCHEMA_VERSION: i64 = 1;

static REGISTER_VEC: Once = Once::new();

/// Register `sqlite-vec` exactly once per process so every later `Connection`
/// exposes the `vec0` virtual table with no runtime `load_extension`.
fn register_sqlite_vec() {
    // sqlite-vec and rusqlite each declare their own (ABI-identical) SQLite FFI
    // types, so the init fn must be transmuted to the signature rusqlite's
    // `sqlite3_auto_extension` expects — this mirrors the official sqlite-vec Rust
    // example. The explicit annotation is the type clippy would otherwise ask for.
    type InitFn = unsafe extern "C" fn(
        *mut ffi::sqlite3,
        *mut *mut c_char,
        *const ffi::sqlite3_api_routines,
    ) -> c_int;
    REGISTER_VEC.call_once(|| unsafe {
        ffi::sqlite3_auto_extension(Some(std::mem::transmute::<*const (), InitFn>(
            sqlite3_vec_init as *const (),
        )));
    });
}

/// Open (creating if needed) the B2 index at `path` with the locked pragmas and an
/// idempotent migration. Safe to call on a fresh or an already-built index.
pub fn open(path: &Path) -> Result<Connection> {
    register_sqlite_vec();
    let conn = Connection::open(path)?;
    // execute_batch tolerates the row PRAGMA journal_mode returns.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

/// Create the schema and seed `schema_version` once. `IF NOT EXISTS` +
/// `INSERT OR IGNORE` keep this a no-op on reopen, so `open()` stays idempotent.
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
           key   TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );

         CREATE TABLE IF NOT EXISTS notes (
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
         CREATE INDEX IF NOT EXISTS note_aliases_alias_idx ON note_aliases(alias);",
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

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
pub fn upsert_note(conn: &Connection, row: &NoteRow) -> Result<()> {
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

/// `path → b2id` half of the resolver (data-model.md §1; the resolver *is* the
/// `notes(b2id PK, path UNIQUE)` pair, not a separate table).
pub fn resolve_path_to_b2id(conn: &Connection, path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT b2id FROM notes WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?)
}

/// `b2id → path` half of the resolver.
pub fn resolve_b2id_to_path(conn: &Connection, b2id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT path FROM notes WHERE b2id = ?1", [b2id], |r| {
            r.get(0)
        })
        .optional()?)
}
