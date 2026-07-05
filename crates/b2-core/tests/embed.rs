//! Step 3 — `sqlite-vec` + the embedder seam
//! (planning/specs/index-engine-build.md step 3): a deterministic fake embedder
//! produces reproducible KNN; `embed_model_id`/`embed_dim` are recorded; a
//! model/dim swap recreates the vector space.

mod common;

use b2_core::db;
use b2_core::embed::{Embedder, FakeEmbedder};
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use common::{golden_vault_copy, SRS_ID};
use rusqlite::Connection;

fn ingest_golden(dir: &std::path::Path, embedder: &FakeEmbedder) -> Connection {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, embedder).unwrap();
    conn
}

fn meta(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .ok()
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn fake_embedder_is_deterministic() {
    let e = FakeEmbedder::new(16);
    assert_eq!(
        e.embed("hello world").unwrap(),
        e.embed("hello world").unwrap()
    );
    assert_ne!(
        e.embed("hello world").unwrap(),
        e.embed("a different chunk").unwrap()
    );
    assert_eq!(e.embed("x").unwrap().len(), 16);
}

#[test]
fn embed_batch_matches_embed_per_element() {
    // The default `embed_batch` (which the fake inherits) must be a faithful map of
    // `embed` — that equivalence is what lets the reindex path batch freely.
    let e = FakeEmbedder::new(32);
    let texts = ["alpha", "beta", "", "gamma delta"];
    let refs: Vec<&str> = texts.to_vec();
    let batched = e.embed_batch(&refs).unwrap();
    assert_eq!(batched.len(), texts.len());
    for (t, v) in texts.iter().zip(&batched) {
        assert_eq!(
            *v,
            e.embed(t).unwrap(),
            "batched row must equal single {t:?}"
        );
    }
}

#[test]
fn reindex_with_progress_reports_cumulative_and_fully_embeds() {
    use b2_core::ingest::{ingest_vault_with_progress, ReindexProgress};

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    let mut events: Vec<ReindexProgress> = Vec::new();
    ingest_vault_with_progress(
        &conn,
        &vault,
        &UlidGen,
        &FakeEmbedder::new(64),
        false,
        &mut |p| events.push(p),
    )
    .unwrap();

    // Batched embed still populates a vector for every chunk.
    let total = count(&conn, "chunks");
    assert!(total > 0);
    assert_eq!(count(&conn, "chunks_vec"), total);

    // Progress: reported, note_index in range, notes_total stable, chunks_done
    // non-decreasing and ending exactly at the chunk total.
    assert!(!events.is_empty(), "at least one batch is reported");
    let notes = count(&conn, "notes") as usize;
    assert!(events.iter().all(|e| e.notes_total == notes));
    assert!(events.iter().all(|e| (1..=notes).contains(&e.note_index)));
    for w in events.windows(2) {
        assert!(w[1].chunks_done >= w[0].chunks_done, "cumulative");
    }
    assert_eq!(events.last().unwrap().chunks_done as i64, total);
}

#[test]
fn reindex_is_incremental_and_force_reembeds_everything() {
    use b2_core::vault::Vault;

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();

    // First index: both notes are new → both embedded.
    let first = vault.reindex().unwrap();
    assert_eq!(first.indexed, 2);
    assert_eq!(first.embedded, 2, "a fresh index embeds every note");

    // Nothing changed on disk → the incremental reindex re-embeds nothing.
    let again = vault.reindex().unwrap();
    assert_eq!(again.indexed, 2);
    assert_eq!(again.embedded, 0, "unchanged notes reuse their vectors");

    // Edit exactly one note's BODY → only that note re-embeds.
    let srs = root.join("notes/spaced-repetition.md");
    let text = std::fs::read_to_string(&srs).unwrap();
    std::fs::write(&srs, format!("{text}\n\nA newly appended paragraph.")).unwrap();
    let edited = vault.reindex().unwrap();
    assert_eq!(edited.embedded, 1, "only the changed note re-embeds");

    // --force re-embeds everything regardless of change.
    let forced = vault.reindex_with_progress(true, &mut |_| {}).unwrap();
    assert_eq!(forced.embedded, 2, "force re-embeds every note");
}

#[test]
fn ingest_populates_chunks_vec_and_records_meta() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    // one vector per chunk
    assert!(count(&conn, "chunks") > 0);
    assert_eq!(count(&conn, "chunks"), count(&conn, "chunks_vec"));

    assert_eq!(
        meta(&conn, "embed_model_id").as_deref(),
        Some("fake-deterministic-v1")
    );
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("64"));
}

#[test]
fn knn_finds_the_chunk_whose_text_we_query() {
    let tmp = tempfile::TempDir::new().unwrap();
    let embedder = FakeEmbedder::new(64);
    let conn = ingest_golden(tmp.path(), &embedder);

    // pick a known chunk, query with the embedding of its own text
    let (id, text): (i64, String) = conn
        .query_row(
            "SELECT id, text FROM chunks WHERE note_b2id = ?1 ORDER BY seq LIMIT 1",
            [SRS_ID],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();

    let hits = db::vector_search(&conn, &embedder.embed(&text).unwrap(), 3).unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].0, id, "nearest chunk is the one we embedded");
    assert!(
        hits[0].1 < 1e-6,
        "exact match has ~zero distance, got {}",
        hits[0].1
    );
}

#[test]
fn reindex_yields_identical_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let embedder = FakeEmbedder::new(64);

    let vec_for_srs_seq0 = |c: &Connection| -> Vec<u8> {
        c.query_row(
            "SELECT v.embedding FROM chunks_vec v
             JOIN chunks c ON c.id = v.chunk_id
             WHERE c.note_b2id = ?1 AND c.seq = 0",
            [SRS_ID],
            |r| r.get(0),
        )
        .unwrap()
    };

    ingest_vault(&conn, &vault, &UlidGen, &embedder).unwrap();
    let before = vec_for_srs_seq0(&conn);

    // A full re-index re-embeds deterministically → byte-identical vectors.
    ingest_vault(&conn, &vault, &UlidGen, &embedder).unwrap();
    assert_eq!(before, vec_for_srs_seq0(&conn));
}

#[test]
fn changing_dim_recreates_the_vector_space_and_clears_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));
    assert!(count(&conn, "chunks_vec") > 0);

    // A model/dim swap: the only place it can be detected is meta. Vectors are
    // dropped (a full re-embed is required) and the dim is updated.
    db::ensure_embedding_space(&conn, "fake-deterministic-v1", 128).unwrap();
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("128"));
    assert_eq!(
        count(&conn, "chunks_vec"),
        0,
        "swap drops vectors; re-embed needed"
    );
}
