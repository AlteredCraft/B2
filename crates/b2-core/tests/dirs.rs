//! Folders (`list_dirs`, `create_dir`): the filesystem is authoritative (data-model.md §1),
//! so both ops go straight to disk and an empty folder is as real as a full one.

mod common;

use b2_core::Error;
use common::opened_vault;
use std::fs;

#[test]
fn list_dirs_returns_every_folder_sorted_including_empty_ones() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());

    // Made outside B2.
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
    let (vault, root) = opened_vault(tmp.path());

    // Never reindexed. `.b2/` and `.obsidian/` are dot-folders, never vault structure.
    fs::create_dir_all(root.join(".obsidian/plugins")).unwrap();

    let dirs = vault.list_dirs().unwrap();
    assert_eq!(dirs, vec!["concepts", "notes", "resources"]);
}

#[test]
fn create_dir_makes_a_real_folder_on_disk_that_lists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());

    let report = vault.create_dir("projects").unwrap();
    assert_eq!(report.dir, "projects");
    assert!(root.join("projects").is_dir());
    assert!(vault.list_dirs().unwrap().contains(&"projects".to_string()));
}

#[test]
fn create_dir_creates_missing_parents_and_tolerates_a_trailing_slash() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());

    // Like `mkdir -p`.
    let report = vault.create_dir("projects/2026/q3/").unwrap();
    assert_eq!(report.dir, "projects/2026/q3");
    assert!(root.join("projects/2026/q3").is_dir());
}

#[test]
fn create_dir_refuses_an_existing_folder_or_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = opened_vault(tmp.path());

    // Refused, not a silent no-op: the user asked to create something.
    match vault.create_dir("concepts") {
        Err(Error::DirTargetExists(p)) => assert_eq!(p, "concepts"),
        other => panic!("expected DirTargetExists, got {other:?}"),
    }
    match vault.create_dir("concepts/memory.md") {
        Err(Error::DirTargetExists(p)) => assert_eq!(p, "concepts/memory.md"),
        other => panic!("expected DirTargetExists, got {other:?}"),
    }
}

#[test]
fn create_dir_rejects_invalid_and_hidden_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());

    for bad in ["", "  ", "/abs", "../up", "a/../../b", ".b2", "a/.git/b"] {
        assert!(
            matches!(vault.create_dir(bad), Err(Error::DirDestination(_))),
            "expected DirDestination for {bad:?}"
        );
    }
    assert!(!root.join("a").exists());
}
