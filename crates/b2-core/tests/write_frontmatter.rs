//! `Vault::write_frontmatter`, the drawer's write op (GH #79): the body is invariant, a
//! `---` line is the one refusal, the revision guard mirrors `write`'s, malformed YAML
//! saves (warn, don't block), and edges and tags re-project without touching vectors.

mod common;

use b2_core::Error;
use common::{count, index_conn, opened_vault, reindexed_vault};
use rusqlite::Connection;
use std::fs;

const SRS_PATH: &str = "notes/spaced-repetition.md";

#[test]
fn saves_the_block_verbatim_and_leaves_the_body_untouched() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();
    let body_before = note.body.clone();

    let new_fm = "tags: [learning, memory]\nmy_key: kept verbatim\n".to_string();
    let report = vault
        .write_frontmatter(SRS_PATH, &new_fm, &note.revision)
        .unwrap();
    assert_eq!(report.path, SRS_PATH);

    let after = fs::read_to_string(root.join(SRS_PATH)).unwrap();
    assert_eq!(after, format!("---\n{new_fm}---\n{body_before}"));

    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.frontmatter.as_deref(), Some(new_fm.as_str()));
    assert_eq!(reread.body, body_before);
    assert_eq!(reread.revision, report.revision);
    assert_eq!(reread.tags, vec!["learning", "memory"]);
    assert!(reread.frontmatter_readable);
}

/// B2 owns no line in the block (GH #170), so `b2id` edits are just YAML, kept verbatim
/// like any unknown key (W5).
#[test]
fn owns_no_line_in_the_block_so_every_human_edit_saves() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    for block in [
        "title: no id at all\n".to_string(),
        "b2id: 01JDIFFERENT000000000000AA\n".to_string(),
        "b2id:\n".to_string(),
        "b2id: 01JA\nb2id: 01JB\n".to_string(),
    ] {
        let note = vault.read(SRS_PATH).unwrap();
        vault
            .write_frontmatter(SRS_PATH, &block, &note.revision)
            .expect("the block is the human's");
        let on_disk = fs::read_to_string(root.join(SRS_PATH)).unwrap();
        assert!(
            on_disk.starts_with(&format!("---\n{block}---\n")),
            "saved verbatim: {on_disk:?}"
        );
        // Identity is the path, which no edit to this block can reach.
        assert_eq!(vault.read(SRS_PATH).unwrap().path, SRS_PATH);
    }
}

#[test]
fn refuses_a_fence_line_that_would_leak_into_the_body() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let note = vault.read(SRS_PATH).unwrap();
    let on_disk_before = fs::read_to_string(root.join(SRS_PATH)).unwrap();

    // A `---` line would close the block early and shift the rest into the body.
    let err = vault
        .write_frontmatter(
            SRS_PATH,
            "tags: [x]\n---\nleaked into the body\n",
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
        .write_frontmatter(SRS_PATH, "tags: [x]\n", &note.revision)
        .unwrap_err();
    assert!(matches!(err, Error::WriteConflict(p) if p == SRS_PATH));
    assert_eq!(fs::read_to_string(&abs).unwrap(), external);

    // The "Keep mine" path: a fresh read, then write.
    let fresh = vault.read(SRS_PATH).unwrap();
    vault
        .write_frontmatter(SRS_PATH, "tags: [x]\n", &fresh.revision)
        .unwrap();
}

#[test]
fn malformed_yaml_saves_and_surfaces_as_unreadable_not_an_error() {
    // Warn, don't block (W4/W5): broken YAML is the human's to fix, as in vim. B2 keeps
    // the bytes and flags the block unreadable.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());
    let note = vault.read(SRS_PATH).unwrap();
    assert!(note.frontmatter_readable, "golden note starts clean");

    let broken = "title: \"unclosed\ntags: [a\n".to_string();
    vault
        .write_frontmatter(SRS_PATH, &broken, &note.revision)
        .unwrap();

    let reread = vault.read(SRS_PATH).unwrap();
    assert!(!reread.frontmatter_readable, "the warning flag is up");
    assert_eq!(reread.frontmatter.as_deref(), Some(broken.as_str()));
    assert_eq!(
        reread.path, SRS_PATH,
        "identity is the path — unreadable YAML cannot touch it"
    );
    assert!(reread.tags.is_empty(), "unreadable YAML projects no fields");

    let fixed = "tags: [a]\n".to_string();
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
    let (vault, root) = reindexed_vault(tmp.path());
    let conn = index_conn(&root);
    let embeddings_before = count(&conn, "embeddings");
    assert_eq!(embeddings_before, count(&conn, "chunks"));

    // Retype `supports` to `contradicts` by hand (GH #79).
    let note = vault.read(SRS_PATH).unwrap();
    let new_fm =
        "b2_relations:\n  - \"contradicts [[concepts/memory]] — retyped by hand\"\n".to_string();
    vault
        .write_frontmatter(SRS_PATH, &new_fm, &note.revision)
        .unwrap();

    let types: Vec<String> = {
        let mut s = conn
            .prepare(
                "SELECT type FROM edges
                 WHERE src_path = ?1 AND origin = 'frontmatter' ORDER BY type",
            )
            .unwrap();
        s.query_map([SRS_PATH], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(types, vec!["contradicts".to_string()]);

    // The re-chunk keys on the body hash, so nothing re-embeds.
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
    // Model-free, like `Vault::write`.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());
    vault.project(false).unwrap();

    let note = vault.read(SRS_PATH).unwrap();
    vault
        .write_frontmatter(SRS_PATH, "tags: [modelfree]\n", &note.revision)
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
    // One whole-file revision guards both write sites.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let note = vault.read(SRS_PATH).unwrap();
    let fm1 = vault
        .write_frontmatter(SRS_PATH, "tags: [x]\n", &note.revision)
        .unwrap();
    let body = vault.write(SRS_PATH, "New body.\n", &fm1.revision).unwrap();
    let fm2 = vault
        .write_frontmatter(SRS_PATH, "tags: [x]\n", &body.revision)
        .unwrap();
    assert_ne!(fm1.revision, fm2.revision);

    let reread = vault.read(SRS_PATH).unwrap();
    assert_eq!(reread.body, "New body.\n");
    assert_eq!(reread.tags, vec!["x"]);
    assert_eq!(reread.revision, fm2.revision);
}
