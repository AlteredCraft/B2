//! `b2 add` — create a new note and project it (note CRUD's *create*). Driven
//! through the [`Vault`] façade against a temp vault, deterministic under the
//! FakeEmbedder. The new note must land on disk with a valid, stamped frontmatter
//! and be immediately live in the index (graph + search), from the Markdown alone.

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

    // The `.md` suffix was appended; the path is the note's identity (L1).
    assert_eq!(report.path, "notes/widgets.md");

    // The file exists with exactly the template's frontmatter + body — and nothing
    // else: projecting it added no key of B2's (W1).
    let file = root.join("notes/widgets.md");
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("b2id"), "nothing is stamped: {text}");
    // `type:` is not seeded — the template stamps only what can't be reconstructed
    // later; ingest defaults an absent type to "note" (GH #80).
    assert!(!text.contains("type:"), "{text}");
    assert!(text.contains(r#"title: "All about widgets""#), "{text}");
    assert!(text.contains("created:"), "{text}");
    assert!(
        text.contains("Widgets are small self-contained gadgets."),
        "{text}"
    );

    // It round-trips losslessly.
    let parsed = b2_core::note::parse(&text);
    assert_eq!(parsed.as_str(), text);

    // Projected: it resolves in both authored link forms, and search finds it.
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

    // A note whose body links to an existing golden note.
    let report = vault
        .add_note(
            "notes/linker",
            Some("Linker"),
            Some("See [[concepts/memory|Human memory]] for background."),
        )
        .unwrap();

    // The outbound reference edge is live from the new note…
    let out = vault.neighbors(&report.path).unwrap();
    assert!(
        out.iter().any(|n| n.direction == "outbound"
            && n.path == MEMORY_PATH
            && n.relation == "references"),
        "add must project the new note's body links: {out:?}"
    );
    // …and shows up as an inbound backlink on the target.
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
    let (vault, root) = reindexed_vault(tmp.path());

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
fn create_note_writes_a_minimal_note_model_free() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let before = vault.embed_status().unwrap();
    let vectors_before = count(&index_conn(&root), "embeddings");

    let report = vault.create_note("inbox/idea").unwrap();
    assert_eq!(report.path, "inbox/idea.md");

    // On disk: the minimal frontmatter (no title — the display title is the
    // filename, data-model.md §1), body-less, in a freshly-created dir.
    let text = fs::read_to_string(root.join("inbox/idea.md")).unwrap();
    assert!(!text.contains("b2id"), "nothing is stamped: {text}");
    // `type:` is not seeded — ingest defaults it to "note" (GH #80).
    assert!(!text.contains("type:"), "{text}");
    assert!(text.contains("created:"), "{text}");
    assert!(!text.contains("title:"), "{text}");

    // Projected: it resolves in both authored link forms, and the tree lists it.
    assert!(vault.explain("inbox/idea").is_ok());
    assert!(vault.explain(&report.path).is_ok());
    assert!(vault
        .list_notes()
        .unwrap()
        .iter()
        .any(|n| n.path == "inbox/idea.md"));

    // Model-free: the embedding space is untouched. Measured on the vector table itself
    // rather than on the coverage fraction, which cannot see this: a body-less note has
    // no chunks, so it joins the embedded count the moment it is projected — vacuously,
    // having nothing to wait for — and would hide a vector this call had no business
    // storing. Any later embed/reindex owns vectors, never `create_note`.
    let conn = index_conn(&root);
    assert_eq!(count(&conn, "chunks WHERE note_path = 'inbox/idea.md'"), 0);
    assert_eq!(
        count(&conn, "embeddings"),
        vectors_before,
        "create_note must never embed"
    );

    // And the note is in the projection, counted as needing nothing.
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
