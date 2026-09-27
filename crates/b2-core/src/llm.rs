//! The chat seam, `LlmProvider` (ADR-0005). The engine is tested against [`FakeLlm`];
//! `b2-llm`'s real provider drops in through the same trait.
//!
//! Unlike the embedder, chat has no index identity (contrast ADR-0007): output is never
//! stored, so a model swap never reindexes. Streaming is the contract: the token callback's
//! return value cancels. Sync (ADR-0011): cancelling returns early from a blocking read.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::ops::ControlFlow;

/// One side of a chat turn. History is session-only: a persisted transcript would be
/// B2-derived state outside Markdown (S4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One turn of the conversation. Deserializable too, so the desktop hands history back
/// over IPC; it never reaches disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Which flow-④ step a request serves, carried structurally so nothing infers it from
/// prompt text or from an empty passage list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// Step 0: rewrite the latest turn into a standalone retrieval query.
    Condense,
    /// Steps 2–3: answer the question from the numbered passages.
    Chat,
    /// A tool-using turn. Passages reach the model in tool results, so
    /// [`ChatRequest::passages`] is only the citation ledger here.
    Agent,
}

/// One read-only `Vault` op the model may call. `parameters` is a JSON Schema object
/// (OpenAI's `function.parameters`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// One call the model asked for. `arguments` is untrusted, unparsed JSON text: a
/// malformed call gets an error result, not a failed turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The provider's id for the call, echoed back with its result.
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A call and B2's answer, replayed to the model on the next round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExchange {
    pub call: ToolCall,
    pub result: String,
}

/// What the model says when the passages don't support an answer. Shared by
/// `chat::GROUNDED_SYSTEM_PROMPT` and [`FakeLlm`].
pub const NO_EVIDENCE_ANSWER: &str = "I don't find that in your notes.";

/// One context passage for flow ④, kept structured until [`ChatRequest::system_message`]
/// renders it. Passage `i` is cited as `[i + 1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPassage {
    /// The source note (L1), what a citation resolves to.
    pub path: String,
    pub heading_path: Option<String>,
    /// The chunk's stored text, verbatim.
    pub text: String,
}

/// What one provider call is asked to complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequest {
    pub kind: RequestKind,
    /// The instruction text, authored in `chat.rs`.
    pub system: String,
    /// The conversation, oldest first; the last turn is the current message.
    pub turns: Vec<ChatTurn>,
    /// The passages grounding the answer; empty for condensation. For an agent turn,
    /// those tool results handed over so far.
    pub passages: Vec<ContextPassage>,
    /// Tools the model may call; empty outside agent turns and on an agent's last round.
    pub tools: Vec<ToolSpec>,
    /// Calls made so far this turn, replayed after [`turns`](Self::turns).
    pub exchanges: Vec<ToolExchange>,
}

impl ChatRequest {
    /// The system prompt plus the numbered passages, as one system message. Numbered here so
    /// every provider matches [`crate::chat::cited_markers`].
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
    /// The passage as the model reads it, cited as `[marker]`, in the system message or a
    /// tool result alike.
    pub fn block(&self, marker: usize) -> String {
        let heading = match &self.heading_path {
            Some(h) => format!(" — {h}"),
            None => String::new(),
        };
        format!("[{marker}] {}{heading}\n{}\n", self.path, self.text)
    }
}

/// A completion's text and whether it was cut short, so a truncated answer is shown as one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Every token handed to `on_token`, including the one that returned `Break`.
    pub text: String,
    /// Cut short by the callback or the provider; `text` is then a prefix.
    pub cancelled: bool,
    /// Tools the model asked to run instead of answering.
    pub tool_calls: Vec<ToolCall>,
}

/// The chat seam (invariant M1): messages in, streamed text out. No index identity
/// (contrast M2).
pub trait LlmProvider {
    /// Display name for logs and UI; nothing keys on it.
    fn model_id(&self) -> &str;

    /// Stream a completion. `on_token` returning `Break` cancels: stop promptly and drop
    /// the connection.
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<Completion>;
}

/// The fake provider's display id.
pub const FAKE_LLM_MODEL_ID: &str = "fake-llm-v1";

/// Deterministic provider for tests and dev. Chat streams an answer citing every passage,
/// one `[n]` per token (so cancellation can land on an exact token), or
/// [`NO_EVIDENCE_ANSWER`] with none. Condense echoes the last user turn. Agent calls each
/// uncalled no-argument tool once with `{}`, then answers as chat.
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
                // Echo the question as one token.
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
            RequestKind::Chat | RequestKind::Agent if req.passages.is_empty() => {
                vec![NO_EVIDENCE_ANSWER.to_string()]
            }
            RequestKind::Chat | RequestKind::Agent => {
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
