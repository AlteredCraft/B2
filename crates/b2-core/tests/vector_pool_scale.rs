//! Regression: the full-space vector callers must not trip `sqlite-vec`'s KNN
//! `k` ceiling. `discover::candidates` and `search::graph_filtered_search` scan
//! the *whole* embedding space (index-engine.md §4 promises brute-force is
//! comfortable at 50–100k chunks). The `vec0` `MATCH … LIMIT k` operator refuses
//! `k > 4096` ("k value in knn query too large"), so past ~4096 chunks those two
//! commands — and a `search` with a large `--limit` — crashed. They must instead
//! degrade to a full brute-force scan and return results.
//!
//! Scope: with the deterministic *fake* embedder this proves the plumbing (no
//! crash, sane results) across the 4096-chunk boundary, not model quality.

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::{discover, open, search};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// `sqlite-vec`'s hard KNN cap; we build a vault comfortably past it.
const KNN_CAP: usize = 4096;

/// Build a vault of `notes` unlinked notes, each with `paras` blank-line-separated
/// paragraphs (→ one chunk each), and ingest it (project + fake-embed). Returns the
/// connection and the notes' b2ids in creation order. No links, so every note is a
/// discovery candidate for every other, and each note is its own reachable set.
fn big_vault(dir: &Path, notes: usize, paras: usize) -> (Connection, Vec<String>) {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut ids = Vec::new();
    for n in 0..notes {
        // ULID-shaped (26 chars), digits only after the prefix → all valid, unique.
        let b2id = format!("01JN{n:022}");
        let body = (0..paras)
            .map(|p| format!("note {n} paragraph {p}: shared topic alpha beta gamma"))
            .collect::<Vec<_>>()
            .join("\n\n");
        fs::write(
            vault.join(format!("n{n}.md")),
            format!("---\nb2id: {b2id}\ntype: note\ntitle: N{n}\n---\n{body}\n"),
        )
        .unwrap();
        ids.push(b2id);
    }
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();
    (conn, ids)
}

fn chunk_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn similar_survives_a_vault_larger_than_the_knn_cap() {
    // 50 notes × 90 paragraphs = 4500 chunks, comfortably past the 4096 ceiling.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, ids) = big_vault(tmp.path(), 50, 90);
    assert!(
        chunk_count(&conn) as usize > KNN_CAP,
        "the regression only bites past the KNN cap"
    );

    // Before the fix this errored: "k value in knn query too large, provided 4500…".
    let cands = discover::candidates(&conn, &ids[0], 10).unwrap();
    assert!(
        !cands.is_empty(),
        "49 unlinked notes are all candidates for the anchor"
    );
    assert!(
        cands.iter().all(|c| c.note_b2id != ids[0]),
        "the anchor is never its own candidate"
    );
}

#[test]
fn graph_filtered_search_survives_a_vault_larger_than_the_knn_cap() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, ids) = big_vault(tmp.path(), 50, 90);
    assert!(chunk_count(&conn) as usize > KNN_CAP);

    // No links, so within any hop count the anchor reaches only itself.
    let hits = search::graph_filtered_search(
        &conn,
        &FakeEmbedder::new(64),
        "shared topic",
        &ids[0],
        1,
        10,
    )
    .unwrap();
    assert!(!hits.is_empty(), "the anchor's own chunks match the query");
    assert!(
        hits.iter().all(|h| h.note_b2id == ids[0]),
        "only the (disconnected) anchor is reachable"
    );
}

#[test]
fn hybrid_search_survives_a_limit_that_overflows_the_knn_cap() {
    // A large --limit blows the fused pool (limit × 5) past the old KNN cap; this
    // crashed on any embedded vault, independent of size. A modest vault proves it: the
    // scan honours the full limit (here the whole vault, < limit) rather than truncating.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _) = big_vault(tmp.path(), 5, 5);

    let hits = search::hybrid_search(&conn, &FakeEmbedder::new(64), "shared topic", 1000).unwrap();
    // Every chunk matches the query, so an un-truncated pool returns them all
    // (hybrid_search is chunk-level; note dedup happens above it, in the façade).
    assert_eq!(
        hits.len() as i64,
        chunk_count(&conn),
        "no silent cap on an oversized limit — every matching chunk comes back"
    );
}
