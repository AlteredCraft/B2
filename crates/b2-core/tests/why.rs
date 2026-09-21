//! "Why was this suggested?" — `Vault::why_similar`, the chat answer behind a click on a
//! *Similar & unlinked* card. A **tool-using** chat turn: the model is offered B2's
//! read-only tools (`b2_passage_pairs`, `b2_similar`, `b2_neighbors`, `b2_read`), the core
//! runs what it asks for, and the explanation streams with citations. The pair lookup the
//! row was ranked on is never skipped: B2 makes it when the model does not.
//!
//! Like `tests/ask.rs` these prove the **plumbing** against [`FakeLlm`] and the fake
//! embedder — which passages are handed over, what the facts say, how citations resolve,
//! the degrades — not explanation quality, which is a real-model concern.

mod common;

use b2_core::chat::{
    self, MAX_TOOL_ROUNDS, READ_PASSAGES, TOOL_NEIGHBORS, TOOL_PASSAGE_PAIRS, TOOL_READ,
    TOOL_SIMILAR, WHY_AGENT_SYSTEM_PROMPT, WHY_PAIRS, WHY_SYSTEM_PROMPT,
};
use b2_core::chunk::ChunkConfig;
use b2_core::discover;
use b2_core::llm::{
    ChatRequest, Completion, FakeLlm, LlmProvider, RequestKind, Role, ToolCall, NO_EVIDENCE_ANSWER,
};
use b2_core::vault::Vault;
use b2_core::Error;
use common::index_conn;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

const A: &str = "a.md";
const B: &str = "b.md";
const C: &str = "c.md";
const E: &str = "e.md";

fn write_note(vault: &Path, name: &str, body: &str) {
    fs::write(
        vault.join(name),
        format!("---\ntype: note\ntitle: Note {name}\n---\n{body}\n"),
    )
    .unwrap();
}

/// Several short sections, so a fine chunk target cuts each note into several passages
/// and "the nearest pairs" is a real ranking rather than a single forced pair.
fn sections(topic: &str) -> String {
    (1..=4)
        .map(|i| {
            format!(
                "## {topic} part {i}\n\nThe {topic} section number {i} talks about {topic} in \
                 some detail, with enough words that the chunker gives it a passage of its own."
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// a → b → e (a links b, b links e); c is disconnected. From `a`: b is linked, e is an
/// unlinked candidate sharing the neighbour b, c is an unlinked candidate sharing nothing.
fn chain_vault(dir: &Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, A, &format!("See [[b]].\n\n{}", sections("alpha")));
    write_note(&root, B, &format!("See [[e]].\n\n{}", sections("beta")));
    write_note(&root, C, &sections("gamma"));
    write_note(&root, E, &sections("delta"));
    let mut vault = Vault::open(&root).unwrap();
    vault.set_chunk_config(ChunkConfig {
        target_tokens: 30,
        backscan_tokens: 10,
        overlap_frac: 0.0,
        ..ChunkConfig::default()
    });
    vault.reindex().unwrap();
    (vault, root)
}

/// Records the request it was handed, then answers as [`FakeLlm`] does — how the suite
/// reads the assembled prompt without reaching into the orchestration.
#[derive(Default)]
struct Recording {
    seen: RefCell<Vec<ChatRequest>>,
}

impl LlmProvider for Recording {
    fn model_id(&self) -> &str {
        "recording"
    }
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        self.seen.borrow_mut().push(req.clone());
        FakeLlm.complete(req, on_token)
    }
}

fn keep_streaming() -> impl FnMut(&str) -> ControlFlow<()> {
    |_| ControlFlow::Continue(())
}

// --- the evidence read (discover::passage_pairs) ---------------------------------

#[test]
fn the_best_pair_is_the_passage_the_card_showed() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (_vault, root) = chain_vault(tmp.path());
    let conn = index_conn(&root);

    let cands = discover::candidates(&conn, A, 10, false).unwrap();
    assert!(!cands.is_empty());
    for c in cands {
        let pairs = discover::passage_pairs(&conn, A, &c.note_path, WHY_PAIRS).unwrap();
        let best = pairs.first().expect("an embedded candidate has a pair");
        // The explanation must be about the same passage the card printed as evidence,
        // at the same score — one ranking, not two that could disagree.
        assert_eq!(best.candidate_chunk_id, c.evidence_chunk_id);
        assert!((best.score - c.score).abs() < 1e-6, "{best:?} vs {c:?}");
    }
}

#[test]
fn pairs_are_nearest_first_distinct_per_candidate_passage_and_capped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (_vault, root) = chain_vault(tmp.path());
    let conn = index_conn(&root);

    let pairs = discover::passage_pairs(&conn, A, C, 2).unwrap();
    assert_eq!(pairs.len(), 2, "the cap holds when more passages exist");
    assert!(pairs[0].score >= pairs[1].score, "nearest (highest) first");

    let all = discover::passage_pairs(&conn, A, C, 100).unwrap();
    assert!(all.len() > 2, "the fixture cuts several passages per note");
    let distinct: BTreeSet<i64> = all.iter().map(|p| p.candidate_chunk_id).collect();
    assert_eq!(distinct.len(), all.len(), "one pair per candidate passage");
    assert!(all.windows(2).all(|w| w[0].score >= w[1].score));

    // Determinism: the same read twice is the same answer.
    assert_eq!(all, discover::passage_pairs(&conn, A, C, 100).unwrap());
    // Nothing to compare ⇒ nothing, never an error.
    assert!(discover::passage_pairs(&conn, A, C, 0).unwrap().is_empty());
    assert!(discover::passage_pairs(&conn, A, "nope.md", 3)
        .unwrap()
        .is_empty());
}

// --- the flow (Vault::why_similar) -----------------------------------------------

#[test]
fn the_model_is_offered_b2s_tools_and_what_it_calls_is_run() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());
    let llm = Recording::default();

    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();

    let seen = llm.seen.borrow();
    let first = &seen[0];
    assert_eq!(first.kind, RequestKind::Agent);
    assert!(first.system.starts_with(WHY_AGENT_SYSTEM_PROMPT));
    let offered: BTreeSet<&str> = first.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        offered,
        BTreeSet::from([TOOL_PASSAGE_PAIRS, TOOL_SIMILAR, TOOL_NEIGHBORS, TOOL_READ])
    );
    // Nothing is looked up for the model: the lookups are its own to make.
    assert!(first.exchanges.is_empty() && first.passages.is_empty());
    // The conversation is one user turn naming both notes.
    assert_eq!(first.turns.len(), 1);
    assert_eq!(first.turns[0].role, Role::User);
    assert!(first.turns[0].content.contains(A) && first.turns[0].content.contains(C));

    // FakeLlm's script: call every no-argument tool once (`{}` means this pair), then
    // answer. The second request replays those calls with their results.
    assert_eq!(seen.len(), 2);
    let second = &seen[1];
    let called: Vec<&str> = second
        .exchanges
        .iter()
        .map(|e| e.call.name.as_str())
        .collect();
    assert_eq!(
        called,
        [TOOL_PASSAGE_PAIRS, TOOL_SIMILAR, TOOL_NEIGHBORS, TOOL_READ]
    );
    let pairs = &second.exchanges[0].result;
    assert!(pairs.contains("passages [1] and [2]"), "{pairs}");
    assert!(
        pairs.contains("[1] a.md") && pairs.contains("[2] c.md"),
        "{pairs}"
    );
    assert!(
        second.exchanges[1].result.contains("c.md"),
        "similar lists the candidate"
    );
    assert!(
        second.exchanges[2].result.contains("b.md"),
        "neighbors names a's link to b"
    );

    // The passages the tool handed over are the citation ledger — both notes, only them,
    // each numbered once — and are NOT repeated in the system message.
    let paths: BTreeSet<&str> = second.passages.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, BTreeSet::from([A, C]));
    assert!(second.passages.len() <= 2 * WHY_PAIRS + READ_PASSAGES);
    // `b2_read {}` reads the suggested note — the open one is already on screen.
    assert!(second.exchanges[3].result.starts_with("c.md:"));
    let texts: BTreeSet<(&str, &str)> = second
        .passages
        .iter()
        .map(|p| (p.path.as_str(), p.text.as_str()))
        .collect();
    assert_eq!(texts.len(), second.passages.len());
    assert!(!second.system_message().contains("Passages:"));

    // The view says which tools ran, and that the model chose every one of them.
    let used: Vec<(&str, bool)> = view
        .tools
        .iter()
        .map(|t| (t.name.as_str(), t.seeded))
        .collect();
    assert_eq!(
        used,
        [
            (TOOL_PASSAGE_PAIRS, false),
            (TOOL_SIMILAR, false),
            (TOOL_NEIGHBORS, false),
            (TOOL_READ, false),
        ]
    );
}

#[test]
fn a_model_that_skips_the_pair_lookup_has_it_made_for_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());
    let llm = Scripted::new(vec![
        Round::Calls(vec![(TOOL_NEIGHBORS, "{}")]),
        Round::Answer(vec!["See ", "[1]"]),
    ]);

    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();

    // The row was ranked on its matched pairs, so no explanation is written without
    // them: B2 appends the call the model left out, marked as its own.
    let seen = llm.seen.borrow();
    let called: Vec<&str> = seen[1]
        .exchanges
        .iter()
        .map(|e| e.call.name.as_str())
        .collect();
    assert_eq!(called, [TOOL_NEIGHBORS, TOOL_PASSAGE_PAIRS]);
    assert!(seen[1].exchanges[1].call.arguments.contains(C));
    let used: Vec<(&str, bool)> = view
        .tools
        .iter()
        .map(|t| (t.name.as_str(), t.seeded))
        .collect();
    assert_eq!(used, [(TOOL_NEIGHBORS, false), (TOOL_PASSAGE_PAIRS, true)]);
    assert_eq!(view.citations.len(), 1);
    assert_eq!(view.citations[0].path, A);
}

#[test]
fn a_model_that_ignores_its_tools_is_handed_the_evidence_and_its_first_try_is_never_shown() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());
    let llm = Scripted::new(vec![
        Round::Answer(vec!["An ", "answer ", "from ", "nothing."]),
        Round::Answer(vec!["Grounded ", "[2]"]),
    ]);

    let mut streamed = String::new();
    let view = vault
        .why_similar(&llm, A, C, 10, &mut |t| {
            streamed.push_str(t);
            ControlFlow::Continue(())
        })
        .unwrap();

    assert_eq!(
        streamed, "Grounded [2]",
        "the lookup round's text never streams"
    );
    assert_eq!(view.answer, streamed);
    let seen = llm.seen.borrow();
    assert_eq!(seen[1].kind, RequestKind::Chat, "the handoff");
    assert!(!seen[1].passages.is_empty());
    assert_eq!(view.citations[0].path, C);
    assert!(view.tools.len() == 1 && view.tools[0].seeded);
}

/// A provider that plays a fixed script of rounds: each round is either tool calls or
/// the final answer's tokens. Records every request, like [`Recording`].
struct Scripted {
    rounds: RefCell<Vec<Round>>,
    seen: RefCell<Vec<ChatRequest>>,
}

enum Round {
    Calls(Vec<(&'static str, &'static str)>),
    Answer(Vec<&'static str>),
}

impl Scripted {
    fn new(mut rounds: Vec<Round>) -> Self {
        rounds.reverse();
        Self {
            rounds: RefCell::new(rounds),
            seen: RefCell::default(),
        }
    }
}

impl LlmProvider for Scripted {
    fn model_id(&self) -> &str {
        "scripted"
    }
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        self.seen.borrow_mut().push(req.clone());
        let n = self.seen.borrow().len();
        match self
            .rounds
            .borrow_mut()
            .pop()
            .expect("the script has a round left")
        {
            Round::Calls(calls) => Ok(Completion {
                text: String::new(),
                cancelled: false,
                tool_calls: calls
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, arguments))| ToolCall {
                        id: format!("call_{n}_{i}"),
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    })
                    .collect(),
            }),
            Round::Answer(tokens) => {
                let mut text = String::new();
                for t in tokens {
                    text.push_str(t);
                    let _ = on_token(t);
                }
                Ok(Completion {
                    text,
                    cancelled: false,
                    tool_calls: Vec::new(),
                })
            }
        }
    }
}

#[test]
fn a_tool_the_model_calls_is_run_and_its_passages_become_citable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());
    // The model reads the linked note b.md — neither side of the pair — then cites it.
    let llm = Scripted::new(vec![
        Round::Calls(vec![
            (TOOL_READ, r#"{"note":"b.md"}"#),
            (TOOL_PASSAGE_PAIRS, "{}"),
        ]),
        Round::Answer(vec!["Both ", "relate ", "to b ", "[1]"]),
    ]);

    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();

    let seen = llm.seen.borrow();
    assert_eq!(seen.len(), 2);
    let read = &seen[1].exchanges[0];
    assert_eq!(read.call.name, TOOL_READ);
    assert!(read.result.contains("[1] b.md"), "{}", read.result);
    // One ledger per turn: the pair lookup's numbering continues past what `read` took.
    let from_b = seen[1].passages.iter().take_while(|p| p.path == B).count();
    assert!(from_b >= 1);
    let pairs = &seen[1].exchanges[1].result;
    assert!(pairs.contains(&format!("[{}] a.md", from_b + 1)), "{pairs}");

    assert_eq!(view.answer, "Both relate to b [1]");
    assert_eq!(view.citations.len(), 1);
    assert_eq!(view.citations[0].path, B);
    assert!(
        view.tools.iter().all(|t| !t.seeded),
        "the model made every call"
    );
}

#[test]
fn a_bad_tool_call_is_answered_with_an_error_result_not_a_failed_turn() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());
    // Model output is untrusted: an unknown tool, arguments that aren't JSON, a note that
    // doesn't exist, arguments that are JSON but not an object. Each gets a result the model can read.
    let llm = Scripted::new(vec![
        Round::Calls(vec![
            ("b2_delete_everything", "{}"),
            (TOOL_READ, "not json"),
            (TOOL_READ, r#"{"note":"nope.md"}"#),
            (TOOL_SIMILAR, "[]"),
        ]),
        Round::Answer(vec!["ok"]),
    ]);

    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();
    assert_eq!(view.answer, "ok");

    let seen = llm.seen.borrow();
    let results: Vec<&str> = seen[1].exchanges[..4]
        .iter()
        .map(|e| e.result.as_str())
        .collect();
    assert_eq!(results.len(), 4);
    assert!(
        results.iter().all(|r| r.starts_with("error:")),
        "{results:?}"
    );
    assert!(results[0].contains("unknown tool"));
    assert!(results[2].contains("nope.md"));
    // Nothing a failed call returned became citable: the ledger holds only what B2's own
    // pair lookup (made because the model never got one) handed over.
    assert_eq!(seen[1].exchanges[4].call.name, TOOL_PASSAGE_PAIRS);
    assert!(seen[1].passages.iter().all(|p| p.path == A || p.path == C));
}

#[test]
fn the_loop_is_bounded_and_the_last_round_must_answer() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());
    // A model that would call tools forever.
    let mut rounds: Vec<Round> = (1..MAX_TOOL_ROUNDS)
        .map(|_| Round::Calls(vec![(TOOL_SIMILAR, "{}")]))
        .collect();
    rounds.push(Round::Answer(vec!["done"]));
    let llm = Scripted::new(rounds);

    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();
    assert_eq!(view.answer, "done");

    let seen = llm.seen.borrow();
    assert_eq!(seen.len(), MAX_TOOL_ROUNDS);
    assert!(seen[..MAX_TOOL_ROUNDS - 1]
        .iter()
        .all(|r| !r.tools.is_empty()));
    assert!(
        seen[MAX_TOOL_ROUNDS - 1].tools.is_empty(),
        "the final round offers no tools, so the only thing left to do is answer"
    );
}

/// A provider for a model with no tool support: any request that offers tools is
/// refused (Ollama's `400 … does not support tools`), anything else is answered.
#[derive(Default)]
struct NoToolSupport {
    seen: RefCell<Vec<ChatRequest>>,
}

impl LlmProvider for NoToolSupport {
    fn model_id(&self) -> &str {
        "no-tools"
    }
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        self.seen.borrow_mut().push(req.clone());
        if !req.tools.is_empty() {
            return Err(Error::Llm("model does not support tools".into()));
        }
        FakeLlm.complete(req, on_token)
    }
}

#[test]
fn a_model_without_tool_support_is_handed_the_evidence_instead() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());
    let llm = NoToolSupport::default();

    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();

    let seen = llm.seen.borrow();
    assert_eq!(seen.len(), 2, "the refused tool round, then the handoff");
    let handoff = &seen[1];
    assert_eq!(handoff.kind, RequestKind::Chat);
    assert!(handoff.system.starts_with(WHY_SYSTEM_PROMPT));
    assert!(handoff.tools.is_empty() && handoff.exchanges.is_empty());
    // B2's own pair lookup, handed over in the system message.
    assert!(!handoff.passages.is_empty());
    assert!(handoff.passages.iter().all(|p| p.path == A || p.path == C));
    assert!(handoff.system.contains("[1] and [2]"));
    assert!(handoff.system_message().contains("Passages:"));

    assert!(view.answer.starts_with("Grounded in [1]"));
    assert!(!view.citations.is_empty());
    // Only B2's own lookup ran, and the view says so.
    assert_eq!(view.tools.len(), 1);
    assert!(view.tools[0].seeded);
}

#[test]
fn the_facts_report_what_b2s_tools_found_for_the_pair() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());

    // The handoff is where B2 states the graph facts itself (a tool-using turn reads
    // them with `b2_neighbors`), so read them off a model with no tool support.
    let system_for = |candidate: &str| {
        let llm = NoToolSupport::default();
        vault
            .why_similar(&llm, A, candidate, 10, &mut keep_streaming())
            .unwrap();
        let seen = llm.seen.borrow();
        // The rank is the card's, and both kinds of turn are told it.
        if let Some(line) = seen[1].system.lines().find(|l| l.contains("ranked #")) {
            assert!(seen[0].system.contains(line), "{line}");
        }
        seen[1].system.clone()
    };

    // The card's own position: the rank `similar` served it at, out of what it served.
    let served = vault.similar(A, 10).unwrap();
    let rank_of = |p: &str| served.iter().position(|s| s.path == p).unwrap() + 1;

    let c = system_for(C);
    assert!(c.contains(&format!("ranked #{} of {}", rank_of(C), served.len())));
    assert!(c.contains("no direct link"));
    assert!(c.contains("share no linked neighbours"));

    // e is two hops away through b — the shared neighbour is named.
    let e = system_for(E);
    assert!(e.contains("no direct link"));
    assert!(e.contains("Both link to or from: b.md"), "{e}");

    // b is already linked: said plainly, and no rank is claimed for a note discovery
    // would not list.
    let b = system_for(B);
    assert!(b.contains("already directly linked"), "{b}");
    assert!(!b.contains("ranked #"));
}

#[test]
fn citations_resolve_to_the_two_notes_and_the_answer_streams() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());

    let mut streamed = String::new();
    let view = vault
        .why_similar(&FakeLlm, A, C, 10, &mut |tok| {
            streamed.push_str(tok);
            ControlFlow::Continue(())
        })
        .unwrap();

    assert_eq!(view.answer, streamed);
    assert!(!view.cancelled);
    assert!(view.answer.starts_with("Grounded in [1]"));
    assert!(view.citations.len() >= 2);
    for (i, cite) in view.citations.iter().enumerate() {
        assert_eq!(cite.marker, i + 1);
        assert!(cite.path == A || cite.path == C, "{:?}", cite.path);
        assert!(!cite.excerpt.is_empty());
    }
}

#[test]
fn breaking_mid_stream_reports_a_truncated_explanation_honestly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());

    let mut tokens = 0;
    let view = vault
        .why_similar(&FakeLlm, A, C, 10, &mut |_| {
            tokens += 1;
            if tokens == 3 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert!(view.cancelled);
    assert_eq!(view.answer, "Grounded in [1]");
    assert_eq!(view.citations.len(), 1);
}

#[test]
fn an_unknown_note_on_either_side_is_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());

    for (anchor, candidate) in [("nope.md", C), (A, "nope.md")] {
        let err = vault
            .why_similar(&FakeLlm, anchor, candidate, 10, &mut keep_streaming())
            .unwrap_err();
        assert!(matches!(err, Error::NoteNotFound(_)), "{err:?}");
    }
}

#[test]
fn an_unembedded_vault_is_explained_from_what_can_be_read_never_failed() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_note(&root, A, "alpha");
    write_note(&root, C, "gamma");
    let vault = Vault::open(&root).unwrap();
    vault.project(false).unwrap(); // projected, never embedded

    // With tools: the pair lookup says why it has nothing, and reading still works —
    // `b2_read` needs chunks, not vectors.
    let llm = Recording::default();
    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();
    assert!(llm.seen.borrow()[1].exchanges[0]
        .result
        .contains("no stored vectors"));
    assert_eq!(view.citations.len(), 1);
    assert_eq!(view.citations[0].path, C);

    // Without tools there is only the pair evidence, and there is none: the facts say
    // so and the answer is the no-evidence sentence, not a failure.
    let llm = NoToolSupport::default();
    let view = vault
        .why_similar(&llm, A, C, 10, &mut keep_streaming())
        .unwrap();
    assert_eq!(view.answer, NO_EVIDENCE_ANSWER);
    assert!(view.citations.is_empty());
    assert!(llm.seen.borrow()[1].system.contains("no stored vectors"));
}

struct FailingChat;

impl LlmProvider for FailingChat {
    fn model_id(&self) -> &str {
        "failing"
    }
    fn complete(
        &self,
        _req: &ChatRequest,
        _on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        Err(Error::Io(std::io::Error::other("socket closed")))
    }
}

#[test]
fn a_failed_model_call_surfaces_as_an_llm_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = chain_vault(tmp.path());

    let err = vault
        .why_similar(&FailingChat, A, C, 10, &mut keep_streaming())
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm(msg) if msg.contains("socket closed")),
        "{err:?}"
    );
}

// --- prompt assembly (the pure core logic) ---------------------------------------

#[test]
fn the_why_prompt_keeps_the_grounding_rules_of_chat() {
    // The explanation is grounded chat with a narrower subject: it must cite, and it
    // must not reach for general knowledge. The no-evidence sentence is the one the
    // fake obeys, pinned here exactly as it is for the grounded prompt.
    assert!(WHY_SYSTEM_PROMPT.contains(NO_EVIDENCE_ANSWER));
    assert!(WHY_SYSTEM_PROMPT.contains("[n]"));

    let facts = chat::WhyFacts {
        anchor_path: "x.md".into(),
        anchor_title: Some("X".into()),
        candidate_path: "y.md".into(),
        candidate_title: None,
        rank: Some((2, 7)),
        z: Some(1.2345),
        linked: false,
        shared_neighbors: vec![],
        pairs: vec![(1, 2, -0.5)],
        embedded: true,
    };
    let req = chat::build_why_request(&facts, Vec::new());
    assert!(req.system.contains("x.md (\"X\")"));
    assert!(req.system.contains("ranked #2 of 7"));
    assert!(req.system.contains("z = 1.23"));
    assert!(req.system.contains("[1] and [2]"));

    // A hub-heavy vault can share dozens of neighbours. The count is always exact; the
    // names stop at a handful, because a long list is what a small model recites back
    // instead of answering.
    let crowded = chat::WhyFacts {
        shared_neighbors: (1..=8).map(|i| format!("n{i}.md")).collect(),
        ..facts
    };
    let system = chat::build_why_request(&crowded, Vec::new()).system;
    assert!(system.contains("share 8 linked neighbour(s)"), "{system}");
    assert!(system.contains("n5.md and 3 more"), "{system}");
    assert!(!system.contains("n6.md"), "{system}");
}
