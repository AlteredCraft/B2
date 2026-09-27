//! `b2 explain`: a note's connections with their "why", a pure graph read.

mod common;

use common::{reindexed_vault, MEMORY_PATH, SRS_PATH};
use std::fs;

#[test]
fn explain_shows_the_header_and_outbound_edges_with_their_why() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let view = vault.explain("notes/spaced-repetition").unwrap();
    assert_eq!(view.path, SRS_PATH);
    assert_eq!(view.path, "notes/spaced-repetition.md");
    assert_eq!(view.title.as_deref(), Some("spaced-repetition"));

    // A typed `supports` with a why, and a bare body `references`: the two homes
    // (data-model §8).
    assert_eq!(view.connections.len(), 2, "{:?}", view.connections);
    assert!(view.connections.iter().all(|c| c.direction == "outbound"));
    assert!(view.connections.iter().all(|c| c.path == MEMORY_PATH));

    let supports = view
        .connections
        .iter()
        .find(|c| c.label == "supports")
        .expect("a supports edge");
    assert_eq!(supports.origin, "frontmatter", "the typed home");
    assert!(
        supports
            .explanation
            .as_deref()
            .is_some_and(|w| w.contains("forgetting curve")),
        "the typed edge carries its why: {supports:?}"
    );
    let references = view
        .connections
        .iter()
        .find(|c| c.label == "references")
        .expect("the bare body link is a references edge");
    assert_eq!(references.origin, "inline", "the body home");
    // The other note's `created` (GH #22), from the projection.
    assert!(
        view.connections
            .iter()
            .all(|c| c.created.as_deref() == Some("2026-06-20")),
        "neighbors carry their created date: {:?}",
        view.connections
    );
    assert!(
        view.unresolved.is_empty(),
        "resolved note has no unresolved links: {:?}",
        view.unresolved
    );
}

#[test]
fn explain_surfaces_outbound_resource_links() {
    // GH #22: resource links must be visible from the note's side too.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    fs::write(
        root.join("notes/uses-diagram.md"),
        "---\ntype: note\ntitle: Uses diagram\n---\n\
         ![a tiny diagram](../resources/diagram.png)\n\
         See [[concepts/memory|Human memory]].\n",
    )
    .unwrap();
    vault.reindex().unwrap();

    let view = vault.explain("notes/uses-diagram").unwrap();
    assert_eq!(view.connections.len(), 1, "{:?}", view.connections);
    assert_eq!(view.resources.len(), 1, "{:?}", view.resources);
    let r = &view.resources[0];
    assert_eq!(r.path, "resources/diagram.png");
    assert_eq!(r.class, "image");
    assert_eq!(r.relation, "references");
    assert_eq!(r.origin, "inline");
    assert_eq!(r.caption.as_deref(), Some("a tiny diagram"));
    assert!(r.embed, "an image embed reads as embed=true");
    assert!(vault
        .explain("concepts/memory")
        .unwrap()
        .resources
        .is_empty());
}

#[test]
fn explain_surfaces_unresolved_folder_and_typo_links() {
    // GH #12: a link naming a folder or a typo reads as broken, not gone.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    fs::write(
        root.join("guide.md"),
        "---\ntype: note\ntitle: Guide\n---\n\
         - [[Hermes]] is the R&D machine\n\
         See [[concepts/memory|Human memory]] for context.\n",
    )
    .unwrap();
    vault.reindex().unwrap();

    let view = vault.explain("guide").unwrap();
    assert_eq!(view.connections.len(), 1, "{:?}", view.connections);
    assert_eq!(view.connections[0].path, MEMORY_PATH);
    assert_eq!(view.connections[0].direction, "outbound");
    assert_eq!(view.unresolved.len(), 1, "{:?}", view.unresolved);
    assert_eq!(view.unresolved[0].target, "Hermes");
    assert_eq!(view.unresolved[0].relation, "references");
    assert_eq!(view.unresolved[0].origin, "inline");
}

#[test]
fn explain_shows_inbound_backlinks_with_inverse_labels() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // Memory is only pointed at, by SRS.
    let view = vault.explain(MEMORY_PATH).unwrap();
    assert_eq!(view.title.as_deref(), Some("memory"));
    assert!(!view.connections.is_empty());
    assert!(view.connections.iter().all(|c| c.direction == "inbound"));
    assert!(view.connections.iter().all(|c| c.path == SRS_PATH));

    let supported_by = view
        .connections
        .iter()
        .find(|c| c.label == "supported-by")
        .expect("the inverse label of supports");
    assert!(
        supported_by
            .explanation
            .as_deref()
            .is_some_and(|w| w.contains("forgetting curve")),
        "inbound edges keep the edge's why: {supported_by:?}"
    );
}

#[test]
fn explain_resolves_a_stem_and_a_full_path_to_the_same_note() {
    // The two ref forms since GH #170: the extensionless stem and the full path (L1).
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let by_stem = vault.explain("concepts/memory").unwrap();
    let by_path = vault.explain(MEMORY_PATH).unwrap();
    assert_eq!(by_stem.path, by_path.path);
    assert_eq!(by_stem.connections.len(), by_path.connections.len());
}

#[test]
fn explain_surfaces_frontmatter_provenance() {
    // The provenance data-model §0 says explain shows.
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    fs::write(
        root.join("author.md"),
        "---\ntype: note\ntitle: Author\n\
         b2_relations:\n  - \"supports [[concepts/memory|Human memory]] — via frontmatter\"\n---\n\
         A body with no links.\n",
    )
    .unwrap();
    vault.reindex().unwrap();

    let view = vault.explain("author").unwrap();
    let edge = view
        .connections
        .iter()
        .find(|c| c.path == MEMORY_PATH)
        .expect("the frontmatter relation edge");
    assert_eq!(edge.origin, "frontmatter");
    assert_eq!(edge.label, "supports");
}

#[test]
fn explain_reports_an_isolated_note_with_no_connections() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    fs::write(
        root.join("lonely.md"),
        "---\ntype: note\ntitle: Lonely\n---\nNo links at all.\n",
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
    assert!(
        view.unresolved.is_empty(),
        "no links at all ⇒ no unresolved links: {:?}",
        view.unresolved
    );
}
