//! `b2 add` and `create_note`: the new note lands on disk and is immediately live in the
//! index (graph and search).

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{count, index_conn, reindexed_vault, MEMORY_PATH};
use std::fs;

#[test]
fn add_writes_a_minimal_note_and_projects_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let report = vault
        .add_note(
            "notes/widgets",
            Some("All about widgets"),
            Some("Widgets are small self-contained gadgets."),
        )
        .unwrap();

    // The `.md` suffix is appended; the path is the identity (L1).
    assert_eq!(report.path, "notes/widgets.md");

    // No key of B2's is added (W1).
    let file = root.join("notes/widgets.md");
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("b2id"), "nothing is stamped: {text}");
    // Ingest defaults an absent type to "note" (GH #80).
    assert!(!text.contains("type:"), "{text}");
    assert!(text.contains(r#"title: "All about widgets""#), "{text}");
    assert!(text.contains("created:"), "{text}");
    assert!(
        text.contains("Widgets are small self-contained gadgets."),
        "{text}"
    );

    let parsed = b2_core::note::parse(&text);
    assert_eq!(parsed.as_str(), text);

    // Resolves in both authored link forms.
    assert!(vault.explain("notes/widgets").is_ok());
    assert!(vault.explain(&report.path).is_ok());
    let hits = vault.search("widgets", 10).unwrap();
    assert!(
        hits.iter().any(|h| h.path == "notes/widgets.md"),
        "the new note is immediately searchable: {hits:?}"
    );
}

#[test]
fn add_projects_the_edges_its_body_authors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let report = vault
        .add_note(
            "notes/linker",
            Some("Linker"),
            Some("See [[concepts/memory|Human memory]] for background."),
        )
        .unwrap();

    let out = vault.neighbors(&report.path).unwrap();
    assert!(
        out.iter().any(|n| n.direction == "outbound"
            && n.path == MEMORY_PATH
            && n.relation == "references"),
        "add must project the new note's body links: {out:?}"
    );
    let inbound = vault.neighbors(MEMORY_PATH).unwrap();
    assert!(
        inbound
            .iter()
            .any(|n| n.direction == "inbound" && n.path == report.path),
        "the target gains a backlink from the new note: {inbound:?}"
    );
}

#[test]
fn add_creates_missing_parent_directories() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    vault
        .add_note("deeply/nested/dir/note", None, None)
        .unwrap();
    assert!(root.join("deeply/nested/dir/note.md").is_file());
}

#[test]
fn add_works_on_a_never_reindexed_vault() {
    // `add` shapes the index itself.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    let vault = Vault::open(&root).unwrap();

    let report = vault
        .add_note("first", Some("First note"), Some("Body."))
        .unwrap();
    assert_eq!(report.path, "first.md");
    assert!(root.join("first.md").is_file());
    let hits = vault.search("Body", 10).unwrap();
    assert!(hits.iter().any(|h| h.path == "first.md"), "{hits:?}");
}

#[test]
fn add_refuses_to_clobber_an_existing_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let err = vault
        .add_note("concepts/memory.md", None, None)
        .unwrap_err();
    assert!(matches!(err, Error::AddTargetExists(p) if p == "concepts/memory.md"));

    vault.add_note("notes/dup", None, Some("original")).unwrap();
    let before = fs::read_to_string(root.join("notes/dup.md")).unwrap();
    let err = vault
        .add_note("notes/dup", None, Some("overwrite"))
        .unwrap_err();
    assert!(matches!(err, Error::AddTargetExists(_)));
    assert_eq!(
        fs::read_to_string(root.join("notes/dup.md")).unwrap(),
        before,
        "a refused add never touches the existing file"
    );
}

/// The refusal is the create-new open itself, so it covers a folder at the path too.
#[test]
fn add_refuses_a_path_a_folder_occupies() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    fs::create_dir_all(root.join("notes/taken.md")).unwrap();

    let err = vault
        .add_note("notes/taken", None, Some("body"))
        .unwrap_err();
    assert!(matches!(err, Error::AddTargetExists(p) if p == "notes/taken.md"));
    assert!(
        root.join("notes/taken.md").is_dir(),
        "the folder is untouched"
    );
}

#[test]
fn create_note_writes_a_minimal_note_model_free() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let before = vault.embed_status().unwrap();
    let vectors_before = count(&index_conn(&root), "embeddings");

    let report = vault.create_note("inbox/idea").unwrap();
    assert_eq!(report.path, "inbox/idea.md");

    // No title: the display title is the filename (data-model.md §1).
    let text = fs::read_to_string(root.join("inbox/idea.md")).unwrap();
    assert!(!text.contains("b2id"), "nothing is stamped: {text}");
    // GH #80.
    assert!(!text.contains("type:"), "{text}");
    assert!(text.contains("created:"), "{text}");
    assert!(!text.contains("title:"), "{text}");

    assert!(vault.explain("inbox/idea").is_ok());
    assert!(vault.explain(&report.path).is_ok());
    assert!(vault
        .list_notes()
        .unwrap()
        .iter()
        .any(|n| n.path == "inbox/idea.md"));

    // Model-free. Measured on the vector table: a body-less note counts as embedded
    // vacuously, so the coverage fraction would hide a stray vector.
    let conn = index_conn(&root);
    assert_eq!(count(&conn, "chunks WHERE note_path = 'inbox/idea.md'"), 0);
    assert_eq!(
        count(&conn, "embeddings"),
        vectors_before,
        "create_note must never embed"
    );

    let after = vault.embed_status().unwrap();
    assert_eq!(after.total, before.total + 1);
    assert_eq!(
        after.embedded,
        before.embedded + 1,
        "a body-less note waits for no vector, so it must not sit outside the fraction"
    );
}

#[test]
fn create_note_refuses_clobber_and_invalid_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let err = vault.create_note("concepts/memory").unwrap_err();
    assert!(matches!(err, Error::AddTargetExists(p) if p == "concepts/memory.md"));
    for bad in ["../escape", "/abs/path", "  "] {
        assert!(
            matches!(
                vault.create_note(bad).unwrap_err(),
                Error::AddDestination(_)
            ),
            "path {bad:?} must be rejected"
        );
    }
}

#[test]
fn add_rejects_an_invalid_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    for bad in ["../escape.md", "/abs/path.md", "  "] {
        assert!(
            matches!(
                vault.add_note(bad, None, None).unwrap_err(),
                Error::AddDestination(_)
            ),
            "path {bad:?} must be rejected"
        );
    }
}
