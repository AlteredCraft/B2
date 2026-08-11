//! The discovery quality floor (index-engine.md §3's "limit is a cap, not a
//! promise", GH #150): a per-anchor z-score rule over the stage-1 centroid
//! population — [`DiscoveryFloor`] — that ends a candidate list where the anchor's
//! own score distribution says the signal stops, possibly at zero.
//!
//! The fake embedder can't exercise this (hash vectors have no semantic geometry —
//! which is exactly why `Vault::similar` never floors a fake-embedded space; the
//! last test pins that). So these tests inject a **geometric** embedder whose
//! vectors are hand-placed: notes carry `VEC:<tag>` markers, and the embedder maps
//! each tag to a designed unit vector. The scenarios mirror the measured shapes the
//! rule was calibrated on (docs/evals/runlog.md 2026-08-11): a cluster anchor whose
//! mate stands far above a tight noise cloud, and a diffuse anchor whose best
//! candidate is just the least-far member of one undifferentiated cloud.

mod common;

use b2_core::discover::{self, DiscoveryFloor};
use b2_core::embed::{Embedder, FakeEmbedder};
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use b2_core::vault::Vault;
use b2_core::Result;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// Hand-placed unit vectors keyed by a `VEC:<tag>` marker in the chunk text.
/// Dim 4: axis 0 is the "anchor topic", axis 2 the "noise topic"; small designed
/// offsets on axes 1/3 give the noise cloud a nonzero, controlled variance.
struct GeometricEmbedder;

impl GeometricEmbedder {
    fn vector_for(text: &str) -> Vec<f32> {
        let tag_start = text.find("VEC:").map(|i| i + 4);
        let tag: String = tag_start
            .map(|s| {
                text[s..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect()
            })
            .unwrap_or_default();
        let v: Vec<f32> = match tag.as_str() {
            // The anchor and its one true mate: nearly parallel.
            "ANCHOR" => vec![1.0, 0.0, 0.0, 0.0],
            "MATE" => vec![0.995, 0.0998, 0.0, 0.0],
            // A diffuse anchor living inside the noise cloud itself.
            "DIFFUSE" => vec![0.0, 0.0, 1.0, 0.0],
            // The noise cloud: all near axis 2, fanned evenly on axis 3 so the
            // diffuse anchor sees one smooth spread of distances (max-z ≈ 1.6 for
            // 13 evenly spaced values — below the 1.85 gate by construction).
            t if t.starts_with('N') => {
                let i: f32 = t[1..].parse().unwrap_or(0.0);
                vec![0.0, 0.0, 1.0, 0.05 + i * 0.03]
            }
            _ => vec![0.0, 1.0, 0.0, 0.0],
        };
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.into_iter().map(|x| x / norm).collect()
    }
}

impl Embedder for GeometricEmbedder {
    fn model_id(&self) -> &str {
        "test-geometric-v1"
    }
    fn dim(&self) -> usize {
        4
    }
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::vector_for(text))
    }
}

const NOISE_NOTES: usize = 13;

fn write_note(vault: &Path, name: &str, seq: usize, tag: &str) {
    let b2id = format!("01JF{seq:022}");
    fs::write(
        vault.join(name),
        format!(
            "---\nb2id: {b2id}\ntype: note\ntitle: {name}\n---\ntopic marker VEC:{tag} body.\n"
        ),
    )
    .unwrap();
}

/// anchor + mate + a 13-note noise cloud + a diffuse anchor inside the cloud:
/// 16 notes, so every anchor's candidate pool (15) clears FLOOR_MIN_POPULATION.
fn geometric_vault(dir: &Path) -> (Connection, String, String, String) {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "anchor.md", 1, "ANCHOR");
    write_note(&vault, "mate.md", 2, "MATE");
    write_note(&vault, "diffuse.md", 3, "DIFFUSE");
    for i in 0..NOISE_NOTES {
        write_note(&vault, &format!("noise{i}.md"), 10 + i, &format!("N{i}"));
    }
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &GeometricEmbedder).unwrap();
    (
        conn,
        format!("01JF{:022}", 1),
        format!("01JF{:022}", 2),
        format!("01JF{:022}", 3),
    )
}

#[test]
fn floor_keeps_the_mate_and_cuts_the_noise_cloud() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, anchor, mate, _) = geometric_vault(tmp.path());
    let floor = DiscoveryFloor::default();
    let cands = discover::candidates(&conn, &anchor, 10, Some(&floor)).unwrap();
    assert_eq!(
        cands
            .iter()
            .map(|c| c.note_b2id.clone())
            .collect::<Vec<_>>(),
        vec![mate],
        "the mate stands far above the anchor's noise floor; nothing else does"
    );
    let z = cands[0].z.expect("floor statistics were computed");
    assert!(
        z > 1.85,
        "the kept candidate carries the z that kept it: {z}"
    );
}

#[test]
fn floor_suppresses_a_diffuse_anchor_entirely() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _, _, diffuse) = geometric_vault(tmp.path());
    let floor = DiscoveryFloor::default();
    let cands = discover::candidates(&conn, &diffuse, 10, Some(&floor)).unwrap();
    assert!(
        cands.is_empty(),
        "one undifferentiated cloud has no leader: the honest answer is nothing, got {:?}",
        cands.iter().map(|c| &c.note_b2id).collect::<Vec<_>>()
    );
}

#[test]
fn no_floor_serves_the_raw_nearest_with_no_z() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, anchor, mate, _) = geometric_vault(tmp.path());
    let cands = discover::candidates(&conn, &anchor, 10, None).unwrap();
    assert_eq!(cands.len(), 10, "ungated discovery fills to limit");
    assert_eq!(cands[0].note_b2id, mate, "ranking itself is unchanged");
    assert!(
        cands.iter().all(|c| c.z.is_none()),
        "no statistics were computed, so no z is claimed"
    );
}

#[test]
fn floor_is_inert_below_the_minimum_population() {
    // 3 candidates is no distribution: the floor must serve everything rather
    // than gate on statistical noise (the starter-vault posture).
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "anchor.md", 1, "ANCHOR");
    write_note(&vault, "n0.md", 10, "N0");
    write_note(&vault, "n1.md", 11, "N1");
    write_note(&vault, "n2.md", 12, "N2");
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &GeometricEmbedder).unwrap();
    let floor = DiscoveryFloor::default();
    let cands = discover::candidates(&conn, &format!("01JF{:022}", 1), 10, Some(&floor)).unwrap();
    assert_eq!(cands.len(), 3, "tiny pool: floor inert, everything served");
    assert!(cands.iter().all(|c| c.z.is_none()));
}

#[test]
fn facade_floors_a_geometric_space_but_never_a_fake_one() {
    // Through the façade: same 16-note vault, once embedded geometrically (floor
    // applies — diffuse anchor honestly empty) and once with the fake embedder
    // (floor must NOT apply: hash geometry would make the gate arbitrary).
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("v");
    fs::create_dir_all(&vault_dir).unwrap();
    write_note(&vault_dir, "anchor.md", 1, "ANCHOR");
    write_note(&vault_dir, "mate.md", 2, "MATE");
    write_note(&vault_dir, "diffuse.md", 3, "DIFFUSE");
    for i in 0..NOISE_NOTES {
        write_note(
            &vault_dir,
            &format!("noise{i}.md"),
            10 + i,
            &format!("N{i}"),
        );
    }

    let mut v = Vault::open_with_embedder(&vault_dir, Box::new(GeometricEmbedder)).unwrap();
    v.reindex().unwrap();
    assert!(
        v.similar("diffuse.md", 10).unwrap().is_empty(),
        "geometric space: the floor judges, and the diffuse anchor goes empty"
    );
    let kept = v.similar("anchor.md", 10).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].path, "mate.md");
    assert!(kept[0].z.is_some(), "the surfaced view carries its z");

    // Disabling the floor is the caller's explicit raw-nearest ask.
    v.set_discovery_floor(None);
    assert_eq!(v.similar("diffuse.md", 10).unwrap().len(), 10);

    // A fake-embedded space is never floored, whatever the vault's floor setting.
    let fake_dir = tmp.path().join("vf");
    fs::create_dir_all(&fake_dir).unwrap();
    write_note(&fake_dir, "anchor.md", 1, "ANCHOR");
    write_note(&fake_dir, "mate.md", 2, "MATE");
    write_note(&fake_dir, "diffuse.md", 3, "DIFFUSE");
    for i in 0..NOISE_NOTES {
        write_note(&fake_dir, &format!("noise{i}.md"), 10 + i, &format!("N{i}"));
    }
    let mut vf = Vault::open_with_embedder(&fake_dir, Box::new(FakeEmbedder::new(64))).unwrap();
    vf.reindex().unwrap();
    vf.set_discovery_floor(Some(DiscoveryFloor {
        leader_z: 100.0, // would suppress everything if it applied
        member_z: 100.0,
    }));
    assert_eq!(
        vf.similar("diffuse.md", 10).unwrap().len(),
        10,
        "fake space: floor ignored, raw nearest served"
    );
}
