//! Flow ④ — grounded chat (`Vault::ask`, GH #151/#153): condense → retrieve →
//! assemble → stream → cite, asserted end-to-end against the deterministic
//! [`FakeLlm`] (E2: no candle, no tokio, no network). Like `tests/search.rs`,
//! these prove the **plumbing** — orchestration, degrades, cancellation,
//! citation resolution — not answer quality, which is a real-model eval concern.

mod common;

use b2_core::chat::{self, CONDENSE_SYSTEM_PROMPT, GROUNDED_SYSTEM_PROMPT};
use b2_core::llm::{
    ChatRequest, ChatTurn, Completion, ContextPassage, FakeLlm, LlmProvider, RequestKind, Role,
    NO_EVIDENCE_ANSWER,
};
use b2_core::vault::Vault;
use b2_core::Error;
use common::{opened_vault, reindexed_vault};
use std::cell::Cell;
use std::fs;
use std::ops::ControlFlow;
use std::path::PathBuf;

/// A callback that keeps streaming: the plain, uncancelled ask.
fn stream_all(buf: &mut String) -> impl FnMut(&str) -> ControlFlow<()> + '_ {
    move |tok: &str| {
        buf.push_str(tok);
        ControlFlow::Continue(())
    }
}

/// Wraps a provider and counts its calls — how the suite observes whether the
/// condensation step ran without reaching into the orchestration.
struct Counting<'a> {
    inner: &'a dyn LlmProvider,
    calls: Cell<usize>,
}

impl<'a> Counting<'a> {
    fn new(inner: &'a dyn LlmProvider) -> Self {
        Self {
            inner,
            calls: Cell::new(0),
        }
    }
}

impl LlmProvider for Counting<'_> {
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        self.calls.set(self.calls.get() + 1);
        self.inner.complete(req, on_token)
    }
}

/// A purpose-built two-note vault whose notes share no terms, so which query
/// retrieval actually used is observable from which note ranks first.
fn two_topic_vault(dir: &std::path::Path) -> (Vault, PathBuf) {
    let root = dir.join("vault");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("capybara.md"),
        "---\ntype: note\n---\nThe capybara is the largest living rodent.\n",
    )
    .unwrap();
    fs::write(
        root.join("quokka.md"),
        "---\ntype: note\n---\nThe quokka is a small wallaby from Rottnest Island.\n",
    )
    .unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    (vault, root)
}

#[test]
fn ask_grounds_the_answer_and_resolves_citations_end_to_end() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let mut streamed = String::new();
    let view = vault
        .ask(
            &FakeLlm,
            "forgetting curve",
            &[],
            &mut stream_all(&mut streamed),
        )
        .unwrap();

    // The answer is exactly what streamed up through the callback, uncancelled.
    assert_eq!(view.answer, streamed);
    assert!(!view.cancelled);
    assert!(view.answer.starts_with("Grounded in [1]"));

    // FakeLlm cites every passage it was handed, so citations mirror retrieval:
    // markers 1..=k ascending, each resolved to a real note path + evidence. The
    // path IS the citation's handle (L1) — as durable as any path, which is the
    // trade GH #170 made deliberately.
    assert!(!view.citations.is_empty());
    for (i, c) in view.citations.iter().enumerate() {
        assert_eq!(c.marker, i + 1, "markers are ascending and 1-based");
        assert!(c.path.ends_with(".md"), "path resolves: {:?}", c.path);
        assert!(
            vault.read(&c.path).is_ok(),
            "a citation must open: {:?}",
            c.path
        );
        assert!(!c.excerpt.is_empty(), "the cited passage is the evidence");
    }
    // 'forgetting' lives only in spaced-repetition.md — the keyword match must
    // be among the grounding passages, hence among the citations.
    assert!(view
        .citations
        .iter()
        .any(|c| c.path.ends_with("spaced-repetition.md")));
}

#[test]
fn breaking_mid_stream_reports_a_truncated_answer_honestly() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // Break on the third token: "Grounded", " in", " [1]" ← here.
    let mut seen = 0usize;
    let view = vault
        .ask(&FakeLlm, "forgetting curve", &[], &mut |_tok| {
            seen += 1;
            if seen == 3 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();

    assert!(view.cancelled, "the cut must be reported, not papered over");
    assert_eq!(
        view.answer, "Grounded in [1]",
        "the partial text includes the token the break landed on"
    );
    // Citations resolve over what actually arrived — exactly marker 1.
    assert_eq!(view.citations.len(), 1);
    assert_eq!(view.citations[0].marker, 1);

    // Breaking before any marker arrives leaves no citations at all.
    let mut first = true;
    let early = vault
        .ask(&FakeLlm, "forgetting curve", &[], &mut |_tok| {
            if first {
                first = false;
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        })
        .unwrap();
    assert!(early.cancelled);
    assert_eq!(early.answer, "Grounded in");
    assert!(early.citations.is_empty());
}

#[test]
fn a_single_turn_ask_skips_condensation_and_a_follow_up_pays_for_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    // Single turn: exactly one provider call — the answer itself.
    let counting = Counting::new(&FakeLlm);
    let mut sink = String::new();
    vault
        .ask(
            &counting,
            "forgetting curve",
            &[],
            &mut stream_all(&mut sink),
        )
        .unwrap();
    assert_eq!(counting.calls.get(), 1, "no condensation on a single turn");

    // A follow-up condenses first: two calls, one answer.
    let history = [
        ChatTurn::user("what is the forgetting curve?"),
        ChatTurn::assistant("It describes memory decay over time [1]."),
    ];
    let counting = Counting::new(&FakeLlm);
    let mut sink = String::new();
    vault
        .ask(
            &counting,
            "tell me more about that",
            &history,
            &mut stream_all(&mut sink),
        )
        .unwrap();
    assert_eq!(counting.calls.get(), 2, "condense, then answer");
}

/// A provider whose condensation call fails or comes back cancelled with
/// garbage, while chat behaves like [`FakeLlm`].
/// Step 0 must degrade to the raw question — never break chat, never retrieve
/// on the garbage.
struct BrokenCondense {
    /// `None` → error the condense call; `Some(text)` → return it *cancelled*.
    cancelled_text: Option<&'static str>,
}

impl LlmProvider for BrokenCondense {
    fn model_id(&self) -> &str {
        "broken-condense"
    }
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        if req.kind == RequestKind::Condense {
            return match self.cancelled_text {
                None => Err(Error::Llm("condense: connection dropped".into())),
                Some(text) => Ok(Completion {
                    text: text.to_string(),
                    cancelled: true,
                }),
            };
        }
        FakeLlm.complete(req, on_token)
    }
}

#[test]
fn condensation_failure_degrades_to_the_raw_question() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = two_topic_vault(tmp.path());
    let history = [ChatTurn::user("earlier turn")];

    // The condense call errors outright: chat must still answer, and the top
    // passage must be the raw question's note — proof retrieval used it.
    let mut sink = String::new();
    let view = vault
        .ask(
            &BrokenCondense {
                cancelled_text: None,
            },
            "quokka",
            &history,
            &mut stream_all(&mut sink),
        )
        .unwrap();
    assert!(!view.citations.is_empty());
    assert_eq!(
        view.citations[0].path, "quokka.md",
        "the raw question drove retrieval, so its keyword note ranks first"
    );

    // The condense call returns *cancelled* text naming the other note: the
    // partial text must be discarded, not used as the retrieval query.
    let mut sink = String::new();
    let view = vault
        .ask(
            &BrokenCondense {
                cancelled_text: Some("capybara"),
            },
            "quokka",
            &history,
            &mut stream_all(&mut sink),
        )
        .unwrap();
    assert_eq!(
        view.citations[0].path, "quokka.md",
        "a cancelled condensation degrades to the raw question"
    );
}

#[test]
fn ask_answers_bm25_only_on_a_projected_but_unembedded_vault() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = opened_vault(tmp.path());
    vault.project(false).unwrap();

    // No embedding space exists — retrieval is keyword-only (M4), so the only
    // grounding passages are the actual term matches, and chat still works.
    let mut streamed = String::new();
    let view = vault
        .ask(&FakeLlm, "forgetting", &[], &mut stream_all(&mut streamed))
        .unwrap();
    assert!(!view.cancelled);
    assert!(!view.citations.is_empty(), "chat keeps working unembedded");
    assert!(
        view.citations
            .iter()
            .all(|c| c.path.ends_with("spaced-repetition.md")),
        "keyword-only retrieval grounds only on the matching note"
    );
}

/// Emits a fixed answer citing one real and one hallucinated marker, so the
/// resolution rules are observable end-to-end: the real one resolves, the
/// fake one contributes nothing, and the answer text is never rewritten.
struct Hallucinating;

impl LlmProvider for Hallucinating {
    fn model_id(&self) -> &str {
        "hallucinating"
    }
    fn complete(
        &self,
        _req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        let mut text = String::new();
        for tok in ["See", " [1]", " and", " [99]", "."] {
            text.push_str(tok);
            if on_token(tok).is_break() {
                return Ok(Completion {
                    text,
                    cancelled: true,
                });
            }
        }
        Ok(Completion {
            text,
            cancelled: false,
        })
    }
}

#[test]
fn a_hallucinated_marker_resolves_to_nothing_and_stays_in_the_text() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let mut sink = String::new();
    let view = vault
        .ask(
            &Hallucinating,
            "forgetting curve",
            &[],
            &mut stream_all(&mut sink),
        )
        .unwrap();

    assert_eq!(
        view.answer, "See [1] and [99].",
        "output is never rewritten"
    );
    assert_eq!(view.citations.len(), 1, "only the real marker resolves");
    assert_eq!(view.citations[0].marker, 1);
}

/// Empty retrieval is not condensation: a grounded request whose passage list
/// is empty (nothing in the vault matched) gets the honest no-evidence answer,
/// not an echo of the question — `RequestKind` is what keeps the two apart.
#[test]
fn empty_retrieval_answers_no_evidence_rather_than_echoing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();

    let mut streamed = String::new();
    let view = vault
        .ask(
            &FakeLlm,
            "anything at all",
            &[],
            &mut stream_all(&mut streamed),
        )
        .unwrap();

    assert_eq!(view.answer, NO_EVIDENCE_ANSWER);
    assert!(
        view.citations.is_empty(),
        "nothing retrieved, nothing cited"
    );
    assert!(!view.cancelled);
}

/// A failed *answer* call is a real error (contrast condensation, which
/// degrades): the adapter needs to say "can't reach the model server", not
/// render an empty answer as if the vault had nothing to say. Whatever variant
/// the provider returns is normalized to [`Error::Llm`], so adapters have one
/// variant to match for that message.
struct FailingChat {
    err: fn() -> Error,
}

impl LlmProvider for FailingChat {
    fn model_id(&self) -> &str {
        "failing-chat"
    }
    fn complete(
        &self,
        _req: &ChatRequest,
        _on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        Err((self.err)())
    }
}

#[test]
fn a_failed_answer_call_surfaces_as_an_llm_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let err = vault
        .ask(
            &FailingChat {
                err: || Error::Llm("connection refused".into()),
            },
            "forgetting curve",
            &[],
            &mut |_| ControlFlow::Continue(()),
        )
        .unwrap_err();
    assert!(matches!(err, Error::Llm(_)));

    // A provider leaking a non-Llm variant is normalized at the seam boundary,
    // so the documented contract holds whatever the implementation returns.
    let err = vault
        .ask(
            &FailingChat {
                err: || Error::Io(std::io::Error::other("socket closed")),
            },
            "forgetting curve",
            &[],
            &mut |_| ControlFlow::Continue(()),
        )
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm(msg) if msg.contains("socket closed")),
        "normalized, detail preserved: {err:?}"
    );
}

/// Chat is a reader with `search`'s posture: a mismatched embedding space
/// fails fast rather than grounding the answer on incomparable vectors.
#[test]
fn ask_shares_searchs_model_mismatch_fail_fast() {
    use b2_core::embed::FakeEmbedder;

    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    common::golden_vault_copy(&root);
    let vault = Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(64))).unwrap();
    vault.reindex().unwrap();
    drop(vault);

    let swapped = Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(128))).unwrap();
    let err = swapped
        .ask(&FakeLlm, "forgetting", &[], &mut |_| {
            ControlFlow::Continue(())
        })
        .unwrap_err();
    assert!(matches!(err, Error::ModelMismatch { .. }));
}

// --- prompt assembly & marker parsing (the pure core logic) ---------------------

#[test]
fn the_grounded_request_numbers_passages_and_ends_on_the_question() {
    let passages = vec![
        ContextPassage {
            path: "concepts/memory.md".into(),
            heading_path: Some("Memory > Encoding".into()),
            text: "The brain encodes information.".into(),
        },
        ContextPassage {
            path: "notes/spaced-repetition.md".into(),
            heading_path: None,
            text: "Review at increasing intervals.".into(),
        },
    ];
    let history = [
        ChatTurn::user("first question"),
        ChatTurn::assistant("first answer"),
    ];
    let req = chat::build_request("second question", &history, passages);

    // The conversation the provider sees: history in order, question last.
    assert_eq!(req.turns.len(), 3);
    assert_eq!(req.turns[0].content, "first question");
    assert_eq!(req.turns[1].role, Role::Assistant);
    let last = req.turns.last().unwrap();
    assert_eq!(
        (last.role, last.content.as_str()),
        (Role::User, "second question")
    );

    // The rendered system message: the grounded instruction, then the numbered
    // block — 1-based markers, path + breadcrumb + verbatim text per passage.
    let msg = req.system_message();
    assert!(msg.starts_with(GROUNDED_SYSTEM_PROMPT));
    assert!(msg.contains("I don't find that in your notes"));
    assert!(msg.contains("[1] concepts/memory.md — Memory > Encoding"));
    assert!(msg.contains("The brain encodes information."));
    assert!(msg.contains("[2] notes/spaced-repetition.md"));
    assert!(msg.contains("Review at increasing intervals."));

    // The no-evidence sentence the fake obeys is the one the prompt dictates —
    // the two are pinned together so neither can drift.
    assert!(GROUNDED_SYSTEM_PROMPT.contains(NO_EVIDENCE_ANSWER));

    // A condensation request renders without a passage block.
    let condense = ChatRequest {
        kind: RequestKind::Condense,
        system: CONDENSE_SYSTEM_PROMPT.to_string(),
        turns: req.turns.clone(),
        passages: Vec::new(),
    };
    assert_eq!(condense.system_message(), CONDENSE_SYSTEM_PROMPT);
}

#[test]
fn cited_markers_keeps_real_markers_and_drops_everything_else() {
    // Distinct, ascending, deduped; multi-digit parses.
    assert_eq!(
        chat::cited_markers("[2] then [1], [2] again", 5),
        vec![1, 2]
    );
    assert_eq!(chat::cited_markers("deep cite [12]!", 12), vec![12]);

    // Out of range (0 is out of range — numbering is 1-based), overflow-long
    // digits, unclosed and empty brackets: all resolve to nothing.
    assert_eq!(
        chat::cited_markers("[0] [6] [99999999999999999999] [", 5),
        Vec::<usize>::new()
    );
    assert_eq!(
        chat::cited_markers("[] [notes] no digits", 5),
        Vec::<usize>::new()
    );
    assert_eq!(
        chat::cited_markers("no markers at all", 5),
        Vec::<usize>::new()
    );

    // Zero passages: nothing can resolve, whatever the model claims.
    assert_eq!(chat::cited_markers("[1]", 0), Vec::<usize>::new());
}
