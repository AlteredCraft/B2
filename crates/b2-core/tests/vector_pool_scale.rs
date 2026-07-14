//! Whole-space retrieval stays **exhaustive**: every path that scans the vector
//! space returns complete, un-truncated results, with no silent cap.
//!
//! *History.* This file was born to lock the fix for `sqlite-vec`'s `vec0`
//! `MATCH … LIMIT k` ceiling — `k > 4096` raised "k value in knn query too large"
//! and crashed discovery, graph-filtered search, and an oversized `--limit`. It did
//! so by building a vault of **> 4096 chunks** and asserting the paths survived.
//!
//! That store is gone (schema v3, #36/#38): vectors live in plain tables scored by a
//! full in-process scan (`db::scan_vector_distances` → `for_each_stored_vector`),
//! whose callers either take **every** row (`vector_search_all`) or bound it with a
//! plain `Vec::truncate` (`vector_search`) — structurally incapable of the old
//! `vec0` ceiling. The literal `4096` is now a retired dependency's magic number that
//! no live line of code knows about, so gating on it is neither cheap nor meaningful:
//! under the qmd chunker (#19/#42, size-targeted paragraph coalescing) reaching 4096
//! *real* chunks costs the fast suite ~20s (GH #46), to reprove a boundary that can't
//! recur without reintroducing `sqlite-vec` — a locked-against decision (root
//! `CLAUDE.md`, "No vector extension").
//!
//! So instead of a >4096-chunk fixture, this asserts the property that actually
//! matters now — **exhaustiveness / no silent truncation** — directly on the
//! cap-bearing primitive (`vector_search*`), and proves the discovery and
//! graph-filtered paths return *complete* results on a real, modest, fast multi-note
//! vault. See GH #46 for the reasoning.
//!
//! Scope: the deterministic *fake* embedder — this proves plumbing (no crash,
//! complete results), not model quality.

mod common;

use b2_core::db;
use b2_core::embed::{Embedder, FakeEmbedder};
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::{discover, open, search};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// Build a vault of `notes` unlinked notes, each body `paras` blank-line-separated
/// paragraphs, and ingest it (project + fake-embed). Returns the connection and the
/// notes' b2ids in creation order.
///
/// **Chunking is size-targeted, not one-per-paragraph.** The qmd chunker (#19/#42)
/// coalesces small paragraphs toward its ~450-token (~1800-char) target, so these
/// ~50-char paragraphs pack ~30 to a chunk: a 90-paragraph note projects to ~3
/// chunks, not 90 (the retired paragraph splitter's shape). Callers that need a known
/// chunk total read it back with [`chunk_count`] rather than assuming `notes × paras`.
///
/// No links, so every note is a discovery candidate for every other, and each note is
/// its own reachable set.
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

fn note_chunk_count(conn: &Connection, b2id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM chunks WHERE note_b2id = ?1",
        [b2id],
        |r| r.get(0),
    )
    .unwrap()
}

/// The retired `vec0` store errored on `k > 4096`; the in-process scan cannot. Proven
/// **directly on the primitive that would carry any such cap**, cheaply — with no
/// oversized fixture and no magic number: `vector_search_all` returns *every* stored
/// vector, and `vector_search(k)` returns exactly `min(k, N)` for `k` below, at, and
/// far past `N` (where the old store crashed).
#[test]
fn vector_search_is_exhaustive_and_truncates_only_to_k() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _) = big_vault(tmp.path(), 20, 90); // a real fake-embedded space, ~60 chunks
    let n = chunk_count(&conn) as usize;
    assert!(
        n > 1,
        "the fixture must project several chunks to test truncation"
    );
    let probe = FakeEmbedder::new(64)
        .embed_query("shared topic alpha")
        .unwrap();

    // No `k`: the scan ranks the ENTIRE space — one vector per stored chunk.
    assert_eq!(
        db::vector_search_all(&conn, &probe).unwrap().len(),
        n,
        "the whole-space scan returns every stored vector"
    );
    // `k` below N is honoured exactly…
    assert_eq!(
        db::vector_search(&conn, &probe, n / 2).unwrap().len(),
        n / 2
    );
    // …at N returns the whole space…
    assert_eq!(db::vector_search(&conn, &probe, n).unwrap().len(), n);
    // …and far past N (where `vec0` errored) truncates to N, never crashes or caps.
    assert_eq!(
        db::vector_search(&conn, &probe, n + 5000).unwrap().len(),
        n,
        "an oversized k returns the whole space — no error, no silent cap"
    );
}

/// `discover::candidates` surfaces the full requested set on a real multi-note vault,
/// uncapped and anchor-excluded — the whole-space discovery scan is not silently
/// truncated below the notes it should return.
#[test]
fn similar_returns_the_full_candidate_set_without_a_silent_cap() {
    // 50 unlinked notes → ~150 chunks: a genuine multi-note, multi-chunk scan (fast),
    // not a toy vault. With no links every other note is a candidate for the anchor.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, ids) = big_vault(tmp.path(), 50, 90);

    // A limit below the candidate pool must come back *full* — no silent under-cap.
    // (49 candidates sit under the 200-note shortlist floor, so the two-stage scan is
    // exact, exactly as the old whole-space scan was.)
    let forty = discover::candidates(&conn, &ids[0], 40).unwrap();
    assert_eq!(forty.len(), 40, "the scan honours the full requested limit");

    // A limit past the pool returns *every* unlinked note, and never the anchor.
    let all = discover::candidates(&conn, &ids[0], 1000).unwrap();
    assert_eq!(
        all.len(),
        ids.len() - 1,
        "every unlinked note is a candidate for the anchor"
    );
    assert!(
        all.iter().all(|c| c.note_b2id != ids[0]),
        "the anchor is never its own candidate"
    );
}

/// `graph_filtered_search` scans the *whole* ranked space and keeps the reachable
/// notes — so a `limit` above the reachable chunk count returns **every** reachable
/// chunk, wherever it ranks, un-truncated (and nothing unreachable).
#[test]
fn graph_filtered_search_returns_every_reachable_chunk_without_a_silent_cap() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, ids) = big_vault(tmp.path(), 50, 90);

    // No links: within any hop count the anchor reaches only itself. Its chunks all
    // match the query; asking for more than it has must return all of them — proof
    // the scan considers the entire ranked space, not just a capped prefix.
    let anchor_chunks = note_chunk_count(&conn, &ids[0]);
    let hits = search::graph_filtered_search(
        &conn,
        &FakeEmbedder::new(64),
        "shared topic",
        &ids[0],
        1,
        (anchor_chunks as usize) + 100,
    )
    .unwrap();
    assert_eq!(
        hits.len() as i64,
        anchor_chunks,
        "every one of the disconnected anchor's chunks comes back, un-truncated"
    );
    assert!(
        hits.iter().all(|h| h.note_b2id == ids[0]),
        "only the (disconnected) anchor is reachable"
    );
}

/// A large `--limit` blows the fused pool (`limit × 5`) well past the old `vec0` cap;
/// this crashed on any embedded vault, independent of size. A modest vault proves the
/// fix: the scan honours the full limit (here the whole vault, < limit) rather than
/// truncating.
#[test]
fn hybrid_search_honours_an_oversized_limit_without_truncating() {
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
