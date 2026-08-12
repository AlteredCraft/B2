//! The chat seam — `LlmProvider`, the second enumerated AI seam (invariant M1;
//! GH #151 is the spec of record, GH #153 the cut). Sibling of [`crate::embed`]:
//! the engine is built and tested against the deterministic [`FakeLlm`]; a real
//! provider (`b2-llm`'s sync OpenAI-compat SSE client) drops in through the same
//! trait with no schema or flow change.
//!
//! Two deliberate contrasts with the embedder seam:
//!
//! - **No index identity** (contrast M2): chat output is never stored, so
//!   [`LlmProvider::model_id`] is display/logging only — no `meta` row, no
//!   reindex on model swap, which is what makes "change models at any time"
//!   true by construction.
//! - **Streaming is the contract, not a nicety**: tokens flow up through a
//!   callback as they arrive, and the callback's return value steers
//!   **cooperative cancellation** at token granularity. This is in the trait
//!   from day one because it is the kind of contract that calcifies —
//!   retrofitting it would touch every implementor and call site.
//!
//! Sync, no tokio: the blocking HTTP client lives behind the trait, and
//! cancellation is just returning early from a blocking read loop.

use crate::error::Result;
use std::ops::ControlFlow;

/// One side of a chat turn. `User` turns are the human's; `Assistant` turns are
/// prior model answers the adapter carried forward (session-only history — a
/// persisted transcript would be B2-derived state outside Markdown, S4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One turn of the conversation, oldest first in [`ChatRequest::turns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTurn {
    pub role: Role,
    pub content: String,
}

impl ChatTurn {
    pub fn user(content: &str) -> Self {
        Self {
            role: Role::User,
            content: content.to_string(),
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: Role::Assistant,
            content: content.to_string(),
        }
    }
}

/// One numbered context passage handed to the model — the retrieval unit of
/// flow ④, carried structured (not pre-rendered) so a fake can read it and a
/// wire client can render it once, via [`ChatRequest::system_message`].
/// Numbering is positional and 1-based: passage `i` is cited as `[i + 1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPassage {
    /// Vault-relative path of the note the passage came from.
    pub path: String,
    /// The note's `b2id` — what a citation resolves back to.
    pub b2id: String,
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
    /// The instruction text (prompt assembly is core logic — `chat.rs` authors
    /// it, so it is testable against [`FakeLlm`]).
    pub system: String,
    /// The conversation, oldest first; the final turn is the current user
    /// message.
    pub turns: Vec<ChatTurn>,
    /// The numbered passages grounding a chat answer; empty for condensation.
    pub passages: Vec<ContextPassage>,
}

impl ChatRequest {
    /// Render the system prompt plus the numbered passage block into the one
    /// system message a wire client sends. Rendering lives here — beside the
    /// structured passages — so every provider numbers passages exactly as
    /// citation resolution ([`crate::chat::cited_markers`]) counts them:
    /// 1-based `[n]`, in passage order.
    pub fn system_message(&self) -> String {
        let mut out = self.system.clone();
        if self.passages.is_empty() {
            return out;
        }
        out.push_str("\n\nPassages:\n");
        for (i, p) in self.passages.iter().enumerate() {
            out.push_str(&format!("\n[{}] {}", i + 1, p.path));
            if let Some(h) = &p.heading_path {
                out.push_str(&format!(" — {h}"));
            }
            out.push('\n');
            out.push_str(&p.text);
            out.push('\n');
        }
        out
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
}

/// The chat seam (sibling of `Embedder`; invariant M1). Messages in, streamed
/// text out. Unlike `Embedder::model_id`, `model_id` here is for display and
/// logging only — chat carries **no** index identity (contrast M2): nothing is
/// recorded in `meta`, and swapping models never touches the index.
pub trait LlmProvider {
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

/// Deterministic provider for tests/dev — the [`crate::embed::FakeEmbedder`]
/// sibling. A **chat** request (passages present) streams a fixed grounded
/// answer that cites every passage it was handed, one `[n]` marker per token,
/// so the engine suite can assert the whole flow-④ pipeline — and mid-stream
/// cancellation at an exact token — model-free (E2). A **condensation**
/// request (no passages — the structural mark of step 0) echoes the question:
/// the latest user turn, verbatim.
///
/// The one corner where the two shapes meet: a chat request over *empty*
/// retrieval also carries no passages, so it echoes too. Harmless for a fake
/// whose job is deterministic plumbing — there is nothing to cite either way.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeLlm;

impl LlmProvider for FakeLlm {
    fn model_id(&self) -> &str {
        FAKE_LLM_MODEL_ID
    }

    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<Completion> {
        let tokens: Vec<String> = if req.passages.is_empty() {
            // Condensation: echo the question. One token — cancellation
            // scripting belongs to the multi-token chat branch.
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
        } else {
            // Chat: a grounded answer citing every passage, marker-per-token.
            let mut t: Vec<String> = vec!["Grounded".into(), " in".into()];
            t.extend((1..=req.passages.len()).map(|n| format!(" [{n}]")));
            t.push(".".into());
            t
        };

        let mut text = String::new();
        for tok in &tokens {
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
