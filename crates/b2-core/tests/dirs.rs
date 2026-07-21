//! Folder structure through the [`Vault`] façade — `list_dirs` + `create_dir`.
//! Folders are user-authored vault *structure*, and the filesystem is authoritative
//! for them (data-model.md §1): both ops go straight to disk, never the index, so
//! the listing can't go stale and an **empty** folder is as real as a full one —
//! the file tree must be one-to-one with the filesystem in both directions.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::golden_vault_copy;
use std::fs;
use std::path::{Path, PathBuf};

/// A golden vault copied under a temp dir (no reindex — structure reads are
/// index-free); returns (vault, vault_root).
fn opened(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    (vault, root)
}

#[test]
fn list_dirs_returns_every_folder_sorted_including_empty_ones() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened(tmp.path());

    // An empty folder (with an empty nested child) made outside B2 — Finder, mkdir.
    fs::create_dir_all(root.join("projects/2026")).unwrap();

    let dirs = vault.list_dirs().unwrap();
    assert_eq!(
        dirs,
        vec![
            "concepts",
            "notes",
            "projects",
            "projects/2026",
            "resources"
        ]
    );
}

#[test]
fn list_dirs_is_index_free_and_skips_dot_folders() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened(tmp.path());

    // Never reindexed — `.b2/` exists (Vault::open creates it) and `.obsidian/`
    // simulates a sibling tool; both are dot-folders, never vault structure.
    fs::create_dir_all(root.join(".obsidian/plugins")).unwrap();

    let dirs = vault.list_dirs().unwrap();
    assert_eq!(dirs, vec!["concepts", "notes", "resources"]);
}

#[test]
fn create_dir_makes_a_real_folder_on_disk_that_lists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened(tmp.path());

    let report = vault.create_dir("projects").unwrap();
    assert_eq!(report.dir, "projects");
    assert!(root.join("projects").is_dir());
    assert!(vault.list_dirs().unwrap().contains(&"projects".to_string()));
}

#[test]
fn create_dir_creates_missing_parents_and_tolerates_a_trailing_slash() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened(tmp.path());

    // The UI's inline input allows nesting ("projects/2026"), like `mkdir -p`.
    let report = vault.create_dir("projects/2026/q3/").unwrap();
    assert_eq!(report.dir, "projects/2026/q3");
    assert!(root.join("projects/2026/q3").is_dir());
}

#[test]
fn create_dir_refuses_an_existing_folder_or_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = opened(tmp.path());

    // An existing folder: refused, not silently a no-op — the user asked to
    // *create* something, and it's already there.
    match vault.create_dir("concepts") {
        Err(Error::DirTargetExists(p)) => assert_eq!(p, "concepts"),
        other => panic!("expected DirTargetExists, got {other:?}"),
    }
    // A file in the way: same refusal (the vault never clobbers).
    match vault.create_dir("concepts/memory.md") {
        Err(Error::DirTargetExists(p)) => assert_eq!(p, "concepts/memory.md"),
        other => panic!("expected DirTargetExists, got {other:?}"),
    }
}

#[test]
fn create_dir_rejects_invalid_and_hidden_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened(tmp.path());

    for bad in ["", "  ", "/abs", "../up", "a/../../b", ".b2", "a/.git/b"] {
        assert!(
            matches!(vault.create_dir(bad), Err(Error::DirDestination(_))),
            "expected DirDestination for {bad:?}"
        );
    }
    assert!(!root.join("a").exists());
}
