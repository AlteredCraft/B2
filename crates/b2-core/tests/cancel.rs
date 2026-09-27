//! Cooperative cancel of a reindex at a batch boundary leaves a consistent, resumable
//! index: keyword search and graph complete, a prefix of vectors, and a re-run embeds
//! exactly the remainder.

mod common;

use b2_core::chunk::ChunkConfig;
use b2_core::embed::FakeEmbedder;
use b2_core::ingest::{embed_vault, project_vault, ProjectionCtx};
use b2_core::open;
use common::{count, golden_vault_copy, opened_vault};
use std::ops::ControlFlow;

#[test]
fn cancel_after_first_batch_leaves_a_consistent_resumable_index() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let embedder = FakeEmbedder::new(64);

    // The golden notes are one batch each, so this embeds the first note only.
    let cfg = ChunkConfig::default();
    let ctx = ProjectionCtx::new(&conn, &vault, &cfg);
    project_vault(ctx, false).unwrap();
    let outcome = embed_vault(&conn, &embedder, &mut |_| ControlFlow::Break(())).unwrap();
    assert!(outcome.cancelled, "the run reports itself cancelled");

    // §5.1: keyword and graph are complete at the cancel point.
    let chunks = count(&conn, "chunks");
    assert!(chunks > 0);
    assert_eq!(
        count(&conn, "chunks_fts"),
        chunks,
        "FTS complete for every chunk"
    );
    assert!(
        count(&conn, "edges") > 0,
        "typed graph complete after cancel"
    );

    let vecs_after_cancel = count(&conn, "embeddings");
    assert!(
        vecs_after_cancel > 0 && vecs_after_cancel < chunks,
        "a prefix embedded, the remainder pending: {vecs_after_cancel}/{chunks}"
    );

    // §5.2: resume embeds exactly the remainder.
    project_vault(ctx, false).unwrap();
    let resumed = embed_vault(&conn, &embedder, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!resumed.cancelled);
    assert_eq!(
        count(&conn, "embeddings"),
        chunks,
        "resume fills the remaining vectors — the index is now fully embedded"
    );
}

#[test]
fn facade_report_is_honest_about_a_cancelled_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = opened_vault(tmp.path());

    // The counts describe the partial work truthfully (§3).
    let partial = vault
        .reindex_with_progress(false, &mut |_| ControlFlow::Break(()))
        .unwrap();
    assert!(partial.cancelled);
    assert_eq!(partial.indexed, 2, "every note is still projected");
    assert!(
        partial.embedded >= 1 && partial.embedded < partial.indexed,
        "a prefix embedded: {}/{}",
        partial.embedded,
        partial.indexed
    );

    let finished = vault.reindex().unwrap();
    assert!(!finished.cancelled);
    assert_eq!(
        finished.embedded,
        partial.indexed - partial.embedded,
        "the re-run embeds only the notes the cancel left unfinished"
    );

    let noop = vault.reindex().unwrap();
    assert_eq!(noop.embedded, 0, "nothing left to embed after resume");
    assert!(!noop.cancelled);
}
