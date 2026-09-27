//! Discovery surfacing (ADR-0014): the ranked list is served, `limit` is a cap, and z rides
//! on every row as the band's input. Grading changes what rows carry, never which rows
//! exist. A geometric embedder with hand-placed vectors exercises the grading the fake
//! embedder can't.

mod common;

use common::{write_geometric_note as write_note, write_split_note, GeometricEmbedder};

use b2_core::discover;
use b2_core::embed::FakeEmbedder;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use b2_core::vault::Vault;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

const NOISE_NOTES: usize = 13;

/// Anchor, mate, a 13-note noise cloud and a diffuse anchor inside it: 16 notes, so every
/// pool clears STATS_MIN_POPULATION and z exists.
fn geometric_vault(dir: &Path) -> (Connection, String, String, String) {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "anchor.md", "ANCHOR");
    write_note(&vault, "mate.md", "MATE");
    write_note(&vault, "diffuse.md", "DIFFUSE");
    for i in 0..NOISE_NOTES {
        write_note(&vault, &format!("noise{i}.md"), &format!("N{i}"));
    }
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &GeometricEmbedder).unwrap();
    (
        conn,
        "anchor.md".to_string(),
        "mate.md".to_string(),
        "diffuse.md".to_string(),
    )
}

#[test]
fn the_ranked_list_is_served_with_z_on_every_row() {
    // The mate leads by a wide margin, and z says so; the rest of the field is served.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, anchor, mate, _) = geometric_vault(tmp.path());
    let cands = discover::candidates(&conn, &anchor, 10, true).unwrap();
    assert_eq!(
        cands.len(),
        10,
        "limit caps the list; nothing else truncates it"
    );
    assert_eq!(cands[0].note_path, mate, "the mate leads the ranking");
    assert!(
        cands.iter().all(|c| c.z.is_some()),
        "a graded population's z travels on every row, not only the kept ones"
    );
    let z0 = cands[0].z.unwrap();
    let z1 = cands[1].z.unwrap();
    assert!(
        z0 > z1 + 1.0,
        "the band's input still says the mate towers over the field: {z0} vs {z1}"
    );
    // z descends with the rows.
    for pair in cands.windows(2) {
        assert!(
            pair[0].z.unwrap() >= pair[1].z.unwrap(),
            "z is monotone in the served order"
        );
    }
}

#[test]
fn a_diffuse_anchor_serves_its_ranked_nearest() {
    // GH #197: one undifferentiated cloud is also the geometry of a coherent
    // single-subject vault (GH #196), so serve the ranked nearest with middling bands.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _, _, diffuse) = geometric_vault(tmp.path());
    let cands = discover::candidates(&conn, &diffuse, 10, true).unwrap();
    assert_eq!(
        cands.len(),
        10,
        "the pane an existence gate darkened now serves the ranked list"
    );
    assert!(
        cands.iter().all(|c| c.z.is_some()),
        "graded: the bands can say every card is middling, which the empty pane could not"
    );
    let again = discover::candidates(&conn, &diffuse, 10, true).unwrap();
    assert_eq!(cands, again, "the served order is deterministic");
}

#[test]
fn ungraded_serving_changes_banding_never_membership() {
    // A7: no statistic setting may move membership or order.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, anchor, mate, _) = geometric_vault(tmp.path());
    let graded = discover::candidates(&conn, &anchor, 10, true).unwrap();
    let ungraded = discover::candidates(&conn, &anchor, 10, false).unwrap();
    assert_eq!(ungraded.len(), 10, "ungraded discovery fills to limit");
    assert_eq!(ungraded[0].note_path, mate, "ranking itself is unchanged");
    assert!(
        ungraded.iter().all(|c| c.z.is_none()),
        "no statistics were computed, so no z is claimed"
    );
    assert_eq!(
        graded
            .iter()
            .map(|c| (&c.note_path, c.score, c.evidence_chunk_id))
            .collect::<Vec<_>>(),
        ungraded
            .iter()
            .map(|c| (&c.note_path, c.score, c.evidence_chunk_id))
            .collect::<Vec<_>>(),
        "grading changes what rows carry, never which rows exist or their order"
    );
}

#[test]
fn a_buried_gem_outranks_and_is_served() {
    // GH #192: `split.md` has one passage almost parallel to the anchor, while its other
    // half drags its centroid away. Judged after stage 2, its best pair is the signal.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "anchor.md", "ANCHOR");
    write_note(&vault, "mid.md", "MID");
    write_split_note(&vault, "split.md", "NEAR", "FAR");
    for i in 0..NOISE_NOTES {
        write_note(&vault, &format!("noise{i}.md"), &format!("N{i}"));
    }
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &GeometricEmbedder).unwrap();
    assert_eq!(
        b2_core::db::note_chunk_vectors(&conn, "split.md")
            .unwrap()
            .len(),
        2,
        "the fixture only bites if split.md really chunked in two"
    );

    let cands = discover::candidates(&conn, "anchor.md", 10, true).unwrap();
    assert_eq!(
        cands[0].note_path, "split.md",
        "the buried gem leads: its best passage is the judged signal"
    );
    assert_eq!(cands[1].note_path, "mid.md");
    // Score, z and row order never disagree.
    assert!(
        cands[0].score > cands[1].score,
        "score descends with the rows: {} then {}",
        cands[0].score,
        cands[1].score
    );
    let (z0, z1) = (cands[0].z.unwrap(), cands[1].z.unwrap());
    assert!(
        z0 > z1,
        "z (the shown band) descends with the rows: {z0} then {z1}"
    );

    let raw = discover::candidates(&conn, "anchor.md", 10, false).unwrap();
    assert_eq!(raw[0].note_path, "split.md");
    assert_eq!(raw[1].note_path, "mid.md");
}

#[test]
fn tiny_pools_are_served_in_full_and_band_less() {
    // 3 candidates is no distribution, so no z; the threshold moves banding only
    // (GH #196).
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "anchor.md", "ANCHOR");
    write_note(&vault, "n0.md", "N0");
    write_note(&vault, "n1.md", "N1");
    write_note(&vault, "n2.md", "N2");
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &GeometricEmbedder).unwrap();
    let cands = discover::candidates(&conn, "anchor.md", 10, true).unwrap();
    assert_eq!(cands.len(), 3, "tiny pool: everything served");
    assert!(
        cands.iter().all(|c| c.z.is_none()),
        "no statistic over a handful of distances, so none claimed"
    );
}

#[test]
fn crossing_the_stats_population_changes_banding_never_membership() {
    // A7 across the n = 12 threshold, including the inclusive edge.
    let tmp = tempfile::TempDir::new().unwrap();
    for (name, extra, expect_z) in [
        ("under", 10usize, false), // pool 11
        ("at", 11usize, true),     // pool 12
        ("over", 13usize, true),   // pool 14
    ] {
        let vault = tmp.path().join(name).join("vault");
        fs::create_dir_all(&vault).unwrap();
        write_note(&vault, "anchor.md", "ANCHOR");
        write_note(&vault, "mate.md", "MATE");
        for i in 0..extra {
            write_note(&vault, &format!("noise{i}.md"), &format!("N{i}"));
        }
        let conn = open(&tmp.path().join(name).join("b2.sqlite")).unwrap();
        ingest_vault(&conn, &vault, &GeometricEmbedder).unwrap();
        let pool = extra + 1;
        let cands = discover::candidates(&conn, "anchor.md", 100, true).unwrap();
        assert_eq!(
            cands.len(),
            pool,
            "{name}: the whole pool is served on both sides of the threshold"
        );
        assert_eq!(cands[0].note_path, "mate.md", "{name}: ranking unchanged");
        assert!(
            cands.iter().all(|c| c.z.is_some() == expect_z),
            "{name}: the threshold decides banding alone (expect z: {expect_z})"
        );
    }
}

#[test]
fn facade_grades_a_geometric_space_but_never_a_fake_one() {
    // Hash geometry would make any z noise wearing a band.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("v");
    fs::create_dir_all(&vault_dir).unwrap();
    write_note(&vault_dir, "anchor.md", "ANCHOR");
    write_note(&vault_dir, "mate.md", "MATE");
    write_note(&vault_dir, "diffuse.md", "DIFFUSE");
    for i in 0..NOISE_NOTES {
        write_note(&vault_dir, &format!("noise{i}.md"), &format!("N{i}"));
    }

    let v = Vault::open_with_embedder(&vault_dir, Box::new(GeometricEmbedder)).unwrap();
    v.reindex().unwrap();
    let diffuse = v.similar("diffuse.md", 10).unwrap();
    assert_eq!(
        diffuse.len(),
        10,
        "geometric space: the diffuse anchor serves its ranked nearest (GH #197)"
    );
    assert!(
        diffuse.iter().all(|s| s.z.is_some()),
        "and every surfaced view carries its z for the band"
    );
    let kept = v.similar("anchor.md", 10).unwrap();
    assert_eq!(
        kept[0].path, "mate.md",
        "the mate still leads its anchor's list"
    );

    let fake_dir = tmp.path().join("vf");
    fs::create_dir_all(&fake_dir).unwrap();
    write_note(&fake_dir, "anchor.md", "ANCHOR");
    write_note(&fake_dir, "mate.md", "MATE");
    write_note(&fake_dir, "diffuse.md", "DIFFUSE");
    for i in 0..NOISE_NOTES {
        write_note(&fake_dir, &format!("noise{i}.md"), &format!("N{i}"));
    }
    let vf = Vault::open_with_embedder(&fake_dir, Box::new(FakeEmbedder::new(64))).unwrap();
    vf.reindex().unwrap();
    let fake = vf.similar("diffuse.md", 10).unwrap();
    assert_eq!(
        fake.len(),
        10,
        "fake space: ranked nearest served all the same"
    );
    assert!(
        fake.iter().all(|s| s.z.is_none()),
        "but no z is ever claimed over hash vectors"
    );
}
