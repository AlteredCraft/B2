//! `Vault::list_notes` — the vault listing the desktop UI's file tree is built from.
//! Its contract: every indexed note as a lightweight `NoteSummary` (`b2id`, `path`,
//! `title`; no body), ordered by `path`, and each entry `read`-resolvable. A pure
//! read, model-free (FakeEmbedder), against the golden-vault fixture.

mod common;

use common::{reindexed_vault, MEMORY_PATH, SRS_PATH};

#[test]
fn list_notes_returns_every_note_ordered_by_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let notes = vault.list_notes().unwrap();

    // The whole vault, in path order (concepts/… before notes/…).
    let paths: Vec<&str> = notes.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["concepts/memory.md", "notes/spaced-repetition.md"]
    );

    // Identity + display title come through; the title is the filename (data-model.md
    // §1), so `concepts/memory.md` lists as "memory". No body field to carry.
    assert_eq!(notes[0].path, MEMORY_PATH);
    assert_eq!(notes[0].title.as_deref(), Some("memory"));
    assert_eq!(notes[1].path, SRS_PATH);
}

#[test]
fn every_listed_note_is_readable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // The tree only shows what the index knows, so a click on any entry always opens.
    for summary in vault.list_notes().unwrap() {
        let note = vault.read(&summary.path).unwrap();
        assert_eq!(note.path, summary.path);
    }
}
