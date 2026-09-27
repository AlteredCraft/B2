//! Ingest and the link resolver (index-engine.md). Since GH #170 the path is the identity,
//! so `resolve_link_target` only decides whether a target names a note, tolerating a
//! missing `.md`.

mod common;

use b2_core::db;
use b2_core::embed::FakeEmbedder;
use common::{ingest_golden, MEMORY_PATH};

#[test]
fn ingests_golden_vault_and_resolves_a_link_in_both_authored_forms() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::default());

    // Obsidian writes `[[concepts/memory]]`, a Markdown link the extension.
    assert_eq!(
        db::resolve_link_target(&conn, "concepts/memory").unwrap(),
        Some(MEMORY_PATH.to_string()),
        "the extensionless wikilink form resolves"
    );
    assert_eq!(
        db::resolve_link_target(&conn, MEMORY_PATH).unwrap(),
        Some(MEMORY_PATH.to_string()),
        "so does the literal path"
    );
    assert_eq!(
        db::resolve_link_target(&conn, "concepts/nope").unwrap(),
        None,
        "a target naming no note is dangling, not an error (G5)"
    );

    assert_eq!(common::count(&conn, "notes"), 2);
}
