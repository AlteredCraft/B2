//! The accept operation (Flow ③, revised): accepting a suggestion appends to the
//! source note's frontmatter `relations:` (Markdown first, **never the body**),
//! activates the edge as origin=frontmatter, and stays pristine — and the whole
//! thing survives drop→rebuild→replay (planning/data-model.md §0/§4).

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::event::{JsonlSink, NullSink};
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::note::parse;
use b2_core::{open, replay, suggest};
use common::{golden_vault_copy, FixedId, MEMORY_ID, SRS_ID};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

fn setup(dir: &Path) -> (Connection, JsonlSink, PathBuf) {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    let sink = JsonlSink::in_vault(&vault).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &sink, &FakeEmbedder::default()).unwrap();
    (conn, sink, vault)
}

fn generate(conn: &Connection, sink: &JsonlSink) -> String {
    suggest::generate_suggestion(
        conn,
        sink,
        &FixedId("01JSUGGEST0000000000000001"),
        SRS_ID,
        MEMORY_ID,
        "contradicts",
        Some("argues the opposite"),
        "agent:test-model",
        Some("semantic+co-citation"),
        Some(0.82),
        "2026-06-29T00:00:00Z",
    )
    .unwrap()
    .unwrap()
}

fn contradicts_edge(conn: &Connection) -> Option<(String, String)> {
    conn.query_row(
        "SELECT origin, status FROM edges WHERE src_id = ?1 AND dst_id = ?2 AND type = 'contradicts'",
        [SRS_ID, MEMORY_ID],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

#[test]
fn accept_writes_frontmatter_not_body_and_activates_the_edge() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, sink, vault) = setup(tmp.path());
    let srs = vault.join("notes/spaced-repetition.md");
    let body_before = parse(&fs::read_to_string(&srs).unwrap()).body().to_string();

    let edge_id = generate(&conn, &sink);
    let accepted = suggest::accept_suggestion(
        &conn,
        &sink,
        &FakeEmbedder::default(),
        &vault,
        &edge_id,
        "2026-06-29T01:00:00Z",
    )
    .unwrap();
    assert!(accepted);

    let raw_after = fs::read_to_string(&srs).unwrap();
    let n = parse(&raw_after);

    // body byte-unchanged; the edge landed in frontmatter relations:
    assert_eq!(n.body(), body_before, "the body must be untouched");
    assert_eq!(
        n.fields().relations,
        vec!["contradicts [[concepts/memory|Human memory]] — argues the opposite".to_string()]
    );
    assert_eq!(
        parse(&raw_after).as_str(),
        raw_after,
        "round-trip stays lossless"
    );

    // edge is now active/frontmatter; the queue is empty
    assert_eq!(
        contradicts_edge(&conn),
        Some(("frontmatter".into(), "active".into()))
    );
    assert!(suggest::list_suggestions(&conn).unwrap().is_empty());

    // pristine: no provenance hangs off any active edge
    let prov: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM edge_provenance p JOIN edges e ON e.id = p.edge_id WHERE e.status = 'active'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(prov, 0);

    // the acceptance is in the log
    let log = fs::read_to_string(vault.join(".b2/log/events.jsonl")).unwrap();
    assert!(log.contains("suggestion.accepted"));
}

#[test]
fn accepted_edge_survives_drop_rebuild_and_replay() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, sink, vault) = setup(tmp.path());
    let edge_id = generate(&conn, &sink);
    suggest::accept_suggestion(
        &conn,
        &sink,
        &FakeEmbedder::default(),
        &vault,
        &edge_id,
        "2026-06-29T01:00:00Z",
    )
    .unwrap();

    // "rm b2.sqlite": rebuild from Markdown (FM relation → active edge), replay log.
    let conn2 = open(&tmp.path().join("rebuilt.sqlite")).unwrap();
    ingest_vault(
        &conn2,
        &vault,
        &UlidGen,
        &NullSink,
        &FakeEmbedder::default(),
    )
    .unwrap();
    replay::replay_log(&conn2, &sink).unwrap();

    // the active frontmatter edge is back from Markdown; generated+accepted net to
    // no queue row (the generated insert is absorbed by the materialized edge).
    assert_eq!(
        contradicts_edge(&conn2),
        Some(("frontmatter".into(), "active".into()))
    );
    assert!(suggest::list_suggestions(&conn2).unwrap().is_empty());
}

#[test]
fn accepting_a_nonexistent_suggestion_is_a_no_op() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, sink, vault) = setup(tmp.path());
    let accepted = suggest::accept_suggestion(
        &conn,
        &sink,
        &FakeEmbedder::default(),
        &vault,
        "01JNOPE000000000000000000",
        "2026-06-29T01:00:00Z",
    )
    .unwrap();
    assert!(
        !accepted,
        "no pending suggestion with that id → nothing accepted"
    );
}
