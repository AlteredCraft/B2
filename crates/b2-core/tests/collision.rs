//! GH #81 — reindex-time diagnostics for cross-note `b2id` collisions and
//! identity restamps: surfaced (every pass, until resolved), resolved
//! incumbent-wins (never walk-order), and never auto-fixed (W4 — the human
//! decides which file keeps an identity). The S3 carve-out these pin is
//! documented in invariants.md; the mechanics in index-engine.md §8.

mod common;

use b2_core::chunk::ChunkConfig;
use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::{ingest_file, EmbedCtx, ProjectionCtx};
use b2_core::note::parse;
use b2_core::open;
use b2_core::vault::{CollisionPrecedence, Vault};
use b2_core::Error;
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use std::fs;
use std::path::{Path, PathBuf};

/// A golden-vault copy in a tempdir, indexed once (so the index *remembers* who
/// holds which identity — the incumbent signal the collision resolution reads).
fn indexed_vault(tmp: &tempfile::TempDir) -> (PathBuf, Vault) {
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = Vault::open(&root).unwrap();
    let clean = vault.reindex().unwrap();
    assert!(clean.collisions.is_empty(), "the golden vault is clean");
    assert!(clean.restamped.is_empty(), "the golden vault is clean");
    (root, vault)
}

/// Duplicate `concepts/memory.md` the way Finder would: same bytes, new path.
/// The copy's name deliberately sorts *before* the original (`' '` < `'.'`), so
/// any walk-order or sorted-first rule would hand it the identity — these tests
/// pin that the incumbent wins anyway.
fn make_copy(root: &Path) -> String {
    let copy_rel = "concepts/memory copy.md";
    fs::copy(root.join("concepts/memory.md"), root.join(copy_rel)).unwrap();
    copy_rel.to_string()
}

#[test]
fn finder_copy_incumbent_keeps_identity_over_walk_order() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);
    let copy_rel = make_copy(&root);
    let copy_bytes = fs::read(root.join(&copy_rel)).unwrap();

    let report = vault.reindex().unwrap();

    assert_eq!(report.collisions.len(), 1, "one contested id");
    let c = &report.collisions[0];
    assert_eq!(c.b2id, MEMORY_ID);
    assert_eq!(
        c.kept_path, "concepts/memory.md",
        "the incumbent keeps the identity even though the copy sorts first"
    );
    assert_eq!(c.precedence, CollisionPrecedence::Incumbent);
    assert_eq!(c.shadowed_paths, vec![copy_rel.clone()]);

    // The identity still answers at the original path; the copy has no row.
    assert_eq!(vault.read(MEMORY_ID).unwrap().path, "concepts/memory.md");
    assert!(
        matches!(vault.read(&copy_rel), Err(Error::NoteNotFound(_))),
        "the shadowed copy is not indexed"
    );

    // Surfacing only (W4): the copy's bytes are untouched — no restamp, no edit.
    assert_eq!(fs::read(root.join(&copy_rel)).unwrap(), copy_bytes);
    assert_eq!(report.stamped, 0, "a collision never triggers a stamp");
    assert!(report.restamped.is_empty(), "a collision is not a restamp");

    // The notice re-surfaces on every pass until resolved — never quietly ambient.
    let again = vault.reindex().unwrap();
    assert_eq!(again.collisions, report.collisions);
}

#[test]
fn memoryless_rebuild_tie_breaks_sorted_first_and_says_so() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);
    let copy_rel = make_copy(&root);
    drop(vault);

    // Drop the index entirely: no incumbent memory survives (the S3 carve-out,
    // invariants.md — this divergence from the incremental pass is deliberate
    // and flagged, not silent).
    fs::remove_dir_all(root.join(".b2")).unwrap();
    let vault = Vault::open(&root).unwrap();
    let report = vault.reindex().unwrap();

    assert_eq!(report.collisions.len(), 1);
    let c = &report.collisions[0];
    assert_eq!(
        c.kept_path, copy_rel,
        "with no memory, first-in-sorted-walk keeps the row (the copy sorts first)"
    );
    assert_eq!(
        c.precedence,
        CollisionPrecedence::TieBreak,
        "a memory-less pick is flagged as a tie-break, never presented as a ruling"
    );
    assert_eq!(c.shadowed_paths, vec!["concepts/memory.md".to_string()]);
}

#[test]
fn deleting_the_copy_clears_the_notice() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);
    let copy_rel = make_copy(&root);
    vault.reindex().unwrap();

    fs::remove_file(root.join(&copy_rel)).unwrap();
    let report = vault.reindex().unwrap();

    assert!(report.collisions.is_empty(), "resolution heals the notice");
    assert_eq!(vault.read(MEMORY_ID).unwrap().path, "concepts/memory.md");
}

#[test]
fn removing_the_copys_b2id_line_forks_it_into_a_new_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);
    let copy_rel = make_copy(&root);
    vault.reindex().unwrap();

    // The documented fix: strip the copy's `b2id:` line so the next pass stamps
    // a fresh identity (W1's missing-id stamp — legitimate again because the
    // human made the id missing).
    let raw = fs::read_to_string(root.join(&copy_rel)).unwrap();
    let stripped: String = raw
        .lines()
        .filter(|l| !l.starts_with("b2id:"))
        .map(|l| format!("{l}\n"))
        .collect();
    fs::write(root.join(&copy_rel), stripped).unwrap();

    let report = vault.reindex().unwrap();
    assert!(
        report.collisions.is_empty(),
        "the fork resolved the contest"
    );
    assert_eq!(report.stamped, 1, "the copy got a fresh identity");
    assert!(
        report.restamped.is_empty(),
        "a fork is not identity churn: the copy never had a row at that path"
    );

    let forked = vault.read(&copy_rel).unwrap();
    assert_ne!(forked.b2id, MEMORY_ID, "a genuinely new identity");
    assert_eq!(
        vault.read(MEMORY_ID).unwrap().path,
        "concepts/memory.md",
        "the original keeps its identity untouched"
    );
}

#[test]
fn deleting_the_original_lets_the_copy_inherit_the_identity() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);
    let copy_rel = make_copy(&root);
    vault.reindex().unwrap();

    fs::remove_file(root.join("concepts/memory.md")).unwrap();
    let report = vault.reindex().unwrap();

    assert!(
        report.collisions.is_empty(),
        "one claimant left — no contest"
    );
    assert_eq!(
        vault.read(MEMORY_ID).unwrap().path,
        copy_rel,
        "identity follows the id: the copy inherits through the ordinary move repointing"
    );
}

#[test]
fn external_blank_restamps_and_reports_the_identity_churn() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);

    // An external edit removes the id line (a blanked `b2id:` reads the same —
    // #75); the file now presents no identity.
    let mem = root.join("concepts/memory.md");
    let raw = fs::read_to_string(&mem).unwrap();
    let stripped: String = raw
        .lines()
        .filter(|l| !l.starts_with("b2id:"))
        .map(|l| format!("{l}\n"))
        .collect();
    fs::write(&mem, stripped).unwrap();

    let report = vault.reindex().unwrap();
    assert_eq!(
        report.restamped.len(),
        1,
        "the churn is surfaced, not silent"
    );
    let r = &report.restamped[0];
    assert_eq!(r.path, "concepts/memory.md");
    assert_eq!(r.old_b2id, MEMORY_ID);
    assert_ne!(r.new_b2id, MEMORY_ID, "a fresh id — never auto-restored");
    assert_eq!(
        parse(&fs::read_to_string(&mem).unwrap())
            .fields()
            .b2id
            .as_deref(),
        Some(r.new_b2id.as_str()),
        "the reported new id is the one on disk"
    );

    // A restamp is a per-pass event (the old row is gone next pass); the durable
    // trace is the dangling inbound edges G5 keeps surfacing.
    let again = vault.reindex().unwrap();
    assert!(again.restamped.is_empty());
}

#[test]
fn dry_run_previews_stamp_paths_restamps_and_collisions_with_no_writes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);

    // Stage all three anomalies: a Finder copy (collision), a brand-new id-less
    // note (plain stamp), and a blanked id on an indexed note (restamp).
    let copy_rel = make_copy(&root);
    fs::write(root.join("concepts/new.md"), "A note with no id yet.\n").unwrap();
    let srs = root.join("notes/spaced-repetition.md");
    let srs_raw = fs::read_to_string(&srs).unwrap();
    let srs_blanked: String = srs_raw
        .lines()
        .filter(|l| !l.starts_with("b2id:"))
        .map(|l| format!("{l}\n"))
        .collect();
    fs::write(&srs, &srs_blanked).unwrap();

    let plan = vault.plan_reindex(false).unwrap();

    assert_eq!(
        plan.stamp_paths,
        vec![
            "concepts/new.md".to_string(),
            "notes/spaced-repetition.md".to_string()
        ],
        "the read-only 'which notes have no identity yet' view"
    );
    assert_eq!(plan.would_stamp, plan.stamp_paths.len());
    assert_eq!(plan.would_restamp.len(), 1);
    assert_eq!(plan.would_restamp[0].path, "notes/spaced-repetition.md");
    assert_eq!(plan.would_restamp[0].old_b2id, SRS_ID);
    assert_eq!(plan.collisions.len(), 1);
    assert_eq!(plan.collisions[0].kept_path, "concepts/memory.md");
    assert_eq!(
        plan.collisions[0].precedence,
        CollisionPrecedence::Incumbent
    );

    // Dry means dry: nothing on disk moved.
    assert_eq!(fs::read_to_string(&srs).unwrap(), srs_blanked);
    assert_eq!(
        fs::read(root.join(&copy_rel)).unwrap(),
        fs::read(root.join("concepts/memory.md")).unwrap()
    );
}

#[test]
fn single_note_ingest_refuses_an_identity_steal_but_allows_a_move() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (root, vault) = indexed_vault(&tmp);
    drop(vault); // release the façade's connection; drive ingest directly

    let conn = open(&root.join(".b2").join("b2.sqlite")).unwrap();
    let embedder = FakeEmbedder::new(64); // Vault::open's default, so no model swap
    let cfg = ChunkConfig::default();

    // A copy whose incumbent still exists and still claims the id: refused —
    // erroring beats silently shadowing the incumbent out of the index.
    fs::copy(
        root.join("concepts/memory.md"),
        root.join("concepts/stolen.md"),
    )
    .unwrap();
    let ctx = EmbedCtx::new(ProjectionCtx::new(&conn, &root, &UlidGen, &cfg), &embedder);
    let err = ingest_file(ctx, "concepts/stolen.md").unwrap_err();
    assert!(
        matches!(&err, Error::B2idCollision { b2id, holder, .. }
            if b2id == MEMORY_ID && holder == "concepts/memory.md"),
        "got: {err:?}"
    );

    // The same claim with the incumbent *gone* is the ordinary external move:
    // identity follows the id, the path repoints, nothing refuses.
    fs::remove_file(root.join("concepts/memory.md")).unwrap();
    fs::rename(
        root.join("concepts/stolen.md"),
        root.join("concepts/moved.md"),
    )
    .unwrap();
    let ingested = ingest_file(ctx, "concepts/moved.md").unwrap();
    assert_eq!(ingested.b2id, MEMORY_ID);
}
