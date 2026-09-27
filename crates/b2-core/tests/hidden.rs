//! Hidden means hidden (GH #136, data-model.md §1): a dot-prefixed name is not vault
//! material, and no authoring command creates one. Dot-folders are in `dirs.rs`,
//! dot-resources in `resources.rs`; this file owns notes and the write guard.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{count, index_conn};
use std::fs;
use std::path::Path;

/// One visible note, plus hidden Markdown at the root, in a folder, and in a dot-folder.
fn vault_with_hidden_markdown(root: &Path) -> Vault {
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join(".templates")).unwrap();
    fs::write(
        root.join("notes/real.md"),
        "---\ntype: note\ntitle: Real\n---\nA visible note about capybaras.\n",
    )
    .unwrap();
    fs::write(root.join(".scratch.md"), "# Scratch\ncapybaras again.\n").unwrap();
    fs::write(root.join("notes/.draft.md"), "# Draft\ncapybaras again.\n").unwrap();
    fs::write(
        root.join(".templates/daily.md"),
        "# Daily\ncapybaras again.\n",
    )
    .unwrap();
    Vault::open(root).unwrap()
}

/// The skip happens before routing, so hidden files leave no row, chunk or embedding.
#[test]
fn dot_prefixed_markdown_is_not_a_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 1, "only the managed note is a note");

    let paths: Vec<String> = vault
        .list_notes()
        .unwrap()
        .into_iter()
        .map(|n| n.path)
        .collect();
    assert_eq!(paths, vec!["notes/real.md".to_string()]);

    let conn = index_conn(&root);
    assert_eq!(count(&conn, "notes"), 1);
    // Asserted directly in case the note route grows a second entry point.
    let chunk_notes: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT note_path FROM chunks")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows
    };
    assert_eq!(chunk_notes, vec!["notes/real.md".to_string()]);

    let hits = vault.search("capybaras", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "notes/real.md");
}

/// W4: not indexing a file is not touching it (the weaker half of W1).
#[test]
fn a_hidden_markdown_file_is_never_written_to() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);

    let hidden = [".scratch.md", "notes/.draft.md", ".templates/daily.md"];
    let before: Vec<String> = hidden
        .iter()
        .map(|p| fs::read_to_string(root.join(p)).unwrap())
        .collect();

    vault.reindex().unwrap();

    for (path, was) in hidden.iter().zip(&before) {
        assert_eq!(
            &fs::read_to_string(root.join(path)).unwrap(),
            was,
            "{path} must be byte-identical — skipped, not written"
        );
    }
}

/// The dry run must agree with the real pass about what is not a note.
#[test]
fn dry_run_agrees_the_hidden_files_are_not_notes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);

    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(plan.would_index, 1);

    let report = vault.reindex().unwrap();
    assert_eq!(plan.would_index, report.indexed);
    assert_eq!(plan.would_embed, report.embedded);
}

// Non-UTF-8 hidden names are a unit test in `src/pathspec.rs`: APFS refuses to create
// them, so there is no file to walk here.

/// A note renamed into hiding is ghost-pruned (GH #31, S3) with its bytes kept (W4), and
/// renaming it back re-adopts it at the same path, its identity (GH #170).
#[test]
fn a_note_renamed_into_hiding_is_pruned_and_readopted_on_the_way_back() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);
    vault.reindex().unwrap();
    assert_eq!(vault.list_notes().unwrap().len(), 1);

    fs::rename(root.join("notes/real.md"), root.join("notes/.real.md")).unwrap();
    let report = vault.reindex().unwrap();
    assert_eq!(report.notes_pruned, 1, "the hidden note is a ghost row now");
    assert!(vault.list_notes().unwrap().is_empty());

    let hidden = fs::read_to_string(root.join("notes/.real.md")).unwrap();
    assert_eq!(
        hidden,
        "---\ntype: note\ntitle: Real\n---\nA visible note about capybaras.\n"
    );

    fs::rename(root.join("notes/.real.md"), root.join("notes/real.md")).unwrap();
    vault.reindex().unwrap();
    let notes = vault.list_notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].path, "notes/real.md");
}

/// b2 refuses to create what it would never index: notes, resources and folders alike.
#[test]
fn authoring_refuses_a_hidden_destination() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);
    vault.reindex().unwrap();
    fs::write(root.join("notes/pic.png"), b"\x89PNG fake bytes").unwrap();
    vault.reindex().unwrap();

    assert!(matches!(
        vault.add_note(".scratch2.md", None, None).unwrap_err(),
        Error::AddDestination(_)
    ));
    assert!(matches!(
        vault.create_note("notes/.draft2.md").unwrap_err(),
        Error::AddDestination(_)
    ));
    assert!(matches!(
        vault.move_note("notes/real.md", ".hidden.md").unwrap_err(),
        Error::MoveDestination(_)
    ));
    assert!(matches!(
        vault
            .move_note("notes/real.md", ".archive/real.md")
            .unwrap_err(),
        Error::MoveDestination(_)
    ));
    assert!(matches!(
        vault
            .move_resource("notes/pic.png", "notes/.pic.png")
            .unwrap_err(),
        Error::MoveDestination(_)
    ));
    assert!(matches!(
        vault.create_dir(".templates2").unwrap_err(),
        Error::DirDestination(_)
    ));

    // Only a leading dot hides; an interior dot is ordinary.
    assert!(vault.create_note("notes/v1.2.md").is_ok());
    assert!(vault.create_dir("notes/v1.2").is_ok());
}
