//! The vector store and the embedder seam (index-engine.md): reproducible KNN, the
//! recorded model identity, model swaps, and note centroids (GH #38).

mod common;

use b2_core::db;
use b2_core::embed::{Embedder, FakeEmbedder};
use b2_core::ingest::ingest_vault;
use b2_core::open;
use common::{
    count, golden_vault_copy, index_conn, ingest_golden, opened_vault, reindexed_vault, SRS_PATH,
};
use rusqlite::Connection;
use std::ops::ControlFlow;

fn meta(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .ok()
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

/// The fake is built on a production path (`Vault::open`), so zero degrades, not panics.
#[test]
fn a_zero_dimension_fake_is_clamped_to_one() {
    let e = FakeEmbedder::new(0);
    assert_eq!(e.dim(), 1);
    assert_eq!(e.embed("x").unwrap().len(), 1);
}

#[test]
fn embed_batch_matches_embed_per_element() {
    // The default `embed_batch` must equal mapping `embed`, so reindex can batch freely.
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
    use b2_core::ingest::{embed_vault, project_vault, ProjectionCtx, ReindexProgress};

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    let mut events: Vec<ReindexProgress> = Vec::new();
    let cfg = b2_core::chunk::ChunkConfig::default();
    let embedder = FakeEmbedder::new(64);
    project_vault(ProjectionCtx::new(&conn, &vault, &cfg), false).unwrap();
    embed_vault(&conn, &embedder, &mut |p| {
        events.push(p);
        ControlFlow::Continue(())
    })
    .unwrap();

    let total = count(&conn, "chunks");
    assert!(total > 0);
    assert_eq!(count(&conn, "embeddings"), total);

    // A fresh index embeds every note, so the denominator is the full note count.
    assert!(!events.is_empty(), "at least one batch is reported");
    let notes = count(&conn, "notes") as usize;
    assert!(events.iter().all(|e| e.notes_to_embed == notes));
    assert!(events
        .iter()
        .all(|e| (1..=e.notes_to_embed).contains(&e.notes_embedded)));
    assert!(events.iter().all(|e| e.note_chunks > 0));
    assert!(events.iter().all(|e| !e.note_path.is_empty()));
    for w in events.windows(2) {
        assert!(w[1].chunks_done >= w[0].chunks_done, "cumulative");
        assert!(
            w[1].notes_embedded >= w[0].notes_embedded,
            "notes_embedded is monotonic"
        );
    }
    assert_eq!(events.last().unwrap().chunks_done as i64, total);
}

#[test]
fn reindex_is_incremental_and_force_reembeds_everything() {
    use b2_core::vault::Vault;

    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());

    let first = vault.reindex().unwrap();
    assert_eq!(first.indexed, 2);
    assert_eq!(first.embedded, 2, "a fresh index embeds every note");

    let again = vault.reindex().unwrap();
    assert_eq!(again.indexed, 2);
    assert_eq!(again.embedded, 0, "unchanged notes reuse their vectors");

    let srs = root.join("notes/spaced-repetition.md");
    let text = std::fs::read_to_string(&srs).unwrap();
    std::fs::write(&srs, format!("{text}\n\nA newly appended paragraph.")).unwrap();
    let edited = vault.reindex().unwrap();
    assert_eq!(edited.embedded, 1, "only the changed note re-embeds");

    // --force re-chunks everything, but the store is content-addressed (M4), so
    // unchanged text re-embeds nothing.
    let forced = vault
        .reindex_with_progress(true, &mut |_| ControlFlow::Continue(()))
        .unwrap();
    assert_eq!(forced.indexed, 2, "force re-projects every note");
    assert_eq!(
        forced.embedded, 0,
        "identical chunk text needs no second forward pass"
    );

    // Changing the chunking moves every hash, so force does embed.
    let mut rechunked = Vault::open(&root).unwrap();
    rechunked.set_chunk_config(b2_core::chunk::ChunkConfig {
        target_tokens: 20,
        ..Default::default()
    });
    let forced = rechunked
        .reindex_with_progress(true, &mut |_| ControlFlow::Continue(()))
        .unwrap();
    assert!(
        forced.embedded > 0,
        "re-cut chunks are new text, so they embed: {forced:?}"
    );
}

#[test]
fn ingest_populates_embeddings_and_records_meta() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    assert!(count(&conn, "chunks") > 0);
    assert_eq!(count(&conn, "chunks"), count(&conn, "embeddings"));

    assert_eq!(
        meta(&conn, "embed_model_id").as_deref(),
        Some("fake-deterministic-v1")
    );
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("64"));
}

/// `note_centroids` shares the vectors' lifecycle: after any embed pass each embedded note's
/// centroid equals `centroid_of` its current vectors, and a stale one never survives.
#[test]
fn centroids_track_the_stored_chunk_vectors() {
    use b2_core::embed::{centroid_of, pack_f32};

    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());

    let conn = index_conn(&root);
    let assert_centroids_current = |conn: &Connection| {
        let notes_with_vectors: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT c.note_path) FROM chunks c
                 JOIN embeddings e ON e.text_hash = c.text_hash",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count(conn, "note_centroids"),
            notes_with_vectors,
            "one centroid per embedded note"
        );
        let mut stmt = conn
            .prepare("SELECT note_path, centroid FROM note_centroids")
            .unwrap();
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for (note, stored) in rows {
            let vectors: Vec<Vec<f32>> = db::note_chunk_vectors(conn, &note)
                .unwrap()
                .into_iter()
                .map(|(_, v)| v)
                .collect();
            let expected = centroid_of(&vectors).expect("an embedded note has vectors");
            assert_eq!(
                stored,
                pack_f32(&expected),
                "centroid of {note} summarizes its current vectors"
            );
        }
    };
    assert_centroids_current(&conn);

    let srs = root.join("notes/spaced-repetition.md");
    let text = std::fs::read_to_string(&srs).unwrap();
    std::fs::write(&srs, format!("{text}\n\nFreshly appended centroid bait.")).unwrap();
    vault.reindex().unwrap();
    assert_centroids_current(&conn);
}

#[test]
fn knn_finds_the_chunk_whose_text_we_query() {
    let tmp = tempfile::TempDir::new().unwrap();
    let embedder = FakeEmbedder::new(64);
    let conn = ingest_golden(tmp.path(), &embedder);

    // Query with the embedding of a known chunk's own text.
    let (id, text): (i64, String) = conn
        .query_row(
            "SELECT id, text FROM chunks WHERE note_path = ?1 ORDER BY seq LIMIT 1",
            [SRS_PATH],
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
            "SELECT v.vector FROM embeddings v
             JOIN chunks c ON c.text_hash = v.text_hash
             WHERE c.note_path = ?1 AND c.seq = 0",
            [SRS_PATH],
            |r| r.get(0),
        )
        .unwrap()
    };

    ingest_vault(&conn, &vault, &embedder).unwrap();
    let before = vec_for_srs_seq0(&conn);

    ingest_vault(&conn, &vault, &embedder).unwrap();
    assert_eq!(before, vec_for_srs_seq0(&conn));
}

#[test]
fn changing_dim_recreates_the_vector_space_and_clears_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));
    assert!(count(&conn, "embeddings") > 0);

    // A swap is detected via meta; vectors and centroids are dropped.
    db::ensure_embedding_space(&conn, "fake-deterministic-v1", 128).unwrap();
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("128"));
    assert_eq!(
        count(&conn, "embeddings"),
        0,
        "swap drops vectors; re-embed needed"
    );
    assert_eq!(
        count(&conn, "note_centroids"),
        0,
        "swap drops centroids with the vectors they summarize"
    );
}

/// Concurrent embed passes leave one intact vector space (ADR-0021, C1). The GH #55 lock is
/// the CLI's alone, so a desktop and a CLI reindex really overlap. The quiet failure is a
/// late `DROP` leaving a "complete" embed over a half-empty space.
#[test]
fn concurrent_embed_passes_leave_one_intact_vector_space() {
    use std::sync::{Arc, Barrier};

    const ROUNDS: usize = 3;
    const PASSES: i64 = 8;
    /// Distinct text per pass, so eight writes are eight rows, not one (M4).
    fn text_of(seq: i64) -> String {
        format!("chunk text {seq}")
    }

    for round in 0..ROUNDS {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("b2.sqlite");
        // Projected, unembedded: one note, one chunk per racing pass.
        {
            let conn = open(&db_path).unwrap();
            conn.execute(
                "INSERT INTO notes(path, body_hash, indexed_at)
                 VALUES ('n.md', 'hash', '2026-07-26T00:00:00Z')",
                [],
            )
            .unwrap();
            for seq in 0..PASSES {
                conn.execute(
                    "INSERT INTO chunks
                       (id, note_path, seq, char_start, char_end, token_count, text, text_hash)
                     VALUES (?1, 'n.md', ?1, 0, 1, 1, ?2, ?3)",
                    rusqlite::params![seq, text_of(seq), db::text_hash(&text_of(seq))],
                )
                .unwrap();
            }
        }

        let start = Arc::new(Barrier::new(PASSES as usize));
        let passes: Vec<_> = (0..PASSES)
            .map(|chunk_id| {
                let db_path = db_path.clone();
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    let conn = open(&db_path).unwrap();
                    start.wait();
                    // What `embed_vault` does.
                    db::ensure_embedding_space(&conn, "fake-deterministic-v1", 128)?;
                    db::set_vector(&conn, &db::text_hash(&text_of(chunk_id)), &[0.5; 128])
                })
            })
            .collect();
        for pass in passes {
            pass.join()
                .unwrap()
                .unwrap_or_else(|e| panic!("round {round}: a concurrent embed pass failed: {e}"));
        }

        let conn = open(&db_path).unwrap();
        assert_eq!(
            count(&conn, "embeddings"),
            PASSES,
            "round {round}: a rebuild dropped vectors another pass had already written"
        );
    }
}

/// Before any reindex, `open` never touches the vector space, so a vault can hold vectors
/// from another model. Ranking them would be silently wrong, so `search` refuses.
#[test]
fn search_fails_fast_on_a_model_swap_and_a_reindex_heals_it() {
    use b2_core::vault::Vault;
    use b2_core::Error;

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);

    let vault = Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(64))).unwrap();
    vault.reindex().unwrap();
    assert!(!vault.search("forgetting", 5).unwrap().is_empty());
    drop(vault);

    // A different dimension is a model swap to the recorded identity.
    let swapped = Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(128))).unwrap();
    let err = swapped.search("forgetting", 5).unwrap_err();
    assert!(
        matches!(err, Error::ModelMismatch { .. }),
        "a swap must fail fast, not rank on incomparable vectors: {err:?}"
    );

    // A misconfigured model can never wipe a vault's embeddings.
    let conn = index_conn(&root);
    assert!(count(&conn, "embeddings") > 0, "vectors survive the reopen");
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("64"));
    drop(conn);

    // The documented fix.
    swapped.reindex().unwrap();
    assert!(!swapped.search("forgetting", 5).unwrap().is_empty());
    let conn = index_conn(&root);
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("128"));
}
