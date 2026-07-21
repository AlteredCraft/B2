//! `Vault::write` — the editing surface's one write op (desktop-editing.md §8
//! Step 1): a byte-honest body splice guarded by a content-hash revision, followed
//! by a **model-free** re-projection. The invariants under test (§7): frontmatter
//! bytes are invariant under save; the revision chain never self-conflicts while
//! external writes always conflict; the save path needs no model; and a saved note
//! converges to exactly what a full rebuild would produce once an embed pass runs.

mod common;

use b2_core::db;
use b2_core::vault::Vault;
use b2_core::{open, Error};
use common::golden_vault_copy;
use rusqlite::Connection;
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

const SRS_PATH: &str = "notes/spaced-repetition.md";

/// A reindexed (projected + fake-embedded) golden vault under a temp dir.
fn reindexed(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// Open a second connection onto a vault's index for direct assertions.
fn index_conn(root: &Path) -> Connection {
    open(&root.join(".b2").join("b2.sqlite")).unwrap()
}

#[test]
fn write_replaces_body_and_preserves_frontmatter_bytes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let before = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    let fm_end = before.find("\n---\n").unwrap() + "\n---\n".len();
    let note = vault.read(SRS_PATH).unwrap();

    let new_body = "A completely new body.\n\nWith [[concepts/memory]] still linked.\n";
    let report = vault.write(SRS_PATH, new_body, &note.revision).unwrap();
    assert_eq!(report.path, SRS_PATH);

    // The frontmatter region is byte-identical; the body is the buffer verbatim.
    let after = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    assert_eq!(&after[..fm_end], &before[..fm_end], "frontmatter untouched");
    assert_eq!(&after[fm_end..], new_body, "body is the buffer, verbatim");

    // A fresh read round-trips the new body and the returned revision.
    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.body, new_body);
    assert_eq!(reread.revision, report.revision);
}

#[test]
fn write_conflicts_when_the_file_changed_on_disk() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();

    // An external editor changes the file after our read…
    let abs = root.join(SRS_PATH);
    let external = format!(
        "{}\nAn external append.\n",
        fs::read_to_string(&abs).unwrap()
    );
    fs::write(&abs, &external).unwrap();

    // …so a save based on the stale revision is refused, and nothing is written.
    let err = vault
        .write(SRS_PATH, "my edit", &note.revision)
        .unwrap_err();
    assert!(matches!(err, Error::WriteConflict(p) if p == SRS_PATH));
    assert_eq!(
        fs::read_to_string(&abs).unwrap(),
        external,
        "a conflicted save must not touch the file"
    );

    // The "Keep mine" path: a fresh read (current revision) + write succeeds.
    let fresh = vault.read(SRS_PATH).unwrap();
    vault.write(SRS_PATH, "my edit", &fresh.revision).unwrap();
    assert_eq!(vault.read(SRS_PATH).unwrap().body, "my edit");
}

#[test]
fn sequential_writes_chain_revisions_without_conflict() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    // Each save is based on the revision the previous save returned — the
    // serialized chain (§3 "last save wins — by construction").
    let note = vault.read(SRS_PATH).unwrap();
    let first = vault
        .write(SRS_PATH, "draft one\n", &note.revision)
        .unwrap();
    let second = vault
        .write(SRS_PATH, "draft two\n", &first.revision)
        .unwrap();
    assert_ne!(first.revision, second.revision);
    assert_eq!(vault.read(SRS_PATH).unwrap().body, "draft two\n");
}

#[test]
fn write_reprojects_keyword_graph_and_clears_stale_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    let conn = index_conn(&root);
    assert_eq!(count(&conn, "embeddings"), count(&conn, "chunks"));

    // The golden SRS note links memory twice (references + supports). Save a body
    // that keeps ONE link and adds fresh text.
    let note = vault.read(SRS_PATH).unwrap();
    let new_body = "Rewritten body about zettelkasten workflows.\n\nSee [[concepts/memory]].\n";
    vault.write(SRS_PATH, new_body, &note.revision).unwrap();

    // Keyword index reflects the new text immediately (model-free)…
    let hits = vault.search("zettelkasten", 10).unwrap();
    assert!(hits.iter().any(|h| h.path == SRS_PATH), "FTS is current");

    // …edges were re-derived from the new body (two typed links → one bare link)…
    let outbound: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM edges e JOIN notes n ON n.b2id = e.src_id WHERE n.path = ?1",
            [SRS_PATH],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outbound, 1, "edges re-projected from the saved body");

    // …and the re-chunked note's vectors were cleared into the pending set, which
    // an embed pass then fills exactly (§7 invariant 5 — convergence).
    let missing = db::chunks_missing_vectors(&conn).unwrap();
    assert!(!missing.is_empty(), "saved chunks await embedding");
    assert!(missing.iter().all(|(_, path, _, _)| path == SRS_PATH));
    let embed = vault.embed(&mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(embed.embedded, 1, "the embed pass fills the saved note");
    assert_eq!(count(&conn, "embeddings"), count(&conn, "chunks"));
}

#[test]
fn write_needs_no_embedding_space() {
    // A projected-only vault (no vector tables, no model anywhere): saving works — the
    // model-free proof (§7 invariant 4).
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.project(false).unwrap();

    let note = vault.read(SRS_PATH).unwrap();
    vault
        .write(
            SRS_PATH,
            "Saved with no vectors in sight.\n",
            &note.revision,
        )
        .unwrap();

    let conn = index_conn(&root);
    assert!(
        !db::embedding_space_exists(&conn).unwrap(),
        "a save must not create the embedding space"
    );
    // The saved text is keyword-searchable straight away (keyword-first, §5 of the split).
    let hits = vault.search("vectors in sight", 10).unwrap();
    assert!(hits.iter().any(|h| h.path == SRS_PATH));
}

#[test]
fn write_an_empty_body_and_recover() {
    // Select-all-delete under autosave is a real input: an empty buffer must save
    // (a frontmatter-only file; zero chunks replace the note's old rows) without
    // upsetting the index, and the revision chain must continue out of the empty
    // state. `chunk_body` documents the empty case; this pins it end to end.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();
    let report = vault.write(SRS_PATH, "", &note.revision).unwrap();

    // The file is frontmatter only, verbatim; a fresh read round-trips it.
    let on_disk = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    assert!(on_disk.ends_with("---\n"), "frontmatter only: {on_disk:?}");
    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.body, "");
    assert_eq!(reread.revision, report.revision);

    // The note projected to zero chunks but is still indexed and searchable-around.
    let conn = index_conn(&root);
    let chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks c JOIN notes n ON n.b2id = c.note_b2id WHERE n.path = ?1",
            [SRS_PATH],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunks, 0, "an empty body projects zero chunks");
    assert!(vault
        .list_notes()
        .unwrap()
        .iter()
        .any(|n| n.path == SRS_PATH));
    vault.search("memory", 10).unwrap();

    // …and the chain continues out of the empty state.
    let next = vault
        .write(SRS_PATH, "Recovered.\n", &report.revision)
        .unwrap();
    assert_ne!(next.revision, report.revision);
    assert_eq!(vault.read(SRS_PATH).unwrap().body, "Recovered.\n");
}

#[test]
fn write_returns_the_revision_of_the_final_on_disk_bytes() {
    // The contract the save chain hangs on (§4 step 5): whatever the save left on
    // disk — body splice, and any ordinary-path work like a `b2id` stamp — the
    // returned revision hashes those FINAL bytes, so the next save never
    // self-conflicts. (An indexed note's file always carries its stamp — projection
    // stamps on first sight — so the stamp arm is defensive; the contract is what
    // the chain needs proven.)
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    fs::write(
        root.join("fresh.md"),
        "---\ntype: note\ntitle: Fresh\n---\nNo b2id yet.\n",
    )
    .unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.project(false).unwrap(); // stamps fresh.md on first sight

    let note = vault.read("fresh").unwrap();
    let report = vault
        .write("fresh", "A saved body.\n", &note.revision)
        .unwrap();
    let on_disk = fs::read_to_string(root.join("fresh.md")).unwrap();
    assert!(on_disk.contains("b2id:"), "identity travels in the file");
    assert!(on_disk.ends_with("A saved body.\n"));
    assert_eq!(
        report.revision,
        blake3::hash(on_disk.as_bytes()).to_hex().to_string(),
        "the returned revision hashes the final on-disk bytes"
    );
    // …and the chain continues from it without conflict.
    vault.write("fresh", "Again.\n", &report.revision).unwrap();
}
