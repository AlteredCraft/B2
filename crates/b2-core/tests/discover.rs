//! Discovery candidates (index-engine.md §3) are the complement of the graph: notes near
//! an anchor but not already connected, with 2-hop (triadic-closure) notes kept. Plumbing
//! only, under the fake embedder.

mod common;

use b2_core::db;
use b2_core::discover::{self, CandidateNote};
use b2_core::embed::FakeEmbedder;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use common::{ingest_golden, write_note, MEMORY_PATH, SRS_PATH};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const A: &str = "a.md";
const B: &str = "b.md";
const C: &str = "c.md";
const E: &str = "e.md";

/// a → b → e; c is disconnected. e is 2 hops from a, a triadic-closure candidate.
fn linked_chain_vault(dir: &Path) -> Connection {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, A, "shared topic alpha. See [[b]].");
    write_note(&vault, B, "shared topic beta. See [[e]].");
    write_note(&vault, C, "shared topic gamma.");
    write_note(&vault, E, "shared topic delta.");
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &FakeEmbedder::new(64)).unwrap();
    conn
}

fn note_set(cands: &[CandidateNote]) -> BTreeSet<String> {
    cands.iter().map(|c| c.note_path.clone()).collect()
}

#[test]
fn candidates_are_the_complement_not_self_or_direct_neighbors() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    let notes = note_set(&discover::candidates(&conn, A, 10, false).unwrap());

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

    let cands = discover::candidates(&conn, A, 10, false).unwrap();
    for w in cands.windows(2) {
        assert!(w[0].score >= w[1].score, "scores must be descending");
    }
}

#[test]
fn limit_keeps_the_best_ranked_prefix() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    let full = discover::candidates(&conn, A, 10, false).unwrap();
    assert!(full.len() >= 2, "the chain vault has ≥2 candidates for a");

    let capped = discover::candidates(&conn, A, 1, false).unwrap();
    assert_eq!(capped.len(), 1);
    assert_eq!(capped[0], full[0], "limit keeps the best-ranked prefix");
}

#[test]
fn evidence_chunk_belongs_to_its_candidate_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    for c in discover::candidates(&conn, A, 10, false).unwrap() {
        let owner = db::note_for_chunk(&conn, c.evidence_chunk_id).unwrap();
        assert_eq!(
            owner.as_deref(),
            Some(c.note_path.as_str()),
            "the evidence chunk must belong to the candidate it scored"
        );
    }
}

#[test]
fn generation_is_deterministic() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = linked_chain_vault(tmp.path());

    assert_eq!(
        discover::candidates(&conn, A, 10, false).unwrap(),
        discover::candidates(&conn, A, 10, false).unwrap(),
        "same vault + anchor → identical candidates"
    );
}

#[test]
fn a_directly_connected_pair_yields_no_candidates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    assert!(discover::candidates(&conn, SRS_PATH, 10, false)
        .unwrap()
        .is_empty());
    assert!(discover::candidates(&conn, MEMORY_PATH, 10, false)
        .unwrap()
        .is_empty());
}

/// Two-stage discovery (GH #38) must equal an exhaustive max-sim, recomputed here from
/// the stored vectors, whenever the shortlist covers the candidates (always, below 200
/// notes). Fake vectors make the order arbitrary, so full equality is a strong check.
#[test]
fn two_stage_equals_exhaustive_max_sim_when_shortlist_covers() {
    use b2_core::embed::{l2_sq, unpack_f32};
    use std::collections::HashMap;

    const NOTES: usize = 40;
    const PARAS: usize = 4;

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let mut paths = Vec::new();
    for n in 0..NOTES {
        let body = (0..PARAS)
            .map(|p| format!("note {n} para {p}: topic {}", (n * 31 + p * 7) % 97))
            .collect::<Vec<_>>()
            .join("\n\n");
        let name = format!("n{n}.md");
        write_note(&vault, &name, &body);
        paths.push(name);
    }
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &FakeEmbedder::new(64)).unwrap();

    let anchor = &paths[0];
    let anchor_vecs: Vec<Vec<f32>> = db::note_chunk_vectors(&conn, anchor)
        .unwrap()
        .into_iter()
        .map(|(_, v)| v)
        .collect();

    // Strictly-less keeps the first-seen chunk, as discover does.
    let mut best: HashMap<String, (f32, i64)> = HashMap::new();
    db::for_each_stored_vector(&conn, |chunk_id, blob| {
        let note = &db::note_for_chunk(&conn, chunk_id).unwrap().unwrap();
        if note == anchor {
            return; // no links, so the anchor is the whole exclusion set
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
        .map(|(note_path, (d, evidence_chunk_id))| CandidateNote {
            note_path,
            score: -(d.sqrt() as f64),
            evidence_chunk_id,
            z: None,
        })
        .collect();
    expected.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap()
            .then(a.note_path.cmp(&b.note_path))
    });

    let got = discover::candidates(&conn, anchor, NOTES, false).unwrap();
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
        discover::candidates(&conn, "01JZZZZZZZZZZZZZZZZZZZZZZZZZ", 10, false)
            .unwrap()
            .is_empty(),
        "an anchor with no chunks has no candidates"
    );
    assert!(
        discover::candidates(&conn, A, 0, false).unwrap().is_empty(),
        "limit 0 short-circuits to empty"
    );
}
