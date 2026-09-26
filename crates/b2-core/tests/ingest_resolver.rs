//! Step 1 — ingest into `notes` and the link resolver
//! (index-engine.md): ingest the golden vault, then resolve a wikilink in both the
//! extensionless and `.md` forms.
//!
//! Since GH #170 the resolver is one-way and one-step: a note's identity **is** its
//! vault-relative path, so there is nothing to translate — `resolve_link_target`
//! only decides whether the authored target names a note, and the `+ ".md"` ladder
//! is the whole of the tolerance it offers.

mod common;

use b2_core::db;
use b2_core::embed::FakeEmbedder;
use common::{ingest_golden, MEMORY_PATH};

#[test]
fn ingests_golden_vault_and_resolves_a_link_in_both_authored_forms() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::default());

    // The resolver canonicalizes both authored forms onto the one identity: the
    // Obsidian habit writes `[[concepts/memory]]`, a Markdown link writes the
    // extension, and each must name the same note.
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

    // both golden notes landed
    assert_eq!(common::count(&conn, "notes"), 2);
}
