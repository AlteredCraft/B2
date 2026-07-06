//! `Vault::list_notes` — the vault listing the desktop UI's file tree is built from.
//! Its contract: every indexed note as a lightweight `NoteSummary` (`b2id`, `path`,
//! `title`; no body), ordered by `path`, and each entry `read`-resolvable. A pure
//! read, model-free (FakeEmbedder), against the golden-vault fixture.

mod common;

use b2_core::vault::Vault;
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
fn list_notes_returns_every_note_ordered_by_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = reindexed(tmp.path());

    let notes = vault.list_notes().unwrap();

    // The whole vault, in path order (concepts/… before notes/…).
    let paths: Vec<&str> = notes.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["concepts/memory.md", "notes/spaced-repetition.md"]
    );

    // Identity + display title come through; no body field to carry.
    assert_eq!(notes[0].b2id, MEMORY_ID);
    assert_eq!(notes[0].title.as_deref(), Some("Human memory"));
    assert_eq!(notes[1].b2id, SRS_ID);
}

#[test]
fn every_listed_note_is_readable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = reindexed(tmp.path());

    // The tree only shows what the index knows, so a click on any entry always opens.
    for summary in vault.list_notes().unwrap() {
        let note = vault.read(&summary.path).unwrap();
        assert_eq!(note.b2id, summary.b2id);
    }
}

#[test]
fn a_never_reindexed_vault_lists_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();

    // Index-first honesty: no rows before the first reindex, no error (mirrors search).
    assert!(vault.list_notes().unwrap().is_empty());
}
