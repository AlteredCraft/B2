//! `b2 mv` — move/rename a note and repair inbound links (user-stories.md Story 1,
//! the locked invariant "rename keeps every backlink resolving"). Driven through
//! the [`Vault`] façade against the golden vault (and a small purpose-built vault
//! for prefix-safety), fully deterministic under the FakeEmbedder.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use std::fs;
use std::path::{Path, PathBuf};

/// A reindexed golden vault under a temp dir; returns (vault, vault_root).
fn reindexed(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

/// The inbound set of a note, as sortable `(label, b2id)` pairs — the shape the
/// graph exposes and the thing a move must leave unchanged.
fn inbound(vault: &Vault, note_ref: &str) -> Vec<(String, String)> {
    let mut ns: Vec<(String, String)> = vault
        .neighbors(note_ref)
        .unwrap()
        .into_iter()
        .filter(|n| n.direction == "inbound")
        .map(|n| (n.label, n.b2id))
        .collect();
    ns.sort();
    ns
}

#[test]
fn move_rewrites_inbound_links_and_the_graph_is_unchanged() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    // The backlink set of memory, before the move (SRS supports + references it).
    let before = inbound(&vault, MEMORY_ID);
    assert_eq!(
        before,
        vec![
            ("referenced-by".to_string(), SRS_ID.to_string()),
            ("supported-by".to_string(), SRS_ID.to_string()),
        ]
    );

    let report = vault
        .move_note("concepts/memory.md", "concepts/human-memory.md")
        .unwrap();

    assert_eq!(report.from, "concepts/memory.md");
    assert_eq!(report.to, "concepts/human-memory.md");
    assert_eq!(
        report.rewrote,
        vec!["notes/spaced-repetition.md".to_string()]
    );
    assert_eq!(report.links_rewritten, 2, "the bare + the typed link");

    // The file moved on disk.
    assert!(!root.join("concepts/memory.md").exists());
    assert!(root.join("concepts/human-memory.md").exists());

    // The inbound text was rewritten to the new path; no stale link remains.
    let srs = fs::read_to_string(root.join("notes/spaced-repetition.md")).unwrap();
    assert!(srs.contains("[[concepts/human-memory|Human memory]]"));
    assert!(!srs.contains("[[concepts/memory|"));

    // The graph is identical — edges key on b2id, so the backlink set is unchanged,
    // reachable by the new path AND by the (unchanged) b2id.
    assert_eq!(inbound(&vault, MEMORY_ID), before);
    assert_eq!(inbound(&vault, "concepts/human-memory.md"), before);
    // The old path no longer resolves.
    assert!(matches!(
        vault.neighbors("concepts/memory.md").unwrap_err(),
        Error::NoteNotFound(_)
    ));
}

#[test]
fn move_changes_only_the_link_path_every_other_byte_identical() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let memory_before = fs::read_to_string(root.join("concepts/memory.md")).unwrap();
    let srs_before = fs::read_to_string(root.join("notes/spaced-repetition.md")).unwrap();

    vault
        .move_note("concepts/memory.md", "concepts/human-memory.md")
        .unwrap();

    // The moved note's content is byte-for-byte what it was (only its path changed).
    let memory_after = fs::read_to_string(root.join("concepts/human-memory.md")).unwrap();
    assert_eq!(memory_after, memory_before);

    // The inbound file differs by *exactly* the rewritten target token — nothing
    // else. (Story 1: "only their link `path` changed — every other byte identical".)
    let srs_after = fs::read_to_string(root.join("notes/spaced-repetition.md")).unwrap();
    assert_eq!(
        srs_after,
        srs_before.replace("[[concepts/memory|", "[[concepts/human-memory|")
    );
}

#[test]
fn move_leaves_unrelated_files_byte_identical() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    // A note that links to nothing relevant.
    let bystander = root.join("unrelated.md");
    fs::write(
        &bystander,
        "---\nb2id: 01JUNREL000000000000000ZZ\ntype: note\ntitle: Unrelated\n---\nNo links here.\n",
    )
    .unwrap();
    vault.reindex().unwrap();
    let before = fs::read_to_string(&bystander).unwrap();

    vault
        .move_note("concepts/memory.md", "concepts/human-memory.md")
        .unwrap();

    assert_eq!(fs::read_to_string(&bystander).unwrap(), before);
}

#[test]
fn move_without_md_suffix_appends_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let report = vault.move_note(MEMORY_ID, "concepts/human-memory").unwrap();

    assert_eq!(report.to, "concepts/human-memory.md");
    assert!(root.join("concepts/human-memory.md").exists());
}

#[test]
fn move_into_a_new_subdirectory_creates_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    vault
        .move_note("concepts/memory.md", "archive/deep/memory.md")
        .unwrap();

    assert!(root.join("archive/deep/memory.md").is_file());
    // Backlinks still resolve after crossing directories.
    assert_eq!(inbound(&vault, "archive/deep/memory").len(), 2);
}

#[test]
fn move_onto_an_existing_file_is_refused() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let err = vault
        .move_note("concepts/memory.md", "notes/spaced-repetition.md")
        .unwrap_err();
    assert!(matches!(err, Error::MoveTargetExists(p) if p == "notes/spaced-repetition.md"));
    // Nothing moved.
    assert!(root.join("concepts/memory.md").exists());
}

#[test]
fn an_invalid_destination_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    for dest in ["../escape.md", "/abs/path.md", "  "] {
        assert!(
            matches!(
                vault.move_note("concepts/memory.md", dest).unwrap_err(),
                Error::MoveDestination(_)
            ),
            "destination {dest:?} must be rejected"
        );
    }
    // Moving a note onto itself is a no-op error, not a silent clobber.
    assert!(matches!(
        vault
            .move_note("concepts/memory.md", "concepts/memory.md")
            .unwrap_err(),
        Error::MoveDestination(_)
    ));
}

#[test]
fn moving_an_unknown_note_is_note_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    let err = vault
        .move_note("does/not/exist", "wherever.md")
        .unwrap_err();
    assert!(matches!(err, Error::NoteNotFound(r) if r == "does/not/exist"));
}

/// A purpose-built vault for the dir-move suite: `docs/` holds two linked notes
/// and a resource, with inbound links from outside in both syntaxes and an
/// unindexed dotfile that must travel with the folder.
fn dir_move_vault(root: &Path) -> Vault {
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/alpha.md"),
        "---\nb2id: 01JALPHA000000000000000A\ntype: note\ntitle: Alpha\n---\n\
         Sibling: [[docs/beta|Beta]]. Image: ![p](pic.png)\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/beta.md"),
        "---\nb2id: 01JBETA0000000000000000B\ntype: note\ntitle: Beta\n---\nBody.\n",
    )
    .unwrap();
    fs::write(root.join("docs/pic.png"), b"\x89PNG fake bytes").unwrap();
    fs::write(root.join("docs/.keep"), "unindexed dotfile").unwrap();
    fs::write(
        root.join("hub.md"),
        "---\nb2id: 01JHUB00000000000000000C\ntype: note\ntitle: Hub\n---\n\
         See [[docs/alpha|Alpha]] and ![pic](docs/pic.png).\n",
    )
    .unwrap();
    let vault = Vault::open(root).unwrap();
    vault.reindex().unwrap();
    vault
}

#[test]
fn move_dir_moves_every_file_including_unindexed() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);

    let report = vault.move_dir("docs", "media").unwrap();

    assert_eq!(report.from, "docs");
    assert_eq!(report.to, "media");
    assert_eq!(report.moved_notes, 2);
    assert_eq!(report.moved_resources, 1);
    assert!(!root.join("docs").exists(), "the old folder is gone");
    for f in ["media/alpha.md", "media/beta.md", "media/pic.png"] {
        assert!(root.join(f).is_file(), "{f} must exist after the move");
    }
    assert!(
        root.join("media/.keep").is_file(),
        "unindexed files travel with the folder"
    );
}

#[test]
fn move_dir_rewrites_inbound_and_intra_folder_links_and_graph_is_unchanged() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);

    let before = inbound(&vault, "01JALPHA000000000000000A");
    let report = vault.move_dir("docs", "media").unwrap();

    // The outside file's wikilink and Markdown resource link are both rewritten.
    let hub = fs::read_to_string(root.join("hub.md")).unwrap();
    assert!(hub.contains("[[media/alpha|Alpha]]"));
    assert!(hub.contains("![pic](media/pic.png)"));

    // The vault-root wikilink BETWEEN co-moved notes is rewritten too.
    let alpha = fs::read_to_string(root.join("media/alpha.md")).unwrap();
    assert!(alpha.contains("[[media/beta|Beta]]"));

    // `rewrote` reports post-move paths; the intra-folder relative resource link
    // (`![p](pic.png)`) was a natural no-op, so alpha counts for its wikilink only.
    assert_eq!(
        report.rewrote,
        vec!["hub.md".to_string(), "media/alpha.md".to_string()]
    );
    assert_eq!(
        report.links_rewritten, 3,
        "hub's two links + alpha's sibling link"
    );

    // The graph is identical — edges key on b2id — and new paths resolve.
    assert_eq!(inbound(&vault, "01JALPHA000000000000000A"), before);
    assert_eq!(inbound(&vault, "media/alpha").len(), before.len());
    assert!(matches!(
        vault.neighbors("docs/alpha").unwrap_err(),
        Error::NoteNotFound(_)
    ));
}

#[test]
fn move_dir_keeps_relative_intra_folder_resource_links_byte_stable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);

    let beta_before = fs::read_to_string(root.join("docs/beta.md")).unwrap();
    vault.move_dir("docs", "media").unwrap();

    // beta had no links to rewrite — byte-identical at its new path.
    assert_eq!(
        fs::read_to_string(root.join("media/beta.md")).unwrap(),
        beta_before
    );
    // alpha's relative `![p](pic.png)` survives verbatim (both ends moved).
    let alpha = fs::read_to_string(root.join("media/alpha.md")).unwrap();
    assert!(alpha.contains("![p](pic.png)"));
    // And the inventory + backlinks resolved at the new resource path.
    let view = vault.explain_resource("media/pic.png").unwrap();
    let mut sources: Vec<String> = view.backlinks.into_iter().map(|b| b.path).collect();
    sources.sort();
    assert_eq!(
        sources,
        vec!["hub.md".to_string(), "media/alpha.md".to_string()]
    );
}

#[test]
fn move_dir_incremental_equals_full_rebuild() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);
    vault.move_dir("docs", "archive/media").unwrap();

    let notes_after_move = vault.list_notes().unwrap();
    let neighbors_after_move = inbound(&vault, "01JALPHA000000000000000A");
    drop(vault);

    // Drop the disposable index and rebuild from the Markdown alone.
    fs::remove_dir_all(root.join(".b2")).unwrap();
    let rebuilt = Vault::open(&root).unwrap();
    rebuilt.reindex().unwrap();

    assert_eq!(rebuilt.list_notes().unwrap(), notes_after_move);
    assert_eq!(
        inbound(&rebuilt, "01JALPHA000000000000000A"),
        neighbors_after_move
    );
}

#[test]
fn move_dir_creates_missing_destination_parents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);

    vault.move_dir("docs", "deep/nested/media").unwrap();
    assert!(root.join("deep/nested/media/alpha.md").is_file());
    assert_eq!(inbound(&vault, "deep/nested/media/alpha").len(), 1);
}

#[test]
fn move_dir_invalid_destinations_are_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);

    // Into its own subtree, onto itself, and the usual invalid shapes.
    for dest in ["docs/inner", "docs", "../out", "/abs", "  ", ".b2/x"] {
        assert!(
            matches!(
                vault.move_dir("docs", dest).unwrap_err(),
                Error::MoveDestination(_)
            ),
            "destination {dest:?} must be rejected"
        );
    }
    // A prefix-sharing sibling name is NOT "inside" the moved folder.
    vault.move_dir("docs", "docs2").unwrap();
    assert!(root.join("docs2/alpha.md").is_file());
}

#[test]
fn move_dir_onto_an_existing_entry_is_refused_but_unknown_source_is_dir_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);
    fs::create_dir_all(root.join("existing")).unwrap();

    let err = vault.move_dir("docs", "existing").unwrap_err();
    assert!(matches!(err, Error::MoveTargetExists(p) if p == "existing"));
    assert!(root.join("docs/alpha.md").is_file(), "nothing moved");

    // A file at the destination refuses the same way.
    let err = vault.move_dir("docs", "hub.md").unwrap_err();
    assert!(matches!(err, Error::MoveTargetExists(_)));

    let err = vault.move_dir("nope", "wherever").unwrap_err();
    assert!(matches!(err, Error::DirNotFound(p) if p == "nope"));
}

#[cfg(target_os = "macos")]
#[test]
fn move_dir_case_only_rename_succeeds_on_a_case_insensitive_fs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);

    // On case-sensitive APFS variants this is an ordinary rename; on the default
    // case-insensitive APFS the destination "exists" as the source itself and the
    // same-dirent carve-out must let it through. Either way it succeeds.
    let report = vault.move_dir("docs", "Docs").unwrap();
    assert_eq!(report.to, "Docs");
    assert_eq!(
        vault
            .list_notes()
            .unwrap()
            .iter()
            .filter(|n| n.path.starts_with("Docs/"))
            .count(),
        2
    );
}

#[test]
fn move_repairs_only_the_moved_target_not_prefix_siblings() {
    // A purpose-built vault where an inbound file links to BOTH the moved note and
    // a prefix-sharing sibling — the sibling link must survive untouched.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(root.join("concepts")).unwrap();
    fs::write(
        root.join("concepts/memory.md"),
        "---\nb2id: 01JMEM0000000000000000000A\ntype: concept\ntitle: Memory\n---\nBody.\n",
    )
    .unwrap();
    fs::write(
        root.join("concepts/memory-palace.md"),
        "---\nb2id: 01JMPALACE00000000000000A\ntype: concept\ntitle: Memory palace\n---\nBody.\n",
    )
    .unwrap();
    fs::write(
        root.join("hub.md"),
        "---\nb2id: 01JHUB00000000000000000A\ntype: note\ntitle: Hub\n---\n\
         See [[concepts/memory|Memory]] and [[concepts/memory-palace|Palace]].\n",
    )
    .unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();

    let report = vault
        .move_note("concepts/memory.md", "concepts/recall.md")
        .unwrap();
    assert_eq!(
        report.links_rewritten, 1,
        "only the memory link, not the palace"
    );

    let hub = fs::read_to_string(root.join("hub.md")).unwrap();
    assert!(hub.contains("[[concepts/recall|Memory]]"));
    assert!(
        hub.contains("[[concepts/memory-palace|Palace]]"),
        "the prefix-sharing sibling link is untouched"
    );
}
