//! The projection/embedding split (planning/specs/projection-embedding-split.md §8
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
    // against `chunks_vec` (which does not exist yet), this would error.
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
        "projection must not create chunks_vec"
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
    assert_eq!(count(&conn, "chunks_vec"), count(&conn, "chunks"));

    // Second embed: the DB-derived pending set is empty → fills 0, changes nothing.
    let second = embed_vault(&conn, &embedder, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!second.cancelled);
    assert!(second.embedded.is_empty(), "a second embed fills nothing");
    assert_eq!(count(&conn, "chunks_vec"), count(&conn, "chunks"));
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
                "SELECT c.text, v.embedding FROM chunks c
                 JOIN chunks_vec v ON v.chunk_id = c.id
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
