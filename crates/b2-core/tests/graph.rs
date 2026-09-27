//! The typed graph projection, `neighbors`, and `incremental ≡ full` (index-engine.md).

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::graph::{neighbors, unresolved_outbound, Direction};
use b2_core::ingest::{ingest_file, ingest_vault, EmbedCtx, ProjectionCtx};
use b2_core::open;
use common::{golden_vault_copy, ingest_golden, MEMORY_PATH, SRS_PATH};
use rusqlite::Connection;
use std::fs;

/// `(src_path, dst_path, dst_path_raw, type, origin, occ, explanation)`; id is derived
/// from the rest, so it is excluded.
type EdgeTuple = (
    String,
    Option<String>,
    String,
    String,
    String,
    i64,
    Option<String>,
);

fn edge_snapshot(conn: &Connection) -> Vec<EdgeTuple> {
    let mut stmt = conn
        .prepare(
            "SELECT src_path, dst_path, dst_path_raw, type, origin, occurrence_index, explanation
             FROM edges
             ORDER BY src_path, type, dst_path_raw, occurrence_index",
        )
        .unwrap();
    stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn golden_graph_has_inline_references_and_frontmatter_supports() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::default());

    let edges = edge_snapshot(&conn);
    assert_eq!(
        edges,
        vec![
            // A bare wikilink in prose.
            (
                SRS_PATH.to_string(),
                Some(MEMORY_PATH.to_string()),
                "concepts/memory".to_string(),
                "references".to_string(),
                "inline".to_string(),
                0,
                None,
            ),
            // A `b2_relations:` entry with explanation (data-model §2/§8).
            (
                SRS_PATH.to_string(),
                Some(MEMORY_PATH.to_string()),
                "concepts/memory".to_string(),
                "supports".to_string(),
                "frontmatter".to_string(),
                0,
                Some("applies the forgetting curve".to_string()),
            ),
        ]
    );
}

/// One stored edge reads as its verb from the source and its inverse from the target, with
/// no reciprocal row. The façade's view is in `tests/vault.rs`.
#[test]
fn neighbors_label_by_direction_at_both_ends() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::default());

    let inbound = neighbors(&conn, MEMORY_PATH).unwrap();
    let mut labels: Vec<&str> = inbound.iter().map(|n| n.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["referenced-by", "supported-by"]);
    assert!(inbound
        .iter()
        .all(|n| n.other == SRS_PATH && n.direction == Direction::Inbound));

    let outbound = neighbors(&conn, SRS_PATH).unwrap();
    let mut labels: Vec<&str> = outbound.iter().map(|n| n.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["references", "supports"]);
    assert!(outbound
        .iter()
        .all(|n| n.other == MEMORY_PATH && n.direction == Direction::Outbound));
    assert_eq!(inbound.len(), outbound.len(), "one edge set, two views");
}

#[test]
fn unresolved_outbound_surfaces_folder_and_typo_links() {
    // GH #12: `neighbors` and `unresolved_outbound` together cover every outbound link,
    // including a folder name or a typo.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let guide = "guide.md";
    fs::write(
        vault.join(guide),
        "---\ntype: note\ntitle: Guide\n---\n\
         - [[Hermes]] is the R&D machine\n\
         See [[concepts/memory|Human memory]] for context.\n\
         A [[concepts/memoryy]] typo.\n",
    )
    .unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &FakeEmbedder::default()).unwrap();

    let ns = neighbors(&conn, guide).unwrap();
    assert_eq!(ns.len(), 1, "only the memory link resolves: {ns:?}");
    assert_eq!(ns[0].other, MEMORY_PATH);
    assert_eq!(ns[0].direction, Direction::Outbound);

    // Ordered by target.
    let dangling = unresolved_outbound(&conn, guide).unwrap();
    let targets: Vec<&str> = dangling.iter().map(|u| u.target.as_str()).collect();
    assert_eq!(targets, vec!["Hermes", "concepts/memoryy"]);
    assert!(dangling.iter().all(|u| u.edge_type == "references"));
    assert!(dangling.iter().all(|u| u.origin == "inline"));

    // No false positives.
    assert!(unresolved_outbound(&conn, SRS_PATH).unwrap().is_empty());
}

#[test]
fn one_note_reindex_equals_full() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    ingest_vault(&conn, &vault, &FakeEmbedder::default()).unwrap();
    let after_full = edge_snapshot(&conn);

    let cfg = b2_core::chunk::ChunkConfig::default();
    let embedder = FakeEmbedder::default();
    let ctx = EmbedCtx::new(ProjectionCtx::new(&conn, &vault, &cfg), &embedder);
    ingest_file(ctx, "notes/spaced-repetition.md").unwrap();
    let after_incremental = edge_snapshot(&conn);

    assert_eq!(
        after_full, after_incremental,
        "incremental re-index must match full"
    );

    ingest_vault(&conn, &vault, &FakeEmbedder::default()).unwrap();
    assert_eq!(
        after_full,
        edge_snapshot(&conn),
        "full reindex must be idempotent"
    );
}
