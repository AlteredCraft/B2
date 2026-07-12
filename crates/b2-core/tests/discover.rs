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

/// Two-stage discovery (coarse centroid shortlist → exact rescore, #38) must equal
/// the **exhaustive** whole-space max-sim whenever the shortlist covers the
/// candidate set — which it always does at test scale (the shortlist floor is 200
/// notes). Ground truth is recomputed here straight from the stored vectors,
/// independent of `discover`'s code path: same max-sim, same tie rules, over every
/// chunk in the vault. With the fake embedder the *ordering* is arbitrary (random
/// vectors), which is exactly why full equality — notes, scores, evidence chunks —
/// is a strong plumbing check.
#[test]
fn two_stage_equals_exhaustive_max_sim_when_shortlist_covers() {
    use b2_core::embed::{l2_sq, unpack_f32};
    use std::collections::HashMap;

    const NOTES: usize = 40;
    const PARAS: usize = 4;

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut ids = Vec::new();
    for n in 0..NOTES {
        let b2id = format!("01JN{n:022}");
        let body = (0..PARAS)
            .map(|p| format!("note {n} para {p}: topic {}", (n * 31 + p * 7) % 97))
            .collect::<Vec<_>>()
            .join("\n\n");
        write_note(&vault, &format!("n{n}.md"), &b2id, &body);
        ids.push(b2id);
    }
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();

    let anchor = &ids[0];
    let anchor_vecs: Vec<Vec<f32>> = db::note_chunk_vectors(&conn, anchor)
        .unwrap()
        .into_iter()
        .map(|(_, v)| v)
        .collect();

    // Exhaustive ground truth: every stored vector, min over the anchor's vectors,
    // best chunk per note (strictly-less keeps the first-seen chunk, as discover does).
    let chunk_note = db::chunk_note_map(&conn).unwrap();
    let mut best: HashMap<String, (f32, i64)> = HashMap::new();
    db::for_each_stored_vector(&conn, |chunk_id, blob| {
        let note = &chunk_note[&chunk_id];
        if note == anchor {
            return; // no links in this vault → the anchor is the whole exclusion set
        }
        let v = unpack_f32(blob);
        for a in &anchor_vecs {
            let d = l2_sq(a, &v);
            let cur = best.entry(note.clone()).or_insert((f32::INFINITY, 0));
            if d < cur.0 {
                *cur = (d, chunk_id);
            }
        }
    })
    .unwrap();
    let mut expected: Vec<CandidateNote> = best
        .into_iter()
        .map(|(note_b2id, (d, evidence_chunk_id))| CandidateNote {
            note_b2id,
            score: -(d.sqrt() as f64),
            evidence_chunk_id,
        })
        .collect();
    expected.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap()
            .then(a.note_b2id.cmp(&b.note_b2id))
    });

    let got = discover::candidates(&conn, anchor, NOTES).unwrap();
    assert_eq!(got.len(), NOTES - 1, "every other note is a candidate");
    assert_eq!(
        got, expected,
        "two-stage discovery must reproduce the exhaustive scan exactly \
         (notes, scores, and evidence chunks)"
    );
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
