//! The relator seam (planning/vision-and-scope.md "Connection discovery v1"): the
//! `FakeRelator` is deterministic and content-addressed on the note pair, always
//! emits a **core** verb when it fires, and exercises the decline (prune) path — the
//! properties the connection-discovery pipeline will build on, provable with no model.

use b2_core::relate::{Candidate, FakeRelator, NoteCtx, Relator};
use b2_core::relation;

/// A candidate wrapping a bare note id — text/evidence are irrelevant to the fake,
/// which is content-addressed on the b2id pair.
fn candidate(b2id: &str) -> Candidate<'_> {
    Candidate {
        note: NoteCtx {
            b2id,
            title: None,
            text: "",
        },
        evidence_chunk: "",
        signal: "test",
        score: 1.0,
    }
}

fn anchor(b2id: &str) -> NoteCtx<'_> {
    NoteCtx {
        b2id,
        title: None,
        text: "",
    }
}

#[test]
fn model_id_is_stable() {
    assert_eq!(FakeRelator::new().model_id(), "fake-relator-v1");
}

#[test]
fn same_pair_yields_the_same_verdict() {
    let r = FakeRelator::new();
    let first = r.relate(&anchor("A"), &candidate("B")).unwrap();
    let second = r.relate(&anchor("A"), &candidate("B")).unwrap();
    assert_eq!(first, second, "verdict must be a pure function of the pair");
}

#[test]
fn direction_matters() {
    // Hashing is anchor-then-candidate, so (A→B) and (B→A) are distinct judgments —
    // a directed edge, like the graph itself.
    let r = FakeRelator::new();
    let ab = r.relate(&anchor("A"), &candidate("B")).unwrap();
    let ba = r.relate(&anchor("B"), &candidate("A")).unwrap();
    assert_ne!(ab, ba);
}

#[test]
fn fired_proposals_are_well_formed_core_verbs() {
    // Over a spread of synthetic pairs, every proposal the fake emits is a core verb
    // with an explanation and a confidence in the documented [0.5, 1.0] band.
    let r = FakeRelator::new();
    let mut fired = 0;
    for i in 0..200 {
        let src = format!("src-{i}");
        let dst = format!("dst-{i}");
        if let Some(p) = r.relate(&anchor(&src), &candidate(&dst)).unwrap() {
            fired += 1;
            assert!(
                relation::is_core(&p.edge_type),
                "discovery only ever emits core verbs, got `{}`",
                p.edge_type
            );
            assert!(!p.explanation.is_empty());
            assert!(
                (0.5..=1.0).contains(&p.confidence),
                "confidence out of band: {}",
                p.confidence
            );
        }
    }
    assert!(fired > 0, "the fake should fire on most pairs");
}

#[test]
fn the_decline_path_is_reachable() {
    // The prune path is deliberate and deterministic: across a spread of pairs some
    // decline, so the pipeline can be tested against both outcomes without a model.
    let r = FakeRelator::new();
    let declines = (0..200)
        .filter(|i| {
            let src = format!("src-{i}");
            let dst = format!("dst-{i}");
            r.relate(&anchor(&src), &candidate(&dst)).unwrap().is_none()
        })
        .count();
    assert!(declines > 0, "the fake must exercise the decline path");
}
