//! Flow ④, grounded chat (`Vault::ask`, GH #151/#153): condense, retrieve, assemble,
//! stream, cite. Plumbing only, against the deterministic [`FakeLlm`] (E2).

mod common;

use b2_core::chat::{self, FocusRequest, CONDENSE_SYSTEM_PROMPT, GROUNDED_SYSTEM_PROMPT};
use b2_core::llm::{
    ChatRequest, ChatTurn, Completion, ContextPassage, FakeLlm, LlmProvider, RequestKind, Role,
    NO_EVIDENCE_ANSWER,
};
use b2_core::vault::Vault;
use b2_core::Error;
use common::{keep_streaming, opened_vault, reindexed_vault, stream_into, SRS_PATH};
use std::cell::{Cell, RefCell};
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

// --- the focus note (GH #253) -----------------------------------------------------

/// Records every request it is handed, then answers as [`FakeLlm`].
struct Recording {
    requests: RefCell<Vec<ChatRequest>>,
}

impl Recording {
    fn new() -> Self {
        Self {
            requests: RefCell::new(Vec::new()),
        }
    }

    fn take(&self) -> Vec<ChatRequest> {
        self.requests.take()
    }
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
        self.requests.borrow_mut().push(req.clone());
        FakeLlm.complete(req, on_token)
    }
}

/// A focus on `note` at the default cap.
fn focus(note: &str) -> Option<FocusRequest<'_>> {
    Some(FocusRequest {
        note,
        max_passages: chat::FOCUS_PASSAGES,
    })
}

/// What retrieval hands the model for `query`, as passages.
fn retrieved(vault: &Vault, query: &str) -> Vec<ContextPassage> {
    vault
        .search_chunks(query, chat::ASK_PASSAGES)
        .unwrap()
        .into_iter()
        .map(|c| ContextPassage {
            path: c.path,
            heading_path: c.heading_path,
            text: c.text,
        })
        .collect()
}

/// How many passages the index holds for `path`.
fn chunk_count(root: &std::path::Path, path: &str) -> usize {
    let conn = common::index_conn(root);
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE note_path = ?1",
            [path],
            |r| r.get(0),
        )
        .unwrap();
    usize::try_from(n).unwrap()
}

/// The pre-focus requests, spelled out field by field rather than through `build_request`,
/// so a change to assembly can't move the expectation along with it.
#[test]
fn without_a_focus_the_requests_are_the_plain_grounded_and_condense_requests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let rec = Recording::new();
    vault
        .ask(&rec, "forgetting curve", &[], None, &mut keep_streaming())
        .unwrap();
    let expected = ChatRequest {
        kind: RequestKind::Chat,
        system: GROUNDED_SYSTEM_PROMPT.to_string(),
        turns: vec![ChatTurn::user("forgetting curve")],
        passages: retrieved(&vault, "forgetting curve"),
        tools: Vec::new(),
        exchanges: Vec::new(),
    };
    assert_eq!(rec.take(), vec![expected]);

    let history = [
        ChatTurn::user("what is the forgetting curve?"),
        ChatTurn::assistant("It describes memory decay over time [1]."),
    ];
    let rec = Recording::new();
    vault
        .ask(&rec, "tell me more", &history, None, &mut keep_streaming())
        .unwrap();
    let mut turns = history.to_vec();
    turns.push(ChatTurn::user("tell me more"));
    let condense = ChatRequest {
        kind: RequestKind::Condense,
        system: CONDENSE_SYSTEM_PROMPT.to_string(),
        turns: turns.clone(),
        passages: Vec::new(),
        tools: Vec::new(),
        exchanges: Vec::new(),
    };
    // FakeLlm's condense echoes the question, so retrieval ran on it.
    let answer = ChatRequest {
        kind: RequestKind::Chat,
        system: GROUNDED_SYSTEM_PROMPT.to_string(),
        turns,
        passages: retrieved(&vault, "tell me more"),
        tools: Vec::new(),
        exchanges: Vec::new(),
    };
    assert_eq!(rec.take(), vec![condense, answer]);
}

#[test]
fn a_focus_note_leads_the_passages_and_both_prompts_name_its_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, root) = reindexed_vault(tmp.path());
    let focus_chunks = chunk_count(&root, SRS_PATH);
    assert!(focus_chunks > 0 && focus_chunks <= chat::FOCUS_PASSAGES);

    // A question that names nothing: only the focus gives it a subject.
    let history = [ChatTurn::user("earlier"), ChatTurn::assistant("answer")];
    let rec = Recording::new();
    let view = vault
        .ask(
            &rec,
            "clean up this doc",
            &history,
            focus(SRS_PATH),
            &mut keep_streaming(),
        )
        .unwrap();
    let requests = rec.take();
    let [condense, answer] = requests.as_slice() else {
        panic!("expected a condense and an answer request: {requests:?}");
    };

    let sentence =
        format!("The user is viewing {SRS_PATH}; \"this note\" or \"this doc\" means it.");
    assert!(condense.system.starts_with(CONDENSE_SYSTEM_PROMPT));
    assert!(condense.system.contains(&sentence), "{}", condense.system);
    assert!(answer.system.starts_with(GROUNDED_SYSTEM_PROMPT));
    assert!(answer.system.contains(&sentence), "{}", answer.system);
    assert!(
        !answer.system.contains("passages are included"),
        "a note under the cap is not called trimmed: {}",
        answer.system
    );

    // Every passage of the note, in order, then retrieval's minus anything repeated.
    let (lead, rest) = answer.passages.split_at(focus_chunks);
    assert!(lead.iter().all(|p| p.path == SRS_PATH), "{lead:?}");
    let fill: Vec<ContextPassage> = retrieved(&vault, "clean up this doc")
        .into_iter()
        .filter(|p| !lead.contains(p))
        .collect();
    assert_eq!(rest, fill.as_slice());

    assert_eq!(view.citations[0].marker, 1);
    assert_eq!(view.citations[0].path, SRS_PATH);
}

#[test]
fn a_focus_passage_that_retrieval_also_finds_is_numbered_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());
    // 'forgetting' lives only in spaced-repetition.md, so retrieval returns its passages too.
    assert!(retrieved(&vault, "forgetting")
        .iter()
        .any(|p| p.path == SRS_PATH));

    let rec = Recording::new();
    vault
        .ask(
            &rec,
            "forgetting",
            &[],
            focus(SRS_PATH),
            &mut keep_streaming(),
        )
        .unwrap();
    let requests = rec.take();
    let passages = &requests[0].passages;
    for (i, p) in passages.iter().enumerate() {
        assert!(
            !passages[i + 1..].contains(p),
            "passage {} is repeated: {p:?}",
            i + 1
        );
    }
}

/// A vault whose `long.md` has more passages than the default focus cap, and how many.
fn long_note_vault(dir: &std::path::Path) -> (Vault, usize) {
    let root = dir.join("vault");
    fs::create_dir_all(&root).unwrap();
    let sections: String = (1..=chat::FOCUS_PASSAGES + 4)
        .map(|i| {
            let filler = format!("section{i} words for the long note, padded out. ").repeat(30);
            format!("## Part {i}\n\n{filler}\n\n")
        })
        .collect();
    common::write_note(&root, "long.md", &sections);
    common::write_note(&root, "other.md", "An unrelated note about tides.");
    let vault = Vault::open(&root).unwrap();
    vault.reindex().unwrap();
    let total = chunk_count(&root, "long.md");
    assert!(
        total > chat::FOCUS_PASSAGES,
        "the fixture must overflow the cap"
    );
    (vault, total)
}

/// The one request an ask about `long.md` sends, at `max_passages`.
fn long_note_request(vault: &Vault, max_passages: usize) -> ChatRequest {
    let rec = Recording::new();
    let focus = FocusRequest {
        note: "long.md",
        max_passages,
    };
    vault
        .ask(
            &rec,
            "summarise this",
            &[],
            Some(focus),
            &mut keep_streaming(),
        )
        .unwrap();
    let mut requests = rec.take();
    assert_eq!(requests.len(), 1);
    requests.remove(0)
}

#[test]
fn a_focus_note_longer_than_the_cap_says_how_much_of_it_was_included() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, total) = long_note_vault(tmp.path());

    let req = long_note_request(&vault, chat::FOCUS_PASSAGES);
    assert!(req.passages[..chat::FOCUS_PASSAGES]
        .iter()
        .all(|p| p.path == "long.md"));
    let line = format!(
        "Only the first {} of its {total} passages are included.",
        chat::FOCUS_PASSAGES
    );
    assert!(req.system.contains(&line), "{}", req.system);
}

#[test]
fn the_callers_cap_decides_how_many_focus_passages_lead_and_the_trim_line_follows_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, total) = long_note_vault(tmp.path());

    let wide = long_note_request(&vault, chat::FOCUS_PASSAGES);
    let narrow = long_note_request(&vault, 3);
    assert_eq!(
        narrow.passages[..3],
        wide.passages[..3],
        "a smaller cap takes the same opening passages, in order"
    );
    assert!(
        narrow.system.contains(&format!(
            "Only the first 3 of its {total} passages are included."
        )),
        "{}",
        narrow.system
    );

    let whole = long_note_request(&vault, total);
    assert!(
        !whole.system.contains("passages are included"),
        "a cap the note fits under trims nothing: {}",
        whole.system
    );
    assert!(whole.passages[..total].iter().all(|p| p.path == "long.md"));
}

#[test]
fn an_unknown_focus_note_is_note_not_found_and_asks_the_model_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _root) = reindexed_vault(tmp.path());

    let rec = Recording::new();
    let err = vault
        .ask(
            &rec,
            "clean up this doc",
            &[ChatTurn::user("earlier")],
            focus("no/such-note.md"),
            &mut keep_streaming(),
        )
        .unwrap_err();
    assert!(
        matches!(&err, Error::NoteNotFound(r) if r == "no/such-note.md"),
        "{err:?}"
    );
    assert!(rec.take().is_empty(), "nothing is asked of the model");
}

/// "Clean up this note" is work on the text, not a question the text answers: with every
/// passage present, gemma4 still gave the no-evidence reply without this sentence (GH #253,
/// PR #254's 821fd33).
#[test]
fn a_focused_prompt_treats_work_on_the_note_as_a_task_only_when_it_has_text() {
    let task = "is a task to do with its text";
    let passage = ContextPassage {
        path: "notes/a.md".into(),
        heading_path: None,
        text: "the passage".into(),
    };
    let with_text = chat::Focus {
        path: "notes/a.md".into(),
        included: 1,
        total: 1,
    };
    let req = chat::build_request("clean up this note", &[], vec![passage], Some(&with_text));
    assert!(req.system.contains(task), "{}", req.system);
    assert!(
        req.system.contains("Reserve the no-evidence reply"),
        "{}",
        req.system
    );

    // An unindexed or empty note has no text to work on.
    let no_text = chat::Focus {
        path: "notes/a.md".into(),
        included: 0,
        total: 0,
    };
    let req = chat::build_request("clean up this note", &[], Vec::new(), Some(&no_text));
    assert!(!req.system.contains(task), "{}", req.system);

    // The condense prompt rewrites a query; it does no work on the note.
    let rec = Recording::new();
    chat::condense_query(&rec, "tidy it", &[ChatTurn::user("x")], Some("notes/a.md"));
    let condense = rec.take();
    assert!(!condense[0].system.contains(task), "{}", condense[0].system);
}
