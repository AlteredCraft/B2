//! Resources slice 1 — inventory & graph
//! (planning/specs/resources-inventory-graph.md).
//!
//! Step 0: the v4 schema — the `resources` table exists, `edges` carries the
//! resource-target columns (`dst_resource_path`, `embed`, `caption`), dangling
//! means *neither* target resolved, and the version gate drops a v3 index.

use b2_core::{open, SCHEMA_VERSION};
use rusqlite::Connection;

/// Column names of `table` via `pragma table_info`, for shape assertions.
fn columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    cols
}

#[test]
fn v4_schema_has_resources_table_and_widened_edges() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    let resources = columns(&conn, "resources");
    for col in ["path", "class", "size", "mtime", "content_hash", "indexed_at"] {
        assert!(resources.iter().any(|c| c == col), "resources.{col} missing");
    }

    let edges = columns(&conn, "edges");
    for col in ["dst_resource_path", "embed", "caption"] {
        assert!(edges.iter().any(|c| c == col), "edges.{col} missing");
    }

    // The class vocabulary is closed (research §3): a value outside it must refuse.
    let bad = conn.execute(
        "INSERT INTO resources(path, class, size, content_hash, indexed_at)
         VALUES ('x.xyz', 'mystery', 0, 'h', 'now')",
        [],
    );
    assert!(bad.is_err(), "an unknown class must violate the CHECK");
}

/// The schema-version gate: a v3 index is dropped wholesale and rebuilt at v4 —
/// no migration code, ever (the disposable-index tenet).
#[test]
fn v3_index_is_dropped_and_rebuilt_at_v4() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("b2.sqlite");

    {
        let conn = open(&db_path).unwrap();
        conn.execute_batch(
            "INSERT INTO notes(b2id, path, type, body_hash, indexed_at)
               VALUES ('01JX000000000000000000000A', 'a.md', 'note', 'h', 'now');
             INSERT INTO resources(path, class, size, content_hash, indexed_at)
               VALUES ('img.png', 'image', 3, 'h', 'now');",
        )
        .unwrap();
        // Simulate an index built by the previous schema.
        conn.execute("UPDATE meta SET value = '3' WHERE key = 'schema_version'", [])
            .unwrap();
    }

    let conn = open(&db_path).unwrap();
    let version: String = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION.to_string());
    let notes: i64 = conn
        .query_row("SELECT count(*) FROM notes", [], |r| r.get(0))
        .unwrap();
    let resources: i64 = conn
        .query_row("SELECT count(*) FROM resources", [], |r| r.get(0))
        .unwrap();
    assert_eq!((notes, resources), (0, 0), "the gate must drop v3 rows");
}

/// A resource-targeted edge is FK-checked, deduped by the partial unique index,
/// and **re-dangles** (dst_resource_path → NULL, dst_path_raw retained) when its
/// target row is pruned — the ON DELETE SET NULL that keeps pruning one statement.
#[test]
fn resource_edges_are_fk_checked_and_redangle_on_prune() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    conn.execute_batch(
        "INSERT INTO notes(b2id, path, type, body_hash, indexed_at)
           VALUES ('01JX000000000000000000000A', 'a.md', 'note', 'h', 'now');",
    )
    .unwrap();

    // FK: the target row must exist.
    let orphan = conn.execute(
        "INSERT INTO edges(id, src_id, dst_resource_path, dst_path_raw, type, origin)
         VALUES ('e0', '01JX000000000000000000000A', 'missing.png', 'missing.png',
                 'references', 'inline')",
        [],
    );
    assert!(orphan.is_err(), "dst_resource_path must be FK-enforced");

    conn.execute_batch(
        "INSERT INTO resources(path, class, size, content_hash, indexed_at)
           VALUES ('img.png', 'image', 3, 'h', 'now');
         INSERT INTO edges(id, src_id, dst_resource_path, dst_path_raw, type, origin, embed, caption)
           VALUES ('e1', '01JX000000000000000000000A', 'img.png', 'img.png',
                   'references', 'inline', 1, 'a sailboat');",
    )
    .unwrap();

    // Dedup: same (src, resource, type, occurrence) must refuse — NULL dst_id makes
    // the note-edge UNIQUE constraint inert here, hence the partial index.
    let dup = conn.execute(
        "INSERT INTO edges(id, src_id, dst_resource_path, dst_path_raw, type, origin)
         VALUES ('e2', '01JX000000000000000000000A', 'img.png', 'img.png',
                 'references', 'inline')",
        [],
    );
    assert!(dup.is_err(), "duplicate resource edge must violate the partial unique index");

    // Prune the resource: the edge survives as dangling, raw text retained.
    conn.execute("DELETE FROM resources WHERE path = 'img.png'", [])
        .unwrap();
    let (dst_resource, raw): (Option<String>, String) = conn
        .query_row(
            "SELECT dst_resource_path, dst_path_raw FROM edges WHERE id = 'e1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(dst_resource, None, "prune must re-dangle the edge");
    assert_eq!(raw, "img.png", "the authored raw target must survive the prune");
}
