//! `b2 add` — create a new note and project it (note CRUD's *create*). Driven
//! through the [`Vault`] façade against a temp vault, deterministic under the
//! FakeEmbedder. The new note must land on disk with a valid, stamped frontmatter
//! and be immediately live in the index (graph + search), from the Markdown alone.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_ID};
use std::fs;
use std::path::{Path, PathBuf};

/// A reindexed golden vault under a temp dir; returns (vault, vault_root). Gives
/// `add` real notes to link to for the edge-projection test.
fn reindexed(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

#[test]
fn add_writes_a_stamped_note_and_projects_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let report = vault
        .add_note(
            "notes/widgets",
            Some("All about widgets"),
            Some("Widgets are small self-contained gadgets."),
        )
        .unwrap();

    // The `.md` suffix was appended and the b2id is a real, non-empty id.
    assert_eq!(report.path, "notes/widgets.md");
    assert!(!report.b2id.is_empty());

    // The file exists with the expected, stamped frontmatter + body.
    let file = root.join("notes/widgets.md");
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains(&format!("b2id: {}", report.b2id)), "{text}");
    // `type:` is not seeded — the template stamps only what can't be reconstructed
    // later; ingest defaults an absent type to "note" (GH #80).
    assert!(!text.contains("type:"), "{text}");
    assert!(text.contains(r#"title: "All about widgets""#), "{text}");
    assert!(text.contains("created:"), "{text}");
    assert!(
        text.contains("Widgets are small self-contained gadgets."),
        "{text}"
    );

    // It round-trips losslessly (the stamp is the only mutation ingest made).
    let parsed = b2_core::note::parse(&text);
    assert_eq!(parsed.as_str(), text);

    // Projected: it resolves by path and by b2id, and keyword search finds it.
    assert!(vault.explain("notes/widgets").is_ok());
    assert!(vault.explain(&report.b2id).is_ok());
    let hits = vault.search("widgets", 10).unwrap();
    assert!(
        hits.iter().any(|h| h.path == "notes/widgets.md"),
        "the new note is immediately searchable: {hits:?}"
    );
}

#[test]
fn add_projects_the_edges_its_body_authors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    // A note whose body links to an existing golden note.
    let report = vault
        .add_note(
            "notes/linker",
            Some("Linker"),
            Some("See [[concepts/memory|Human memory]] for background."),
        )
        .unwrap();

    // The outbound reference edge is live from the new note…
    let out = vault.neighbors(&report.b2id).unwrap();
    assert!(
        out.iter().any(|n| n.direction == "outbound"
            && n.b2id == MEMORY_ID
            && n.relation == "references"),
        "add must project the new note's body links: {out:?}"
    );
    // …and shows up as an inbound backlink on the target.
    let inbound = vault.neighbors(MEMORY_ID).unwrap();
    assert!(
        inbound
            .iter()
            .any(|n| n.direction == "inbound" && n.b2id == report.b2id),
        "the target gains a backlink from the new note: {inbound:?}"
    );
}

#[test]
fn add_creates_missing_parent_directories() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    vault
        .add_note("deeply/nested/dir/note", None, None)
        .unwrap();
    assert!(root.join("deeply/nested/dir/note.md").is_file());
}

#[test]
fn add_works_on_a_never_reindexed_vault() {
    // No prior `reindex`: `add` shapes the index and projects the note itself.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    let vault = Vault::open(&root).unwrap();

    let report = vault
        .add_note("first", Some("First note"), Some("Body."))
        .unwrap();
    assert_eq!(report.path, "first.md");
    assert!(root.join("first.md").is_file());
    // Immediately searchable.
    let hits = vault.search("Body", 10).unwrap();
    assert!(hits.iter().any(|h| h.path == "first.md"), "{hits:?}");
}

#[test]
fn add_refuses_to_clobber_an_existing_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    // Onto an existing golden note.
    let err = vault
        .add_note("concepts/memory.md", None, None)
        .unwrap_err();
    assert!(matches!(err, Error::AddTargetExists(p) if p == "concepts/memory.md"));

    // Onto a note we just added (and its content is left intact).
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

#[test]
fn create_note_writes_a_stamped_minimal_note_model_free() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    let before = vault.embed_status().unwrap();

    let report = vault.create_note("inbox/idea").unwrap();
    assert_eq!(report.path, "inbox/idea.md");
    assert!(!report.b2id.is_empty());

    // On disk: the minimal frontmatter (no title — the display title is the
    // filename, data-model.md §1), stamped, body-less, in a freshly-created dir.
    let text = fs::read_to_string(root.join("inbox/idea.md")).unwrap();
    assert!(text.contains(&format!("b2id: {}", report.b2id)), "{text}");
    // `type:` is not seeded — ingest defaults it to "note" (GH #80).
    assert!(!text.contains("type:"), "{text}");
    assert!(text.contains("created:"), "{text}");
    assert!(!text.contains("title:"), "{text}");

    // Projected: it resolves by path and b2id, and the tree lists it.
    assert!(vault.explain("inbox/idea").is_ok());
    assert!(vault.explain(&report.b2id).is_ok());
    assert!(vault
        .list_notes()
        .unwrap()
        .iter()
        .any(|n| n.path == "inbox/idea.md"));

    // Model-free: the embedding space is untouched — coverage gains no embedded
    // note (an empty body has no chunks; a later embed/reindex owns any vectors).
    let after = vault.embed_status().unwrap();
    assert_eq!(
        after.embedded, before.embedded,
        "create_note must never embed"
    );
    assert_eq!(after.total, before.total + 1);
}

#[test]
fn create_note_refuses_clobber_and_invalid_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

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
    let (vault, _root) = reindexed(tmp.path());

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
