//! Step 5 — hybrid retrieval (planning/specs/completed/index-engine-build.md step 5):
//! BM25 ⊕ vector → RRF fusion (k=60), resolved to notes, plus the graph-filtered
//! vector⨝edge join (index-engine.md §3) — the substrate connection discovery
//! runs on.
//!
//! Scope: with the deterministic *fake* embedder, vector ranking is not semantic,
//! so these prove the **plumbing** (fusion math + the join), not model quality —
//! that is the real-embedder eval suite (testability stack, point 5).

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::search::{self, RRF_K};
use b2_core::{open, search::Hit};
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

fn ingest_golden(dir: &Path) -> Connection {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();
    conn
}

fn note_set(hits: &[Hit]) -> std::collections::BTreeSet<String> {
    hits.iter().map(|h| h.note_b2id.clone()).collect()
}

#[test]
fn rrf_uses_k_60() {
    assert_eq!(RRF_K, 60);
}

#[test]
fn rrf_ranks_a_doc_present_in_both_lists_above_single_list_winners() {
    // 20 is rank-1 in BM25 and rank-0 in vector → appearing in both lifts it
    // above 10, which is rank-0 in BM25 but only rank-2 in vector. This is the
    // "hybrid beats either alone" property, at the fusion-math level.
    let bm25 = vec![10, 20, 30];
    let vector = vec![20, 40, 10];
    let fused = search::rrf_fuse(&[bm25, vector], RRF_K);

    assert_eq!(fused[0].0, 20, "doc in both lists wins");
    // every id present, fused score positive and descending
    assert_eq!(fused.len(), 4);
    for w in fused.windows(2) {
        assert!(w[0].1 >= w[1].1, "scores must be descending");
    }
}

#[test]
fn keyword_search_finds_chunks_by_term() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    let ids = search::keyword_search(&conn, "forgetting", 10).unwrap();
    assert!(!ids.is_empty());
    // 'forgetting' lives only in spaced-repetition's body.
    let note: String = conn
        .query_row(
            "SELECT note_b2id FROM chunks WHERE id = ?1",
            [ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, SRS_ID);
}

#[test]
fn keyword_search_tolerates_natural_language_punctuation() {
    // Real semantic search invites NL queries: apostrophes, quotes, punctuation are
    // FTS5 *syntax* and would raise a parse error if passed raw (the bug the eval
    // surfaced). They must be sanitized to a safe MATCH, still matching real terms.
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    for q in [
        "why can't I remember? the \"forgetting\" curve!",
        "forgetting...",
        "-- forgetting --",
    ] {
        let ids = search::keyword_search(&conn, q, 10).unwrap();
        assert!(!ids.is_empty(), "query {q:?} should still find the term");
    }

    // A query with no usable terms is empty, not an error (vector half still runs).
    assert!(search::keyword_search(&conn, "!!! ??? ...", 10)
        .unwrap()
        .is_empty());
}

#[test]
fn fts5_query_sanitizes_to_ored_literals() {
    assert_eq!(
        search::fts5_query("can't sleep"),
        "\"can\" OR \"t\" OR \"sleep\""
    );
    assert_eq!(search::fts5_query("  !!! "), "");
    assert_eq!(search::fts5_query("forgetting"), "\"forgetting\"");
}

#[test]
fn hybrid_search_combines_signals_and_resolves_to_notes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    let hits = search::hybrid_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5).unwrap();
    assert!(!hits.is_empty());
    // every hit resolves to a real note, and SRS (the only keyword match) is present
    assert!(hits.iter().all(|h| !h.note_b2id.is_empty()));
    assert!(note_set(&hits).contains(SRS_ID));
}

#[test]
fn graph_filtered_search_restricts_to_reachable_notes() {
    // A 3-note vault: a → b (linked), c disconnected, all share a keyword.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("a.md"),
        "---\nb2id: 01JA0000000000000000000001\ntype: note\ntitle: A\n---\nshared topic alpha. See [[b]].\n",
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        "---\nb2id: 01JB0000000000000000000002\ntype: note\ntitle: B\n---\nshared topic beta.\n",
    )
    .unwrap();
    fs::write(
        vault.join("c.md"),
        "---\nb2id: 01JC0000000000000000000003\ntype: note\ntitle: C\n---\nshared topic gamma.\n",
    )
    .unwrap();

    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();

    // Within 1 hop of A: {A, B}. C is disconnected and must be excluded even
    // though its text matches the query.
    let hits = search::graph_filtered_search(
        &conn,
        &FakeEmbedder::new(64),
        "shared topic",
        "01JA0000000000000000000001",
        1,
        10,
    )
    .unwrap();

    let notes = note_set(&hits);
    assert!(!notes.is_empty());
    assert!(
        !notes.contains("01JC0000000000000000000003"),
        "disconnected note must be filtered out"
    );
    assert!(notes
        .iter()
        .all(|n| n == "01JA0000000000000000000001" || n == "01JB0000000000000000000002"));
}

#[test]
fn search_chunks_exposes_passage_level_hits() {
    // The sub-note view (`Vault::search_chunks`) the retrieval eval scores passage
    // ranks through (specs/eval-strategy.md): same retrieval as `search`, no note
    // dedup, each hit resolved to its note path + heading breadcrumb + the chunk's
    // FULL text — containment-scorable, unlike `SearchResult`'s display snippet.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.reindex().unwrap();

    let hits = vault.search_chunks("forgetting curve", 10).unwrap();
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .all(|h| !h.path.is_empty() && !h.text.is_empty()));
    // 'forgetting' lives only in spaced-repetition; its hit must carry the full
    // chunk text (the term itself), not a trimmed snippet.
    let srs = hits
        .iter()
        .find(|h| h.b2id == SRS_ID)
        .expect("the one keyword-matching note must surface at chunk level");
    assert!(srs.path.ends_with("spaced-repetition.md"));
    assert!(srs.text.contains("forgetting"));
}

#[test]
fn graph_filter_with_zero_hops_is_just_the_anchor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    // 0 hops from memory → only memory's own chunks are eligible.
    let hits =
        search::graph_filtered_search(&conn, &FakeEmbedder::new(64), "brain", MEMORY_ID, 0, 10)
            .unwrap();
    assert!(hits.iter().all(|h| h.note_b2id == MEMORY_ID));
}
