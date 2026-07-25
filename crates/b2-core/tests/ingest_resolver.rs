//! Step 1 — ingest into `notes`/`note_aliases` and the `b2id ⇄ path` resolver
//! (index-engine.md): ingest the golden vault and resolve `memory ⇄ path` both
//! ways, then prove `aliases:` projects into the alias table.
//!
//! *Stamping is not here.* It is `tests/stamp.rs`'s subject end to end (the exact
//! inserted bytes, the invalid-YAML case, and the reindex-settles-after-one-pass
//! loop), with `tests/props.rs` proving the surgical-insertion property over
//! generated input. This file is only about what ingest *projects*.

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::{db, open};
use common::{ingest_golden, MEMORY_ID};
use std::fs;

#[test]
fn ingests_golden_vault_and_resolves_b2id_path_both_ways() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::default());

    // resolver, both directions, for concepts/memory.md
    let b2id = db::resolve_path_to_b2id(&conn, "concepts/memory.md")
        .unwrap()
        .expect("memory note should resolve");
    assert_eq!(b2id, MEMORY_ID);
    let path = db::resolve_b2id_to_path(&conn, &b2id)
        .unwrap()
        .expect("b2id should resolve back to a path");
    assert_eq!(path, "concepts/memory.md");

    // both golden notes landed (they already carry a b2id — nothing to stamp)
    assert_eq!(common::count(&conn, "notes"), 2);
}

#[test]
fn aliases_are_projected_and_searchable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("srs.md"),
        "---\nb2id: 01JALIAS00000000000000000A\ntype: concept\ntitle: \"Spaced repetition\"\naliases: [SRS, spacing-effect]\n---\nBody.\n",
    )
    .unwrap();

    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();

    let alias_hit: String = conn
        .query_row(
            "SELECT note_b2id FROM note_aliases WHERE alias = 'SRS'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(alias_hit, "01JALIAS00000000000000000A");
}
