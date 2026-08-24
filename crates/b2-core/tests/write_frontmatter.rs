//! `Vault::write_frontmatter` — the drawer's write op, `Vault::write`'s frontmatter sibling
//! (GH #79). Under test: the body (bytes AND boundary) is invariant under a frontmatter save;
//! the one refusal — a `---` line, which would end the block early — holds before any byte
//! reaches disk; the revision guard mirrors `write`'s; malformed-but-human YAML saves fine
//! (warn-don't-block); and the saved block re-projects edges and tags without touching chunks
//! or vectors. B2 owns no line inside this block, so it is wholly the human's — which is what
//! the "every key survives whatever the human writes" test pins.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{count, golden_vault_copy, index_conn, reindexed_vault};
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

/// The other side of the removed identity guard (GH #170): B2 owns **no** line
/// inside this block, so every edit a human can express saves. The four inputs are
/// exactly the ones that used to be refused — a block with no `b2id`, a different
/// one, a blanked one, a duplicated one — and each is now just YAML, round-tripped
/// verbatim like any other unknown key (W5).
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
        // And the note is still the note: identity is the path, which no edit to
        // this block can reach.
        assert_eq!(vault.read(SRS_PATH).unwrap().path, SRS_PATH);
    }
}

#[test]
fn refuses_a_fence_line_that_would_leak_into_the_body() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let note = vault.read(SRS_PATH).unwrap();
    let on_disk_before = fs::read_to_string(root.join(SRS_PATH)).unwrap();

    // A `---` line would close the block early and shift the rest into the body —
    // refused, because the body is not this op's to change.
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

    // An external editor changes the file after our read…
    let abs = root.join(SRS_PATH);
    let external = format!(
        "{}\nAn external append.\n",
        fs::read_to_string(&abs).unwrap()
    );
    fs::write(&abs, &external).unwrap();

    // …so a save based on the stale revision is refused, and nothing is written.
    let err = vault
        .write_frontmatter(SRS_PATH, "tags: [x]\n", &note.revision)
        .unwrap_err();
    assert!(matches!(err, Error::WriteConflict(p) if p == SRS_PATH));
    assert_eq!(fs::read_to_string(&abs).unwrap(), external);

    // The "Keep mine" path: a fresh read (current revision) + write succeeds.
    let fresh = vault.read(SRS_PATH).unwrap();
    vault
        .write_frontmatter(SRS_PATH, "tags: [x]\n", &fresh.revision)
        .unwrap();
}

#[test]
fn malformed_yaml_saves_and_surfaces_as_unreadable_not_an_error() {
    // Warn, don't block (W4/W5): broken YAML in the human's keys is the human's to
    // fix — the same edit made in vim would land on disk too. B2 keeps identity
    // keeps the bytes verbatim, and flags the block unreadable on every
    // subsequent read.
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

    // And the fix heals it through the same op: readable again, fields back.
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

    // Retype the golden `supports` relation to `contradicts` by editing the block —
    // the hand-authoring path the drawer makes in-app (legitimate per GH #79).
    let note = vault.read(SRS_PATH).unwrap();
    let new_fm =
        "b2_relations:\n  - \"contradicts [[concepts/memory]] — retyped by hand\"\n".to_string();
    vault
        .write_frontmatter(SRS_PATH, &new_fm, &note.revision)
        .unwrap();

    // The typed edge re-derived from the new block…
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
    // One whole-file revision guards both write sites: a frontmatter save chains
    // off a body save's revision and vice versa, never self-conflicting.
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
