//! Flow ④, grounded chat (`Vault::ask`, GH #151/#153): condense, retrieve, assemble,
//! stream, cite. Plumbing only, against the deterministic [`FakeLlm`] (E2).

mod common;

use b2_core::chat::{self, CONDENSE_SYSTEM_PROMPT, GROUNDED_SYSTEM_PROMPT};
use b2_core::llm::{
    ChatRequest, ChatTurn, Completion, ContextPassage, FakeLlm, LlmProvider, RequestKind, Role,
    NO_EVIDENCE_ANSWER,
};
use b2_core::vault::Vault;
use b2_core::Error;
use common::{keep_streaming, opened_vault, reindexed_vault, stream_into};
use std::cell::Cell;
use std::fs;
use std::ops::ControlFlow;
use std::path::PathBuf;

/// Counts a provider's calls, to observe whether condensation ran.
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

/// Two notes sharing no terms, so the note ranked first shows which query retrieval used.
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
            None,
            &mut stream_into(&mut streamed),
        )
        .unwrap();

    assert_eq!(view.answer, streamed);
    assert!(!view.cancelled);
    assert!(view.answer.starts_with("Grounded in [1]"));

    // FakeLlm cites every passage, so citations mirror retrieval. The path is the
    // citation's handle (L1, GH #170).
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
    // 'forgetting' lives only in spaced-repetition.md.
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
        .ask(&FakeLlm, "forgetting curve", &[], None, &mut |_tok| {
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
    // Citations resolve over what actually arrived.
    assert_eq!(view.citations.len(), 1);
    assert_eq!(view.citations[0].marker, 1);

    let mut first = true;
    let early = vault
        .ask(&FakeLlm, "forgetting curve", &[], None, &mut |_tok| {
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

    let counting = Counting::new(&FakeLlm);
    let mut sink = String::new();
    vault
        .ask(
            &counting,
            "forgetting curve",
            &[],
            None,
            &mut stream_into(&mut sink),
        )
        .unwrap();
    assert_eq!(counting.calls.get(), 1, "no condensation on a single turn");

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
            None,
            &mut stream_into(&mut sink),
        )
        .unwrap();
    assert_eq!(counting.calls.get(), 2, "condense, then answer");
}

/// A condense call that fails or returns cancelled garbage; chat behaves like [`FakeLlm`].
/// Condensation must degrade to the raw question.
struct BrokenCondense {
    /// `None` errors the condense call; `Some(text)` returns it cancelled.
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
                    tool_calls: Vec::new(),
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

    let mut sink = String::new();
    let view = vault
        .ask(
            &BrokenCondense {
                cancelled_text: None,
            },
            "quokka",
            &history,
            None,
            &mut stream_into(&mut sink),
        )
        .unwrap();
    assert!(!view.citations.is_empty());
    assert_eq!(
        view.citations[0].path, "quokka.md",
        "the raw question drove retrieval, so its keyword note ranks first"
    );

    // Cancelled text naming the other note must be discarded.
    let mut sink = String::new();
    let view = vault
        .ask(
            &BrokenCondense {
                cancelled_text: Some("capybara"),
            },
            "quokka",
            &history,
            None,
            &mut stream_into(&mut sink),
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

    // No embedding space: retrieval is keyword-only (M4).
    let mut streamed = String::new();
    let view = vault
        .ask(
            &FakeLlm,
            "forgetting",
            &[],
            None,
            &mut stream_into(&mut streamed),
        )
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

/// A fixed answer citing one real and one hallucinated marker.
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
                    tool_calls: Vec::new(),
                });
            }
        }
        Ok(Completion {
            text,
            cancelled: false,
            tool_calls: Vec::new(),
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
            None,
            &mut stream_into(&mut sink),
        )
        .unwrap();

    assert_eq!(
        view.answer, "See [1] and [99].",
        "output is never rewritten"
    );
    assert_eq!(view.citations.len(), 1, "only the real marker resolves");
    assert_eq!(view.citations[0].marker, 1);
}

/// A grounded request with no passages is not a condensation (`RequestKind` keeps them
/// apart), so it gets the no-evidence answer, not an echo.
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
            None,
            &mut stream_into(&mut streamed),
        )
        .unwrap();

    assert_eq!(view.answer, NO_EVIDENCE_ANSWER);
    assert!(
        view.citations.is_empty(),
        "nothing retrieved, nothing cited"
    );
    assert!(!view.cancelled);
}

/// A failed answer call is a real error, unlike condensation. Every variant is normalized
/// to [`Error::Llm`], so adapters match one.
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
            None,
            &mut keep_streaming(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::Llm(_)));

    let err = vault
        .ask(
            &FailingChat {
                err: || Error::Io(std::io::Error::other("socket closed")),
            },
            "forgetting curve",
            &[],
            None,
            &mut keep_streaming(),
        )
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm(msg) if msg.contains("socket closed")),
        "normalized, detail preserved: {err:?}"
    );
}

/// Chat never grounds on incomparable vectors.
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
        .ask(&FakeLlm, "forgetting", &[], None, &mut keep_streaming())
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
    let req = chat::build_request("second question", &history, passages, None);

    assert_eq!(req.turns.len(), 3);
    assert_eq!(req.turns[0].content, "first question");
    assert_eq!(req.turns[1].role, Role::Assistant);
    let last = req.turns.last().unwrap();
    assert_eq!(
        (last.role, last.content.as_str()),
        (Role::User, "second question")
    );

    let msg = req.system_message();
    assert!(msg.starts_with(GROUNDED_SYSTEM_PROMPT));
    assert!(msg.contains("I don't find that in your notes"));
    assert!(msg.contains("[1] concepts/memory.md — Memory > Encoding"));
    assert!(msg.contains("The brain encodes information."));
    assert!(msg.contains("[2] notes/spaced-repetition.md"));
    assert!(msg.contains("Review at increasing intervals."));

    // Pinned together so neither can drift.
    assert!(GROUNDED_SYSTEM_PROMPT.contains(NO_EVIDENCE_ANSWER));

    let condense = ChatRequest {
        kind: RequestKind::Condense,
        system: CONDENSE_SYSTEM_PROMPT.to_string(),
        turns: req.turns.clone(),
        passages: Vec::new(),
        tools: Vec::new(),
        exchanges: Vec::new(),
    };
    assert_eq!(condense.system_message(), CONDENSE_SYSTEM_PROMPT);
}

#[test]
fn cited_markers_keeps_real_markers_and_drops_everything_else() {
    assert_eq!(
        chat::cited_markers("[2] then [1], [2] again", 5),
        vec![1, 2]
    );
    assert_eq!(chat::cited_markers("deep cite [12]!", 12), vec![12]);

    // Numbering is 1-based, so 0 is out of range.
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

    assert_eq!(chat::cited_markers("[1]", 0), Vec::<usize>::new());
}

// --- the focus note (GH #253) ------------------------------------------------------
//
// "This doc" means the note in front of the user. The adapter passes its path; the engine
// hands the note's own passages over first, named in the prompt, and reports what it did.

/// A note whose body chunks into well over `FOCUS_PASSAGES` passages.
fn write_long_note(root: &std::path::Path, name: &str, sections: usize) {
    let filler = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do. ".repeat(25);
    let mut body = String::from("---\ntype: note\n---\n");
    for i in 0..sections {
        body.push_str(&format!("## Section {i}\n\n{filler}\n\n"));
    }
    fs::write(root.join(name), body).unwrap();
}

#[test]
fn a_focus_note_leads_the_passages_and_is_cited_first() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = two_topic_vault(tmp.path());

    // The question names nothing retrieval can match: only the focus grounds it.
    let view = vault
        .ask(
            &FakeLlm,
            "can you clean up this doc for me",
            &[],
            Some("quokka.md"),
            &mut keep_streaming(),
        )
        .unwrap();

    assert_ne!(view.answer, NO_EVIDENCE_ANSWER);
    let first = view
        .citations
        .first()
        .expect("the focus note is passage [1]");
    assert_eq!((first.marker, first.path.as_str()), (1, "quokka.md"));
    assert_eq!(
        view.citations
            .iter()
            .filter(|c| c.path == "quokka.md")
            .count(),
        1,
        "retrieval must not hand the focus note over a second time: {view:?}"
    );
    let focus = view
        .focus
        .expect("a focused answer says what it was focused on");
    assert_eq!(focus.path, "quokka.md");
    assert_eq!((focus.passages_shown, focus.passages_total), (1, 1));
}

#[test]
fn an_unfocused_answer_carries_no_focus() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = two_topic_vault(tmp.path());
    let view = vault
        .ask(&FakeLlm, "capybara", &[], None, &mut keep_streaming())
        .unwrap();
    assert_eq!(view.focus, None);
    let json = serde_json::to_value(&view).unwrap();
    assert!(
        json.get("focus").is_none(),
        "the --json contract is unchanged without a focus: {json}"
    );
}

#[test]
fn focus_passages_are_capped_and_the_trim_is_reported_not_hidden() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    write_long_note(&root, "long.md", 60);
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();

    let view = vault
        .ask(
            &FakeLlm,
            "summarise this note",
            &[],
            Some("long.md"),
            &mut keep_streaming(),
        )
        .unwrap();

    let focus = view.focus.expect("focused");
    assert!(
        focus.passages_total > chat::FOCUS_PASSAGES,
        "the fixture must exceed the cap to test it: {}",
        focus.passages_total
    );
    assert_eq!(focus.passages_shown, chat::FOCUS_PASSAGES);
    assert_eq!(
        view.citations
            .iter()
            .filter(|c| c.path == "long.md")
            .count(),
        chat::FOCUS_PASSAGES,
        "exactly the cap's worth of the note was handed over"
    );
}

#[test]
fn an_unknown_focus_note_is_an_error_not_a_silently_unfocused_answer() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = two_topic_vault(tmp.path());
    let err = vault
        .ask(
            &FakeLlm,
            "what is this?",
            &[],
            Some("nope/missing.md"),
            &mut keep_streaming(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::NoteNotFound(_)), "{err:?}");
}

#[test]
fn the_focused_prompt_names_the_note_and_says_when_it_was_trimmed() {
    let passages = vec![ContextPassage {
        path: "quokka.md".into(),
        heading_path: None,
        text: "The quokka is a small wallaby.".into(),
    }];
    let whole = b2_core::vault::FocusView {
        path: "quokka.md".into(),
        passages_shown: 1,
        passages_total: 1,
    };
    let req = chat::build_request("clean up this doc", &[], passages.clone(), Some(&whole));
    let msg = req.system_message();
    assert!(
        msg.starts_with(GROUNDED_SYSTEM_PROMPT),
        "the grounding rules come first"
    );
    assert!(msg.contains("quokka.md"), "{msg}");
    assert!(
        msg.contains("[1]"),
        "the note's passages are pointed at by marker: {msg}"
    );
    assert!(
        !msg.contains("only the first"),
        "nothing trimmed, nothing claimed: {msg}"
    );

    let trimmed = b2_core::vault::FocusView {
        passages_shown: 1,
        passages_total: 40,
        ..whole
    };
    let msg = chat::build_request("x", &[], passages, Some(&trimmed)).system_message();
    assert!(msg.contains("only the first 1 of 40"), "{msg}");

    // Without a focus the prompt is byte-identical to before.
    let plain = chat::build_request("x", &[], Vec::new(), None);
    assert_eq!(plain.system, GROUNDED_SYSTEM_PROMPT);

    // Condensation gets the same hint, so a follow-up's "it" can mean the note.
    assert!(chat::condense_system_prompt(Some("quokka.md")).contains("quokka.md"));
    assert_eq!(chat::condense_system_prompt(None), CONDENSE_SYSTEM_PROMPT);
}
