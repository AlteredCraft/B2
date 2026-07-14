//! `b2 explain` — a note's connections with their "why". Driven through the
//! [`Vault`] façade against the golden vault, deterministic under the FakeEmbedder.
//! `explain` is a pure graph read; these pin the header + the per-edge shape
//! (label, target, explanation, origin) it presents, and the orphan case.

mod common;

use b2_core::vault::Vault;
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use std::fs;
use std::path::{Path, PathBuf};

fn reindexed(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

#[test]
fn explain_shows_the_header_and_outbound_edges_with_their_why() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    let view = vault.explain("notes/spaced-repetition").unwrap();
    // Header: the note resolved to its identity + display fields.
    assert_eq!(view.b2id, SRS_ID);
    assert_eq!(view.path, "notes/spaced-repetition.md");
    assert_eq!(view.title.as_deref(), Some("spaced-repetition"));

    // Two outbound edges to memory — a typed `elaborates` (with a "why") and a bare
    // `references` (none). Both are body-authored, so origin=inline.
    assert_eq!(view.connections.len(), 2, "{:?}", view.connections);
    assert!(view.connections.iter().all(|c| c.direction == "outbound"));
    assert!(view.connections.iter().all(|c| c.b2id == MEMORY_ID));
    assert!(view.connections.iter().all(|c| c.origin == "inline"));

    let elaborates = view
        .connections
        .iter()
        .find(|c| c.label == "elaborates")
        .expect("an elaborates edge");
    assert!(
        elaborates
            .explanation
            .as_deref()
            .is_some_and(|w| w.contains("forgetting curve")),
        "the typed edge carries its why: {elaborates:?}"
    );
    assert!(
        view.connections.iter().any(|c| c.label == "references"),
        "the bare body link is a references edge"
    );
}

#[test]
fn explain_shows_inbound_backlinks_with_inverse_labels() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    // Memory is only pointed *at* (by SRS) — inbound edges, inverse-labelled.
    let view = vault.explain(MEMORY_ID).unwrap();
    assert_eq!(view.title.as_deref(), Some("memory"));
    assert!(!view.connections.is_empty());
    assert!(view.connections.iter().all(|c| c.direction == "inbound"));
    assert!(view.connections.iter().all(|c| c.b2id == SRS_ID));

    let elaborated_by = view
        .connections
        .iter()
        .find(|c| c.label == "elaborated-by")
        .expect("the inverse label of elaborates");
    assert!(
        elaborated_by
            .explanation
            .as_deref()
            .is_some_and(|w| w.contains("forgetting curve")),
        "inbound edges keep the edge's why: {elaborated_by:?}"
    );
}

#[test]
fn explain_resolves_by_path_and_by_b2id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());

    let by_path = vault.explain("concepts/memory").unwrap();
    let by_id = vault.explain(MEMORY_ID).unwrap();
    assert_eq!(by_path.b2id, by_id.b2id);
    assert_eq!(by_path.connections.len(), by_id.connections.len());
}

#[test]
fn explain_surfaces_frontmatter_provenance() {
    // An edge accepted into (or authored in) frontmatter reads as origin=frontmatter,
    // distinct from a human body link — the provenance data-model §0 says explain shows.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    fs::write(
        root.join("author.md"),
        "---\nb2id: 01JAUTH000000000000000001\ntype: note\ntitle: Author\n\
         relations:\n  - \"elaborates [[concepts/memory|Human memory]] — via frontmatter\"\n---\n\
         A body with no links.\n",
    )
    .unwrap();
    vault.reindex().unwrap();

    let view = vault.explain("author").unwrap();
    let edge = view
        .connections
        .iter()
        .find(|c| c.b2id == MEMORY_ID)
        .expect("the frontmatter relation edge");
    assert_eq!(edge.origin, "frontmatter");
    assert_eq!(edge.label, "elaborates");
}

#[test]
fn explain_reports_an_isolated_note_with_no_connections() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed(tmp.path());
    fs::write(
        root.join("lonely.md"),
        "---\nb2id: 01JLONELY0000000000000001\ntype: note\ntitle: Lonely\n---\nNo links at all.\n",
    )
    .unwrap();
    vault.reindex().unwrap();

    let view = vault.explain("lonely").unwrap();
    assert_eq!(view.title.as_deref(), Some("lonely"));
    assert!(
        view.connections.is_empty(),
        "an isolated note has no connections: {:?}",
        view.connections
    );
}

#[test]
fn explain_unknown_note_is_note_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed(tmp.path());
    assert!(matches!(
        vault.explain("does/not/exist").unwrap_err(),
        Error::NoteNotFound(r) if r == "does/not/exist"
    ));
}
