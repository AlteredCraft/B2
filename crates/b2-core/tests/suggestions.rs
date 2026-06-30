//! Step 4 — the `.b2/` event log + replay
//! (planning/specs/index-engine-build.md step 4): a suggestion is inert (in the
//! queue, never on disk), drop→replay reproduces the queue (Flow ⑤), and a
//! rejection tombstone blocks re-proposal.

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::event::{EventSink, JsonlSink, NullSink};
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::{open, replay, suggest};
use common::{golden_vault_copy, FixedId, MEMORY_ID, SRS_ID};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// (id, src, dst, dst_path_raw, type, status, explanation, by, source, confidence, created, decided)
type SuggestionRow = (
    String,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<f64>,
    String,
    Option<String>,
);

fn review_snapshot(conn: &Connection) -> Vec<SuggestionRow> {
    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.dst_id, e.dst_path_raw, e.type, e.status, e.explanation,
                    p.by, p.source, p.confidence, p.created, p.decided
             FROM edges e JOIN edge_provenance p ON p.edge_id = e.id
             WHERE e.status IN ('suggested','rejected')
             ORDER BY e.id",
        )
        .unwrap();
    stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, Option<f64>>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, Option<String>>(10)?,
        ))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

/// Ingest the golden vault into `vault` with a durable log, returning the conn.
fn setup(dir: &Path) -> (Connection, JsonlSink, std::path::PathBuf) {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    let sink = JsonlSink::in_vault(&vault).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &sink, &FakeEmbedder::default()).unwrap();
    (conn, sink, vault)
}

#[test]
fn a_suggestion_is_listed_but_never_written_to_disk() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, sink, vault) = setup(tmp.path());

    let before_srs = fs::read_to_string(vault.join("notes/spaced-repetition.md")).unwrap();
    let before_mem = fs::read_to_string(vault.join("concepts/memory.md")).unwrap();

    let edge_id = suggest::generate_suggestion(
        &conn,
        &sink,
        &FixedId("01JSUGGEST0000000000000001"),
        SRS_ID,
        MEMORY_ID,
        "contradicts",
        Some("seems to argue the opposite"),
        "agent:test-model",
        Some("semantic+co-citation"),
        Some(0.82),
        "2026-06-29T00:00:00Z",
    )
    .unwrap()
    .expect("a fresh pair+type should be proposable");
    assert_eq!(edge_id, "01JSUGGEST0000000000000001");

    // It shows in the queue with its full decision fuel…
    let queue = suggest::list_suggestions(&conn).unwrap();
    assert_eq!(queue.len(), 1);
    let s = &queue[0];
    assert_eq!(s.src_id, SRS_ID);
    assert_eq!(s.dst_id.as_deref(), Some(MEMORY_ID));
    assert_eq!(s.edge_type, "contradicts");
    assert_eq!(s.confidence, Some(0.82));
    assert_eq!(s.by, "agent:test-model");

    // …and not one byte of any note changed (inert until accepted).
    assert_eq!(
        fs::read_to_string(vault.join("notes/spaced-repetition.md")).unwrap(),
        before_srs
    );
    assert_eq!(
        fs::read_to_string(vault.join("concepts/memory.md")).unwrap(),
        before_mem
    );

    // The durable record lives in the in-vault log.
    let log = fs::read_to_string(vault.join(".b2/log/events.jsonl")).unwrap();
    assert!(log.contains("suggestion.generated"));
    assert!(log.contains("01JSUGGEST0000000000000001"));
}

#[test]
fn drop_and_replay_reproduces_the_queue() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, sink, vault) = setup(tmp.path());

    suggest::generate_suggestion(
        &conn,
        &sink,
        &FixedId("01JSUGGEST0000000000000001"),
        SRS_ID,
        MEMORY_ID,
        "contradicts",
        Some("opposite"),
        "agent:test-model",
        Some("semantic"),
        Some(0.82),
        "2026-06-29T00:00:00Z",
    )
    .unwrap()
    .unwrap();
    let before = review_snapshot(&conn);
    assert_eq!(before.len(), 1);

    // "rm b2.sqlite": rebuild a fresh index from Markdown, then replay the log.
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

    assert_eq!(
        before,
        review_snapshot(&conn2),
        "replay must reproduce the queue exactly"
    );
}

#[test]
fn a_rejection_tombstone_blocks_re_proposal() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, sink, _vault) = setup(tmp.path());

    let edge_id = suggest::generate_suggestion(
        &conn,
        &sink,
        &FixedId("01JSUGGEST0000000000000001"),
        SRS_ID,
        MEMORY_ID,
        "contradicts",
        None,
        "agent:test-model",
        None,
        Some(0.5),
        "2026-06-29T00:00:00Z",
    )
    .unwrap()
    .unwrap();

    suggest::reject_suggestion(&conn, &sink, &edge_id, "2026-06-29T01:00:00Z").unwrap();

    // The live queue is empty; the tombstone remains as a rejected edge.
    assert!(suggest::list_suggestions(&conn).unwrap().is_empty());

    // Proposing the same (src, dst, type) again is refused.
    let again = suggest::generate_suggestion(
        &conn,
        &sink,
        &FixedId("01JSUGGEST0000000000000002"),
        SRS_ID,
        MEMORY_ID,
        "contradicts",
        None,
        "agent:test-model",
        None,
        Some(0.9),
        "2026-06-29T02:00:00Z",
    )
    .unwrap();
    assert!(
        again.is_none(),
        "a rejected pair+type must not be re-proposed"
    );
}

#[test]
fn replaying_an_accepted_suggestion_leaves_no_queue_row() {
    // Replay treats an accepted suggestion as a no-op for the queue — its active
    // edge re-derives from Markdown (Flow ①), so generated+accepted nets to
    // nothing in the review tables. (The accept *operation* — writing the inline
    // link — is Flow ③, a later slice; here we drive replay from the log directly.)
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, sink, vault) = setup(tmp.path());

    let edge_id = suggest::generate_suggestion(
        &conn,
        &sink,
        &FixedId("01JSUGGEST0000000000000001"),
        SRS_ID,
        MEMORY_ID,
        "contradicts",
        None,
        "agent:test-model",
        None,
        Some(0.7),
        "2026-06-29T00:00:00Z",
    )
    .unwrap()
    .unwrap();
    sink.append(&b2_core::event::Event::SuggestionAccepted {
        edge_id: edge_id.clone(),
        decided: "2026-06-29T03:00:00Z".to_string(),
    })
    .unwrap();

    // Fresh rebuild + replay: the generated row is created then removed by accept.
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

    assert!(
        review_snapshot(&conn2).is_empty(),
        "accepted suggestion leaves no queue row"
    );
}
