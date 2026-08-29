//! The projection/embedding split (index-engine.md): `project` alone builds the complete
//! keyword + graph index with **no** vectors and no embedding space; `embed` fills
//! exactly the DB-derived missing vectors; and project→embed is **observably**
//! equivalent to the fused `reindex` (counts, chunk text, text→vector, edges — never
//! rowid equality, per §7.1). Model-free throughout (fake embedder).

mod common;

use b2_core::chunk::ChunkConfig;
use b2_core::db;
use b2_core::embed::FakeEmbedder;
use b2_core::ingest::{
    embed_vault, ingest_file, ingest_vault, project_file, project_vault, EmbedCtx, ProjectionCtx,
};
use b2_core::open;
use b2_core::vault::Vault;
use common::{count, golden_vault_copy};
use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

#[test]
fn project_only_builds_keyword_graph_index_with_no_vectors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    // Projection alone: no embedder anywhere near the call. If it issued any query
    // against `embeddings` (which does not exist yet), this would error.
    let cfg = ChunkConfig::default();
    let outcome = project_vault(ProjectionCtx::new(&conn, &vault_dir, &cfg), false).unwrap();
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
    let cfg = ChunkConfig::default();

    project_vault(ProjectionCtx::new(&conn, &vault_dir, &cfg), false).unwrap();

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

    // One fresh copy through the split façade ops…
    let split = Vault::open(&split_root).unwrap();
    let p = split.project(false).unwrap();
    let e = split.embed(&mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!e.cancelled);

    // …a sibling fresh copy through the composed reindex.
    let fused = Vault::open(&fused_root).unwrap();
    let r = fused.reindex().unwrap();

    // The reports agree…
    assert_eq!((p.indexed, e.embedded), (r.indexed, r.embedded));

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
    let cfg = ChunkConfig::default();

    let outcome = project_vault(ProjectionCtx::new(&conn, &vault_dir, &cfg), false).unwrap();

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

/// A file replaced out of band — deleted and another renamed onto its path — is
/// simply that path's note now. The `UNIQUE constraint failed: notes.path` crash this
/// regresses came from two identities contending for one path, and GH #170 removed the
/// contention rather than the reconciliation: the path *is* the identity, so
/// `ON CONFLICT(path)` is the whole of it.
#[test]
fn reindex_reconciles_a_path_taken_over_by_another_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, "foo.md", "Alpha body.");
    write_note(&root, "bar.md", "Beta body about tidal pools.");

    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.reindex().unwrap().indexed, 2);

    // Out-of-b2 edit: delete foo.md, rename bar.md → foo.md.
    fs::remove_file(root.join("foo.md")).unwrap();
    fs::rename(root.join("bar.md"), root.join("foo.md")).unwrap();

    // The incremental reindex converges on the current truth: one note at foo.md,
    // carrying bar's content — byte-identical to a from-scratch rebuild (S3).
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

/// Write a minimal note at `root/name` — the same bytes in every root, so two roots
/// build comparable indexes (a note's identity is its path, which the caller picks).
fn write_note(root: &Path, name: &str, body: &str) {
    fs::write(root.join(name), format!("---\n---\n\n{body}\n")).unwrap();
}

#[test]
fn reindex_prunes_a_deleted_note_like_a_full_rebuild() {
    // A note file deleted outside b2 with *no replacement* must not linger as a ghost
    // row (#31): before this, only a path reuse or a from-scratch rebuild evicted it,
    // so an incremental reindex diverged from `full-reindex ≡ incremental-update`.
    // foo links to bar so the deletion also exercises inbound-edge re-dangling.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, "foo.md", "Alpha body. See [[bar]].");
    write_note(&root, "bar.md", "Beta body about tidal pools.");
    write_note(&root, "baz.md", "Gamma body, unlinked.");

    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.reindex().unwrap().indexed, 3);

    // Out-of-b2 deletion, no replacement — the path is never reused.
    fs::remove_file(root.join("bar.md")).unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(report.indexed, 2);
    assert_eq!(report.notes_pruned, 1, "the ghost row is pruned");

    // Gone from every read surface: the listing…
    let notes = vault.list_notes().unwrap();
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|n| n.path != "bar.md"));
    // …search (its chunks and FTS entries cascaded with the row)…
    assert!(vault
        .search("tidal", 10)
        .unwrap()
        .iter()
        .all(|h| h.path != "bar.md"));
    // …and discovery (its vectors are gone; the unlinked survivor still surfaces).
    let candidates = vault.similar("foo.md", 5).unwrap();
    assert!(candidates.iter().any(|c| c.path == "baz.md"));
    assert!(candidates.iter().all(|c| c.path != "bar.md"));
    drop(vault);

    let conn = open(&root.join(".b2").join("b2.sqlite")).unwrap();
    // FTS stayed in lockstep through the cascade (no ghost text in the index)…
    assert_eq!(count(&conn, "chunks_fts"), count(&conn, "chunks"));
    // …and foo's `[[bar]]` re-dangled: phase 2 re-resolved it against the pruned
    // resolver, keeping the authored link visible for repair (#12), not dropped.
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

    // The invariant itself: the incrementally-reconciled index equals a from-scratch
    // rebuild of the same final vault (the same files at the same paths).
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
    // The #31 carve-out: a file the walk *saw* but could not read yields no
    // projection this run — and "not projected" must not mean "deleted", since the
    // file is plainly still on disk at its path. Its existing row stays.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, "foo.md", "Alpha body.");
    write_note(&root, "bar.md", "Beta body.");

    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.reindex().unwrap().indexed, 2);

    // bar.md turns unreadable in place (a stray non-UTF-8 write) — still on disk.
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
    // Pruning is a *whole-vault* reconciliation: only `project_vault` sees every file,
    // so only it may decide a note is gone. The single-note paths (`project_file` /
    // `ingest_file`, the add/mv/link/write substrate) touch their one note and must
    // leave every other row alone — even a genuine ghost.
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

    // Neither single-note path evicts the now-ghost row…
    let cfg = ChunkConfig::default();
    let proj = ProjectionCtx::new(&conn, &vault_dir, &cfg);
    project_file(proj, "foo.md").unwrap();
    assert_eq!(count(&conn, "notes"), 2, "project_file prunes nothing");
    ingest_file(EmbedCtx::new(proj, &embedder), "foo.md").unwrap();
    assert_eq!(count(&conn, "notes"), 2, "ingest_file prunes nothing");

    // …only the whole-vault pass reconciles the deletion.
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

/// A note with **no body** — frontmatter only, or an empty file — counts as embedded, and
/// a reindex forecasts no work for it. It has no chunks, so there is no vector it could
/// ever be waiting for: "embedded" is vacuously true, and the alternative is a note that
/// reads as forever-pending.
///
/// **The bug this pins is a stuck fraction, not a rounding error.** The coverage predicate
/// required `>= 1 chunk`, so an empty note could never join the numerator. On a real vault
/// that is a permanent `1183/1188` — five `Untitled.md` and stub entity notes — and the
/// fraction is not decoration: every UI surface keyed on `embedded < total` read it as
/// unfinished work. The chat pane said grounding was keyword-first "while this vault
/// embeds", the graph pane withheld ghost connections pending an embed that would never
/// come, search wore a permanent "keyword-first" caveat, and `autoIndexOnOpen` plus the
/// fs-watch's `vectorsPending` scheduled a no-op embed pass on every open and every pulse.
/// A vault could not reach "done" by any amount of reindexing.
///
/// So the assertions come in pairs: the *fraction* is what the user sees, and `would_embed`
/// is the work it kept commissioning.
#[test]
fn a_note_with_no_body_counts_as_embedded_and_forecasts_no_work() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);

    // The two shapes a real vault produces: a stub note that is all frontmatter (the
    // entity/`Untitled.md` case), and a file with nothing in it at all.
    fs::write(root.join("stub.md"), "---\ntags:\n  - People\n---\n").unwrap();
    fs::write(root.join("blank.md"), "").unwrap();

    let vault = Vault::open(&root).unwrap();
    let report = vault.reindex().unwrap();
    assert_eq!(
        report.indexed, 4,
        "both empty notes are projected like any other"
    );

    // Non-vacuity: they really do contribute no chunks, so this is the chunkless case and
    // not some other note quietly carrying the fraction.
    let conn = open(&root.join(".b2/b2.sqlite")).unwrap();
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

    // And the work the stuck fraction used to commission: none.
    let plan = vault.plan_reindex(false).unwrap();
    assert_eq!(
        plan.would_embed, 0,
        "an empty note has nothing to embed, so a reindex must not keep re-offering to"
    );
}
