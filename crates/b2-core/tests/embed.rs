//! Step 3 — the vector store + the embedder seam
//! (index-engine.md): a deterministic fake embedder
//! produces reproducible KNN; `embed_model_id`/`embed_dim` are recorded; a
//! model/dim swap recreates the vector space; note centroids (discovery's coarse
//! stage, #38) track the stored chunk vectors.

mod common;

use b2_core::db;
use b2_core::embed::{Embedder, FakeEmbedder};
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use common::{count, golden_vault_copy, ingest_golden, SRS_ID};
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
        &b2_core::chunk::ChunkConfig::default(),
        &FakeEmbedder::new(64),
        false,
        &mut |p| {
            events.push(p);
            ControlFlow::Continue(())
        },
    )
    .unwrap();

    // Batched embed still populates a vector for every chunk.
    let total = count(&conn, "chunks");
    assert!(total > 0);
    assert_eq!(count(&conn, "embeddings"), total);

    // Progress: reported, per-note fields populated, notes_embedded within the
    // stable denominator and monotonic, chunks_done non-decreasing and ending
    // exactly at the chunk total. A fresh index embeds every note, so the "notes to
    // embed" denominator equals the full note count here.
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
    let forced = vault
        .reindex_with_progress(true, &mut |_| ControlFlow::Continue(()))
        .unwrap();
    assert_eq!(forced.embedded, 2, "force re-embeds every note");
}

#[test]
fn ingest_populates_embeddings_and_records_meta() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    // one vector per chunk
    assert!(count(&conn, "chunks") > 0);
    assert_eq!(count(&conn, "chunks"), count(&conn, "embeddings"));

    assert_eq!(
        meta(&conn, "embed_model_id").as_deref(),
        Some("fake-deterministic-v1")
    );
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("64"));
}

/// `note_centroids` is derived data with the vectors' own lifecycle: after any embed
/// pass, every note with stored vectors carries a centroid, and it equals
/// `centroid_of` over exactly those vectors — including after a body edit re-chunks
/// and re-embeds the note (the stale centroid must not survive).
#[test]
fn centroids_track_the_stored_chunk_vectors() {
    use b2_core::embed::{centroid_of, pack_f32};
    use b2_core::vault::Vault;

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();

    let conn = open(&root.join(".b2").join("b2.sqlite")).unwrap();
    let assert_centroids_current = |conn: &Connection| {
        let notes_with_vectors: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT c.note_b2id) FROM chunks c
                 JOIN embeddings e ON e.chunk_id = c.id",
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
            .prepare("SELECT note_b2id, centroid FROM note_centroids")
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

    // Edit one note's body → re-project + re-embed → its centroid must follow.
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
            "SELECT v.vector FROM embeddings v
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
    assert!(count(&conn, "embeddings") > 0);

    // A model/dim swap: the only place it can be detected is meta. Vectors are
    // dropped (a full re-embed is required) and the dim is updated. Centroids share
    // the vectors' lifecycle, so the swap empties them too.
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

/// Concurrent embed passes leave **one** intact vector space (#114, invariant C1).
///
/// `ensure_embedding_space` is the file's second drop-and-rebuild, and it had the same
/// defect as the schema migration: read whether the space matches, then drop and
/// recreate — unwrapped and unserialized. Two embed passes can genuinely overlap, and
/// nothing structural prevents it: the `#55` advisory lock is taken by `b2-cli` alone,
/// so a desktop reindex and a `b2 reindex` are exactly this test.
///
/// Where the migration race needed twenty rounds to bite, this one is near-certain —
/// every caller runs the batch, so the window is four statements wide rather than a
/// stale-version read. Measured against the unfixed engine: **70 of 80 workers errored**
/// (`table embeddings already exists`) and **every** round lost vectors, in each of three
/// runs. Both halves are asserted here, and losing vectors is the quiet one — a `DROP`
/// landing after another pass has started writing takes the vectors it already wrote
/// with it, leaving an index that reports a complete embed over a half-empty space.
#[test]
fn concurrent_embed_passes_leave_one_intact_vector_space() {
    use std::sync::{Arc, Barrier};

    const ROUNDS: usize = 3;
    const PASSES: i64 = 8;

    for round in 0..ROUNDS {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("b2.sqlite");
        // A projected-but-unembedded index: one note, one chunk per racing pass.
        {
            let conn = open(&db_path).unwrap();
            conn.execute(
                "INSERT INTO notes(b2id, path, type, body_hash, indexed_at)
                 VALUES ('01NOTE', 'n.md', 'note', 'hash', '2026-07-26T00:00:00Z')",
                [],
            )
            .unwrap();
            for seq in 0..PASSES {
                conn.execute(
                    "INSERT INTO chunks(id, note_b2id, seq, char_start, char_end, token_count, text)
                     VALUES (?1, '01NOTE', ?1, 0, 1, 1, 'text')",
                    [seq],
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
                    // What `embed_vault` does: ensure the space, then write vectors into it.
                    db::ensure_embedding_space(&conn, "fake-deterministic-v1", 128)?;
                    db::set_chunk_vector(&conn, chunk_id, &[0.5; 128])
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

/// The other half of the model-swap contract. `ensure_embedding_space` (above)
/// covers what a *reindex* does — drop the stale vectors and re-embed. This covers
/// what happens **before** anyone reindexes: `open` deliberately never touches the
/// vector space, so a vault can sit with vectors from one model while a different
/// one is configured. Ranking those stored vectors against a query vector from the
/// new model would be silently wrong, so `search` refuses instead.
#[test]
fn search_fails_fast_on_a_model_swap_and_a_reindex_heals_it() {
    use b2_core::vault::Vault;
    use b2_core::Error;

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);

    // Index the vault under a 64-dim embedder.
    let vault = Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(64))).unwrap();
    vault.reindex().unwrap();
    assert!(!vault.search("forgetting", 5).unwrap().is_empty());
    drop(vault);

    // Reopen with a different dimension — a model swap, as far as the recorded
    // identity is concerned.
    let swapped = Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(128))).unwrap();
    let err = swapped.search("forgetting", 5).unwrap_err();
    assert!(
        matches!(err, Error::ModelMismatch { .. }),
        "a swap must fail fast, not rank on incomparable vectors: {err:?}"
    );

    // `open` left the stored vectors alone (so a misconfigured model can never wipe
    // a vault's embeddings) — the refusal is a query-time guard, not a migration.
    let conn = open(&root.join(".b2").join("b2.sqlite")).unwrap();
    assert!(count(&conn, "embeddings") > 0, "vectors survive the reopen");
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("64"));
    drop(conn);

    // The documented fix: reindex re-creates the space at the new dimension.
    swapped.reindex().unwrap();
    assert!(!swapped.search("forgetting", 5).unwrap().is_empty());
    let conn = open(&root.join(".b2").join("b2.sqlite")).unwrap();
    assert_eq!(meta(&conn, "embed_dim").as_deref(), Some("128"));
}
