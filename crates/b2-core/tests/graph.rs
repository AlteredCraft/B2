//! Step 2 — the typed graph projection + `neighbors`, and the
//! `incremental ≡ full` invariant (planning/specs/completed/index-engine-build.md step 2).

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::graph::{neighbors, unresolved_outbound, Direction};
use b2_core::id::UlidGen;
use b2_core::ingest::{ingest_file, ingest_vault};
use b2_core::open;
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use rusqlite::Connection;
use std::fs;

/// (src_id, dst_id, dst_path_raw, type, origin, occ, explanation), ordered — the
/// comparable shape of the whole edge set (id excluded; it is a deterministic
/// function of the rest). Every edge is authored + active, so there is no `status`.
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
            "SELECT src_id, dst_id, dst_path_raw, type, origin, occurrence_index, explanation
             FROM edges
             ORDER BY src_id, type, dst_path_raw, occurrence_index",
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

fn ingest_golden(dir: &std::path::Path) -> Connection {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();
    conn
}

#[test]
fn golden_graph_has_references_and_elaborates_inline_active() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    let edges = edge_snapshot(&conn);
    assert_eq!(
        edges,
        vec![
            // elaborates: spaced-repetition → memory (typed line, with explanation)
            (
                SRS_ID.to_string(),
                Some(MEMORY_ID.to_string()),
                "concepts/memory".to_string(),
                "elaborates".to_string(),
                "inline".to_string(),
                0,
                Some("applies the forgetting curve".to_string()),
            ),
            // references: spaced-repetition → memory (prose bare wikilink)
            (
                SRS_ID.to_string(),
                Some(MEMORY_ID.to_string()),
                "concepts/memory".to_string(),
                "references".to_string(),
                "inline".to_string(),
                0,
                None,
            ),
        ]
    );
}

#[test]
fn neighbors_of_memory_are_referenced_by_and_elaborated_by() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    let ns = neighbors(&conn, MEMORY_ID).unwrap();
    let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
    labels.sort_unstable();
    assert_eq!(labels, vec!["elaborated-by", "referenced-by"]);

    // both are inbound edges from spaced-repetition (B2 stores no reciprocal link)
    assert!(ns
        .iter()
        .all(|n| n.other == SRS_ID && n.direction == Direction::Inbound));
}

#[test]
fn neighbors_of_spaced_repetition_are_outbound() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    let ns = neighbors(&conn, SRS_ID).unwrap();
    let mut labels: Vec<&str> = ns.iter().map(|n| n.label.as_str()).collect();
    labels.sort_unstable();
    // outbound labels are the verbs themselves
    assert_eq!(labels, vec!["elaborates", "references"]);
    assert!(ns
        .iter()
        .all(|n| n.other == MEMORY_ID && n.direction == Direction::Outbound));
}

#[test]
fn unresolved_outbound_surfaces_folder_and_typo_links() {
    // GH #12: a note is one `.md` file, so a `[[Hermes]]` naming a *folder* (or a
    // typo) resolves to nothing. Those dangling links must be surfaced, not dropped —
    // `neighbors` keeps only resolved edges, `unresolved_outbound` returns the rest,
    // and together they cover every outbound link the note authored.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let guide = "01JGUIDE00000000000000001";
    fs::write(
        vault.join("guide.md"),
        format!(
            "---\nb2id: {guide}\ntype: note\ntitle: Guide\n---\n\
             - [[Hermes]] is the R&D machine\n\
             See [[concepts/memory|Human memory]] for context.\n\
             A [[concepts/memoryy]] typo.\n"
        ),
    )
    .unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();

    // Only the one resolvable link is an outbound neighbor.
    let ns = neighbors(&conn, guide).unwrap();
    assert_eq!(ns.len(), 1, "only the memory link resolves: {ns:?}");
    assert_eq!(ns[0].other, MEMORY_ID);
    assert_eq!(ns[0].direction, Direction::Outbound);

    // The folder + typo links are surfaced as unresolved (ordered by target), each an
    // inline `references` edge that resolved to nothing.
    let dangling = unresolved_outbound(&conn, guide).unwrap();
    let targets: Vec<&str> = dangling.iter().map(|u| u.target.as_str()).collect();
    assert_eq!(targets, vec!["Hermes", "concepts/memoryy"]);
    assert!(dangling.iter().all(|u| u.edge_type == "references"));
    assert!(dangling.iter().all(|u| u.origin == "inline"));

    // A fully-resolved note has none — no false positives.
    assert!(unresolved_outbound(&conn, SRS_ID).unwrap().is_empty());
}

#[test]
fn one_note_reindex_equals_full() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();
    let after_full = edge_snapshot(&conn);

    // Re-project a single note against the already-complete index.
    ingest_file(
        &conn,
        &vault,
        "notes/spaced-repetition.md",
        &UlidGen,
        &FakeEmbedder::default(),
    )
    .unwrap();
    let after_incremental = edge_snapshot(&conn);

    assert_eq!(
        after_full, after_incremental,
        "incremental re-index must match full"
    );

    // And a second full reindex is identical too (idempotent).
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();
    assert_eq!(
        after_full,
        edge_snapshot(&conn),
        "full reindex must be idempotent"
    );
}
