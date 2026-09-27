//! The provider itself: [`OpenAiCompatProvider`], one `POST {base}/chat/completions` with
//! `stream: true` per call, its SSE response read by [`crate::sse`].
//!
//! Everything here is blocking: stopping on the callback's `Break` is a `return`, with no
//! runtime or cancellation token (ADR-0011).

use crate::sse;
use crate::{LlmConfig, LlmError};
use b2_core::llm::{ChatRequest, Completion, LlmProvider, Role};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Read};
use std::ops::ControlFlow;
use std::time::Duration;

/// How long to wait for the TCP/TLS handshake (a dead localhost port refuses at once).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a single socket read may block: a gap budget, not a generation budget. Wide
/// enough for a large model's first token on a cold CPU.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Overall deadline for [`OpenAiCompatProvider::probe`]; the probe must never become the
/// wait it exists to prevent.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// How many times a chat request may be sent: one retry, only while no response exists.
const SEND_ATTEMPTS: usize = 2;

/// How much of an unstructured error body to keep.
const MAX_ERROR_BODY: usize = 500;

/// The largest streamed answer this crate will read, so an endless server can't exhaust
/// memory. Nothing else bounds it.
const MAX_STREAM_BYTES: u64 = 16 * 1024 * 1024;

/// B2's real chat provider: any OpenAI-compatible endpoint, streamed. The adapters
/// [`probe`](Self::probe) it before handing it to `Vault::ask`.
pub struct OpenAiCompatProvider {
    config: LlmConfig,
    agent: ureq::Agent,
}

impl OpenAiCompatProvider {
    /// Wire a provider to `config`. Opens no connection, so it can't fail.
    pub fn new(config: LlmConfig) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .build();
        Self { config, agent }
    }

    /// Fail fast before a human waits (ADR-0020): `GET {base}/models`.
    ///
    /// 1. Is this a chat endpoint? A transport failure is unreachable; any HTTP refusal is
    ///    a refusal, since a 404 can't be told apart from a wrong path. A 2xx whose body
    ///    isn't a model list is tolerated.
    /// 2. Does it serve the configured model? Checked only against a parsed, non-empty
    ///    list, and leniently: a false refusal would block a working setup.
    pub fn probe(&self) -> Result<(), LlmError> {
        let url = self.config.endpoint("/models");
        let response = match self
            .request(self.agent.get(&url))
            .timeout(PROBE_TIMEOUT)
            .call()
        {
            Ok(r) => r,
            // The caller interprets the status (`setup::probe_setup`).
            Err(ureq::Error::Status(status, response)) => {
                tracing::debug!(target: "b2::llm", status, url, "the model list was refused");
                return Err(LlmError::Refused {
                    endpoint: self.config.base_url.clone(),
                    status,
                    message: error_detail(response),
                });
            }
            Err(ureq::Error::Transport(t)) => return Err(self.unreachable(&t)),
        };
        let listed: Vec<String> = match response
            .into_string()
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_str::<ModelList>(&b).map_err(|e| e.to_string()))
        {
            Ok(list) => list.data.into_iter().map(|m| m.id).collect(),
            // Reachable, but nothing to check the model against.
            Err(e) => {
                tracing::debug!(target: "b2::llm", error = %e, "unparseable model list");
                return Ok(());
            }
        };
        if !listed.is_empty() && !model_listed(&self.config.model, &listed) {
            return Err(LlmError::ModelMissing {
                model: self.config.model.clone(),
                endpoint: self.config.base_url.clone(),
                available: listed,
            });
        }
        Ok(())
    }

    /// One streamed chat completion, keeping the typed [`LlmError`] until the trait
    /// boundary.
    fn stream(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<Completion, LlmError> {
        let url = self.config.endpoint("/chat/completions");
        let body = serde_json::to_string(&WireRequest::from_chat(&self.config.model, req))
            .map_err(|e| LlmError::Stream(format!("could not encode the request: {e}")))?;
        tracing::debug!(
            target: "b2::llm",
            model = self.config.model,
            url,
            passages = req.passages.len(),
            turns = req.turns.len(),
            "streaming a chat completion"
        );
        // One retry: a pooled connection the server closed fails the send, and `ureq`
        // won't retry a POST. Only before a response exists ([`worth_resending`]); an
        // error inside the stream is never retried.
        let mut attempt = 0;
        let response = loop {
            attempt += 1;
            let result = self
                .request(self.agent.post(&url))
                .set("content-type", "application/json")
                .set("accept", "text/event-stream")
                // No gzip: a compression window can hold tokens back.
                .set("accept-encoding", "identity")
                .send_string(&body);
            match result {
                Ok(r) => break r,
                Err(ureq::Error::Status(status, r)) => return Err(http_error(status, r)),
                Err(ureq::Error::Transport(t))
                    if attempt < SEND_ATTEMPTS && worth_resending(&t) =>
                {
                    tracing::debug!(
                        target: "b2::llm",
                        detail = %t,
                        "the request didn't reach the model server; retrying once on a fresh connection"
                    );
                }
                Err(ureq::Error::Transport(t)) => return Err(self.unreachable(&t)),
            }
        };
        // `take` is the only bound on the stream (`into_reader` applies none). Hitting it
        // reads as EOF: the partial text, marked cancelled.
        let completion = sse::stream_completion(
            BufReader::new(response.into_reader()).take(MAX_STREAM_BYTES),
            self.config.max_tool_calls,
            on_token,
        )?;
        tracing::debug!(
            target: "b2::llm",
            chars = completion.text.len(),
            cancelled = completion.cancelled,
            "chat completion ended"
        );
        Ok(completion)
    }

    /// Apply the bearer token, when one is configured.
    fn request(&self, request: ureq::Request) -> ureq::Request {
        match &self.config.api_key {
            Some(key) => request.set("authorization", &format!("Bearer {key}")),
            None => request,
        }
    }

    /// The transport failure, named by the configured endpoint (E4).
    fn unreachable(&self, detail: &ureq::Transport) -> LlmError {
        LlmError::Unreachable {
            endpoint: self.config.base_url.clone(),
            detail: detail.to_string(),
        }
    }
}

impl LlmProvider for OpenAiCompatProvider {
    /// The configured model id, for display and logging only (contrast M2).
    fn model_id(&self) -> &str {
        &self.config.model
    }

    /// Stream a completion, collapsing [`LlmError`] into [`b2_core::Error::Llm`] so
    /// `b2-core` stays free of this crate's types. Adapters see the typed failures through
    /// [`probe`](Self::probe).
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        self.stream(req, on_token).map_err(|e| match e {
            // Keeps its type: a caller that degrades on a failed tool round must not hide it.
            LlmError::TooManyToolCalls { limit } => b2_core::Error::ToolCallLimit { limit },
            other => b2_core::Error::Llm(other.to_string()),
        })
    }
}

/// Is re-sending invisible, because the server can't be generating yet? Yes when it never
/// connected, or the pooled connection was already dead (`Io` + `ConnectionAborted`). A
/// timeout must not retry (the server may be generating); it shares the `Io` kind, so the
/// `io::ErrorKind` separates them.
fn worth_resending(t: &ureq::Transport) -> bool {
    match t.kind() {
        ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed => true,
        ureq::ErrorKind::Io => matches!(
            std::error::Error::source(t)
                .and_then(|s| s.downcast_ref::<std::io::Error>())
                .map(|e| e.kind()),
            Some(
                std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::UnexpectedEof
            )
        ),
        _ => false,
    }
}

/// Does the server's model list name this model? Case-insensitive with an implicit
/// `:latest` on either side, since Ollama accepts `llama3.2` but lists `llama3.2:latest`.
fn model_listed(model: &str, available: &[String]) -> bool {
    fn normalize(s: &str) -> String {
        // Lowercase before stripping, so `:LATEST` strips too.
        let s = s.trim().to_lowercase();
        s.strip_suffix(":latest").unwrap_or(&s).to_string()
    }
    let want = normalize(model);
    available.iter().any(|a| normalize(a) == want)
}

/// An HTTP error response as a diagnosis.
fn http_error(status: u16, response: ureq::Response) -> LlmError {
    LlmError::Http {
        status,
        message: error_detail(response),
    }
}

/// The server's own `error.message`, else a bounded slice of the body.
fn error_detail(response: ureq::Response) -> String {
    let body = response.into_string().unwrap_or_default();
    serde_json::from_str::<WireError>(&body)
        .ok()
        .map(|e| e.error.message())
        .unwrap_or_else(|| truncate(body.trim(), MAX_ERROR_BODY))
}

/// Bound a diagnostic string at a char boundary, marking that it was cut.
fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s.to_string(),
        Some((i, _)) => format!("{}…", &s[..i]),
    }
}

/// The request body: the one wire shape this crate speaks.
#[derive(Debug, Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    /// Always `true` (GH #151).
    stream: bool,
    /// Omitted when empty, so a server with no tool support never sees the key.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool<'a>>,
}

impl<'a> WireRequest<'a> {
    /// Render a [`ChatRequest`]: the system message ([`ChatRequest::system_message`]
    /// numbers the passages), then the turns, oldest first.
    fn from_chat(model: &'a str, req: &'a ChatRequest) -> Self {
        let mut messages = Vec::with_capacity(req.turns.len() + 1);
        messages.push(WireMessage {
            role: "system",
            content: req.system_message(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        });
        messages.extend(req.turns.iter().map(|t| WireMessage {
            role: match t.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            },
            content: t.content.clone(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }));
        // Each call as its own assistant message, then its `tool` result: valid on every
        // server.
        for exchange in &req.exchanges {
            messages.push(WireMessage {
                role: "assistant",
                content: String::new(),
                tool_calls: vec![WireToolCall {
                    id: &exchange.call.id,
                    kind: "function",
                    function: WireFunctionCall {
                        name: &exchange.call.name,
                        arguments: &exchange.call.arguments,
                    },
                }],
                tool_call_id: None,
            });
            messages.push(WireMessage {
                role: "tool",
                content: exchange.result.clone(),
                tool_calls: Vec::new(),
                tool_call_id: Some(&exchange.call.id),
            });
        }
        Self {
            model,
            messages,
            stream: true,
            tools: req
                .tools
                .iter()
                .map(|t| WireTool {
                    kind: "function",
                    function: WireFunction {
                        name: &t.name,
                        description: &t.description,
                        parameters: &t.parameters,
                    },
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<WireToolCall<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

/// A call the model made, replayed to it (`messages[].tool_calls[]`).
#[derive(Debug, Serialize)]
struct WireToolCall<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    function: WireFunctionCall<'a>,
}

#[derive(Debug, Serialize)]
struct WireFunctionCall<'a> {
    name: &'a str,
    /// JSON text.
    arguments: &'a str,
}

/// A tool offered to the model (`tools[]`).
#[derive(Debug, Serialize)]
struct WireTool<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    function: WireFunction<'a>,
}

#[derive(Debug, Serialize)]
struct WireFunction<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

/// `GET /models` — only the ids are read.
#[derive(Debug, Deserialize)]
struct ModelList {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    #[serde(default)]
    id: String,
}

/// An error *response* body, OpenAI-shaped.
#[derive(Debug, Deserialize)]
pub(crate) struct WireError {
    pub(crate) error: ErrorDetail,
}

/// The `error` field: an object with a `message` (OpenAI, Ollama) or a bare string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ErrorDetail {
    Structured { message: String },
    Text(String),
}

impl ErrorDetail {
    pub(crate) fn message(self) -> String {
        match self {
            ErrorDetail::Structured { message } => message,
            ErrorDetail::Text(text) => text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_list_match_is_lenient_about_the_latest_tag() {
        let available = vec!["llama3.2:latest".to_string(), "qwen3:8b".to_string()];
        assert!(model_listed("llama3.2", &available));
        assert!(model_listed("llama3.2:latest", &available));
        assert!(model_listed("Llama3.2", &available));
        assert!(model_listed("qwen3:8b", &available));
        assert!(model_listed(
            "llama3.2",
            &["Llama3.2:LATEST".to_string(), "QWEN3:8B".to_string()]
        ));
        assert!(model_listed("qwen3:8b", &["QWEN3:8B".to_string()]));
    }

    #[test]
    fn model_list_match_still_catches_an_absent_model() {
        let available = vec!["llama3.2:latest".to_string()];
        // A different tag is a different model.
        assert!(!model_listed("qwen3", &available));
        assert!(!model_listed("llama3.2:70b", &available));
        assert!(!model_listed("llama3", &available));
    }

    #[test]
    fn the_wire_request_leads_with_the_assembled_system_message() {
        let req = b2_core::chat::build_request(
            "how does it work?",
            &[b2_core::llm::ChatTurn::user("earlier")],
            vec![b2_core::llm::ContextPassage {
                path: "notes/a.md".into(),
                heading_path: None,
                text: "the passage".into(),
            }],
        );
        let wire = WireRequest::from_chat("llama3.2", &req);
        assert!(wire.stream, "streaming is the contract");
        assert_eq!(wire.messages.len(), 3, "system + history + question");
        assert_eq!(wire.messages[0].role, "system");
        assert!(
            wire.messages[0].content.contains("[1] notes/a.md"),
            "passages are numbered as citation resolution counts them"
        );
        assert_eq!(wire.messages[1].role, "user");
        assert_eq!(wire.messages[2].content, "how does it work?");
    }

    #[test]
    fn a_plain_ask_sends_no_tools_key_at_all() {
        let req = b2_core::chat::build_request("q", &[], Vec::new());
        let body = serde_json::to_string(&WireRequest::from_chat("llama3.2", &req)).unwrap();
        assert!(!body.contains("tools"), "{body}");
        assert!(!body.contains("tool_call"), "{body}");
    }

    #[test]
    fn a_tool_turn_offers_the_tools_and_replays_each_call_with_its_result() {
        use b2_core::llm::{ToolCall, ToolExchange};
        let facts = b2_core::chat::WhyFacts {
            anchor_path: "a.md".into(),
            anchor_title: None,
            candidate_path: "c.md".into(),
            candidate_title: None,
            rank: None,
            z: None,
            linked: false,
            shared_neighbors: Vec::new(),
            pairs: Vec::new(),
            embedded: true,
        };
        let req = b2_core::chat::build_why_agent_request(
            &facts,
            b2_core::chat::why_tools(),
            vec![ToolExchange {
                call: ToolCall {
                    id: "b2_seed_1".into(),
                    name: b2_core::chat::TOOL_PASSAGE_PAIRS.into(),
                    arguments: r#"{"note":"a.md"}"#.into(),
                },
                result: "[1] a.md\nthe passage\n".into(),
            }],
            vec![b2_core::llm::ContextPassage {
                path: "a.md".into(),
                heading_path: None,
                text: "the passage".into(),
            }],
        );
        let body: serde_json::Value =
            serde_json::to_value(WireRequest::from_chat("llama3.2", &req)).unwrap();

        let tools = body["tools"].as_array().expect("tools offered");
        assert_eq!(tools.len(), b2_core::chat::why_tools().len());
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "b2_passage_pairs");
        assert_eq!(tools[0]["function"]["parameters"]["type"], "object");

        let messages = body["messages"].as_array().unwrap();
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["system", "user", "assistant", "tool"]);
        // Passages arrive in the tool result, not also in the system message.
        assert!(!messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("Passages:"));
        let call = &messages[2]["tool_calls"][0];
        assert_eq!(call["id"], "b2_seed_1");
        assert_eq!(call["type"], "function");
        assert_eq!(
            call["function"]["arguments"], r#"{"note":"a.md"}"#,
            "JSON text, not an object"
        );
        assert_eq!(messages[3]["tool_call_id"], "b2_seed_1");
        assert!(messages[3]["content"]
            .as_str()
            .unwrap()
            .contains("the passage"));
    }

    #[test]
    fn an_error_body_parses_structured_or_bare() {
        let structured: WireError =
            serde_json::from_str(r#"{"error":{"message":"model not found","type":"api"}}"#)
                .expect("OpenAI-shaped error parses");
        assert_eq!(structured.error.message(), "model not found");
        let bare: WireError = serde_json::from_str(r#"{"error":"model not found"}"#)
            .expect("bare-string error parses");
        assert_eq!(bare.error.message(), "model not found");
    }

    #[test]
    fn endpoint_tolerates_a_trailing_slash() {
        let with = LlmConfig {
            base_url: "http://localhost:11434/v1/".into(),
            ..LlmConfig::default()
        };
        let without = LlmConfig {
            base_url: "http://localhost:11434/v1".into(),
            ..LlmConfig::default()
        };
        assert_eq!(
            with.endpoint("/chat/completions"),
            without.endpoint("/chat/completions")
        );
    }
}
