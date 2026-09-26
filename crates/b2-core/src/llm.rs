//! The chat seam — `LlmProvider`, the second enumerated AI seam (ADR-0005). Sibling of
//! [`crate::embed`]: the engine is built and tested against the deterministic [`FakeLlm`],
//! and `b2-llm`'s real provider drops in through the same trait with no schema or flow
//! change.
//!
//! Two deliberate contrasts with the embedder seam:
//!
//! - **No index identity** (contrast ADR-0007): chat output is never stored, so
//!   [`LlmProvider::model_id`] is display only — no `meta` row, no reindex on a model
//!   swap, which is what makes "change models at any time" true by construction.
//! - **Streaming is the contract, not a nicety**: tokens flow up through a callback, whose
//!   return steers cooperative cancellation at token granularity. In the trait from day
//!   one because retrofitting it would touch every implementor and call site.
//!
//! Sync, no runtime (ADR-0011): cancellation is returning early from a blocking read loop.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::ops::ControlFlow;

/// One side of a chat turn. `User` turns are the human's; `Assistant` turns are
/// prior model answers the adapter carried forward (session-only history — a
/// persisted transcript would be B2-derived state outside Markdown, S4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One turn of the conversation, oldest first in [`ChatRequest::turns`].
///
/// Serializable **both ways**, unlike the read-only view types: history is the adapter's
/// and session-only, so a GUI carrying a conversation across the IPC hands it back turn by
/// turn rather than defining a parallel DTO. It crosses a process boundary; it never
/// reaches disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: Role,
    pub content: String,
}

impl ChatTurn {
    /// A turn the human typed — the shape adapters append per question asked.
    pub fn user(content: &str) -> Self {
        Self {
            role: Role::User,
            content: content.to_string(),
        }
    }

    /// A prior model answer the adapter carries forward as context.
    pub fn assistant(content: &str) -> Self {
        Self {
            role: Role::Assistant,
            content: content.to_string(),
        }
    }
}

/// Which flow-④ step a request serves — carried **structurally** so a provider
/// (or the fake) never infers it from prompt text, and so a grounded request
/// whose retrieval came back empty is never mistaken for condensation (they
/// both carry no passages, for different reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// Step 0: rewrite the latest turn into a standalone retrieval query.
    Condense,
    /// Steps 2–3: answer the question from the numbered passages.
    Chat,
    /// A tool-using turn ([`ChatRequest::tools`] is what the model may call): the
    /// passages reach the model inside tool results, so [`ChatRequest::passages`] is
    /// the *citation ledger* for this kind and is not rendered into the system message.
    Agent,
}

/// One B2 tool the model may call — a read-only `Vault` op described for the model.
/// `parameters` is a JSON Schema object, passed to the provider as is (the OpenAI
/// `function.parameters` shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// One call the model asked for. `arguments` is the model's JSON **text**, unparsed:
/// it is untrusted output, so whoever runs the tool parses it and answers a malformed
/// call with an error *result* rather than failing the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The provider's id for the call, echoed back with its result.
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A call and what B2 answered — one step of a tool-using turn, replayed to the model
/// on the next round so it can read what it asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExchange {
    pub call: ToolCall,
    pub result: String,
}

/// What the grounded prompt instructs the model to say when the passages don't
/// support an answer. Declared beside the seam because both sides must agree on
/// it: the flow's system prompt cites it (`chat::GROUNDED_SYSTEM_PROMPT`
/// contains it verbatim — asserted by the suite), and [`FakeLlm`] obeys it for
/// a chat request with no passages.
pub const NO_EVIDENCE_ANSWER: &str = "I don't find that in your notes.";

/// One numbered context passage handed to the model — the retrieval unit of
/// flow ④, carried structured (not pre-rendered) so a fake can read it and a
/// wire client can render it once, via [`ChatRequest::system_message`].
/// Numbering is positional and 1-based: passage `i` is cited as `[i + 1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPassage {
    /// Vault-relative path of the note the passage came from — its identity (L1),
    /// and what a citation resolves back to.
    pub path: String,
    /// The chunk's heading breadcrumb, when the chunker recorded one.
    pub heading_path: Option<String>,
    /// The passage text, verbatim (the chunk's stored text).
    pub text: String,
}

/// What one provider call is asked to complete: a system prompt, the
/// conversation so far (the final turn is the current user message), and the
/// numbered context passages grounding the answer. A condensation request
/// (flow ④ step 0) carries **no passages** — nothing has been retrieved yet;
/// retrieval is what condensation feeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequest {
    /// Which step this request serves (condense vs. grounded chat).
    pub kind: RequestKind,
    /// The instruction text (prompt assembly is core logic — `chat.rs` authors
    /// it, so it is testable against [`FakeLlm`]).
    pub system: String,
    /// The conversation, oldest first; the final turn is the current user
    /// message.
    pub turns: Vec<ChatTurn>,
    /// The numbered passages grounding a chat answer; empty for condensation. For
    /// [`RequestKind::Agent`] these are the passages tool results have handed over so
    /// far — what `[n]` markers resolve against — and are not rendered again.
    pub passages: Vec<ContextPassage>,
    /// The tools the model may call this round; empty for every non-agent request, and
    /// for the last round of an agent turn (which must answer).
    pub tools: Vec<ToolSpec>,
    /// The calls made so far this turn with their results, oldest first — replayed
    /// after [`turns`](Self::turns).
    pub exchanges: Vec<ToolExchange>,
}

impl ChatRequest {
    /// Render the system prompt plus the numbered passage block into the one
    /// system message a wire client sends. Rendering lives here — beside the
    /// structured passages — so every provider numbers passages exactly as
    /// citation resolution ([`crate::chat::cited_markers`]) counts them:
    /// 1-based `[n]`, in passage order.
    pub fn system_message(&self) -> String {
        let mut out = self.system.clone();
        // An agent turn's passages already reached the model inside tool results.
        if self.passages.is_empty() || self.kind == RequestKind::Agent {
            return out;
        }
        out.push_str("\n\nPassages:\n");
        for (i, p) in self.passages.iter().enumerate() {
            out.push('\n');
            out.push_str(&p.block(i + 1));
        }
        out
    }
}

impl ContextPassage {
    /// The passage as the model reads it, cited as `[marker]`: the marker, the path and
    /// any heading breadcrumb on one line, then the text. The one layout, whichever way
    /// a passage reaches the model — the system message's block or a tool result.
    pub fn block(&self, marker: usize) -> String {
        let heading = match &self.heading_path {
            Some(h) => format!(" — {h}"),
            None => String::new(),
        };
        format!("[{marker}] {}{heading}\n{}\n", self.path, self.text)
    }
}

/// How a completion ended and what it produced: everything delivered to the
/// callback so far, plus the completed-vs-cancelled marker — so an adapter can
/// render a truncated answer honestly rather than passing it off as whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// The accumulated text: every token handed to `on_token`, in order,
    /// including the one whose callback returned `Break`.
    pub text: String,
    /// `true` when the stream was cut short — by the callback breaking, or by
    /// the provider's own early stop. `text` is then an honest prefix.
    pub cancelled: bool,
    /// The tools the model asked to run instead of (or before) answering. Empty for a
    /// plain answer, and always empty when the request offered no tools.
    pub tool_calls: Vec<ToolCall>,
}

/// The chat seam (sibling of `Embedder`; invariant M1). Messages in, streamed
/// text out. Unlike `Embedder::model_id`, `model_id` here is for display and
/// logging only — chat carries **no** index identity (contrast M2): nothing is
/// recorded in `meta`, and swapping models never touches the index.
pub trait LlmProvider {
    /// The provider's display name for logs and UI badges — never an identity
    /// anything keys on (no `meta` row; contrast `Embedder::model_id`).
    fn model_id(&self) -> &str;

    /// Stream a completion. `on_token` receives tokens as they arrive and
    /// steers the stream: returning `ControlFlow::Break(())` cancels — the
    /// implementation must stop promptly and drop the connection (the pane's
    /// Esc, a closed pane, the CLI's cancel all land here). Returns the text
    /// accumulated so far plus how the stream ended (completed / cancelled).
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<Completion>;
}

/// The fake provider's display id — the `FAKE_MODEL_ID` sibling. Nothing keys
/// on it (chat carries no index identity); it exists so logs read honestly.
pub const FAKE_LLM_MODEL_ID: &str = "fake-llm-v1";

/// Deterministic provider for tests/dev — the [`crate::embed::FakeEmbedder`] sibling,
/// keyed on [`ChatRequest::kind`]. A **chat** request streams a fixed grounded answer
/// citing every passage it was handed, one `[n]` marker per token, so the suite can assert
/// the whole flow-④ pipeline — and mid-stream cancellation at an exact token — model-free;
/// handed no passages it answers [`NO_EVIDENCE_ANSWER`]. A **condensation** request echoes
/// the latest user turn verbatim. An **agent** request is a two-step script read off the
/// request's structure: while an offered tool that needs no arguments has not been called
/// yet, call each such tool once with `{}`; after that, answer as a chat request does.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeLlm;

impl LlmProvider for FakeLlm {
    /// The fixed display id [`FAKE_LLM_MODEL_ID`] — logging only, like every
    /// provider's.
    fn model_id(&self) -> &str {
        FAKE_LLM_MODEL_ID
    }

    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<Completion> {
        if req.kind == RequestKind::Agent {
            let tool_calls: Vec<ToolCall> = req
                .tools
                .iter()
                .filter(|t| {
                    let needs_args = t
                        .parameters
                        .get("required")
                        .and_then(|r| r.as_array())
                        .is_some_and(|r| !r.is_empty());
                    !needs_args && !req.exchanges.iter().any(|e| e.call.name == t.name)
                })
                .enumerate()
                .map(|(i, t)| ToolCall {
                    id: format!("fake_call_{}", req.exchanges.len() + i + 1),
                    name: t.name.clone(),
                    arguments: "{}".to_string(),
                })
                .collect();
            if !tool_calls.is_empty() {
                return Ok(Completion {
                    text: String::new(),
                    cancelled: false,
                    tool_calls,
                });
            }
        }
        let tokens: Vec<String> = match req.kind {
            RequestKind::Condense => {
                // Echo the question. One token — cancellation scripting
                // belongs to the multi-token chat branch.
                let echo = req
                    .turns
                    .iter()
                    .rev()
                    .find(|t| t.role == Role::User)
                    .map(|t| t.content.clone())
                    .unwrap_or_default();
                if echo.is_empty() {
                    Vec::new()
                } else {
                    vec![echo]
                }
            }
            // Nothing retrieved, nothing to cite: the grounded prompt's
            // no-evidence response, as a real model would give it.
            RequestKind::Chat | RequestKind::Agent if req.passages.is_empty() => {
                vec![NO_EVIDENCE_ANSWER.to_string()]
            }
            RequestKind::Chat | RequestKind::Agent => {
                // A grounded answer citing every passage, marker-per-token.
                let mut t: Vec<String> = vec!["Grounded".into(), " in".into()];
                t.extend((1..=req.passages.len()).map(|n| format!(" [{n}]")));
                t.push(".".into());
                t
            }
        };

        let mut text = String::new();
        for tok in &tokens {
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
