//! Cooperative-cancel of a reindex (planning/specs/completed/async-indexing.md §3): the embed
//! phase can be stopped at a batch boundary via `ControlFlow::Break`, and the result
//! is a **consistent, resumable** index — every note has chunks + FTS + edges (keyword
//! search + graph complete), only a *prefix* has vectors, and an incremental re-run
//! embeds exactly the remainder (`incremental ≡ eventual full`). Model-free: the fake
//! embedder makes cancel-after-N-batches deterministic.

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault_with_progress;
use b2_core::open;
use b2_core::vault::Vault;
use common::golden_vault_copy;
use rusqlite::Connection;
use std::ops::ControlFlow;

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn cancel_after_first_batch_leaves_a_consistent_resumable_index() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let embedder = FakeEmbedder::new(64);

    // Break at the very first embed batch. The golden vault's notes are small (one
    // batch each), so this embeds the first note only.
    let outcome =
        ingest_vault_with_progress(&conn, &vault, &UlidGen, &embedder, false, &mut |_| {
            ControlFlow::Break(())
        })
        .unwrap();
    assert!(outcome.cancelled, "the run reports itself cancelled");

    // §5.1 — keyword + graph are COMPLETE at the cancel point: every note has chunks,
    // FTS mirrors them, and every authored edge is projected (Phase 2 runs post-cancel).
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

    // …only VECTORS are partial: a prefix of chunks embedded, the rest pending.
    let vecs_after_cancel = count(&conn, "embeddings");
    assert!(
        vecs_after_cancel > 0 && vecs_after_cancel < chunks,
        "a prefix embedded, the remainder pending: {vecs_after_cancel}/{chunks}"
    );

    // §5.2 — resume: an ordinary (uncancelled) reindex embeds exactly the remainder and
    // finishes. No corruption, no double-work.
    let resumed =
        ingest_vault_with_progress(&conn, &vault, &UlidGen, &embedder, false, &mut |_| {
            ControlFlow::Continue(())
        })
        .unwrap();
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
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();

    // Cancel at the first batch: fewer notes embed than are indexed, and `cancelled`
    // is set — the counts describe the partial work truthfully (§3).
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

    // Re-running to completion embeds exactly the remainder and is not cancelled.
    let finished = vault.reindex().unwrap();
    assert!(!finished.cancelled);
    assert_eq!(
        finished.embedded,
        partial.indexed - partial.embedded,
        "the re-run embeds only the notes the cancel left unfinished"
    );

    // And a second clean reindex now re-embeds nothing (fully consistent, incremental).
    let noop = vault.reindex().unwrap();
    assert_eq!(noop.embedded, 0, "nothing left to embed after resume");
    assert!(!noop.cancelled);
}
