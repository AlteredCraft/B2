//! The projection/embedding split (planning/specs/completed/projection-embedding-split.md §8
//! Step 1, plus Step 2's keyword-first fallback): `project` alone builds the complete
//! keyword + graph index with **no** vectors and no embedding space; `embed` fills
//! exactly the DB-derived missing vectors; and project→embed is **observably**
//! equivalent to the fused `reindex` (counts, chunk text, text→vector, edges — never
//! rowid equality, per §7.1). Model-free throughout (fake embedder).

mod common;

use b2_core::db;
use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::{embed_vault, project_vault};
use b2_core::open;
use b2_core::vault::Vault;
use common::golden_vault_copy;
use rusqlite::Connection;
use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn project_only_builds_keyword_graph_index_with_no_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    // Projection alone: no embedder anywhere near the call. If it issued any query
    // against `embeddings` (which does not exist yet), this would error.
    let outcome = project_vault(&conn, &vault_dir, &UlidGen, false).unwrap();
    assert_eq!(outcome.notes.len(), 2);

    // The keyword + graph index is complete…
    let chunks = count(&conn, "chunks");
    assert!(chunks > 0);
    assert_eq!(
        count(&conn, "chunks_fts"),
        chunks,
        "FTS mirrors every chunk"
    );
    assert!(count(&conn, "edges") > 0, "typed graph projected");
    // …and the embedding space was never created (that is the embed pass's job).
    assert!(
        !db::embedding_space_exists(&conn).unwrap(),
        "projection must not create the vector tables"
    );
}

#[test]
fn embed_fills_exactly_the_missing_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let embedder = FakeEmbedder::new(64);

    project_vault(&conn, &vault_dir, &UlidGen, false).unwrap();

    // First embed: every chunk lacks a vector → both notes embed, space is full.
    let first = embed_vault(&conn, &embedder, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!first.cancelled);
    assert_eq!(first.embedded.len(), 2, "both projected notes embed");
    assert_eq!(count(&conn, "embeddings"), count(&conn, "chunks"));

    // Second embed: the DB-derived pending set is empty → fills 0, changes nothing.
    let second = embed_vault(&conn, &embedder, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!second.cancelled);
    assert!(second.embedded.is_empty(), "a second embed fills nothing");
    assert_eq!(count(&conn, "embeddings"), count(&conn, "chunks"));
}

/// The observable projection of an index: note count, `(note, seq) → chunk text`,
/// `chunk text → vector bytes`, and the full edge rows — everything §7.1 calls
/// observable, and deliberately **not** chunk rowids.
#[derive(Debug, PartialEq)]
struct Observable {
    notes: i64,
    chunk_texts: Vec<(String, i64, String)>,
    text_to_vector: Vec<(String, Vec<u8>)>,
    edges: Vec<EdgeKey>,
}

/// An edge's identity + typing, minus the internal columns: `(id, src, dst, type,
/// origin, occurrence)`.
type EdgeKey = (String, String, Option<String>, String, String, i64);

fn observable_state(root: &Path) -> Observable {
    let conn = open(&root.join(".b2").join("b2.sqlite")).unwrap();
    let notes = count(&conn, "notes");
    let chunk_texts = {
        let mut stmt = conn
            .prepare("SELECT note_b2id, seq, text FROM chunks ORDER BY note_b2id, seq")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    let text_to_vector = {
        let mut stmt = conn
            .prepare(
                "SELECT c.text, v.vector FROM chunks c
                 JOIN embeddings v ON v.chunk_id = c.id
                 ORDER BY c.note_b2id, c.seq",
            )
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    let edges = {
        let mut stmt = conn
            .prepare(
                "SELECT id, src_id, dst_id, type, origin, occurrence_index
                 FROM edges ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    Observable {
        notes,
        chunk_texts,
        text_to_vector,
        edges,
    }
}

#[test]
fn project_then_embed_matches_reindex() {
    let tmp = tempfile::TempDir::new().unwrap();
    let split_root = tmp.path().join("split");
    let fused_root = tmp.path().join("fused");
    golden_vault_copy(&split_root);
    golden_vault_copy(&fused_root);

    // One fresh copy through the split façade ops…
    let split = Vault::open(&split_root).unwrap();
    let p = split.project(false).unwrap();
    let e = split.embed(&mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!e.cancelled);

    // …a sibling fresh copy through the composed reindex.
    let fused = Vault::open(&fused_root).unwrap();
    let r = fused.reindex().unwrap();

    // The reports agree…
    assert_eq!(
        (p.indexed, p.stamped, e.embedded),
        (r.indexed, r.stamped, r.embedded)
    );

    // …and so does every observable aspect of the two indexes (§7.1).
    drop(split);
    drop(fused);
    let split_obs = observable_state(&split_root);
    let fused_obs = observable_state(&fused_root);
    assert_eq!(split_obs.notes, fused_obs.notes);
    assert_eq!(
        split_obs.chunk_texts, fused_obs.chunk_texts,
        "identical chunk text per (note, seq)"
    );
    assert_eq!(
        split_obs.text_to_vector, fused_obs.text_to_vector,
        "identical text→vector map"
    );
    assert_eq!(split_obs.edges, fused_obs.edges, "identical typed graph");
}

// --- resilience: one unreadable file must never abort the whole reindex ----------
//
// A real vault holds the odd non-UTF-8 or unreadable `.md`. Before this, a single such
// file made `fs::read_to_string` fail and took the entire projection (and thus the
// reindex) down with a generic error. The pass must skip it and index everything else.

#[test]
fn project_skips_unreadable_file_and_indexes_the_rest() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    // A `.md` file that is not valid UTF-8 (a stray 0xFF byte). `read_to_string` fails
    // with `InvalidData` on it — the exact shape a large primary vault trips over.
    fs::write(vault_dir.join("bad.md"), [b'#', b' ', 0xff, b'\n']).unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    let outcome = project_vault(&conn, &vault_dir, &UlidGen, false).unwrap();

    // Both readable notes projected; the bad one is skipped, not fatal.
    assert_eq!(outcome.notes.len(), 2, "both readable notes still index");
    assert_eq!(
        outcome.skipped.len(),
        1,
        "the bad file is skipped, not fatal"
    );
    assert_eq!(outcome.skipped[0].path, "bad.md");
    assert_eq!(outcome.skipped[0].reason, "not valid UTF-8 text");
    // The good notes' keyword index is intact.
    assert!(count(&conn, "chunks") > 0);
}

#[test]
fn reindex_reconciles_a_path_taken_over_by_another_note() {
    // A file renamed/replaced outside `b2 mv` can hand its path to a *different* b2id,
    // leaving the prior holder's row stale. Projection must reconcile (drop the stale
    // holder) rather than abort on `notes.path` UNIQUE — an incremental reindex must
    // equal a from-scratch rebuild (index-engine's core invariant). Regression for the
    // `UNIQUE constraint failed: notes.path` reindex crash.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("foo.md"),
        "---\nb2id: 01AAAAAAAAAAAAAAAAAAAAAAAA\n---\n\nAlpha body.\n",
    )
    .unwrap();
    fs::write(
        root.join("bar.md"),
        "---\nb2id: 01BBBBBBBBBBBBBBBBBBBBBBBB\n---\n\nBeta body.\n",
    )
    .unwrap();

    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.reindex().unwrap().indexed, 2);

    // Out-of-b2 edit: delete foo.md, rename bar.md → foo.md. foo.md now carries B, and
    // the index's (A, foo.md) row is stale — its path is taken over by B.
    fs::remove_file(root.join("foo.md")).unwrap();
    fs::rename(root.join("bar.md"), root.join("foo.md")).unwrap();

    // The incremental reindex must succeed and converge on the current truth: one note,
    // b2id B at foo.md — byte-identical to what a from-scratch rebuild would produce.
    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 1);
    let notes = vault.list_notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].path, "foo.md");
    assert_eq!(notes[0].b2id, "01BBBBBBBBBBBBBBBBBBBBBBBB");
}

#[test]
fn reindex_completes_and_reports_skipped_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    fs::write(root.join("bad.md"), [0xff, 0xfe, b'x']).unwrap();
    let vault = Vault::open(&root).unwrap();

    // The composed reindex succeeds (no abort) and reports the skip truthfully.
    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 2);
    assert_eq!(report.embedded, 2);
    assert!(!report.cancelled);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].path, "bad.md");

    // …and the vault is fully usable: keyword search over the good notes still answers.
    assert!(!vault.search("forgetting", 5).unwrap().is_empty());
}

// --- Step 2: a projected (unembedded) vault is a usable vault (§5 / §7.3) --------

#[test]
fn projected_vault_answers_keyword_search_and_similar_degrades_empty() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    vault.project(false).unwrap();

    // Keyword search answers before any embedding — BM25-only, no model touched.
    let hits = vault.search("forgetting", 10).unwrap();
    assert!(
        !hits.is_empty(),
        "keyword search is live after project alone"
    );
    assert_eq!(hits[0].path, "notes/spaced-repetition.md");
    assert!(hits[0].snippet.contains("forgetting"));
    assert!(hits[0].score > 0.0);

    // The graph resolves, and discovery degrades to empty — never an error.
    assert!(!vault.neighbors("concepts/memory").unwrap().is_empty());
    assert!(
        vault.similar("concepts/memory", 5).unwrap().is_empty(),
        "similar waits for vectors, honestly empty"
    );
}

/// The honest "N/M embedded" coverage read (#26): 0/M while projected-but-unembedded,
/// M/M once fully embedded, and a precise partial fraction when a projected note still
/// lacks vectors — the signal an adapter flags "keyword-only for now" from. Model-free.
#[test]
fn embed_status_reports_the_coverage_fraction() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();

    // Projected but unembedded: every note counts toward the total, none is embedded, and
    // the embedding space doesn't exist yet — reads as 0/M, no error (the query short-
    // circuits before touching the absent `embeddings` table).
    vault.project(false).unwrap();
    let s = vault.embed_status().unwrap();
    assert_eq!(
        (s.embedded, s.total),
        (0, 2),
        "projected-but-unembedded: 0/M"
    );

    // After a full embed: every note is embedded — M/M, semantic ranking complete.
    vault.embed(&mut |_| ControlFlow::Continue(())).unwrap();
    let s = vault.embed_status().unwrap();
    assert_eq!((s.embedded, s.total), (2, 2), "fully embedded: M/M");

    // A newly added note (projected, not yet embedded) makes coverage partial — the
    // precise fraction #26 surfaces, distinct from the binary "is a model installed".
    // The two unchanged notes keep their vectors (project never re-embeds them).
    fs::write(
        root.join("fresh.md"),
        "---\nb2id: 01CCCCCCCCCCCCCCCCCCCCCCCC\n---\n\nA fresh unembedded note.\n",
    )
    .unwrap();
    vault.project(false).unwrap();
    let s = vault.embed_status().unwrap();
    assert_eq!(
        (s.embedded, s.total),
        (2, 3),
        "one note pending vectors: N/M partial"
    );
}
