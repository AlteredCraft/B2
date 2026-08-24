//! Discovery surfacing (ADR-0014): **the ranked candidate list is served**. `limit` is a
//! cap, the order is the deterministic best-passage sort, the z travels with every row as
//! the strength band's input, and no statistic gates membership. Tiny pools are served
//! band-less (no statistic exists, so none is claimed) and a fake-embedded space is served
//! statistic-less — grading changes what the rows *carry*, never which rows exist.
//!
//! The fake embedder can't exercise the grading half, which is exactly why `Vault::similar`
//! never grades a fake-embedded space (the last test pins that). So these tests inject a
//! **geometric** embedder whose vectors are hand-placed: notes carry `VEC:<tag>` markers and
//! the embedder maps each tag to a designed unit vector. The scenarios keep the measured
//! shapes the retired rule was calibrated (and falsified) on: a cluster anchor whose mate
//! stands far above a tight noise cloud, and a diffuse anchor whose best candidate is just
//! the least-far member of one undifferentiated cloud — which the retired gate emptied, and
//! which now serves its ranked nearest, because "everything here is middling" is an answer a
//! strength band can carry and an empty pane cannot.

mod common;

use b2_core::discover;
use b2_core::embed::{Embedder, FakeEmbedder};
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
            // The buried-gem pair (see `a_buried_gem_outranks_and_is_served`):
            // `MID` is one middling chunk, nearer the anchor than a split note's
            // *centroid* but further than that note's best *chunk* (`NEAR`, whose
            // other half `FAR` drags the centroid away).
            "MID" => vec![0.8, 0.6, 0.0, 0.0],
            "NEAR" => vec![0.995, 0.0998, 0.0, 0.0],
            "FAR" => vec![0.0, 0.0, 1.0, 0.0],
            // The noise cloud: all near axis 2, fanned evenly on axis 3 so the
            // diffuse anchor sees one smooth spread of distances (max-z ≈ 1.6
            // for 13 evenly spaced values — under the retired leader gate by
            // construction, which is what made that anchor's pane dark).
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

fn write_note(vault: &Path, name: &str, tag: &str) {
    fs::write(
        vault.join(name),
        format!("---\ntype: note\ntitle: {name}\n---\ntopic marker VEC:{tag} body.\n"),
    )
    .unwrap();
}

/// anchor + mate + a 13-note noise cloud + a diffuse anchor inside the cloud:
/// 16 notes, so every anchor's candidate pool (15) clears STATS_MIN_POPULATION
/// and the z statistics exist to travel.
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
    // The three named notes, by the thing that identifies them: their paths (L1).
    (
        conn,
        "anchor.md".to_string(),
        "mate.md".to_string(),
        "diffuse.md".to_string(),
    )
}

#[test]
fn the_ranked_list_is_served_with_z_on_every_row() {
    // Under the retired gate this anchor served exactly one candidate (the mate
    // cleared the member bar; the cloud did not). Always-serve keeps the same
    // ranking — the mate leads by a wide margin — and hands the human the rest
    // of the field with the z that *says* it is a wide margin, instead of
    // deciding for them that the field isn't worth seeing.
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
    // One order, two names: z descends with the rows, strictly (distinct scores).
    for pair in cands.windows(2) {
        assert!(
            pair[0].z.unwrap() >= pair[1].z.unwrap(),
            "z is monotone in the served order"
        );
    }
}

#[test]
fn a_diffuse_anchor_serves_its_ranked_nearest() {
    // THE INVERSION GH #197 RULED: this anchor's pool is one undifferentiated
    // cloud, and the retired leader gate emptied its pane on exactly that
    // reading. But "one undifferentiated cloud" and "a coherent single-subject
    // vault" are the same geometry (GH #196 measured a real one dark on 16 of
    // 17 notes), so the honest answer is the ranked nearest with middling
    // bands — a relative answer to what was always a relative question.
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
    // Deterministic order: two runs agree row for row.
    let again = discover::candidates(&conn, &diffuse, 10, true).unwrap();
    assert_eq!(cands, again, "the served order is deterministic");
}

#[test]
fn ungraded_serving_changes_banding_never_membership() {
    // `grade: false` (the façade's choice for a fake-embedded space) must serve
    // the SAME rows in the SAME order and differ only in what they carry — the
    // A7 continuity claim: no statistic setting may move membership.
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

/// A note long enough to chunk in two, each half carrying its own `VEC:` tag.
/// ~1400 chars per half against the 450-token (≈1800-char) target: short enough
/// that the two halves don't make a third chunk, long enough that the H2 between
/// them is inside the backscan and becomes the boundary. The 15% overlap re-shares
/// only filler, leaving each chunk's *first* `VEC:` its own.
fn write_split_note(vault: &Path, name: &str, first: &str, second: &str) {
    let filler = "alpha beta gamma delta epsilon zeta eta theta iota kappa. ".repeat(24);
    fs::write(
        vault.join(name),
        format!(
            "---\ntype: note\ntitle: {name}\n---\nmarker VEC:{first} {filler}\n\n\
             ## Second section\n\nmarker VEC:{second} {filler}\n"
        ),
    )
    .unwrap();
}

#[test]
fn a_buried_gem_outranks_and_is_served() {
    // The multi-topic shape the GH #192 reorder exists for: `split.md` holds a passage
    // almost parallel to the anchor (its stage-2 max-sim is far the best) while its second
    // half drags its *centroid* away. The retired stage-1 floor judged that centroid, so
    // `mid.md` outranked the gem — and a corpus-measured version of the same shape was
    // suppressed outright (GH #187) or served to loner anchors on content it did not contain
    // (GH #189). Judged after stage 2, the gem's own best pair is the signal.
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
    // One order, three names: score, z, and the rows may never disagree.
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

    // Ungraded, the exact stage-2 score is still the order — grading changes
    // what rows carry, never how the rows are ranked.
    let raw = discover::candidates(&conn, "anchor.md", 10, false).unwrap();
    assert_eq!(raw[0].note_path, "split.md");
    assert_eq!(raw[1].note_path, "mid.md");
}

#[test]
fn tiny_pools_are_served_in_full_and_band_less() {
    // 3 candidates is no distribution: everything is served (as everywhere),
    // and no z is claimed — the statistics threshold moves *banding only*.
    // Under the retired gate this same threshold was a serve-everything/
    // serve-nothing cliff crossed by adding four notes (GH #196's amplifier C);
    // now crossing it changes what the cards carry and nothing else.
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
    // The continuity claim (A7) across the n = 12 threshold: a vault just
    // under it, one exactly at it (the inclusive edge — an off-by-one on the
    // `>=` would slip past the other two cases), and one over it all serve
    // their complete pools; the only difference is whether the rows carry a z.
    let tmp = tempfile::TempDir::new().unwrap();
    for (name, extra, expect_z) in [
        ("under", 10usize, false), // pool 11 — one short of the threshold
        ("at", 11usize, true),     // pool 12 — the threshold itself, inclusive
        ("over", 13usize, true),   // pool 14 — comfortably past it
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
        let pool = extra + 1; // mate + noise notes
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
    // Through the façade: same 16-note vault, once embedded geometrically (the
    // z travels — the diffuse anchor's list included, the pane the retired gate
    // used to darken) and once with the fake embedder (no statistic claimed:
    // hash geometry would make any z noise wearing a band).
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

    // A fake-embedded space serves the same way but claims no statistic.
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
