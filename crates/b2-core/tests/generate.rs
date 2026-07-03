//! Connection-discovery pipeline ② (planning/tasks.md "Wire the generate pipeline"):
//! the glue that turns a candidate → a relator verdict → a suggestion, end to end
//! and deterministic. Proven with **no LLM** — a set of purpose-built relator stubs
//! drive each pipeline branch exactly (fire-core / decline / tail-verb), and the
//! content-addressed [`FakeRelator`] proves the real seam runs through.

mod common;

use b2_core::discover::{self, GenerateOutcome};
use b2_core::embed::FakeEmbedder;
use b2_core::event::{JsonlSink, NullSink};
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::relate::{Candidate, FakeRelator, NoteCtx, Proposal, Relator};
use b2_core::{open, relation, replay, suggest, Result};
use common::SeqId;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

const A: &str = "01JA0000000000000000000001";
const B: &str = "01JB0000000000000000000002";
const C: &str = "01JC0000000000000000000003";
const E: &str = "01JE0000000000000000000005";

const CREATED: &str = "2026-07-01T00:00:00Z";

// --- relator stubs: one per pipeline branch, so each path is driven exactly ---

/// Always fires the same core verb — every candidate becomes a suggestion.
struct AlwaysCore;
impl Relator for AlwaysCore {
    fn model_id(&self) -> &str {
        "always-core"
    }
    fn relate(&self, _a: &NoteCtx, _c: &Candidate) -> Result<Option<Proposal>> {
        Ok(Some(Proposal {
            edge_type: "relates".to_string(),
            explanation: "stub: always relates".to_string(),
            confidence: 0.9,
        }))
    }
}

/// Always declines — exercises the prune path.
struct AlwaysDecline;
impl Relator for AlwaysDecline {
    fn model_id(&self) -> &str {
        "always-decline"
    }
    fn relate(&self, _a: &NoteCtx, _c: &Candidate) -> Result<Option<Proposal>> {
        Ok(None)
    }
}

/// Counts `relate` calls (interior-mutable, single-threaded) so a test can prove the
/// pre-call dedup skips settled pairs **without** paying for the model. Fires a core
/// verb like [`AlwaysCore`].
struct CountingCore {
    calls: std::cell::Cell<usize>,
}
impl CountingCore {
    fn new() -> Self {
        Self {
            calls: std::cell::Cell::new(0),
        }
    }
}
impl Relator for CountingCore {
    fn model_id(&self) -> &str {
        "counting-core"
    }
    fn relate(&self, _a: &NoteCtx, _c: &Candidate) -> Result<Option<Proposal>> {
        self.calls.set(self.calls.get() + 1);
        Ok(Some(Proposal {
            edge_type: "relates".to_string(),
            explanation: "stub: counting".to_string(),
            confidence: 0.9,
        }))
    }
}

/// Always fires a *tail* (non-core) verb — the `is_core` gate must drop it. Stands
/// in for a real relator that strays outside the closed core; `FakeRelator` never
/// does, so this stub is the only way to test the guardrail.
struct TailVerb;
impl Relator for TailVerb {
    fn model_id(&self) -> &str {
        "tail-verb"
    }
    fn relate(&self, _a: &NoteCtx, _c: &Candidate) -> Result<Option<Proposal>> {
        Ok(Some(Proposal {
            edge_type: "inspired-by".to_string(), // a tolerated tail verb, not core
            explanation: "stub: tail verb".to_string(),
            confidence: 0.9,
        }))
    }
}

fn write_note(vault: &Path, name: &str, b2id: &str, body: &str) {
    fs::write(
        vault.join(name),
        format!("---\nb2id: {b2id}\ntype: note\ntitle: {name}\n---\n{body}\n"),
    )
    .unwrap();
}

/// The discover.rs fixture: a → b → e (a links b, b links e); c disconnected. So
/// anchor a's complement candidates are {c, e} — near in vector space but not within
/// 1 hop (c is far, e is 2 hops / triadic closure). Built with the 64-dim fake
/// embedder so nearness is content-addressed and deterministic.
fn linked_chain(dir: &Path) -> (Connection, PathBuf) {
    let vault = dir.join("vault");
    fs::create_dir_all(&vault).unwrap();
    write_note(&vault, "a.md", A, "shared topic alpha. See [[b]].");
    write_note(&vault, "b.md", B, "shared topic beta. See [[e]].");
    write_note(&vault, "c.md", C, "shared topic gamma.");
    write_note(&vault, "e.md", E, "shared topic delta.");
    let conn = open(&dir.join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &NullSink, &FakeEmbedder::new(64)).unwrap();
    (conn, vault)
}

/// The queue reduced to its stable, id-bearing shape, sorted — for cross-run and
/// post-replay equality.
fn queue_shape(conn: &Connection) -> Vec<(String, String, Option<String>, String)> {
    let mut q: Vec<_> = suggest::list_suggestions(conn)
        .unwrap()
        .into_iter()
        .map(|s| (s.edge_id, s.src_id, s.dst_id, s.edge_type))
        .collect();
    q.sort();
    q
}

#[test]
fn suggestions_appear_for_complement_candidates() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _vault) = linked_chain(tmp.path());

    let out =
        discover::generate_for_anchor(&conn, &NullSink, &SeqId::new(), &AlwaysCore, A, 10, CREATED)
            .unwrap();
    assert_eq!(
        out,
        GenerateOutcome {
            generated: 2,
            declined: 0,
            non_core: 0,
            existing: 0
        },
        "anchor a has two complement candidates (c, e); AlwaysCore fires on both"
    );

    // The suggestions carry the full decision fuel, wired straight from the pieces:
    // src/dst from candidate-gen, verb/explanation/confidence from the relator,
    // `by` from its model id, `source` from the candidate-gen signal.
    let queue = suggest::list_suggestions(&conn).unwrap();
    assert_eq!(queue.len(), 2);
    for s in &queue {
        assert_eq!(s.src_id, A);
        let dst = s.dst_id.as_deref().expect("a concrete target");
        assert!(
            dst == C || dst == E,
            "every suggestion targets a complement candidate, not a neighbor"
        );
        assert_eq!(s.edge_type, "relates");
        assert_eq!(s.by, "agent:always-core");
        assert_eq!(s.source.as_deref(), Some("semantic:maxsim"));
        assert_eq!(s.confidence, Some(0.9));
        assert_eq!(s.explanation.as_deref(), Some("stub: always relates"));
    }
}

#[test]
fn a_non_core_proposal_is_dropped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _vault) = linked_chain(tmp.path());

    let out =
        discover::generate_for_anchor(&conn, &NullSink, &SeqId::new(), &TailVerb, A, 10, CREATED)
            .unwrap();
    assert_eq!(
        out,
        GenerateOutcome {
            generated: 0,
            declined: 0,
            non_core: 2,
            existing: 0
        },
        "a tail verb is validated out — discovery only ever persists core verbs"
    );
    assert!(
        suggest::list_suggestions(&conn).unwrap().is_empty(),
        "nothing reaches the queue"
    );
}

#[test]
fn a_decline_yields_no_suggestion() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _vault) = linked_chain(tmp.path());

    let out = discover::generate_for_anchor(
        &conn,
        &NullSink,
        &SeqId::new(),
        &AlwaysDecline,
        A,
        10,
        CREATED,
    )
    .unwrap();
    assert_eq!(
        out,
        GenerateOutcome {
            generated: 0,
            declined: 2,
            non_core: 0,
            existing: 0
        },
        "both candidates are declined by the relator"
    );
    assert!(suggest::list_suggestions(&conn).unwrap().is_empty());
}

#[test]
fn re_running_generates_nothing_new() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _vault) = linked_chain(tmp.path());

    let first =
        discover::generate_all(&conn, &NullSink, &SeqId::new(), &AlwaysCore, 10, CREATED).unwrap();
    assert!(first.generated > 0, "the first run populates the queue");
    let after_first = queue_shape(&conn);

    // A suggested edge is not `active`, so it never shrinks the candidate *pool* — but
    // pre-call dedup now skips each already-suggested pair *before* the relator, so a
    // re-run produces nothing new and every prior suggestion lands in `existing`.
    let second =
        discover::generate_all(&conn, &NullSink, &SeqId::new(), &AlwaysCore, 10, CREATED).unwrap();
    assert_eq!(second.generated, 0, "nothing new on a re-run");
    assert_eq!(
        second.existing, first.generated,
        "every prior suggestion is now skipped as already-existing"
    );
    assert_eq!(
        queue_shape(&conn),
        after_first,
        "the queue is unchanged by the second run"
    );
}

#[test]
fn a_re_run_skips_the_relator_for_settled_pairs() {
    // The cost invariant: pending suggestions are skipped *before* the relator on a
    // re-run, so it makes no model calls for them. This is what makes `suggest`
    // idempotent in cost, not just in effect.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _vault) = linked_chain(tmp.path());
    let relator = CountingCore::new();

    let first =
        discover::generate_all(&conn, &NullSink, &SeqId::new(), &relator, 10, CREATED).unwrap();
    assert!(first.generated > 0);
    let calls_after_first = relator.calls.get();
    assert_eq!(
        calls_after_first,
        first.generated + first.declined + first.non_core,
        "first run: one relator call per pair that reached the model"
    );

    let second =
        discover::generate_all(&conn, &NullSink, &SeqId::new(), &relator, 10, CREATED).unwrap();
    assert_eq!(
        relator.calls.get(),
        calls_after_first,
        "re-run: zero new relator calls — every settled pair is skipped before the call"
    );
    assert_eq!(second.generated, 0);
    assert_eq!(second.existing, first.generated);
}

#[test]
fn a_rejected_pair_is_never_re_judged() {
    // A rejection tombstone must also stop the re-pay, not just re-creation: a rejected
    // pair keeps its edge row, so pre-call dedup skips it forever — no call, no re-add.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _vault) = linked_chain(tmp.path());
    let relator = CountingCore::new();

    discover::generate_all(&conn, &NullSink, &SeqId::new(), &relator, 10, CREATED).unwrap();
    let ids: Vec<String> = suggest::list_suggestions(&conn)
        .unwrap()
        .into_iter()
        .map(|s| s.edge_id)
        .collect();
    assert!(!ids.is_empty());
    for id in &ids {
        suggest::reject_suggestion(&conn, &NullSink, id, CREATED).unwrap();
    }
    let calls_after_reject = relator.calls.get();

    let out =
        discover::generate_all(&conn, &NullSink, &SeqId::new(), &relator, 10, CREATED).unwrap();
    assert_eq!(
        relator.calls.get(),
        calls_after_reject,
        "a rejected pair is never re-judged — no fresh model call"
    );
    assert_eq!(out.generated, 0, "and never re-proposed");
    assert!(
        suggest::list_suggestions(&conn).unwrap().is_empty(),
        "the tombstoned pairs stay out of the queue"
    );
}

#[test]
fn generation_is_deterministic_across_a_rebuild() {
    // Same vault built twice (b2ids are fixed in frontmatter) + the same idgen ⇒
    // identical suggestion ids and pairs. This is the determinism the sorted-anchor
    // iteration and ranked candidates buy us (tasks.md ②).
    let tmp1 = tempfile::TempDir::new().unwrap();
    let (conn1, _v1) = linked_chain(tmp1.path());
    discover::generate_all(&conn1, &NullSink, &SeqId::new(), &AlwaysCore, 10, CREATED).unwrap();

    let tmp2 = tempfile::TempDir::new().unwrap();
    let (conn2, _v2) = linked_chain(tmp2.path());
    discover::generate_all(&conn2, &NullSink, &SeqId::new(), &AlwaysCore, 10, CREATED).unwrap();

    assert_eq!(
        queue_shape(&conn1),
        queue_shape(&conn2),
        "same vault + same idgen ⇒ identical suggestion ids and pairs"
    );
}

#[test]
fn the_fake_relator_runs_end_to_end() {
    // Exercise the real seam (not just stubs): the content-addressed FakeRelator
    // declines ~1/4 and otherwise fires a core verb, so the pipeline both prunes and
    // produces, and never leaks a non-core verb.
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, _vault) = linked_chain(tmp.path());

    let out = discover::generate_all(
        &conn,
        &NullSink,
        &SeqId::new(),
        &FakeRelator::new(),
        10,
        CREATED,
    )
    .unwrap();
    assert!(out.generated > 0, "the fake fires on most pairs");
    assert_eq!(out.non_core, 0, "the fake only ever emits core verbs");

    for s in suggest::list_suggestions(&conn).unwrap() {
        assert!(
            relation::is_core(&s.edge_type),
            "persisted verb must be core, got `{}`",
            s.edge_type
        );
        assert_eq!(s.by, "agent:fake-relator-v1");
        assert_eq!(s.source.as_deref(), Some("semantic:maxsim"));
    }
}

#[test]
fn the_generated_queue_survives_drop_rebuild_replay() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (conn, vault) = linked_chain(tmp.path());
    let sink = JsonlSink::in_vault(&vault).unwrap();

    discover::generate_all(&conn, &sink, &SeqId::new(), &AlwaysCore, 10, CREATED).unwrap();
    let before = queue_shape(&conn);
    assert!(!before.is_empty());

    // "rm b2.sqlite": rebuild the index from Markdown alone (suggestions are inert,
    // never on disk), then replay the durable log to restore the queue.
    let conn2 = open(&tmp.path().join("rebuilt.sqlite")).unwrap();
    ingest_vault(&conn2, &vault, &UlidGen, &NullSink, &FakeEmbedder::new(64)).unwrap();
    replay::replay_log(&conn2, &sink).unwrap();

    assert_eq!(
        before,
        queue_shape(&conn2),
        "replay reproduces the generated queue exactly"
    );
}
