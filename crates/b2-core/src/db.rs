//! Opening the index, the schema migration, and the projection helpers for the
//! Markdown-derived tiers: `notes`/`note_aliases`, `chunks` (+FTS5), and the
//! typed `edges` graph, plus the `b2id ⇄ path` resolver.
//!
//! `sqlite-vec` is registered as a SQLite *auto-extension* (statically linked, no
//! runtime `load_extension`), and every connection is opened `WAL` +
//! `foreign_keys=ON` per planning/specs/index-engine-build.md §0. Every table here
//! is a derived projection of `(Markdown ∪ log)` — nothing is a source of truth.

use crate::chunk::Chunk;
use crate::embed::pack_f32;
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
/// The DDL mirrors planning/specs/index-engine-build.md §1 (chunks_vec lands in
/// step 3; suggested-edge provenance in step 4).
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

         CREATE TABLE IF NOT EXISTS edges (
           id               TEXT PRIMARY KEY,
           src_id           TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,
           dst_id           TEXT,
           dst_path_raw     TEXT NOT NULL,
           type             TEXT NOT NULL,
           origin           TEXT NOT NULL CHECK (origin IN ('inline','frontmatter','suggested')),
           status           TEXT NOT NULL CHECK (status IN ('active','suggested','rejected')),
           explanation      TEXT,
           occurrence_index INTEGER NOT NULL DEFAULT 0,
           CHECK ( (origin = 'suggested'               AND status IN ('suggested','rejected'))
                OR (origin IN ('inline','frontmatter') AND status = 'active') ),
           UNIQUE (src_id, dst_id, type, occurrence_index)
         );
         CREATE INDEX IF NOT EXISTS edges_src_idx      ON edges(src_id);
         CREATE INDEX IF NOT EXISTS edges_dst_type_idx ON edges(dst_id, type);
         CREATE INDEX IF NOT EXISTS edges_status_idx   ON edges(status);
         CREATE INDEX IF NOT EXISTS edges_dangling_idx ON edges(dst_path_raw) WHERE dst_id IS NULL;

         CREATE TABLE IF NOT EXISTS edge_provenance (
           edge_id    TEXT PRIMARY KEY REFERENCES edges(id) ON DELETE CASCADE,
           by         TEXT NOT NULL,
           source     TEXT,
           confidence REAL,
           created    TEXT NOT NULL,
           decided    TEXT
         );",
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', ?1)",
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

/// Every note's `b2id`, sorted ascending — the deterministic anchor iteration
/// order for whole-vault connection discovery (tasks.md ②), so the ids minted for
/// its suggestions are reproducible under a fixed [`IdGen`](crate::id::IdGen).
pub fn all_note_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT b2id FROM notes ORDER BY b2id")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// chunks (FTS kept in lockstep by the triggers in migrate())
// ---------------------------------------------------------------------------

/// Replace a note's chunks (delete + reinsert) and return the new chunk ids in
/// `seq` order. The FTS triggers emit the `'delete'` sentinel for the removed
/// rows; `chunks_vec` has no FK/trigger back to `chunks`, so its stale rows are
/// cleared here explicitly. Together this is what makes an incremental re-index
/// equal a full rebuild. The caller embeds the returned ids (Flow ①).
pub fn replace_chunks(conn: &Connection, note_b2id: &str, chunks: &[Chunk]) -> Result<Vec<i64>> {
    if embedding_space_exists(conn)? {
        let old_ids: Vec<i64> = {
            let mut stmt = conn.prepare("SELECT id FROM chunks WHERE note_b2id = ?1")?;
            let rows = stmt.query_map([note_b2id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<i64>>>()?
        };
        for id in old_ids {
            conn.execute("DELETE FROM chunks_vec WHERE chunk_id = ?1", [id])?;
        }
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
// embeddings — chunks_vec is created at the embedder's dim (not in migrate()),
// because the vec0 dimension is a DDL literal pinned to meta.embed_dim (§1.0).
// ---------------------------------------------------------------------------

/// Whether the `chunks_vec` virtual table currently exists.
pub fn embedding_space_exists(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'chunks_vec'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Ensure `chunks_vec` exists at `dim`, recording `(embed_model_id, embed_dim)`
/// in `meta`. If either differs from what is recorded — a model swap — the table
/// is dropped and recreated empty, so a full re-embed follows (index-engine.md
/// §8). `meta` is the only place a swap can be detected, so vectors never go
/// silently stale.
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

    // dim is an integer we control (never user input) → safe to interpolate.
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS chunks_vec;
         CREATE VIRTUAL TABLE chunks_vec USING vec0(
           chunk_id  INTEGER PRIMARY KEY,
           embedding FLOAT[{dim}]
         );"
    ))?;
    upsert_meta(conn, "embed_model_id", model_id)?;
    upsert_meta(conn, "embed_dim", &dim.to_string())?;
    Ok(())
}

/// The `(embed_model_id, embed_dim)` a prior ingest recorded in `meta`, if any.
/// `None` means the vault has never been embedded (no `chunks_vec` yet). This is
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
        "INSERT INTO chunks_vec(chunk_id, embedding) VALUES (?1, ?2)",
        params![chunk_id, pack_f32(embedding)],
    )?;
    Ok(())
}

/// A note's chunk ids in `seq` order — the note's own vectors/text, e.g. the queries
/// discovery candidate generation runs from (each chunk KNN-searches `chunks_vec`).
pub fn chunks_for_note(conn: &Connection, note_b2id: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM chunks WHERE note_b2id = ?1 ORDER BY seq")?;
    let rows = stmt.query_map([note_b2id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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

/// A chunk's text (None if the chunk id is unknown) — the search-hit → snippet
/// resolution the CLI shows.
pub fn chunk_text(conn: &Connection, chunk_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT text FROM chunks WHERE id = ?1", [chunk_id], |r| {
            r.get(0)
        })
        .optional()?)
}

/// A note's body as the index holds it: its chunk texts in `seq` order, blank-line
/// joined (empty if the note has no chunks). This is the `NoteCtx.text` a *real*
/// relator reads (tasks.md ② sub-decision — reuse the already-indexed chunks rather
/// than re-read the file); [`FakeRelator`](crate::relate::FakeRelator) ignores it,
/// so discovery is provable without a model.
pub fn note_text(conn: &Connection, note_b2id: &str) -> Result<String> {
    let mut stmt = conn.prepare("SELECT text FROM chunks WHERE note_b2id = ?1 ORDER BY seq")?;
    let rows = stmt.query_map([note_b2id], |r| r.get::<_, String>(0))?;
    let parts = rows.collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(parts.join("\n\n"))
}

/// A note's `title` (None if the note is absent or has no title) — the alias for a
/// `[[path|title]]` link written on accept.
pub fn note_title(conn: &Connection, b2id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT title FROM notes WHERE b2id = ?1", [b2id], |r| {
            r.get::<_, Option<String>>(0)
        })
        .optional()?
        .flatten())
}

/// Total chunk count (used to size a full KNN pool for graph-filtered search).
pub fn chunk_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?)
}

/// A chunk's stored embedding, unpacked from `chunks_vec` (`None` if the chunk has
/// no vector row). Reading a note's own vectors back is what lets discovery KNN from
/// them without re-embedding — passage↔passage, no `embed_query` (tasks.md ①). Call
/// only when the embedding space exists (`embedding_space_exists`), else the read
/// hits a missing table.
pub fn chunk_vector(conn: &Connection, chunk_id: i64) -> Result<Option<Vec<f32>>> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT embedding FROM chunks_vec WHERE chunk_id = ?1",
            [chunk_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(blob.map(|b| crate::embed::unpack_f32(&b)))
}

/// Brute-force KNN over `chunks_vec`: the `k` nearest chunk ids to `query`, with
/// their distances, nearest first (build spec §1.2 / Flow ②).
pub fn vector_search(conn: &Connection, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
    let mut stmt = conn.prepare(
        "SELECT chunk_id, distance FROM chunks_vec
         WHERE embedding MATCH ?1 ORDER BY distance LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pack_f32(query), k as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)? as f32))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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
    pub dst_path_raw: String,
    pub r#type: String,
    pub origin: String,
    pub status: String,
    pub explanation: Option<String>,
    pub occurrence_index: i64,
}

/// Replace a note's authored (`inline`/`frontmatter`) edges. Log-derived
/// `suggested`/`rejected` rows are left untouched, so a Markdown re-parse never
/// clobbers the review queue (Flow ①).
pub fn replace_authored_edges(conn: &Connection, src_id: &str, edges: &[EdgeRow]) -> Result<()> {
    conn.execute(
        "DELETE FROM edges WHERE src_id = ?1 AND origin IN ('inline','frontmatter')",
        [src_id],
    )?;
    for e in edges {
        conn.execute(
            "INSERT INTO edges
               (id, src_id, dst_id, dst_path_raw, type, origin, status, explanation, occurrence_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                e.id,
                e.src_id,
                e.dst_id,
                e.dst_path_raw,
                e.r#type,
                e.origin,
                e.status,
                e.explanation,
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

/// Resolve a wikilink target (`dst_path_raw`, written without the `.md`
/// extension in Obsidian) to a `b2id`. Tries the literal path, then with `.md`
/// appended. `None` means the link is dangling.
pub fn resolve_link_target(conn: &Connection, link_path: &str) -> Result<Option<String>> {
    if let Some(id) = resolve_path_to_b2id(conn, link_path)? {
        return Ok(Some(id));
    }
    resolve_path_to_b2id(conn, &format!("{link_path}.md"))
}

// ---------------------------------------------------------------------------
// review state: suggested/rejected edges + edge_provenance
// (projected from the log by suggest/replay — never authored here)
// ---------------------------------------------------------------------------

/// Whether any edge already exists for `(src_id, dst_id, type)` — in any status.
/// Generation uses this to refuse re-proposing an already-active, pending, or
/// rejected (tombstoned) connection (data-model.md §4).
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

/// Insert a suggested edge (`origin='suggested'`, `status='suggested'`,
/// `occurrence_index=0`). Returns whether a row was inserted: `false` means the
/// `(src, dst, type, 0)` tuple is already taken — which on **replay** happens when
/// an accepted suggestion's edge was already materialized from frontmatter by
/// Flow ①, so the `generated` event is harmlessly absorbed (the caller then skips
/// the provenance row to avoid a dangling FK). The companion provenance row is
/// inserted separately.
pub fn insert_suggested_edge(
    conn: &Connection,
    edge_id: &str,
    src_id: &str,
    dst_id: Option<&str>,
    dst_path_raw: &str,
    edge_type: &str,
    explanation: Option<&str>,
) -> Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO edges
           (id, src_id, dst_id, dst_path_raw, type, origin, status, explanation, occurrence_index)
         VALUES (?1, ?2, ?3, ?4, ?5, 'suggested', 'suggested', ?6, 0)",
        params![
            edge_id,
            src_id,
            dst_id,
            dst_path_raw,
            edge_type,
            explanation
        ],
    )?;
    Ok(n == 1)
}

/// Insert the provenance row for a suggested/rejected edge.
pub fn insert_edge_provenance(
    conn: &Connection,
    edge_id: &str,
    by: &str,
    source: Option<&str>,
    confidence: Option<f64>,
    created: &str,
    decided: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO edge_provenance(edge_id, by, source, confidence, created, decided)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![edge_id, by, source, confidence, created, decided],
    )?;
    Ok(())
}

/// Tombstone a suggestion: flip the edge to `status='rejected'` and stamp the
/// provenance `decided` time. The row stays as rejection memory (never re-proposed).
pub fn mark_edge_rejected(conn: &Connection, edge_id: &str, decided: &str) -> Result<()> {
    conn.execute(
        "UPDATE edges SET status = 'rejected' WHERE id = ?1",
        [edge_id],
    )?;
    conn.execute(
        "UPDATE edge_provenance SET decided = ?2 WHERE edge_id = ?1",
        params![edge_id, decided],
    )?;
    Ok(())
}

/// Delete an edge (its `edge_provenance` row cascades). Used when an accepted
/// suggestion leaves the queue on replay — its active edge comes from Markdown.
pub fn delete_edge(conn: &Connection, edge_id: &str) -> Result<()> {
    conn.execute("DELETE FROM edges WHERE id = ?1", [edge_id])?;
    Ok(())
}
