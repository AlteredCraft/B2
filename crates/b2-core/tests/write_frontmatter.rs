//! `Vault::write_frontmatter` — the drawer's write op, `Vault::write`'s
//! frontmatter sibling (GH #79). The invariants under test: the body (bytes AND
//! boundary) is invariant under a frontmatter save; the `b2id` identity guard
//! refuses a changed/removed/duplicated id before any byte reaches disk; the
//! revision guard mirrors `write`'s; malformed-but-human YAML saves fine
//! (warn-don't-block, surfaced via `NoteView::frontmatter_readable`); and the
//! saved block re-projects edges/tags without touching chunks or vectors.

mod common;

use b2_core::vault::Vault;
use b2_core::{open, Error};
use common::golden_vault_copy;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

const SRS_PATH: &str = "notes/spaced-repetition.md";
const SRS_ID: &str = "01JSRS0000000000000000000B";

/// A reindexed (projected + fake-embedded) golden vault under a temp dir.
fn reindexed(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

/// Open a second connection onto a vault's index for direct assertions.
fn index_conn(root: &Path) -> Connection {
    open(&root.join(".b2").join("b2.sqlite")).unwrap()
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn saves_the_block_verbatim_and_leaves_the_body_untouched() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();
    let body_before = note.body.clone();

    let new_fm = format!("b2id: {SRS_ID}\ntags: [learning, memory]\nmy_key: kept verbatim\n");
    let report = vault
        .write_frontmatter(SRS_PATH, &new_fm, &note.revision)
        .unwrap();
    assert_eq!(report.path, SRS_PATH);

    // On disk: the new block between untouched fences, the body byte-identical.
    let after = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    assert_eq!(after, format!("---\n{new_fm}---\n{body_before}"));

    // A fresh read round-trips the block, the body, and the returned revision.
    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.frontmatter.as_deref(), Some(new_fm.as_str()));
    assert_eq!(reread.body, body_before);
    assert_eq!(reread.revision, report.revision);
    // The projection followed: the new tags landed, the note re-reads clean.
    assert_eq!(reread.tags, vec!["learning", "memory"]);
    assert!(reread.frontmatter_readable);
}

#[test]
fn refuses_a_changed_removed_or_duplicated_b2id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    let note = vault.read(SRS_PATH).unwrap();
    let on_disk_before = fs::read_to_string(root.join(SRS_PATH)).unwrap();

    // Removed, changed, blanked, and duplicated all refuse — identity is the one
    // line B2 protects (L1) — and none of them touch the file.
    for bad in [
        "title: no id at all\n".to_string(),
        "b2id: 01JDIFFERENT000000000000AA\n".to_string(),
        "b2id:\n".to_string(),
        format!("b2id: {SRS_ID}\nb2id: {SRS_ID}\n"),
    ] {
        let err = vault
            .write_frontmatter(SRS_PATH, &bad, &note.revision)
            .unwrap_err();
        assert!(
            matches!(&err, Error::FrontmatterIdentity(p) if p == SRS_PATH),
            "expected identity refusal for {bad:?}, got {err:?}"
        );
    }
    assert_eq!(
        fs::read_to_string(root.join(SRS_PATH)).unwrap(),
        on_disk_before,
        "a refused save must not touch the file"
    );

    // Re-quoting the same id is identity-preserving, not an edit of it.
    vault
        .write_frontmatter(SRS_PATH, &format!("b2id: \"{SRS_ID}\"\n"), &note.revision)
        .unwrap();
}

#[test]
fn refuses_a_fence_line_that_would_leak_into_the_body() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    let note = vault.read(SRS_PATH).unwrap();
    let on_disk_before = fs::read_to_string(root.join(SRS_PATH)).unwrap();

    // A `---` line would close the block early and shift the rest into the body —
    // refused, because the body is not this op's to change.
    let err = vault
        .write_frontmatter(
            SRS_PATH,
            &format!("b2id: {SRS_ID}\n---\nleaked into the body\n"),
            &note.revision,
        )
        .unwrap_err();
    assert!(matches!(err, Error::Frontmatter(_)));
    assert_eq!(
        fs::read_to_string(root.join(SRS_PATH)).unwrap(),
        on_disk_before
    );
}

#[test]
fn conflicts_when_the_file_changed_on_disk() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    let note = vault.read(SRS_PATH).unwrap();

    // An external editor changes the file after our read…
    let abs = root.join(SRS_PATH);
    let external = format!(
        "{}\nAn external append.\n",
        fs::read_to_string(&abs).unwrap()
    );
    fs::write(&abs, &external).unwrap();

    // …so a save based on the stale revision is refused, and nothing is written.
    let err = vault
        .write_frontmatter(SRS_PATH, &format!("b2id: {SRS_ID}\n"), &note.revision)
        .unwrap_err();
    assert!(matches!(err, Error::WriteConflict(p) if p == SRS_PATH));
    assert_eq!(fs::read_to_string(&abs).unwrap(), external);

    // The "Keep mine" path: a fresh read (current revision) + write succeeds.
    let fresh = vault.read(SRS_PATH).unwrap();
    vault
        .write_frontmatter(SRS_PATH, &format!("b2id: {SRS_ID}\n"), &fresh.revision)
        .unwrap();
}

#[test]
fn malformed_yaml_saves_and_surfaces_as_unreadable_not_an_error() {
    // Warn, don't block (W4/W5): broken YAML in the human's keys is the human's to
    // fix — the same edit made in vim would land on disk too. B2 keeps identity
    // (the b2id line still raw-scans, #75), keeps the bytes verbatim, and flags
    // the block unreadable on every subsequent read.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());
    let note = vault.read(SRS_PATH).unwrap();
    assert!(note.frontmatter_readable, "golden note starts clean");

    let broken = format!("b2id: {SRS_ID}\ntitle: \"unclosed\ntags: [a\n");
    vault
        .write_frontmatter(SRS_PATH, &broken, &note.revision)
        .unwrap();

    let reread = vault.read(SRS_PATH).unwrap();
    assert!(!reread.frontmatter_readable, "the warning flag is up");
    assert_eq!(reread.frontmatter.as_deref(), Some(broken.as_str()));
    assert_eq!(reread.b2id, SRS_ID, "identity survives via the raw scan");
    assert!(reread.tags.is_empty(), "unreadable YAML projects no fields");

    // And the fix heals it through the same op: readable again, fields back.
    let fixed = format!("b2id: {SRS_ID}\ntags: [a]\n");
    vault
        .write_frontmatter(SRS_PATH, &fixed, &reread.revision)
        .unwrap();
    let healed = vault.read(SRS_PATH).unwrap();
    assert!(healed.frontmatter_readable);
    assert_eq!(healed.tags, vec!["a"]);
}

#[test]
fn reprojects_edges_from_the_new_block_without_touching_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    let conn = index_conn(&root);
    let embeddings_before = count(&conn, "embeddings");
    assert_eq!(embeddings_before, count(&conn, "chunks"));

    // Retype the golden `supports` relation to `contradicts` by editing the block —
    // the hand-authoring path the drawer makes in-app (legitimate per GH #79).
    let note = vault.read(SRS_PATH).unwrap();
    let new_fm = format!(
        "b2id: {SRS_ID}\nb2_relations:\n  - \"contradicts [[concepts/memory]] — retyped by hand\"\n"
    );
    vault
        .write_frontmatter(SRS_PATH, &new_fm, &note.revision)
        .unwrap();

    // The typed edge re-derived from the new block…
    let types: Vec<String> = {
        let mut s = conn
            .prepare(
                "SELECT e.type FROM edges e JOIN notes n ON n.b2id = e.src_id
                 WHERE n.path = ?1 AND e.origin = 'frontmatter' ORDER BY e.type",
            )
            .unwrap();
        s.query_map([SRS_PATH], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(types, vec!["contradicts".to_string()]);

    // …and the unchanged body kept every chunk vector: a frontmatter save never
    // re-embeds (the re-chunk keys on the body hash).
    assert_eq!(count(&conn, "embeddings"), embeddings_before);
    assert!(db_pending_is_empty(&conn));
}

fn db_pending_is_empty(conn: &Connection) -> bool {
    b2_core::db::chunks_missing_vectors(conn)
        .unwrap()
        .is_empty()
}

#[test]
fn needs_no_embedding_space() {
    // A projected-only vault (no vector tables, no model anywhere): the drawer
    // save works — the same model-free posture as `Vault::write`.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.project(false).unwrap();

    let note = vault.read(SRS_PATH).unwrap();
    vault
        .write_frontmatter(
            SRS_PATH,
            &format!("b2id: {SRS_ID}\ntags: [modelfree]\n"),
            &note.revision,
        )
        .unwrap();

    let conn = index_conn(&root);
    assert!(
        !b2_core::db::embedding_space_exists(&conn).unwrap(),
        "a frontmatter save must not create the embedding space"
    );
    assert_eq!(vault.read(SRS_PATH).unwrap().tags, vec!["modelfree"]);
}

#[test]
fn sequential_saves_chain_revisions_and_mix_with_body_saves() {
    // One whole-file revision guards both write sites: a frontmatter save chains
    // off a body save's revision and vice versa, never self-conflicting.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();
    let fm1 = vault
        .write_frontmatter(SRS_PATH, &format!("b2id: {SRS_ID}\n"), &note.revision)
        .unwrap();
    let body = vault.write(SRS_PATH, "New body.\n", &fm1.revision).unwrap();
    let fm2 = vault
        .write_frontmatter(
            SRS_PATH,
            &format!("b2id: {SRS_ID}\ntags: [x]\n"),
            &body.revision,
        )
        .unwrap();
    assert_ne!(fm1.revision, fm2.revision);

    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.body, "New body.\n");
    assert_eq!(reread.tags, vec!["x"]);
    assert_eq!(reread.revision, fm2.revision);
}
