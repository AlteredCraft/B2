//! Delete a note / resource / folder — the destructive complement of `mv`:
//! remove the file(s) from disk and the projection rows from the index, leaving
//! inbound links **dangling** (never rewritten — the deleted target is simply
//! gone, exactly as if the file had been removed externally and the vault fully
//! reindexed). Driven through the [`Vault`] façade against the golden vault,
//! fully deterministic under the FakeEmbedder.

mod common;

use b2_core::vault::Vault;
use b2_core::{open, Error};
use common::{golden_vault_copy, MEMORY_ID};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

const MEMORY_PATH: &str = "concepts/memory.md";
const SRS_PATH: &str = "notes/spaced-repetition.md";

/// A reindexed golden vault under a temp dir; returns (vault, vault_root).
fn reindexed(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

/// Open a second connection onto a vault's index for direct assertions.
fn index_conn(root: &Path) -> Connection {
    open(&root.join(".b2").join("b2.sqlite")).unwrap()
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// Every edge row's identity + resolution, ordered — the shape that must match a
/// from-scratch rebuild for `delete ≡ external-delete + reindex` to hold.
fn edge_rows(conn: &Connection) -> Vec<(String, String, Option<String>, String, String, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT id, src_id, dst_id, dst_path_raw, type, occurrence_index
             FROM edges ORDER BY id",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap();
    rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
}

#[test]
fn delete_note_removes_file_and_rows_and_dangles_inbound_links() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    let srs_before = fs::read_to_string(root.join(SRS_PATH)).unwrap();

    let report = vault.delete_note(MEMORY_PATH).unwrap();
    assert_eq!(report.b2id, MEMORY_ID);
    assert_eq!(report.path, MEMORY_PATH);
    assert_eq!(report.dangled, vec![SRS_PATH.to_string()]);

    // The file is gone; the linking note's bytes are untouched (a delete never
    // rewrites bodies — the links dangle, they aren't repaired).
    assert!(!root.join(MEMORY_PATH).exists());
    assert_eq!(fs::read_to_string(root.join(SRS_PATH)).unwrap(), srs_before);

    // The note no longer resolves by path or b2id.
    assert!(matches!(
        vault.read(MEMORY_PATH).unwrap_err(),
        Error::NoteNotFound(_)
    ));
    assert!(matches!(
        vault.read(MEMORY_ID).unwrap_err(),
        Error::NoteNotFound(_)
    ));

    // Its projection rows are gone: only SRS's rows remain.
    let conn = index_conn(&root);
    assert_eq!(count(&conn, "notes"), 1);
    let chunk_owners: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE note_b2id = ?1",
            [MEMORY_ID],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunk_owners, 0);

    // The linker's edges re-derived: no neighbor left, the links now dangle.
    let explain = vault.explain(SRS_PATH).unwrap();
    assert!(explain.connections.is_empty(), "no resolved edges remain");
    let targets: Vec<&str> = explain
        .unresolved
        .iter()
        .map(|u| u.target.as_str())
        .collect();
    assert_eq!(
        targets,
        vec!["concepts/memory", "concepts/memory"],
        "the body link and the frontmatter relation both dangle"
    );
}

#[test]
fn delete_note_equals_external_delete_plus_full_reindex() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Vault A: the delete op.
    let (vault_a, root_a) = {
        let dir = tmp.path().join("a");
        fs::create_dir_all(&dir).unwrap();
        reindexed(&dir)
    };
    vault_a.delete_note(MEMORY_PATH).unwrap();

    // Vault B: the file removed externally, then a full reindex reconciles.
    let (vault_b, root_b) = {
        let dir = tmp.path().join("b");
        fs::create_dir_all(&dir).unwrap();
        reindexed(&dir)
    };
    fs::remove_file(root_b.join(MEMORY_PATH)).unwrap();
    let report = vault_b.reindex().unwrap();
    assert_eq!(report.notes_pruned, 1);

    let (conn_a, conn_b) = (index_conn(&root_a), index_conn(&root_b));
    assert_eq!(edge_rows(&conn_a), edge_rows(&conn_b));
    assert_eq!(count(&conn_a, "notes"), count(&conn_b, "notes"));
    assert_eq!(count(&conn_a, "chunks"), count(&conn_b, "chunks"));
    assert_eq!(count(&conn_a, "embeddings"), count(&conn_b, "embeddings"));

    // And the delete is stable under a further reindex: nothing left to prune.
    let again = vault_a.reindex().unwrap();
    assert_eq!(again.notes_pruned, 0);
    assert_eq!(edge_rows(&index_conn(&root_a)), edge_rows(&conn_b));
}

#[test]
fn delete_note_unknown_ref_refuses() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    assert!(matches!(
        vault.delete_note("no/such-note.md").unwrap_err(),
        Error::NoteNotFound(_)
    ));
    // Nothing was touched.
    assert!(root.join(MEMORY_PATH).exists());
    assert_eq!(count(&index_conn(&root), "notes"), 2);
}

#[test]
fn delete_resource_removes_file_and_inventory_and_dangles_links() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    // Give SRS a body link at the resource, through the ordinary save path.
    let note = vault.read(SRS_PATH).unwrap();
    let body = format!(
        "{}\nSee [[resources/data.txt]] for the raw data.\n",
        note.body
    );
    vault.write(SRS_PATH, &body, &note.revision).unwrap();
    assert_eq!(
        vault.explain(SRS_PATH).unwrap().resources.len(),
        1,
        "the resource link resolved before the delete"
    );

    let report = vault.delete_resource("resources/data.txt").unwrap();
    assert_eq!(report.path, "resources/data.txt");
    assert_eq!(report.dangled, vec![SRS_PATH.to_string()]);

    assert!(!root.join("resources/data.txt").exists());
    let listed = vault.list_resources().unwrap();
    assert!(listed.iter().all(|r| r.path != "resources/data.txt"));

    // The link now dangles rather than resolving to a resource.
    let explain = vault.explain(SRS_PATH).unwrap();
    assert!(explain.resources.is_empty());
    assert!(explain
        .unresolved
        .iter()
        .any(|u| u.target == "resources/data.txt"));

    // Rebuild-equivalence: a further full reindex changes nothing.
    let conn = index_conn(&root);
    let before = edge_rows(&conn);
    let again = vault.reindex().unwrap();
    assert_eq!(again.resources_pruned, 0);
    assert_eq!(edge_rows(&index_conn(&root)), before);
}

#[test]
fn delete_resource_unknown_path_refuses() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());
    assert!(matches!(
        vault.delete_resource("resources/nope.png").unwrap_err(),
        Error::ResourceNotFound(_)
    ));
}

#[test]
fn delete_dir_removes_subtree_and_dangles_outside_links() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let report = vault.delete_dir("concepts").unwrap();
    assert_eq!(report.dir, "concepts");
    assert_eq!(report.deleted_notes, 1);
    assert_eq!(report.deleted_resources, 0);
    assert_eq!(report.dangled, vec![SRS_PATH.to_string()]);

    assert!(!root.join("concepts").exists());
    assert!(matches!(
        vault.read(MEMORY_ID).unwrap_err(),
        Error::NoteNotFound(_)
    ));

    // The surviving linker dangles, exactly as a single-note delete leaves it.
    let explain = vault.explain(SRS_PATH).unwrap();
    assert!(explain.connections.is_empty());
    assert_eq!(explain.unresolved.len(), 2);

    // Stable under a further reindex.
    let again = vault.reindex().unwrap();
    assert_eq!(again.notes_pruned, 0);
}

#[test]
fn delete_dir_containing_the_linker_leaves_the_target_intact() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    // Deleting `notes/` removes SRS (the linker). Memory survives untouched, and
    // no surviving file needs re-projection (the linker died with the folder).
    let report = vault.delete_dir("notes").unwrap();
    assert_eq!(report.deleted_notes, 1);
    assert!(report.dangled.is_empty());

    assert!(!root.join("notes").exists());
    let memory = vault.read(MEMORY_PATH).unwrap();
    assert_eq!(memory.b2id, MEMORY_ID);
    assert!(vault.neighbors(MEMORY_ID).unwrap().is_empty());
    assert_eq!(count(&index_conn(&root), "notes"), 1);
}

#[test]
fn delete_dir_removes_resources_and_their_inventory() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let report = vault.delete_dir("resources").unwrap();
    assert_eq!(report.deleted_notes, 0);
    assert_eq!(report.deleted_resources, 4);
    assert!(!root.join("resources").exists());
    assert!(vault.list_resources().unwrap().is_empty());
}

#[test]
fn delete_dir_missing_or_invalid_refuses() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    assert!(matches!(
        vault.delete_dir("no-such-folder").unwrap_err(),
        Error::DirNotFound(_)
    ));
    assert!(matches!(
        vault.delete_dir("../up").unwrap_err(),
        Error::DirNotFound(_)
    ));
    assert!(matches!(
        vault.delete_dir("").unwrap_err(),
        Error::DirNotFound(_)
    ));
    // Nothing was touched.
    assert_eq!(count(&index_conn(&root), "notes"), 2);
}
