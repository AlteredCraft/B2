//! Importing an outside file through the [`Vault`] façade — the kernel behind the
//! desktop's drag-a-file-from-Finder-onto-the-tree gesture (and its OS-picker twin).
//!
//! What these pin, beyond "the file arrives": an import is a **byte-honest copy**
//! (B2 authors nothing — a dropped `.md` keeps its own frontmatter, a dropped binary
//! keeps its bytes), it **projects** what it placed so the tree/search see it with no
//! reindex, it routes on the extension exactly as the vault walk does (note vs
//! resource, data-model.md §10), and it never clobbers or lands anywhere but where it
//! was aimed. The arriving *copy of a note* is the case that used to need its own
//! refusal — the copy carried the original's `b2id`, so projecting it would have
//! transferred that identity and every inbound edge (GH #81). Since GH #170 there is
//! no identity to steal: the copy is simply a second note at a second path, which is
//! what it looks like in Finder too, and that is what this file now pins.

mod common;

use b2_core::Error;
use common::{count, index_conn, opened_vault, reindexed_vault, MEMORY_PATH};
use std::fs;

/// The PNG header bytes — a binary payload that is *not* valid UTF-8, so an import
/// that quietly treated everything as text would fail this rather than pass.
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
    // Projected on the way in: the tree lists it with no reindex.
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

    // A note from somewhere else: its own frontmatter, its own keys.
    let doc = "---\ntitle: \"From elsewhere\"\ncreated: 2026-01-02\nsource: web\n---\n\nA clipped paragraph about hydroponics.\n";
    let report = vault
        .import_file("notes", "clipped.md", doc.as_bytes())
        .unwrap();

    assert_eq!(report.path, "notes/clipped.md");
    assert!(report.note, "a `.md` is routed as a note");

    // Byte-honest, and now byte-*identical*: B2 adds nothing at all (W1), because
    // the destination path it was given is already the note's identity (L1).
    let on_disk = fs::read_to_string(root.join("notes/clipped.md")).unwrap();
    assert_eq!(on_disk, doc, "the bytes are the human's, verbatim");

    // Projected: readable and keyword-searchable immediately, no reindex.
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

    // The edge is derived from the arriving Markdown like any other note's.
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

    // A folder that doesn't exist yet is created, mirroring `add`'s parent creation.
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

    // The safety property: the human dropped on `notes/`, so nothing may land
    // outside it — whatever the file claims to be called.
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

/// A copy of a note you already have is, since GH #170, simply a second note — the
/// same thing it is in Finder. The refusal this replaces (`Error::B2idCollision`)
/// existed because the copy carried the incumbent's stamped id and projecting it
/// would have moved that identity, and every inbound edge, onto the copy. With the
/// path as the identity there is nothing to steal: two paths, two notes.
#[test]
fn an_arriving_copy_of_a_note_is_just_a_second_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let original = fs::read_to_string(root.join(MEMORY_PATH)).unwrap();
    let report = vault
        .import_file("notes", "memory-copy.md", original.as_bytes())
        .unwrap();
    assert_eq!(report.path, "notes/memory-copy.md");

    // Both exist, both index, and the original keeps every inbound edge it had —
    // the copy took nothing from it.
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

    // A file outside the vault, as an OS file picker would hand it over.
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
    // The destination is reserved by a create-new open, so the occupied case never
    // reaches a write — where a plain `fs::copy` would have truncated the incumbent.
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
    // `opened_vault` — projected by nothing yet. Import is model-free (a
    // `ProjectionCtx`, like `create_note`), so it works before any embedding space
    // exists; the note's vectors fill on the next embed pass.
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

    // S2/S3: what the import projected is what a full rebuild derives — an import is
    // ordinary vault material the moment it lands, with nothing special recorded.
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
