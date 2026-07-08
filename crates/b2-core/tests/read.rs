//! `Vault::read` — the one façade op the Desktop UI MVP adds
//! (planning/specs/completed/desktop-ui-mvp.md §4). Its contract: resolve a note by path
//! **or** `b2id`, return the note's raw Markdown body **from disk** (source of
//! truth, frontmatter stripped) plus the display metadata. A pure read, model-free
//! (FakeEmbedder), against the golden-vault fixture.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use std::path::Path;

/// A reindexed golden vault under a temp dir; returns the open vault.
fn reindexed(dir: &Path) -> Vault {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    vault
}

#[test]
fn read_returns_body_and_metadata_with_frontmatter_stripped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = reindexed(tmp.path());

    let note = vault.read("concepts/memory.md").unwrap();

    // identity + display metadata come from the frontmatter fields.
    assert_eq!(note.b2id, MEMORY_ID);
    assert_eq!(note.path, "concepts/memory.md");
    assert_eq!(note.title.as_deref(), Some("Human memory"));
    assert_eq!(note.r#type.as_deref(), Some("concept"));
    assert_eq!(note.created.as_deref(), Some("2026-06-20"));

    // the body is the Markdown *after* the frontmatter — the raw source, not a
    // projection. It must not carry any frontmatter (no fence, no b2id line).
    assert!(note.body.contains("The brain encodes"));
    assert!(
        !note.body.contains("---"),
        "frontmatter fence must be stripped"
    );
    assert!(!note.body.contains("b2id:"), "frontmatter must be stripped");
    assert!(
        !note.body.contains("title:"),
        "frontmatter must be stripped"
    );
}

#[test]
fn read_returns_the_raw_frontmatter_block_verbatim() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = reindexed(tmp.path());

    let note = vault.read("concepts/memory.md").unwrap();
    let fm = note.frontmatter.expect("golden note has frontmatter");

    // The verbatim YAML between the fences — the source keys, not a re-serialization.
    // Byte-honest: the title keeps its source quotes here (`"Human memory"`), whereas
    // the projected `title` field is the parsed, unquoted value.
    assert!(fm.contains(r#"title: "Human memory""#));
    assert_eq!(note.title.as_deref(), Some("Human memory"));
    assert!(fm.contains(&format!("b2id: {MEMORY_ID}")));
    assert!(fm.contains("type: concept"));
    assert!(!fm.contains("---"), "fences are excluded from the block");
    // …and it is genuinely separate from the body (frontmatter isn't duplicated there).
    assert!(!note.body.contains("title:"));
}

#[test]
fn read_body_is_verbatim_markdown_including_wikilinks() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = reindexed(tmp.path());

    // The body is byte-honest Markdown: wikilinks and headings survive verbatim so
    // the adapter renders them (clickable wikilinks are the MVP's navigation).
    let note = vault.read("notes/spaced-repetition").unwrap();
    assert!(note.body.contains("[[concepts/memory|Human memory]]"));
    assert!(note.body.contains("## Relations"));
    assert!(note
        .body
        .contains("elaborates [[concepts/memory|Human memory]]"));
}

#[test]
fn read_resolves_path_stem_and_b2id_to_the_same_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = reindexed(tmp.path());

    let by_path = vault.read("notes/spaced-repetition.md").unwrap();
    let by_stem = vault.read("notes/spaced-repetition").unwrap();
    let by_id = vault.read(SRS_ID).unwrap();

    assert_eq!(by_path, by_stem);
    assert_eq!(by_path, by_id);
    assert_eq!(by_id.b2id, SRS_ID);
}

#[test]
fn read_surfaces_tags_from_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    // a note with tags + a body, to exercise the metadata that the golden notes lack.
    std::fs::write(
        root.join("tagged.md"),
        "---\ntype: note\ntitle: Tagged\ntags: [alpha, beta]\n---\nHello body.\n",
    )
    .unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();

    let note = vault.read("tagged").unwrap();
    assert_eq!(note.tags, vec!["alpha".to_string(), "beta".to_string()]);
    assert_eq!(note.title.as_deref(), Some("Tagged"));
    assert_eq!(note.body.trim(), "Hello body.");
}

#[test]
fn read_unknown_ref_is_note_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = reindexed(tmp.path());

    let err = vault.read("does/not/exist").unwrap_err();
    assert!(matches!(err, Error::NoteNotFound(r) if r == "does/not/exist"));
}
