//! Step 0 — opening the index with the locked pragmas, and the `meta` migration.
//!
//! `sqlite-vec` is registered as a SQLite *auto-extension* (statically linked, no
//! runtime `load_extension`), and every connection is opened `WAL` +
//! `foreign_keys=ON` per planning/specs/index-engine-build.md §0. `meta` is the
//! only table step 0 lands; it is authoritative bookkeeping for the index itself
//! (§1.0) and, like every table here, rebuildable — nothing is a source of truth.

use rusqlite::{ffi, Connection, Result};
use sqlite_vec::sqlite3_vec_init;
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::sync::Once;

/// The B2 index schema version stamped into `meta.schema_version`. Bumping it is
/// the migration gate — which B2 schema built a given `b2.sqlite` (§1.0).
pub const SCHEMA_VERSION: i64 = 1;

static REGISTER_VEC: Once = Once::new();

/// Register `sqlite-vec` exactly once per process so every later `Connection`
/// exposes the `vec0` virtual table with no runtime `load_extension`
/// (the macOS extension-loading friction noted in index-engine.md §8 is gone
/// because the extension is compiled in and auto-registered).
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

/// Create `meta` and seed `schema_version` once. `IF NOT EXISTS` + `INSERT OR
/// IGNORE` make this a no-op on reopen, so `open()` stays idempotent.
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
           key   TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );",
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}
