//! `Vault::read`: resolve a note by path, with or without the `.md` (GH #170), and return
//! its raw body from disk, frontmatter stripped, plus display metadata.

mod common;

use b2_core::vault::Vault;
use common::{golden_vault_copy, reindexed_vault, MEMORY_PATH, SRS_PATH};

#[test]
fn read_returns_body_and_metadata_with_frontmatter_stripped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let note = vault.read("concepts/memory.md").unwrap();

    // The title is the filename (data-model.md §1); type and created are frontmatter's.
    assert_eq!(note.path, MEMORY_PATH);
    assert_eq!(note.path, "concepts/memory.md");
    assert_eq!(note.title.as_deref(), Some("memory"));
    assert_eq!(note.r#type.as_deref(), Some("concept"));
    assert_eq!(note.created.as_deref(), Some("2026-06-20"));

    // The raw source, not a projection.
    assert!(note.body.contains("The brain encodes"));
    assert!(
        !note.body.contains("---"),
        "frontmatter fence must be stripped"
    );
    assert!(
        !note.body.contains("title:"),
        "frontmatter must be stripped"
    );
}

#[test]
fn read_returns_the_raw_frontmatter_block_verbatim() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let note = vault.read("concepts/memory.md").unwrap();
    let fm = note.frontmatter.expect("golden note has frontmatter");

    // Verbatim, not re-serialized: the inert `title:` key is still there (data-model.md §1).
    assert!(fm.contains(r#"title: "Human memory""#));
    assert_eq!(note.title.as_deref(), Some("memory"));
    assert!(fm.contains("type: concept"));
    assert!(!fm.contains("---"), "fences are excluded from the block");
    assert!(!note.body.contains("title:"));
}

#[test]
fn read_body_is_verbatim_markdown_including_wikilinks() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // Wikilinks survive verbatim for the adapter; the typed relation stays in frontmatter.
    let note = vault.read("notes/spaced-repetition").unwrap();
    assert!(note.body.contains("[[concepts/memory|Human memory]]"));
    assert!(
        !note.body.contains("supports [["),
        "no typed syntax in the body"
    );
    assert!(note
        .frontmatter
        .as_deref()
        .is_some_and(|fm| fm.contains("supports [[concepts/memory|Human memory]]")));
}

#[test]
fn read_resolves_a_path_and_its_stem_to_the_same_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let by_path = vault.read("notes/spaced-repetition.md").unwrap();
    let by_stem = vault.read("notes/spaced-repetition").unwrap();

    assert_eq!(by_path, by_stem);
    assert_eq!(
        by_path.path, SRS_PATH,
        "and it resolves to the canonical path"
    );
}

#[test]
fn read_surfaces_tags_from_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    // The golden notes have no tags.
    std::fs::write(
        root.join("tagged.md"),
        "---\ntype: note\ntitle: Tagged\ntags: [alpha, beta]\n---\nHello body.\n",
    )
    .unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();

    let note = vault.read("tagged").unwrap();
    assert_eq!(note.tags, vec!["alpha".to_string(), "beta".to_string()]);
    // Title is the filename, not `title: Tagged`.
    assert_eq!(note.title.as_deref(), Some("tagged"));
    assert_eq!(note.body.trim(), "Hello body.");
}
