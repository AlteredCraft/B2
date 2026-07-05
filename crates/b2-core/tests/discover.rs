//! Connection-discovery candidate generation (planning/tasks.md ①, resolved
//! 2026-07-01): candidates are the *complement* of the graph — notes near an anchor
//! in vector space but **not** already connected (self + direct neighbors excluded),
//! with 2-hop (triadic-closure) notes deliberately kept.
//!
//! Scope: with the deterministic *fake* embedder, vector nearness is content-
//! addressed, not semantic, so these prove the **plumbing** — the exclusion set, the
//! max-sim ranking, determinism — not model quality (that is the real-embedder eval,
//! deferred to step ②).

mod common;

use b2_core::db;
use b2_core::discover::{self, CandidateNote};
use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use common::{golden_vault_copy, MEMORY_ID, SRS_ID};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const A: &str = "01JA0000000000000000000001";
const B: &str = "01JB0000000000000000000002";
const C: &str = "01JC0000000000000000000003";
const E: &str = "01JE0000000000000000000005";

fn write_note(vault: &Path, name: &str, b2id: &str, body: &str) {
    fs::write(
        vault.join(name),
        format!("---\nb2id: {b2id}\ntype: note\ntitle: {name}\n---\n{body}\n"),
    )
    .unwrap();
}

/// a → b → e (a links b, b links e); c is disconnected. So within 1 hop of a is
/// `{a, b}`; e is 2 hops (a triadic-closure candidate that must survive), c is far.
fn linked_chain_vault(dir: &Path) -> Connection {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "a.md", A, "shared topic alpha. See [[b]].");
    write_note(&vault, "b.md", B, "shared topic beta. See [[e]].");
    write_note(&vault, "c.md", C, "shared topic gamma.");
    write_note(&vault, "e.md", E, "shared topic delta.");
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();
    conn
}

fn ingest_golden(dir: &Path) -> Connection {
    let vault = dir.join("vault");
    golden_vault_copy(&vault);
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();
    conn
}

fn note_set(cands: &[CandidateNote]) -> BTreeSet<String> {
    cands.iter().map(|c| c.note_b2id.clone()).collect()
}

#[test]
fn candidates_are_the_complement_not_self_or_direct_neighbors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    let notes = note_set(&discover::candidates(&conn, A, 10).unwrap());

    assert!(!notes.contains(A), "the anchor is never its own candidate");
    assert!(
        !notes.contains(B),
        "a direct (1-hop) neighbor is already connected"
    );
    assert!(
        notes.contains(C),
        "a disconnected but near note is a candidate"
    );
    assert!(
        notes.contains(E),
        "a 2-hop note (triadic closure) survives the 1-hop exclusion"
    );
    assert_eq!(notes, BTreeSet::from([C.to_string(), E.to_string()]));
}

#[test]
fn candidates_are_ranked_best_first() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    let cands = discover::candidates(&conn, A, 10).unwrap();
    for w in cands.windows(2) {
        assert!(w[0].score >= w[1].score, "scores must be descending");
    }
}

#[test]
fn limit_keeps_the_best_ranked_prefix() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    let full = discover::candidates(&conn, A, 10).unwrap();
    assert!(full.len() >= 2, "the chain vault has ≥2 candidates for a");

    let capped = discover::candidates(&conn, A, 1).unwrap();
    assert_eq!(capped.len(), 1);
    assert_eq!(capped[0], full[0], "limit keeps the best-ranked prefix");
}

#[test]
fn evidence_chunk_belongs_to_its_candidate_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    for c in discover::candidates(&conn, A, 10).unwrap() {
        let owner = db::note_for_chunk(&conn, c.evidence_chunk_id).unwrap();
        assert_eq!(
            owner.as_deref(),
            Some(c.note_b2id.as_str()),
            "the evidence chunk must belong to the candidate it scored"
        );
    }
}

#[test]
fn generation_is_deterministic() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    assert_eq!(
        discover::candidates(&conn, A, 10).unwrap(),
        discover::candidates(&conn, A, 10).unwrap(),
        "same vault + anchor → identical candidates"
    );
}

#[test]
fn a_directly_connected_pair_yields_no_candidates() {
    // The golden vault is two notes, directly connected (spaced-repetition elaborates
    // /references human-memory), so each is within 1 hop of the other → no candidates.
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path());

    assert!(discover::candidates(&conn, SRS_ID, 10).unwrap().is_empty());
    assert!(discover::candidates(&conn, MEMORY_ID, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn unknown_or_chunkless_anchor_and_zero_limit_yield_no_candidates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    assert!(
        discover::candidates(&conn, "01JZZZZZZZZZZZZZZZZZZZZZZZZZ", 10)
            .unwrap()
            .is_empty(),
        "an anchor with no chunks has no candidates"
    );
    assert!(
        discover::candidates(&conn, A, 0).unwrap().is_empty(),
        "limit 0 short-circuits to empty"
    );
}
