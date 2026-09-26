//! `b2 mv` — move/rename a note and repair inbound links (invariants.md,
//! the locked invariant "rename keeps every backlink resolving"). Driven through
//! the [`Vault`] façade against the golden vault (and a small purpose-built vault
//! for prefix-safety), fully deterministic under the FakeEmbedder.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{reindexed_vault, MEMORY_PATH, SRS_PATH};
use std::fs;
use std::path::Path;

/// The inbound set of a note, as sortable `(label, src_path)` pairs — the shape the
/// graph exposes and the thing a move must carry to the destination intact.
fn inbound(vault: &Vault, note_ref: &str) -> Vec<(String, String)> {
    let mut ns: Vec<(String, String)> = vault
        .neighbors(note_ref)
        .unwrap()
        .into_iter()
        .filter(|n| n.direction == "inbound")
        .map(|n| (n.label, n.path))
        .collect();
    ns.sort();
    ns
}

#[test]
fn move_rewrites_inbound_links_and_the_graph_is_unchanged() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    // The backlink set of memory, before the move (SRS supports + references it).
    let before = inbound(&vault, MEMORY_PATH);
    assert_eq!(
        before,
        vec![
            ("referenced-by".to_string(), SRS_PATH.to_string()),
            ("supported-by".to_string(), SRS_PATH.to_string()),
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
    assert_eq!(
        report.links_rewritten, 2,
        "the body link + the frontmatter relation's link"
    );

    // The file moved on disk.
    assert!(!root.join("concepts/memory.md").exists());
    assert!(root.join("concepts/human-memory.md").exists());

    // The inbound text was rewritten to the new path; no stale link remains.
    let srs = fs::read_to_string(root.join("notes/spaced-repetition.md")).unwrap();
    assert!(srs.contains("[[concepts/human-memory|Human memory]]"));
    assert!(!srs.contains("[[concepts/memory|"));

    // The graph arrives intact at the destination: the note's identity moved with
    // it (L1), and every backlink came along — index-side through the cascading
    // re-key, Markdown-side through the rewritten link text above.
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
    let (vault, root) = reindexed_vault(tmp.path());

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
    let (vault, root) = reindexed_vault(tmp.path());
    // A note that links to nothing relevant.
    let bystander = root.join("unrelated.md");
    fs::write(
        &bystander,
        "---\ntype: note\ntitle: Unrelated\n---\nNo links here.\n",
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
    let (vault, root) = reindexed_vault(tmp.path());

    let report = vault
        .move_note(MEMORY_PATH, "concepts/human-memory")
        .unwrap();

    assert_eq!(report.to, "concepts/human-memory.md");
    assert!(root.join("concepts/human-memory.md").exists());
}

#[test]
fn move_into_a_new_subdirectory_creates_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

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
    let (vault, root) = reindexed_vault(tmp.path());

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
    let (vault, _root) = reindexed_vault(tmp.path());

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
    let (vault, _root) = reindexed_vault(tmp.path());

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
        "---\ntype: note\ntitle: Alpha\n---\n\
         Sibling: [[docs/beta|Beta]]. Image: ![p](pic.png)\n",
    )
    .unwrap();
    fs::write(
        root.join("docs/beta.md"),
        "---\ntype: note\ntitle: Beta\n---\nBody.\n",
    )
    .unwrap();
    fs::write(root.join("docs/pic.png"), b"\x89PNG fake bytes").unwrap();
    fs::write(root.join("docs/.keep"), "unindexed dotfile").unwrap();
    fs::write(
        root.join("hub.md"),
        "---\ntype: note\ntitle: Hub\n---\n\
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
fn move_dir_moves_an_empty_folder() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);
    fs::create_dir_all(root.join("scratch")).unwrap();

    // An empty folder is a real vault member (fs-authoritative structure): the
    // move resolves against the filesystem, not the index, so the rename works
    // exactly like a full folder's — just with nothing indexed to repoint.
    let report = vault.move_dir("scratch", "archive/scratch").unwrap();
    assert_eq!(report.moved_notes, 0);
    assert_eq!(report.moved_resources, 0);
    assert_eq!(report.links_rewritten, 0);
    assert!(!root.join("scratch").exists());
    assert!(root.join("archive/scratch").is_dir());
}

#[test]
fn move_dir_rewrites_inbound_and_intra_folder_links_and_graph_is_unchanged() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    let vault = dir_move_vault(&root);

    let before = inbound(&vault, "docs/alpha.md");
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

    // The graph arrives intact at the new paths: every moved note re-keyed with
    // the folder, and the inbound link text followed.
    assert_eq!(inbound(&vault, "media/alpha.md"), before);
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
    let neighbors_after_move = inbound(&vault, "archive/media/alpha.md");
    drop(vault);

    // Drop the disposable index and rebuild from the Markdown alone.
    fs::remove_dir_all(root.join(".b2")).unwrap();
    let rebuilt = Vault::open(&root).unwrap();
    rebuilt.reindex().unwrap();

    assert_eq!(rebuilt.list_notes().unwrap(), notes_after_move);
    assert_eq!(
        inbound(&rebuilt, "archive/media/alpha.md"),
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
        "---\ntype: concept\ntitle: Memory\n---\nBody.\n",
    )
    .unwrap();
    fs::write(
        root.join("concepts/memory-palace.md"),
        "---\ntype: concept\ntitle: Memory palace\n---\nBody.\n",
    )
    .unwrap();
    fs::write(
        root.join("hub.md"),
        "---\ntype: note\ntitle: Hub\n---\n\
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

/// **A move re-embeds nothing** — the content-addressed vector store paying for the
/// pivot (M4, GH #170). Identity is the path, so moving a note changes it; what
/// makes that cheap is that vectors are keyed by chunk *text*, which a move does not
/// touch. Asserted on the vectors themselves, not on a count: the moved note's
/// passages must come back byte-identical, at the new path, with no forward pass.
///
/// The in-band half. The out-of-band half is below, and matters more: there the note
/// is projected as a delete plus a create, so "re-embeds nothing" is the *only*
/// thing keeping that path cheap.
#[test]
fn a_move_reuses_every_vector() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let conn = common::index_conn(&root);

    let before = note_vectors(&conn, MEMORY_PATH);
    assert!(!before.is_empty(), "the golden note is embedded");

    let report = vault
        .move_note(MEMORY_PATH, "archive/human-memory.md")
        .unwrap();
    assert_eq!(report.to, "archive/human-memory.md");

    assert_eq!(
        note_vectors(&conn, "archive/human-memory.md"),
        before,
        "the same chunk text addresses the same vectors at the new path"
    );
    assert!(
        note_vectors(&conn, MEMORY_PATH).is_empty(),
        "and nothing is left at the old one"
    );
}

/// The out-of-band move: a `git mv`/Finder rename, which a path-keyed index sees as
/// a delete plus a create. Two claims, and the pivot needs both:
///
///  * the inbound links **surface as dangling** rather than silently resolving or
///    vanishing (G5) — the scope decision GH #170 made, "identification, not repair";
///  * it **re-embeds nothing**, because the re-created note's chunks hash to vectors
///    already stored. That is what keeps the accepted loss to chunk/FTS/edge
///    re-projection instead of a full re-embed of the moved file.
#[test]
fn an_out_of_band_move_dangles_its_backlinks_and_re_embeds_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let conn = common::index_conn(&root);
    let before = note_vectors(&conn, MEMORY_PATH);

    // Behind B2's back — no `b2 mv`, so no link repair and no re-key.
    fs::create_dir_all(root.join("archive")).unwrap();
    fs::rename(root.join(MEMORY_PATH), root.join("archive/human-memory.md")).unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(report.notes_pruned, 1, "the old path is gone");
    assert_eq!(
        report.embedded, 0,
        "the moved note's chunk text is unchanged, so its vectors are already stored"
    );
    assert_eq!(
        note_vectors(&conn, "archive/human-memory.md"),
        before,
        "and they are the same vectors, reachable at the new path"
    );

    // The accepted loss, surfaced rather than hidden: SRS still links the old path,
    // and that link now reads as broken instead of resolving to nothing in silence.
    let dangling = vault.unresolved_links(SRS_PATH).unwrap();
    assert!(
        dangling.iter().any(|u| u.target == "concepts/memory"),
        "the inbound link surfaces as unresolved: {dangling:?}"
    );
    assert!(
        vault
            .neighbors("archive/human-memory.md")
            .unwrap()
            .is_empty(),
        "nothing silently re-points at the new path — B2 identifies, it does not repair"
    );
}

/// A note's stored chunk vectors in `seq` order, as raw blobs — the unit both move
/// tests compare, so "re-embeds nothing" is checked on the bytes rather than on a
/// count that a coincidence could satisfy.
fn note_vectors(conn: &rusqlite::Connection, note_path: &str) -> Vec<Vec<u8>> {
    let mut stmt = conn
        .prepare(
            "SELECT e.vector FROM chunks c JOIN embeddings e ON e.text_hash = c.text_hash
             WHERE c.note_path = ?1 ORDER BY c.seq",
        )
        .unwrap();
    let rows = stmt.query_map([note_path], |r| r.get(0)).unwrap();
    rows.map(Result::unwrap).collect()
}

// --- a failed move changes nothing (GH #230) ------------------------------------
//
// A move writes the vault in two kinds of step: the inbound files' link text, then the
// rename. A move that fails must leave every file byte-identical — reindexing cannot
// repair rewritten link text, because it faithfully projects whatever the Markdown now
// says. Two routes to a failure: a destination refused up front, and an I/O failure
// after the rewrites have been written, which must be rolled back.

/// A purpose-built vault under `dir/vault` from `(path, contents)` pairs, reindexed.
fn small_vault(dir: &Path, files: &[(&str, &str)]) -> (Vault, std::path::PathBuf) {
    let root = dir.join("vault");
    for (path, contents) in files {
        let abs = root.join(path);
        fs::create_dir_all(abs.parent().unwrap()).unwrap();
        fs::write(abs, contents).unwrap();
    }
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

/// Every file under `root` (outside `.b2/`) with its bytes, sorted by path — the
/// whole authored vault, so an assertion on it catches a stray write anywhere.
fn vault_bytes(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if rel == ".b2" {
                continue;
            }
            if path.is_dir() {
                out.push((format!("{rel}/"), Vec::new()));
                walk(root, &path, out);
            } else {
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn a_move_under_a_file_is_refused_and_changes_nothing() {
    // The issue's reproduction: `blocked` is a regular file, so `blocked/a.md` can
    // never exist. The move must be refused before a single inbound link is touched.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[
            ("a.md", "Target note.\n"),
            ("b.md", "See [[a]].\n"),
            ("c.md", "Also [[ a | the target ]] and [[a.md]].\n"),
            ("blocked", "a plain file\n"),
        ],
    );
    let before = vault_bytes(&root);
    let backlinks = inbound(&vault, "a.md");
    assert_eq!(
        backlinks,
        vec![
            ("referenced-by".to_string(), "b.md".to_string()),
            ("referenced-by".to_string(), "c.md".to_string()),
            ("referenced-by".to_string(), "c.md".to_string()),
        ],
        "one per authored link: c.md links twice"
    );

    let err = vault.move_note("a.md", "blocked/a.md").unwrap_err();
    assert!(matches!(err, Error::MoveDestination(_)), "{err:?}");

    assert_eq!(vault_bytes(&root), before, "every file byte-identical");
    assert_eq!(inbound(&vault, "a.md"), backlinks, "the index is untouched");
    // And the Markdown still says what it said: a rebuild resolves the same links.
    vault.reindex().unwrap();
    assert_eq!(
        inbound(&vault, "a.md"),
        backlinks,
        "the original links resolve"
    );
}

#[test]
fn a_move_deeper_under_a_file_is_refused_too() {
    // The blocking file may be any ancestor, not just the immediate parent.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[
            ("a.md", "Target note.\n"),
            ("b.md", "See [[a]].\n"),
            ("blocked", "a plain file\n"),
        ],
    );
    let before = vault_bytes(&root);

    let err = vault.move_note("a.md", "blocked/deeper/a.md").unwrap_err();
    assert!(matches!(err, Error::MoveDestination(_)), "{err:?}");
    assert_eq!(vault_bytes(&root), before);
}

#[test]
fn a_move_whose_rename_fails_restores_every_rewritten_file() {
    // A failure *after* the rewrites: the note vanished from disk (an out-of-band
    // delete the index hasn't seen), so the destination checks pass, both inbound
    // files are rewritten, and only then does the rename fail. The rewrites and the
    // destination folder the move created must all be undone.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[
            ("a.md", "Target note.\n"),
            ("b.md", "See [[a]].\n"),
            ("c.md", "Also [[ a | the target ]] and [[a.md]].\n"),
        ],
    );
    let backlinks = inbound(&vault, "a.md");
    fs::remove_file(root.join("a.md")).unwrap();
    let before = vault_bytes(&root);

    let err = vault.move_note("a.md", "archive/2026/a.md").unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err:?}");

    assert_eq!(
        vault_bytes(&root),
        before,
        "inbound files restored, no archive/ folder left behind"
    );
    assert_eq!(inbound(&vault, "a.md"), backlinks, "the index is untouched");
}

#[test]
fn a_resource_move_under_a_file_is_refused_and_changes_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[
            ("img.png", "png bytes"),
            ("n.md", "![[img.png]] and ![alt](img.png)\n"),
            ("blocked", "a plain file\n"),
        ],
    );
    let before = vault_bytes(&root);

    let err = vault
        .move_resource("img.png", "blocked/img.png")
        .unwrap_err();
    assert!(matches!(err, Error::MoveDestination(_)), "{err:?}");
    assert_eq!(vault_bytes(&root), before);
}

#[test]
fn a_resource_move_whose_rename_fails_restores_every_rewritten_note() {
    // The resource arm of the rollback: both link syntaxes are rewritten, then the
    // rename fails because the file is gone from disk.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[
            ("img.png", "png bytes"),
            ("n.md", "![[img.png|caption]] and ![alt]( img.png )\n"),
        ],
    );
    let backlinks = vault.explain_resource("img.png").unwrap().backlinks;
    assert_eq!(backlinks.len(), 2);
    fs::remove_file(root.join("img.png")).unwrap();
    let before = vault_bytes(&root);

    let err = vault.move_resource("img.png", "media/img.png").unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err:?}");

    assert_eq!(
        vault_bytes(&root),
        before,
        "n.md restored, no media/ folder"
    );
    assert_eq!(
        vault.explain_resource("img.png").unwrap().backlinks,
        backlinks,
        "the index is untouched"
    );
}

#[test]
fn move_dir_under_a_file_is_refused_and_changes_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[
            ("docs/x.md", "Inside, see [[docs/y]].\n"),
            ("docs/y.md", "Also inside.\n"),
            ("hub.md", "See [[docs/x|X]].\n"),
            ("blocked", "a plain file\n"),
        ],
    );
    let before = vault_bytes(&root);

    let err = vault.move_dir("docs", "blocked/docs").unwrap_err();
    assert!(matches!(err, Error::MoveDestination(_)), "{err:?}");
    assert_eq!(vault_bytes(&root), before);
}

#[test]
fn re_running_a_move_interrupted_before_its_rename_finishes_it() {
    // The one window no undo covers is a crash between the rewrites and the rename:
    // the inbound links already name the destination, the note is still at its source,
    // and the index hasn't heard. Simulated by writing that state by hand; re-running
    // the same move must complete it rather than refuse or double-rewrite.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[("a.md", "Target note.\n"), ("b.md", "See [[a|A]].\n")],
    );
    fs::write(root.join("b.md"), "See [[archive/a|A]].\n").unwrap();

    let report = vault.move_note("a.md", "archive/a.md").unwrap();
    assert_eq!(report.links_rewritten, 0, "the rewrite was already done");
    assert!(root.join("archive/a.md").exists() && !root.join("a.md").exists());
    assert_eq!(
        fs::read_to_string(root.join("b.md")).unwrap(),
        "See [[archive/a|A]].\n"
    );
    assert_eq!(
        inbound(&vault, "archive/a.md"),
        vec![("referenced-by".to_string(), "b.md".to_string())],
        "the link resolves at the destination"
    );
}

// --- one grammar, one pipeline -------------------------------------------------
//
// The move reads link text with ingest's own scanner and runs every kind of move through
// one pipeline, so what a move repairs is what ingest projected, and each kind leaves
// the index a rebuild would produce.

/// Every projected edge and inventory row, sorted — the index state a move must leave
/// equal to a from-scratch rebuild (S3). Edge ids are derived from the resolved
/// target, so a wrongly (un)resolved edge shows up here too.
fn projection(root: &Path) -> Vec<String> {
    let conn = common::index_conn(root);
    let mut rows: Vec<String> = Vec::new();
    let mut edges = conn
        .prepare(
            "SELECT id, src_path, dst_path, dst_resource_path, dst_path_raw, type
             FROM edges",
        )
        .unwrap();
    rows.extend(
        edges
            .query_map([], |r| {
                Ok(format!(
                    "edge {} {} {:?} {:?} {} {}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap),
    );
    let mut resources = conn
        .prepare("SELECT path, class, size, content_hash FROM resources")
        .unwrap();
    rows.extend(
        resources
            .query_map([], |r| {
                Ok(format!(
                    "resource {} {} {} {}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap),
    );
    rows.sort();
    rows
}

/// Assert the index at `root` equals what dropping it and rebuilding from the Markdown
/// alone produces.
fn assert_matches_a_rebuild(vault: Vault, root: &Path) {
    let after_move = projection(root);
    drop(vault);
    fs::remove_dir_all(root.join(".b2")).unwrap();
    Vault::open(root).unwrap().reindex().unwrap();
    assert_eq!(projection(root), after_move);
}

/// A vault linking one note and one resource from both syntaxes and both
/// conventions, with a self-link and a linker in a subfolder.
fn linked_vault(dir: &Path) -> (Vault, std::path::PathBuf) {
    small_vault(
        dir,
        &[
            ("a.md", "Me: [[a]]. Pic: ![p](img.png)\n"),
            ("img.png", "png bytes"),
            (
                "b.md",
                "See [[a|A]] and [a](a.md).\n![[img.png|cap]] and ![alt](img.png)\n",
            ),
            (
                "sub/c.md",
                "Up: ![x](../img.png) and [[img.png]] and [[a.md#Part]].\n",
            ),
        ],
    )
}

#[test]
fn a_move_rewrites_a_link_after_a_stray_open_bracket() {
    // Ingest scans per line, so `see [[a]]` is an edge even though an earlier line
    // left a `[[` open. The move used to scan the whole file, pair that stray `[[`
    // with the `]]` below, miss the link, and leave the backlink dangling.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = small_vault(
        tmp.path(),
        &[("a.md", "Target.\n"), ("b.md", "broken [[x\nsee [[a]]\n")],
    );

    let report = vault.move_note("a.md", "archive/a.md").unwrap();
    assert_eq!(report.links_rewritten, 1);
    assert_eq!(
        fs::read_to_string(root.join("b.md")).unwrap(),
        "broken [[x\nsee [[archive/a]]\n"
    );
    assert_eq!(
        inbound(&vault, "archive/a.md"),
        vec![("referenced-by".to_string(), "b.md".to_string())],
        "the backlink resolves at the destination"
    );
}

#[test]
fn a_self_linking_note_is_reported_under_its_new_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = linked_vault(tmp.path());

    let report = vault.move_note("a.md", "z/a.md").unwrap();
    assert_eq!(
        report.rewrote,
        vec![
            "b.md".to_string(),
            "sub/c.md".to_string(),
            "z/a.md".to_string()
        ],
        "every rewritten file is named where it is after the move"
    );
    assert_eq!(
        fs::read_to_string(root.join("z/a.md")).unwrap(),
        "Me: [[z/a]]. Pic: ![p](img.png)\n",
        "the self-link follows the note"
    );
}

#[test]
fn a_heading_link_keeps_its_fragment_across_a_move() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = linked_vault(tmp.path());

    vault.move_note("a.md", "z/a.md").unwrap();
    let c = fs::read_to_string(root.join("sub/c.md")).unwrap();
    assert!(c.contains("[[z/a.md#Part]]"), "{c}");
}

#[test]
fn a_note_move_leaves_the_index_a_rebuild_would_project() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = linked_vault(tmp.path());
    vault.move_note("a.md", "z/a.md").unwrap();
    assert_matches_a_rebuild(vault, &root);
}

#[test]
fn a_resource_move_leaves_the_index_a_rebuild_would_project() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = linked_vault(tmp.path());

    let report = vault.move_resource("img.png", "media/img.png").unwrap();
    assert_eq!(
        report.rewrote,
        vec![
            "a.md".to_string(),
            "b.md".to_string(),
            "sub/c.md".to_string()
        ]
    );
    assert_eq!(
        fs::read_to_string(root.join("sub/c.md")).unwrap(),
        "Up: ![x](../media/img.png) and [[media/img.png]] and [[a.md#Part]].\n",
        "each syntax keeps its own convention"
    );
    assert_matches_a_rebuild(vault, &root);
}

#[test]
fn a_folder_move_leaves_the_index_a_rebuild_would_project() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = linked_vault(tmp.path());
    vault.move_dir("sub", "deep/er").unwrap();
    assert_matches_a_rebuild(vault, &root);
}

#[test]
fn a_resource_move_to_a_note_path_is_refused_and_changes_nothing() {
    // A `.md` path names a note: the moved bytes would be indexed as one by the next
    // rebuild, so the resource would silently stop being a resource.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = linked_vault(tmp.path());
    let before = vault_bytes(&root);
    let backlinks = vault.explain_resource("img.png").unwrap().backlinks;

    for dest in ["img.md", "media/IMG.MD"] {
        let err = vault.move_resource("img.png", dest).unwrap_err();
        assert!(
            matches!(&err, Error::MoveDestination(m) if m.contains("note path")),
            "{dest}: {err:?}"
        );
    }
    assert_eq!(vault_bytes(&root), before);
    assert_eq!(
        vault.explain_resource("img.png").unwrap().backlinks,
        backlinks,
        "the index is untouched"
    );
}
