//! Importing an outside file (the desktop's drag-from-Finder and file picker): a byte-honest
//! copy, projected with no reindex, routed by extension like the walk, never clobbering or
//! landing anywhere but where it was aimed.

mod common;

use b2_core::Error;
use common::{count, index_conn, opened_vault, reindexed_vault, MEMORY_PATH};
use std::fs;

/// Not valid UTF-8, so an import that treated everything as text would fail.
const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0x00];

#[test]
fn a_dropped_binary_lands_in_the_folder_byte_for_byte_and_inventories() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let report = vault
        .import_file("resources", "schematic.png", PNG_BYTES)
        .unwrap();

    assert_eq!(report.path, "resources/schematic.png");
    assert!(!report.note, "a non-`.md` file is routed as a resource");
    assert_eq!(
        fs::read(root.join("resources/schematic.png")).unwrap(),
        PNG_BYTES,
        "the bytes are copied verbatim — B2 authors nothing here"
    );
    // Listed with no reindex.
    let listed = vault.list_resources().unwrap();
    assert!(
        listed.iter().any(|r| r.path == "resources/schematic.png"),
        "{listed:?}"
    );
}

#[test]
fn a_dropped_markdown_file_lands_as_a_note_with_its_own_frontmatter_intact() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let doc = "---\ntitle: \"From elsewhere\"\ncreated: 2026-01-02\nsource: web\n---\n\nA clipped paragraph about hydroponics.\n";
    let report = vault
        .import_file("notes", "clipped.md", doc.as_bytes())
        .unwrap();

    assert_eq!(report.path, "notes/clipped.md");
    assert!(report.note, "a `.md` is routed as a note");

    // B2 adds nothing (W1): the path is already the identity (L1).
    let on_disk = fs::read_to_string(root.join("notes/clipped.md")).unwrap();
    assert_eq!(on_disk, doc, "the bytes are the human's, verbatim");

    assert_eq!(
        vault.read("notes/clipped.md").unwrap().path,
        "notes/clipped.md"
    );
    let hits = vault.search("hydroponics", 10).unwrap();
    assert!(
        hits.iter().any(|h| h.path == "notes/clipped.md"),
        "the import projected into FTS: {hits:?}"
    );
}

#[test]
fn an_imported_note_authors_body_links_into_the_graph() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let doc = format!("---\ncreated: 2026-01-02\n---\n\nSee [[{MEMORY_PATH}]].\n");
    vault
        .import_file("notes", "arrival.md", doc.as_bytes())
        .unwrap();

    let neighbors = vault.neighbors("notes/arrival.md").unwrap();
    assert!(
        neighbors.iter().any(|n| n.path == MEMORY_PATH),
        "{neighbors:?}"
    );
}

#[test]
fn the_vault_root_is_a_destination_and_a_nested_folder_is_created() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    assert_eq!(
        vault.import_file("", "top.png", PNG_BYTES).unwrap().path,
        "top.png"
    );
    assert!(root.join("top.png").is_file());

    // A missing folder is created, as `add` does.
    assert_eq!(
        vault
            .import_file("archive/2026", "old.png", PNG_BYTES)
            .unwrap()
            .path,
        "archive/2026/old.png"
    );
    assert!(root.join("archive/2026/old.png").is_file());
}

#[test]
fn an_occupied_destination_is_refused_rather_than_clobbered() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let before = fs::read_to_string(root.join(MEMORY_PATH)).unwrap();
    let err = vault
        .import_file("concepts", "memory.md", b"replacement")
        .unwrap_err();

    assert!(
        matches!(err, Error::ImportTargetExists(ref p) if p == MEMORY_PATH),
        "{err:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join(MEMORY_PATH)).unwrap(),
        before,
        "the vault never overwrites (data-model.md §1)"
    );
}

#[test]
fn a_name_that_is_really_a_path_cannot_redirect_the_import() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    // Dropped on `notes/`, so nothing may land outside it, whatever the name says.
    for name in ["../escaped.png", "sub/nested.png", ".hidden.png"] {
        let err = vault.import_file("notes", name, PNG_BYTES).unwrap_err();
        assert!(
            matches!(err, Error::ImportDestination(_)),
            "{name}: {err:?}"
        );
    }
    assert!(!root.join("escaped.png").exists());
    assert!(!tmp.path().join("escaped.png").exists());
    assert!(!root.join("notes/sub").exists());
}

/// Since GH #170 a copy of a note is simply a second note: two paths, two identities.
#[test]
fn an_arriving_copy_of_a_note_is_just_a_second_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let original = fs::read_to_string(root.join(MEMORY_PATH)).unwrap();
    let report = vault
        .import_file("notes", "memory-copy.md", original.as_bytes())
        .unwrap();
    assert_eq!(report.path, "notes/memory-copy.md");

    // The copy takes nothing from the original.
    assert_eq!(
        fs::read_to_string(root.join("notes/memory-copy.md")).unwrap(),
        original,
        "the copy is byte-identical to what was dropped"
    );
    assert!(
        vault.read(MEMORY_PATH).is_ok(),
        "the original still resolves"
    );
    assert!(
        !vault.neighbors(MEMORY_PATH).unwrap().is_empty(),
        "the original keeps its backlinks"
    );
    let listed = vault.list_notes().unwrap();
    assert!(listed.iter().any(|n| n.path == MEMORY_PATH));
    assert!(listed.iter().any(|n| n.path == "notes/memory-copy.md"));
}

#[test]
fn import_path_copies_the_picked_file_keeping_its_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let outside = tmp.path().join("Downloads");
    fs::create_dir_all(&outside).unwrap();
    let source = outside.join("paper.pdf");
    fs::write(&source, PNG_BYTES).unwrap();

    let report = vault.import_path("resources", &source).unwrap();

    assert_eq!(report.path, "resources/paper.pdf");
    assert!(!report.note, "a PDF is routed as a resource");
    assert_eq!(
        fs::read(root.join("resources/paper.pdf")).unwrap(),
        PNG_BYTES
    );
    assert!(source.is_file(), "the source is copied, never moved");
    assert!(vault
        .list_resources()
        .unwrap()
        .iter()
        .any(|r| r.path == "resources/paper.pdf"));
}

#[test]
fn import_path_refuses_an_occupied_destination_rather_than_truncating_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let source = tmp.path().join("memory.md");
    fs::write(&source, "a different note entirely\n").unwrap();
    let before = fs::read_to_string(root.join(MEMORY_PATH)).unwrap();

    let err = vault.import_path("concepts", &source).unwrap_err();

    assert!(matches!(err, Error::ImportTargetExists(_)), "{err:?}");
    // A create-new open reserves the destination; `fs::copy` would have truncated it.
    assert_eq!(fs::read_to_string(root.join(MEMORY_PATH)).unwrap(), before);
}

#[test]
fn import_path_refuses_a_folder() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let source = tmp.path().join("a-folder");
    fs::create_dir_all(&source).unwrap();

    let err = vault.import_path("notes", &source).unwrap_err();
    assert!(matches!(err, Error::ImportDestination(_)), "{err:?}");
}

#[test]
fn importing_into_a_never_reindexed_vault_needs_no_model_and_no_index_first() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Import is model-free, so it works before any embedding space exists.
    let (vault, root) = opened_vault(tmp.path());

    let report = vault
        .import_file(
            "notes",
            "first.md",
            b"---\ncreated: 2026-01-02\n---\n\nHi.\n",
        )
        .unwrap();

    assert!(report.note);
    assert!(root.join("notes/first.md").is_file());
    let conn = index_conn(&root);
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='embeddings'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "no embedding space is created by an import (M4)"
    );
}

#[test]
fn a_reindex_after_an_import_changes_nothing_it_did() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    vault
        .import_file("resources", "schematic.png", PNG_BYTES)
        .unwrap();
    let doc = "---\ncreated: 2026-01-02\n---\n\nArrived.\n";
    let imported = vault
        .import_file("notes", "arrival.md", doc.as_bytes())
        .unwrap();
    let after_import = fs::read_to_string(root.join("notes/arrival.md")).unwrap();

    // S2/S3: an import is ordinary vault material the moment it lands.
    vault.reindex().unwrap();

    assert_eq!(vault.read("notes/arrival.md").unwrap().path, imported.path);
    assert_eq!(
        fs::read_to_string(root.join("notes/arrival.md")).unwrap(),
        after_import,
        "the reindex writes nothing back to the imported file"
    );
    let conn = index_conn(&root);
    assert_eq!(count(&conn, "notes"), 3);
    assert_eq!(
        count(&conn, "resources"),
        5,
        "the golden vault's four, plus the import"
    );
}
