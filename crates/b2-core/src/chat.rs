//! Flow ④ — grounded chat (GH #151 §"Flow ④", cut as GH #153): the core logic
//! of `Vault::ask`, condense → retrieve → assemble → stream → cite. This module
//! owns what must be deterministic and testable against [`FakeLlm`] — prompt
//! assembly, the condensation degrade, citation-marker parsing — while the
//! façade ([`crate::vault::Vault::ask`]) owns retrieval and resolving markers
//! into display views, mirroring the `discover.rs`/`vault.rs` split.
//!
//! What the LLM never does here (why the invariants survive, minus M1's
//! wording): chat is a **reader** (C1) whose output is never stored — no
//! response caching, no `meta` rows, session-only history (S4). Model output
//! is untrusted content (E5), enforced where it renders.
//!
//! [`FakeLlm`]: crate::llm::FakeLlm

use crate::llm::{ChatRequest, ChatTurn, ContextPassage, LlmProvider, RequestKind};
use std::collections::BTreeSet;
use std::ops::ControlFlow;

/// How many passages retrieval hands the model — the "~8–12" of the spec
/// (GH #153). Retrieval goes through `Vault::search_chunks`, so the passage
/// view's pool discipline applies unchanged: a 10-ask reaches
/// `chunk_candidate_pool(10) = 60` candidates per signal (GH #141/#142) —
/// chat introduces no new candidate width.
pub const ASK_PASSAGES: usize = 10;

/// The grounded system prompt (flow ④ step 2). Prompt assembly is core logic:
/// authored here, rendered with the numbered passages by
/// [`ChatRequest::system_message`], asserted by the engine suite. The quoted
/// no-evidence sentence is [`crate::llm::NO_EVIDENCE_ANSWER`] verbatim (the
/// suite pins the containment), so the instruction and the fake that obeys it
/// can never drift apart.
pub const GROUNDED_SYSTEM_PROMPT: &str = "You are B2, answering questions about the user's own \
notes. Answer ONLY from the numbered passages below. Cite each claim with the supporting \
passage's [n] marker. If the passages do not support an answer, say \"I don't find that in \
your notes.\" — never answer from general knowledge.";

/// The condensation prompt (flow ④ step 0): rewrite a follow-up into a
/// standalone retrieval query ("tell me more about that" → a query that names
/// *that*). The reply is used as search input, never shown to the human.
pub const CONDENSE_SYSTEM_PROMPT: &str = "Rewrite the user's latest message as one standalone \
search query that names its subject explicitly, using the earlier conversation to fill in \
any pronouns or references. Reply with the query text only — no preamble, no quotes.";

/// Step 0 — condense a follow-up into a standalone retrieval query. Multi-turn
/// only; the caller skips this entirely for a single-turn ask. **This step can
/// never break chat**: on provider failure, a cancelled stream, or a blank
/// reply, it degrades to the raw question text — a worse retrieval query, not
/// an error. Tokens are consumed internally (the condensed query is search
/// input, not answer text), so nothing streams up from this call.
pub fn condense_query(llm: &dyn LlmProvider, question: &str, history: &[ChatTurn]) -> String {
    let req = ChatRequest {
        kind: RequestKind::Condense,
        system: CONDENSE_SYSTEM_PROMPT.to_string(),
        turns: turns_with_question(history, question),
        passages: Vec::new(),
    };
    match llm.complete(&req, &mut |_| ControlFlow::Continue(())) {
        Ok(c) if !c.cancelled && !c.text.trim().is_empty() => c.text.trim().to_string(),
        Ok(_) => {
            tracing::debug!(
                target: "b2::chat",
                "condensation returned nothing usable; degrading to the raw question"
            );
            question.to_string()
        }
        Err(e) => {
            tracing::debug!(
                target: "b2::chat",
                error = %e,
                "condensation failed; degrading to the raw question"
            );
            question.to_string()
        }
    }
}

/// Step 2 — assemble the grounded chat request: the system prompt, the
/// conversation ending in the current question, and the numbered passages.
pub fn build_request(
    question: &str,
    history: &[ChatTurn],
    passages: Vec<ContextPassage>,
) -> ChatRequest {
    ChatRequest {
        kind: RequestKind::Chat,
        system: GROUNDED_SYSTEM_PROMPT.to_string(),
        turns: turns_with_question(history, question),
        passages,
    }
}

/// The conversation as the provider sees it: the adapter-held history, oldest
/// first, with the current question appended as the final user turn.
fn turns_with_question(history: &[ChatTurn], question: &str) -> Vec<ChatTurn> {
    let mut turns = history.to_vec();
    turns.push(ChatTurn::user(question));
    turns
}

/// Step 4 (the parse half) — the distinct `[n]` markers in an answer that
/// actually name a passage: 1-based, `n <= passage_count`, ascending. A
/// hallucinated marker (`[0]`, `[99]` over ten passages, digits too long to
/// parse) simply doesn't resolve — it stays in the answer text verbatim
/// (model output is never rewritten) and contributes no citation. The façade
/// maps each surviving marker onto its passage for the display view.
pub fn cited_markers(text: &str, passage_count: usize) -> Vec<usize> {
    let mut found = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // A byte-wise scan is UTF-8-safe: '[', ']' and ASCII digits never occur
        // as continuation bytes, so a multi-byte character can't fake a marker.
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b']' {
                if let Ok(n) = text[i + 1..j].parse::<usize>() {
                    if (1..=passage_count).contains(&n) {
                        found.insert(n);
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    found.into_iter().collect()
}
