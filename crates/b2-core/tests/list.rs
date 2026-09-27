//! `Vault::list_notes`, the desktop file tree's source: every indexed note, no body,
//! ordered by path, each `read`-resolvable.

mod common;

use common::{reindexed_vault, MEMORY_PATH, SRS_PATH};

#[test]
fn list_notes_returns_every_note_ordered_by_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let notes = vault.list_notes().unwrap();

    let paths: Vec<&str> = notes.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["concepts/memory.md", "notes/spaced-repetition.md"]
    );

    // The title is the filename (data-model.md §1).
    assert_eq!(notes[0].path, MEMORY_PATH);
    assert_eq!(notes[0].title.as_deref(), Some("memory"));
    assert_eq!(notes[1].path, SRS_PATH);
}

#[test]
fn every_listed_note_is_readable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // So a click on any tree entry always opens.
    for summary in vault.list_notes().unwrap() {
        let note = vault.read(&summary.path).unwrap();
        assert_eq!(note.path, summary.path);
    }
}
