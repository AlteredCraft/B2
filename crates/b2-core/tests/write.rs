//! `Vault::write`: a byte-honest body splice guarded by a content-hash revision, then a
//! model-free re-projection. Frontmatter bytes survive, the revision chain never
//! self-conflicts, external writes always conflict, and an embed pass converges.

mod common;

use b2_core::db;
use b2_core::vault::Vault;
use b2_core::Error;
use common::{count, golden_vault_copy, index_conn, opened_vault, reindexed_vault};
use std::fs;
use std::ops::ControlFlow;

const SRS_PATH: &str = "notes/spaced-repetition.md";

#[test]
fn write_replaces_body_and_preserves_frontmatter_bytes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let before = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    let fm_end = before.find("\n---\n").unwrap() + "\n---\n".len();
    let note = vault.read(SRS_PATH).unwrap();

    let new_body = "A completely new body.\n\nWith [[concepts/memory]] still linked.\n";
    let report = vault.write(SRS_PATH, new_body, &note.revision).unwrap();
    assert_eq!(report.path, SRS_PATH);

    let after = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    assert_eq!(&after[..fm_end], &before[..fm_end], "frontmatter untouched");
    assert_eq!(&after[fm_end..], new_body, "body is the buffer, verbatim");

    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.body, new_body);
    assert_eq!(reread.revision, report.revision);
}

#[test]
fn write_conflicts_when_the_file_changed_on_disk() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();

    // An external editor changes the file after our read.
    let abs = root.join(SRS_PATH);
    let external = format!(
        "{}\nAn external append.\n",
        fs::read_to_string(&abs).unwrap()
    );
    fs::write(&abs, &external).unwrap();

    let err = vault
        .write(SRS_PATH, "my edit", &note.revision)
        .unwrap_err();
    assert!(matches!(err, Error::WriteConflict(p) if p == SRS_PATH));
    assert_eq!(
        fs::read_to_string(&abs).unwrap(),
        external,
        "a conflicted save must not touch the file"
    );

    // The "Keep mine" path: a fresh read, then write.
    let fresh = vault.read(SRS_PATH).unwrap();
    vault.write(SRS_PATH, "my edit", &fresh.revision).unwrap();
    assert_eq!(vault.read(SRS_PATH).unwrap().body, "my edit");
}

#[test]
fn sequential_writes_chain_revisions_without_conflict() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // §3: "last save wins, by construction".
    let note = vault.read(SRS_PATH).unwrap();
    let first = vault
        .write(SRS_PATH, "draft one\n", &note.revision)
        .unwrap();
    let second = vault
        .write(SRS_PATH, "draft two\n", &first.revision)
        .unwrap();
    assert_ne!(first.revision, second.revision);
    assert_eq!(vault.read(SRS_PATH).unwrap().body, "draft two\n");
}

#[test]
fn write_reprojects_keyword_graph_and_clears_stale_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let conn = index_conn(&root);
    assert_eq!(count(&conn, "embeddings"), count(&conn, "chunks"));

    // SRS links memory from the body and from frontmatter; keep one body link.
    let note = vault.read(SRS_PATH).unwrap();
    let new_body = "Rewritten body about zettelkasten workflows.\n\nSee [[concepts/memory]].\n";
    vault.write(SRS_PATH, new_body, &note.revision).unwrap();

    let hits = vault.search("zettelkasten", 10).unwrap();
    assert!(hits.iter().any(|h| h.path == SRS_PATH), "FTS is current");

    // A body save never edits the frontmatter home (data-model §2).
    let outbound: Vec<(String, String)> = {
        let mut s = conn
            .prepare(
                "SELECT e.type, e.origin FROM edges e JOIN notes n ON n.path = e.src_path
                 WHERE n.path = ?1 ORDER BY e.type",
            )
            .unwrap();
        s.query_map([SRS_PATH], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(
        outbound,
        vec![
            ("references".to_string(), "inline".to_string()),
            ("supports".to_string(), "frontmatter".to_string()),
        ],
        "edges re-projected from the saved body + the untouched frontmatter"
    );

    // §7 invariant 5: the new chunks await embedding, and a pass fills them exactly.
    let missing = db::chunks_missing_vectors(&conn).unwrap();
    assert!(!missing.is_empty(), "saved chunks await embedding");
    assert!(missing.iter().all(|c| c.note_path == SRS_PATH));
    let embed = vault.embed(&mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(embed.embedded, 1, "the embed pass fills the saved note");
    assert!(
        db::chunks_missing_vectors(&conn).unwrap().is_empty(),
        "every chunk now addresses a stored vector"
    );

    // The superseded vector stays, unreachable, until a whole-vault pass (GH #170), so a
    // save pays no scan on the interactive path.
    assert!(
        count(&conn, "embeddings") > count(&conn, "chunks"),
        "the pre-save text's vector is retained until the next whole-vault pass"
    );
    vault.project(false).unwrap();
    assert_eq!(
        count(&conn, "embeddings"),
        count(&conn, "chunks"),
        "which collects it"
    );
}

#[test]
fn write_needs_no_embedding_space() {
    // §7 invariant 4: saving needs no model.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());
    vault.project(false).unwrap();

    let note = vault.read(SRS_PATH).unwrap();
    vault
        .write(
            SRS_PATH,
            "Saved with no vectors in sight.\n",
            &note.revision,
        )
        .unwrap();

    let conn = index_conn(&root);
    assert!(
        !db::embedding_space_exists(&conn).unwrap(),
        "a save must not create the embedding space"
    );
    let hits = vault.search("vectors in sight", 10).unwrap();
    assert!(hits.iter().any(|h| h.path == SRS_PATH));
}

#[test]
fn write_an_empty_body_and_recover() {
    // Select-all-delete under autosave is a real input; the chain must continue after.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();
    let report = vault.write(SRS_PATH, "", &note.revision).unwrap();

    let on_disk = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    assert!(on_disk.ends_with("---\n"), "frontmatter only: {on_disk:?}");
    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.body, "");
    assert_eq!(reread.revision, report.revision);

    // Zero chunks, but still indexed.
    let conn = index_conn(&root);
    let chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks c JOIN notes n ON n.path = c.note_path WHERE n.path = ?1",
            [SRS_PATH],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(chunks, 0, "an empty body projects zero chunks");
    assert!(vault
        .list_notes()
        .unwrap()
        .iter()
        .any(|n| n.path == SRS_PATH));
    vault.search("memory", 10).unwrap();

    let next = vault
        .write(SRS_PATH, "Recovered.\n", &report.revision)
        .unwrap();
    assert_ne!(next.revision, report.revision);
    assert_eq!(vault.read(SRS_PATH).unwrap().body, "Recovered.\n");
}

#[test]
fn write_returns_the_revision_of_the_final_on_disk_bytes() {
    // §4 step 5: the returned revision hashes the final on-disk bytes, so the next save
    // never self-conflicts.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    fs::write(
        root.join("fresh.md"),
        "---\ntype: note\ntitle: Fresh\n---\nA body.\n",
    )
    .unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.project(false).unwrap();

    let note = vault.read("fresh").unwrap();
    let report = vault
        .write("fresh", "A saved body.\n", &note.revision)
        .unwrap();
    let on_disk = fs::read_to_string(root.join("fresh.md")).unwrap();
    assert!(on_disk.ends_with("A saved body.\n"));
    assert!(
        on_disk.starts_with("---\ntype: note\ntitle: Fresh\n---\n"),
        "the frontmatter is spliced around, never rewritten: {on_disk}"
    );
    assert_eq!(
        report.revision,
        blake3::hash(on_disk.as_bytes()).to_hex().to_string(),
        "the returned revision hashes the final on-disk bytes"
    );
    vault.write("fresh", "Again.\n", &report.revision).unwrap();
}
