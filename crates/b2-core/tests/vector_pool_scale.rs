//! Whole-space retrieval stays exhaustive: every path that scans the vector space returns
//! complete results, with no silent cap. Asserted on the cap-bearing primitive and on a
//! modest vault rather than a 4096-chunk one (ADR-0006, GH #46).

mod common;

use b2_core::db;
use b2_core::embed::{Embedder, FakeEmbedder};
use b2_core::ingest::ingest_vault;
use b2_core::{discover, open, search};
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// Ingest `notes` unlinked notes of `paras` paragraphs each; returns the connection and
/// paths. Chunking is size-targeted (~30 paragraphs a chunk), so read totals back with
/// [`chunk_count`].
fn big_vault(dir: &Path, notes: usize, paras: usize) -> (Connection, Vec<String>) {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut ids = Vec::new();
    for n in 0..notes {
        let body = (0..paras)
            .map(|p| format!("note {n} paragraph {p}: shared topic alpha beta gamma"))
            .collect::<Vec<_>>()
            .join("\n\n");
        fs::write(
            vault.join(format!("n{n}.md")),
            format!("---\ntype: note\ntitle: N{n}\n---\n{body}\n"),
        )
        .unwrap();
        ids.push(format!("n{n}.md"));
    }
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &FakeEmbedder::new(64)).unwrap();
    (conn, ids)
}

fn chunk_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap()
}

/// `vector_search(k)` returns exactly `min(k, N)` for `k` below, at and far past `N`.
#[test]
fn vector_search_is_exhaustive_and_truncates_only_to_k() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _) = big_vault(tmp.path(), 20, 90); // ~60 chunks
    let n = chunk_count(&conn) as usize;
    assert!(
        n > 1,
        "the fixture must project several chunks to test truncation"
    );
    let probe = FakeEmbedder::new(64)
        .embed_query("shared topic alpha")
        .unwrap();

    assert_eq!(
        db::vector_search(&conn, &probe, n / 2).unwrap().len(),
        n / 2
    );
    assert_eq!(db::vector_search(&conn, &probe, n).unwrap().len(), n);
    assert_eq!(
        db::vector_search(&conn, &probe, n + 5000).unwrap().len(),
        n,
        "an oversized k returns the whole space — no error, no silent cap"
    );
}

#[test]
fn similar_returns_the_full_candidate_set_without_a_silent_cap() {
    // ~150 chunks; with no links every other note is a candidate.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, ids) = big_vault(tmp.path(), 50, 90);

    // 49 candidates sit under the 200-note shortlist floor, so the scan is exact.
    let forty = discover::candidates(&conn, &ids[0], 40, false).unwrap();
    assert_eq!(forty.len(), 40, "the scan honours the full requested limit");

    let all = discover::candidates(&conn, &ids[0], 1000, false).unwrap();
    assert_eq!(
        all.len(),
        ids.len() - 1,
        "every unlinked note is a candidate for the anchor"
    );
    assert!(
        all.iter().all(|c| c.note_path != ids[0]),
        "the anchor is never its own candidate"
    );
}

/// A large `--limit` widens the fused pool (`limit × 5`); the scan must honour it.
#[test]
fn hybrid_search_honours_an_oversized_limit_without_truncating() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _) = big_vault(tmp.path(), 5, 5);

    let hits = search::hybrid_search(&conn, &FakeEmbedder::new(64), "shared topic", 1000)
        .unwrap()
        .hits;
    // Every chunk matches, and hybrid_search is chunk-level (dedup is the façade's).
    assert_eq!(
        hits.len() as i64,
        chunk_count(&conn),
        "no silent cap on an oversized limit — every matching chunk comes back"
    );
}
