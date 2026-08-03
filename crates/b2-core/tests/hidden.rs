//! **Hidden means hidden** (GH #136, data-model.md §1): a dot-prefixed name is not
//! vault material of any kind. The walk applies the rule *above* its note/resource
//! routing, so a `.scratch.md` is as invisible as a `.DS_Store` — and no authoring
//! command will create one, since a member b2 never sees is a silent fs/index desync.
//!
//! The sibling cases live where their own surface is tested: dot-*folders* dropping
//! out of the structure listing in `dirs.rs`, dot-*resources* dropping out of the
//! inventory in `resources.rs`. This file owns the note route and the write guard.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{count, index_conn};
use std::fs;
use std::path::Path;

/// A vault whose Markdown is split across the managed subtree and three hidden
/// places: a dot-prefixed note at the root, one inside a managed folder, and an
/// ordinary-looking note inside a dot-folder. None of the three carries a `b2id`,
/// so a stamp would be visible in the bytes.
fn vault_with_hidden_markdown(root: &Path) -> Vault {
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join(".templates")).unwrap();
    fs::write(
        root.join("notes/real.md"),
        "---\nb2id: 01JREAL0000000000000000AA\ntype: note\ntitle: Real\n---\nA visible note about capybaras.\n",
    )
    .unwrap();
    fs::write(root.join(".scratch.md"), "# Scratch\ncapybaras again.\n").unwrap();
    fs::write(root.join("notes/.draft.md"), "# Draft\ncapybaras again.\n").unwrap();
    fs::write(
        root.join(".templates/daily.md"),
        "# Daily\ncapybaras again.\n",
    )
    .unwrap();
    Vault::open(root).unwrap()
}

/// The three hidden files are absent from every projection the walk feeds — and,
/// because the skip happens before routing, absent as *notes* specifically: no
/// row, no chunk, no embedding.
#[test]
fn dot_prefixed_markdown_is_not_a_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 1, "only the managed note is a note");

    let paths: Vec<String> = vault
        .list_notes()
        .unwrap()
        .into_iter()
        .map(|n| n.path)
        .collect();
    assert_eq!(paths, vec!["notes/real.md".to_string()]);

    let conn = index_conn(&root);
    assert_eq!(count(&conn, "notes"), 1);
    // Chunks are keyed by note, so a hidden file that never became a note cannot
    // have contributed any — asserting it directly keeps the claim honest if the
    // note route ever grows a second entry point.
    let chunk_notes: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT note_b2id FROM chunks")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows
    };
    assert_eq!(chunk_notes, vec!["01JREAL0000000000000000AA".to_string()]);

    // …and the term they all share finds only the managed one.
    let hits = vault.search("capybaras", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "notes/real.md");
}

/// W4 holds for a skipped file: not indexing it is not touching it. In particular
/// the one unbidden write b2 makes — stamping a missing `b2id` — must not reach a
/// hidden file, or the "outside the projection" claim would still mutate the vault.
#[test]
fn a_hidden_markdown_file_is_never_stamped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);

    let hidden = [".scratch.md", "notes/.draft.md", ".templates/daily.md"];
    let before: Vec<String> = hidden
        .iter()
        .map(|p| fs::read_to_string(root.join(p)).unwrap())
        .collect();

    let report = vault.reindex().unwrap();
    assert_eq!(report.stamped, 0, "the one managed note already has a b2id");

    for (path, was) in hidden.iter().zip(&before) {
        assert_eq!(
            &fs::read_to_string(root.join(path)).unwrap(),
            was,
            "{path} must be byte-identical — skipped, not written"
        );
    }
}

/// `plan_reindex` shares `collect_vault_files` with the real pass, so the preview
/// must agree about what is *not* a note. A dry run that promised to stamp a
/// `.scratch.md` the real run then skipped would be a lying preview.
#[test]
fn dry_run_agrees_the_hidden_files_are_not_notes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);

    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(plan.would_index, 1);
    assert_eq!(plan.would_stamp, 0);
    assert!(plan.stamp_paths.is_empty());

    let report = vault.reindex().unwrap();
    assert_eq!(plan.would_index, report.indexed);
    assert_eq!(plan.would_stamp, report.stamped);
}

/// A filename is bytes, not text: a leading dot must be read off the bytes, or a
/// name the OS accepts but UTF-8 rejects (`.draft-\xFF.md`) slips past the predicate
/// and routes to `notes`. Unix-only — this is where such a name can exist.
#[cfg(unix)]
#[test]
fn a_non_utf8_dot_prefixed_name_is_still_hidden() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);
    fs::write(
        root.join(OsStr::from_bytes(b".draft-\xFF.md")),
        "# Undecodable\ncapybaras again.\n",
    )
    .unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 1, "only the managed note is a note");
    assert!(
        report.skipped.is_empty(),
        "a hidden file is skipped by the walk, never routed and then reported \
         unreadable: {:?}",
        report.skipped
    );
    assert_eq!(vault.list_notes().unwrap().len(), 1);
}

/// The migration path a pre-#136 vault takes, and the one a rename opens: a `.md`
/// that *was* indexed under a managed name and is then hidden drops out of the
/// projection on the next pass — ghost-pruned (#31), because incremental must equal
/// a from-scratch rebuild (S3) and a rebuild would never have collected it. The file
/// keeps its bytes and its stamped `b2id` (W4), so renaming it back re-adopts the
/// same identity rather than minting a new one.
#[test]
fn a_note_renamed_into_hiding_is_pruned_and_readopted_on_the_way_back() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);
    vault.reindex().unwrap();
    assert_eq!(vault.list_notes().unwrap().len(), 1);

    fs::rename(root.join("notes/real.md"), root.join("notes/.real.md")).unwrap();
    let report = vault.reindex().unwrap();
    assert_eq!(report.notes_pruned, 1, "the hidden note is a ghost row now");
    assert!(vault.list_notes().unwrap().is_empty());

    // Pruned from the index, untouched on disk — identity included.
    let hidden = fs::read_to_string(root.join("notes/.real.md")).unwrap();
    assert!(hidden.contains("b2id: 01JREAL0000000000000000AA"));

    fs::rename(root.join("notes/.real.md"), root.join("notes/real.md")).unwrap();
    vault.reindex().unwrap();
    let notes = vault.list_notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].b2id, "01JREAL0000000000000000AA");
}

/// The write side of the same rule: b2 refuses to *create* what it would then never
/// index. Every authoring destination shares one validator, so the refusal covers a
/// note, a resource, and a folder alike — each mapped onto its own error variant.
#[test]
fn authoring_refuses_a_hidden_destination() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = vault_with_hidden_markdown(&root);
    vault.reindex().unwrap();
    fs::write(root.join("notes/pic.png"), b"\x89PNG fake bytes").unwrap();
    vault.reindex().unwrap();

    assert!(matches!(
        vault.add_note(".scratch2.md", None, None).unwrap_err(),
        Error::AddDestination(_)
    ));
    assert!(matches!(
        vault.create_note("notes/.draft2.md").unwrap_err(),
        Error::AddDestination(_)
    ));
    assert!(matches!(
        vault.move_note("notes/real.md", ".hidden.md").unwrap_err(),
        Error::MoveDestination(_)
    ));
    assert!(matches!(
        vault
            .move_note("notes/real.md", ".archive/real.md")
            .unwrap_err(),
        Error::MoveDestination(_)
    ));
    assert!(matches!(
        vault
            .move_resource("notes/pic.png", "notes/.pic.png")
            .unwrap_err(),
        Error::MoveDestination(_)
    ));
    assert!(matches!(
        vault.create_dir(".templates2").unwrap_err(),
        Error::DirDestination(_)
    ));

    // The refusals are about the *leading* dot, not any dot: an extension-looking
    // interior dot is an ordinary name and must still be creatable.
    assert!(vault.create_note("notes/v1.2.md").is_ok());
    assert!(vault.create_dir("notes/v1.2").is_ok());
}
