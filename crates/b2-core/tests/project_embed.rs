//! The projection/embedding split (index-engine.md): `project` builds the keyword and
//! graph index with no vectors, `embed` fills exactly the missing ones, and the two are
//! observably equivalent to the fused `reindex` (§7.1: never rowid equality).

mod common;

use b2_core::chunk::ChunkConfig;
use b2_core::db;
use b2_core::embed::FakeEmbedder;
use b2_core::ingest::{
    embed_vault, ingest_file, ingest_vault, project_file, project_vault, EmbedCtx, ProjectionCtx,
};
use b2_core::open;
use b2_core::vault::Vault;
use common::{count, golden_vault_copy, index_conn, opened_vault};
use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

#[test]
fn project_only_builds_keyword_graph_index_with_no_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    // No embedder: any query against the not-yet-created `embeddings` would error.
    let cfg = ChunkConfig::default();
    let outcome = project_vault(ProjectionCtx::new(&conn, &vault_dir, &cfg), false).unwrap();
    assert_eq!(outcome.notes.len(), 2);

    let chunks = count(&conn, "chunks");
    assert!(chunks > 0);
    assert_eq!(
        count(&conn, "chunks_fts"),
        chunks,
        "FTS mirrors every chunk"
    );
    assert!(count(&conn, "edges") > 0, "typed graph projected");
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
    let cfg = ChunkConfig::default();

    project_vault(ProjectionCtx::new(&conn, &vault_dir, &cfg), false).unwrap();

    let first = embed_vault(&conn, &embedder, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!first.cancelled);
    assert_eq!(first.embedded.len(), 2, "both projected notes embed");
    assert_eq!(count(&conn, "embeddings"), count(&conn, "chunks"));

    let second = embed_vault(&conn, &embedder, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!second.cancelled);
    assert!(second.embedded.is_empty(), "a second embed fills nothing");
    assert_eq!(count(&conn, "embeddings"), count(&conn, "chunks"));
}

/// Everything §7.1 calls observable in an index, and not chunk rowids.
#[derive(Debug, PartialEq)]
struct Observable {
    notes: i64,
    chunk_texts: Vec<(String, i64, String)>,
    text_to_vector: Vec<(String, Vec<u8>)>,
    edges: Vec<EdgeKey>,
}

/// `(id, src, dst, type, origin, occurrence)`.
type EdgeKey = (String, String, Option<String>, String, String, i64);

fn observable_state(root: &Path) -> Observable {
    let conn = index_conn(root);
    let notes = count(&conn, "notes");
    let chunk_texts = {
        let mut stmt = conn
            .prepare("SELECT note_path, seq, text FROM chunks ORDER BY note_path, seq")
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
                 JOIN embeddings v ON v.text_hash = c.text_hash
                 ORDER BY c.note_path, c.seq",
            )
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    let edges = {
        let mut stmt = conn
            .prepare(
                "SELECT id, src_path, dst_path, type, origin, occurrence_index
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

    let split = Vault::open(&split_root).unwrap();
    let p = split.project(false).unwrap();
    let e = split.embed(&mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!e.cancelled);

    let fused = Vault::open(&fused_root).unwrap();
    let r = fused.reindex().unwrap();

    assert_eq!((p.indexed, e.embedded), (r.indexed, r.embedded));

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
// A real vault holds the odd non-UTF-8 `.md`; the pass skips it and indexes the rest.

#[test]
fn project_skips_unreadable_file_and_indexes_the_rest() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    // A stray 0xFF byte: not valid UTF-8.
    fs::write(vault_dir.join("bad.md"), [b'#', b' ', 0xff, b'\n']).unwrap();
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let cfg = ChunkConfig::default();

    let outcome = project_vault(ProjectionCtx::new(&conn, &vault_dir, &cfg), false).unwrap();

    assert_eq!(outcome.notes.len(), 2, "both readable notes still index");
    assert_eq!(
        outcome.skipped.len(),
        1,
        "the bad file is skipped, not fatal"
    );
    assert_eq!(outcome.skipped[0].path, "bad.md");
    assert_eq!(outcome.skipped[0].reason, "not valid UTF-8 text");
    assert!(count(&conn, "chunks") > 0);
}

/// A file replaced out of band is simply that path's note now: the path is the identity
/// (GH #170), so there is no `UNIQUE` conflict to crash on.
#[test]
fn reindex_reconciles_a_path_taken_over_by_another_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, "foo.md", "Alpha body.");
    write_note(&root, "bar.md", "Beta body about tidal pools.");

    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.reindex().unwrap().indexed, 2);

    fs::remove_file(root.join("foo.md")).unwrap();
    fs::rename(root.join("bar.md"), root.join("foo.md")).unwrap();

    // Converges on one note at foo.md carrying bar's content (S3).
    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 1);
    let notes = vault.list_notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].path, "foo.md");
    assert!(
        vault.read("foo.md").unwrap().body.contains("tidal pools"),
        "the surviving row describes the file that is actually there"
    );
}

/// Write a minimal note at `root/name`, byte-identical across roots so indexes compare.
fn write_note(root: &Path, name: &str, body: &str) {
    fs::write(root.join(name), format!("---\n---\n\n{body}\n")).unwrap();
}

#[test]
fn reindex_prunes_a_deleted_note_like_a_full_rebuild() {
    // A note deleted outside b2 must not linger as a ghost row (GH #31). foo links to
    // bar so the deletion also re-dangles an inbound edge.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, "foo.md", "Alpha body. See [[bar]].");
    write_note(&root, "bar.md", "Beta body about tidal pools.");
    write_note(&root, "baz.md", "Gamma body, unlinked.");

    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.reindex().unwrap().indexed, 3);

    fs::remove_file(root.join("bar.md")).unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 2);
    assert_eq!(report.notes_pruned, 1, "the ghost row is pruned");

    // Gone from the listing, search and discovery.
    let notes = vault.list_notes().unwrap();
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|n| n.path != "bar.md"));
    assert!(vault
        .search("tidal", 10)
        .unwrap()
        .iter()
        .all(|h| h.path != "bar.md"));
    let candidates = vault.similar("foo.md", 5).unwrap();
    assert!(candidates.iter().any(|c| c.path == "baz.md"));
    assert!(candidates.iter().all(|c| c.path != "bar.md"));
    drop(vault);

    let conn = index_conn(&root);
    assert_eq!(count(&conn, "chunks_fts"), count(&conn, "chunks"));
    // foo's `[[bar]]` re-dangles, staying visible for repair (GH #12).
    let (dst_path, dst_path_raw): (Option<String>, String) = conn
        .query_row(
            "SELECT dst_path, dst_path_raw FROM edges WHERE src_path = ?1",
            ["foo.md"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(dst_path, None, "the inbound edge re-dangles");
    assert_eq!(dst_path_raw, "bar");
    drop(conn);

    // The incrementally reconciled index equals a rebuild of the same final vault.
    let fresh_root = tmp.path().join("fresh");
    fs::create_dir_all(&fresh_root).unwrap();
    write_note(&fresh_root, "foo.md", "Alpha body. See [[bar]].");
    write_note(&fresh_root, "baz.md", "Gamma body, unlinked.");
    Vault::open(&fresh_root).unwrap().reindex().unwrap();
    assert_eq!(
        observable_state(&root),
        observable_state(&fresh_root),
        "incremental-after-delete == full rebuild"
    );
}

#[test]
fn prune_spares_a_file_skipped_as_unreadable() {
    // The GH #31 carve-out: a file still on disk but unreadable is not "deleted".
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, "foo.md", "Alpha body.");
    write_note(&root, "bar.md", "Beta body.");

    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.reindex().unwrap().indexed, 2);

    fs::write(root.join("bar.md"), [0xff, 0xfe, b'x']).unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].path, "bar.md");
    assert_eq!(report.notes_pruned, 0, "a skipped file is never pruned");
    let notes = vault.list_notes().unwrap();
    assert_eq!(notes.len(), 2, "the unreadable file keeps its index row");
    assert!(notes.iter().any(|n| n.path == "bar.md"));
}

#[test]
fn single_note_ingest_never_prunes() {
    // Only `project_vault` sees every file, so only it may decide a note is gone.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    fs::create_dir_all(&vault_dir).unwrap();
    write_note(&vault_dir, "foo.md", "Alpha body.");
    write_note(&vault_dir, "bar.md", "Beta body.");
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    let embedder = FakeEmbedder::new(64);
    ingest_vault(&conn, &vault_dir, &embedder).unwrap();
    assert_eq!(count(&conn, "notes"), 2);

    fs::remove_file(vault_dir.join("bar.md")).unwrap();

    let cfg = ChunkConfig::default();
    let proj = ProjectionCtx::new(&conn, &vault_dir, &cfg);
    project_file(proj, "foo.md").unwrap();
    assert_eq!(count(&conn, "notes"), 2, "project_file prunes nothing");
    ingest_file(EmbedCtx::new(proj, &embedder), "foo.md").unwrap();
    assert_eq!(count(&conn, "notes"), 2, "ingest_file prunes nothing");

    let outcome = project_vault(ProjectionCtx::new(&conn, &vault_dir, &cfg), false).unwrap();
    assert_eq!(outcome.notes_pruned, 1);
    assert_eq!(count(&conn, "notes"), 1);
}

#[test]
fn reindex_completes_and_reports_skipped_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    fs::write(root.join("bad.md"), [0xff, 0xfe, b'x']).unwrap();
    let vault = Vault::open(&root).unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 2);
    assert_eq!(report.embedded, 2);
    assert!(!report.cancelled);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].path, "bad.md");

    assert!(!vault.search("forgetting", 5).unwrap().is_empty());
}

// --- Step 2: a projected (unembedded) vault is a usable vault (§5 / §7.3) --------

#[test]
fn projected_vault_answers_keyword_search_and_similar_degrades_empty() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = opened_vault(tmp.path());
    vault.project(false).unwrap();

    let hits = vault.search("forgetting", 10).unwrap();
    assert!(
        !hits.is_empty(),
        "keyword search is live after project alone"
    );
    assert_eq!(hits[0].path, "notes/spaced-repetition.md");
    assert!(hits[0].snippet.contains("forgetting"));
    assert!(hits[0].score > 0.0);

    // Discovery degrades to empty, never an error.
    assert!(!vault.neighbors("concepts/memory").unwrap().is_empty());
    assert!(
        vault.similar("concepts/memory", 5).unwrap().is_empty(),
        "similar waits for vectors, honestly empty"
    );
}

/// The "N/M embedded" coverage read (GH #26) adapters flag "keyword-only for now" from.
#[test]
fn embed_status_reports_the_coverage_fraction() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = opened_vault(tmp.path());

    // No embedding space yet: 0/M, not an error.
    vault.project(false).unwrap();
    let s = vault.embed_status().unwrap();
    assert_eq!(
        (s.embedded, s.total),
        (0, 2),
        "projected-but-unembedded: 0/M"
    );

    vault.embed(&mut |_| ControlFlow::Continue(())).unwrap();
    let s = vault.embed_status().unwrap();
    assert_eq!((s.embedded, s.total), (2, 2), "fully embedded: M/M");

    // A new projected note makes coverage partial.
    fs::write(
        root.join("fresh.md"),
        "---\n---\n\nA fresh unembedded note.\n",
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

/// A note with no body has no chunks, so it counts as embedded and forecasts no work.
/// Otherwise the fraction never reaches M/M, and every surface keyed on
/// `embedded < total` (caveats, auto-embed passes) stays stuck.
#[test]
fn a_note_with_no_body_counts_as_embedded_and_forecasts_no_work() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);

    // All frontmatter, and an empty file.
    fs::write(root.join("stub.md"), "---\ntags:\n  - People\n---\n").unwrap();
    fs::write(root.join("blank.md"), "").unwrap();

    let vault = Vault::open(&root).unwrap();
    let report = vault.reindex().unwrap();
    assert_eq!(
        report.indexed, 4,
        "both empty notes are projected like any other"
    );

    // Non-vacuity: they really are chunkless.
    let conn = index_conn(&root);
    for path in ["stub.md", "blank.md"] {
        let chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE note_path = ?1",
                [path],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(chunks, 0, "{path} has no body, so it has no chunks");
    }

    let s = vault.embed_status().unwrap();
    assert_eq!(
        (s.embedded, s.total),
        (4, 4),
        "a vault whose only unembedded notes are empty is fully embedded — the fraction \
         must be able to reach M/M, or every surface keyed on it is stuck"
    );

    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(
        plan.would_embed, 0,
        "an empty note has nothing to embed, so a reindex must not keep re-offering to"
    );
}
