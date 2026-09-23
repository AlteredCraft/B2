//! **Explain** for a *Similar & unlinked* card (GH #236): `Vault::explain_similar`, the
//! model-free read behind the Compare view. It must describe the row the surface served,
//! from the same numbers that ranked it, so the explanation can never disagree with the
//! list: the same rank, the same z, the same winning passage. And it must answer for any
//! note, not only a served one: a linked note, one past the list, one not embedded yet,
//! each says why it is not a card.
//!
//! Distances need geometry, so most of these run on the hand-placed
//! [`GeometricEmbedder`]; the fake embedder is used only to pin that a hash space is
//! never graded.

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::vault::{SimilarStanding, Vault};
use b2_core::Error;
use common::{write_geometric_note, write_split_note, GeometricEmbedder};
use std::fs;
use std::path::{Path, PathBuf};

const NOISE_NOTES: usize = 13;

/// anchor + mate + diffuse + a 13-note noise cloud: every anchor's pool (15) is big
/// enough to grade.
fn geometric_vault(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    fs::create_dir_all(&root).unwrap();
    write_geometric_note(&root, "anchor.md", "ANCHOR");
    write_geometric_note(&root, "mate.md", "MATE");
    write_geometric_note(&root, "diffuse.md", "DIFFUSE");
    for i in 0..NOISE_NOTES {
        write_geometric_note(&root, &format!("noise{i}.md"), &format!("N{i}"));
    }
    let v = Vault::open_with_embedder(&root, Box::new(GeometricEmbedder)).unwrap();
    v.reindex().unwrap();
    (v, root)
}

fn ranked(s: &SimilarStanding) -> (usize, usize, bool) {
    match s {
        SimilarStanding::Ranked { rank, of, served } => (*rank, *of, *served),
        other => panic!("expected a ranked standing, got {other:?}"),
    }
}

#[test]
fn every_served_row_is_explained_with_its_own_rank_z_and_evidence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, _) = geometric_vault(tmp.path());
    let served = v.similar("anchor.md", 10).unwrap();
    assert_eq!(served.len(), 10);
    for (i, row) in served.iter().enumerate() {
        let ex = v.explain_similar("anchor.md", &row.path, 10).unwrap();
        let (rank, of, is_served) = ranked(&ex.standing);
        assert_eq!(rank, i + 1, "{}: the rank the card was shown at", row.path);
        assert_eq!(of, 15, "the whole scored pool: every unlinked note");
        assert!(is_served, "a top-10 row at limit 10 is served");
        assert_eq!(ex.z, row.z, "{}: the same z as the card's band", row.path);
        let best = ex.pairs.first().expect("an embedded pair has passages");
        assert_eq!(
            best.z, row.z,
            "the best pair is the one the row was scored on, so it carries the row's z"
        );
        assert!((best.score - row.score).abs() < 1e-9, "same score");
        assert_eq!(
            row.evidence,
            best.candidate
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            "the best pair's candidate passage is the card's evidence"
        );
    }
}

#[test]
fn the_population_is_every_scored_candidates_z_nearest_first() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, _) = geometric_vault(tmp.path());
    let ex = v.explain_similar("anchor.md", "mate.md", 10).unwrap();
    assert_eq!(
        ex.population.len(),
        15,
        "one z per scored note, served or not"
    );
    assert!(
        ex.population.windows(2).all(|w| w[0] >= w[1]),
        "in served order, which is descending z: {:?}",
        ex.population
    );
    assert_eq!(
        Some(ex.population[0]),
        ex.z,
        "the mate leads, and is marked by its z"
    );
}

#[test]
fn a_row_past_the_list_reports_its_rank_and_is_not_served() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, _) = geometric_vault(tmp.path());
    let full = v.similar("anchor.md", 100).unwrap();
    let eighth = &full[7].path;
    let ex = v.explain_similar("anchor.md", eighth, 3).unwrap();
    let (rank, of, served) = ranked(&ex.standing);
    assert_eq!((rank, of), (8, 15));
    assert!(!served, "rank 8 is past a 3-card list");
    assert!(
        ex.z.is_some(),
        "a ranked note is graded whether shown or not"
    );
}

#[test]
fn a_buried_gem_reports_its_whole_note_rank_beside_its_best_passage_rank() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_geometric_note(&root, "anchor.md", "ANCHOR");
    write_geometric_note(&root, "mid.md", "MID");
    write_split_note(&root, "split.md", "NEAR", "FAR");
    for i in 0..NOISE_NOTES {
        write_geometric_note(&root, &format!("noise{i}.md"), &format!("N{i}"));
    }
    let v = Vault::open_with_embedder(&root, Box::new(GeometricEmbedder)).unwrap();
    v.reindex().unwrap();

    let gem = v.explain_similar("anchor.md", "split.md", 10).unwrap();
    assert_eq!(
        ranked(&gem.standing).0,
        1,
        "its best passage ranks it first"
    );
    assert_eq!(
        gem.centroid_rank,
        Some(2),
        "judged as a whole note, `mid.md` is nearer: the gem is buried"
    );
    assert_eq!(gem.pairs.len(), 2, "one pair per candidate passage");
    assert!(
        gem.pairs[0].z > gem.pairs[1].z,
        "the near half pairs far better than the far half: {:?}",
        gem.pairs
    );
    assert!(
        gem.pairs[0].candidate.text.contains("VEC:NEAR")
            && gem.pairs[1].candidate.text.contains("VEC:FAR"),
        "each pair carries its passages' text, so the view can show which section matched"
    );
    assert!(gem
        .pairs
        .iter()
        .all(|p| p.anchor.text.contains("VEC:ANCHOR")));

    let mid = v.explain_similar("anchor.md", "mid.md", 10).unwrap();
    assert_eq!(ranked(&mid.standing).0, 2);
    assert_eq!(mid.centroid_rank, Some(1));
}

#[test]
fn a_linked_note_is_explained_as_already_linked() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("anchor.md"),
        "---\ntitle: anchor\n---\ntopic marker VEC:ANCHOR body. See [[mate]].\n",
    )
    .unwrap();
    write_geometric_note(&root, "mate.md", "MATE");
    write_geometric_note(&root, "other.md", "N1");
    let v = Vault::open_with_embedder(&root, Box::new(GeometricEmbedder)).unwrap();
    v.reindex().unwrap();

    let ex = v.explain_similar("anchor.md", "mate.md", 10).unwrap();
    assert_eq!(ex.standing, SimilarStanding::Linked);
    assert_eq!(ex.centroid_rank, None, "a linked note never enters stage 1");
    assert!(
        !ex.pairs.is_empty(),
        "the passages are still facts worth showing, even for a linked note"
    );
}

#[test]
fn shared_neighbors_are_reported_with_titles() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("anchor.md"),
        "---\ntitle: anchor\n---\ntopic marker VEC:ANCHOR body. See [[hub]].\n",
    )
    .unwrap();
    fs::write(
        root.join("mate.md"),
        "---\ntitle: mate\n---\ntopic marker VEC:MATE body. See [[hub]].\n",
    )
    .unwrap();
    write_geometric_note(&root, "hub.md", "N1");
    let v = Vault::open_with_embedder(&root, Box::new(GeometricEmbedder)).unwrap();
    v.reindex().unwrap();

    let ex = v.explain_similar("anchor.md", "mate.md", 10).unwrap();
    assert_eq!(ranked(&ex.standing).0, 1);
    assert_eq!(ex.shared_neighbors.len(), 1);
    assert_eq!(ex.shared_neighbors[0].path, "hub.md");
    assert_eq!(
        ex.shared_neighbors[0].title,
        v.read("hub.md").unwrap().title,
        "with the title the rest of the app shows"
    );
}

#[test]
fn identical_passages_are_flagged() {
    // The template case from a real vault: the same text in two notes shares one
    // content-addressed vector, so the pair is a perfect match that says nothing about
    // what the notes are about. The view must be able to say so.
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, root) = geometric_vault(tmp.path());
    fs::write(
        root.join("twin.md"),
        fs::read_to_string(root.join("anchor.md")).unwrap(),
    )
    .unwrap();
    v.reindex().unwrap();

    let twin = v.explain_similar("anchor.md", "twin.md", 10).unwrap();
    assert!(
        twin.pairs[0].identical,
        "the same passage text on both sides"
    );
    let mate = v.explain_similar("anchor.md", "mate.md", 10).unwrap();
    assert!(!mate.pairs[0].identical, "near is not identical");
}

#[test]
fn an_unembedded_candidate_says_so() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, root) = geometric_vault(tmp.path());
    // Text no other note holds: vectors are content-addressed, so a copy of an existing
    // passage would arrive already embedded.
    fs::write(root.join("fresh.md"), "fresh words VEC:MATE unseen.\n").unwrap();
    v.project(false).unwrap(); // indexed for keywords, no vectors yet

    let ex = v.explain_similar("anchor.md", "fresh.md", 10).unwrap();
    assert_eq!(ex.standing, SimilarStanding::Unembedded);
    assert!(ex.pairs.is_empty(), "no vectors, no pairs");
    assert_eq!(ex.z, None);
}

#[test]
fn an_unembedded_anchor_says_so() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, root) = geometric_vault(tmp.path());
    fs::write(root.join("fresh.md"), "fresh words VEC:ANCHOR unseen.\n").unwrap();
    v.project(false).unwrap();

    let ex = v.explain_similar("fresh.md", "mate.md", 10).unwrap();
    assert_eq!(ex.standing, SimilarStanding::AnchorUnembedded);
    assert!(ex.population.is_empty());
}

#[test]
fn a_note_outside_the_stage_one_shortlist_says_so() {
    // Stage 1 keeps max(limit × 20, 200) notes by centroid. Past that, a note is never
    // scored at all, which is a different answer from "ranked too low".
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_geometric_note(&root, "anchor.md", "DIFFUSE");
    for i in 0..205 {
        // The N-tagged notes fan away from the DIFFUSE axis as `i` grows, so their
        // whole-note order is their index: `n204.md` is last, at rank 205.
        write_geometric_note(&root, &format!("n{i:03}.md"), &format!("N{i}"));
    }
    let v = Vault::open_with_embedder(&root, Box::new(GeometricEmbedder)).unwrap();
    v.reindex().unwrap();

    let far = v.explain_similar("anchor.md", "n204.md", 1).unwrap();
    assert_eq!(
        far.standing,
        SimilarStanding::NotShortlisted { shortlist: 200 }
    );
    assert_eq!(
        far.centroid_rank,
        Some(205),
        "its whole-note rank is still known"
    );
    assert!(far.z.is_none(), "never scored, so never graded");
    assert!(!far.pairs.is_empty(), "its passages can still be compared");
}

#[test]
fn a_fake_space_is_explained_but_never_graded() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_geometric_note(&root, "anchor.md", "ANCHOR");
    for i in 0..NOISE_NOTES {
        write_geometric_note(&root, &format!("noise{i}.md"), &format!("N{i}"));
    }
    let v = Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(64))).unwrap();
    v.reindex().unwrap();

    let first = &v.similar("anchor.md", 10).unwrap()[0].path;
    let ex = v.explain_similar("anchor.md", first, 10).unwrap();
    assert_eq!(ranked(&ex.standing).0, 1, "ranked all the same");
    assert!(ex.z.is_none() && ex.population.is_empty());
    assert!(
        ex.pairs.iter().all(|p| p.z.is_none()),
        "no pair is graded either"
    );
}

#[test]
fn a_note_against_itself_and_unknown_notes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, _) = geometric_vault(tmp.path());
    let same = v.explain_similar("anchor.md", "anchor.md", 10).unwrap();
    assert_eq!(same.standing, SimilarStanding::SameNote);
    assert!(same.pairs.is_empty());

    assert!(matches!(
        v.explain_similar("nope.md", "mate.md", 10),
        Err(Error::NoteNotFound(_))
    ));
    assert!(matches!(
        v.explain_similar("anchor.md", "nope.md", 10),
        Err(Error::NoteNotFound(_))
    ));
}

#[test]
fn the_view_names_both_notes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (v, _) = geometric_vault(tmp.path());
    let ex = v.explain_similar("anchor", "mate", 10).unwrap();
    assert_eq!(
        ex.anchor.path, "anchor.md",
        "refs resolve like every other command"
    );
    assert_eq!(ex.candidate.path, "mate.md");
    assert_eq!(ex.candidate.title, v.read("mate.md").unwrap().title);
    assert_eq!(ex.limit, 10);
}
