//! The provider itself: [`OpenAiCompatProvider`], one `POST
//! {base}/chat/completions` with `stream: true` per call, its SSE response read
//! by [`crate::sse`].
//!
//! Everything here is blocking. That is the design, not a limitation: the seam
//! hands us a `FnMut` callback and asks us to stop when it says `Break`, which
//! over a blocking reader is `return` — no runtime, no channel, no cancellation
//! token (GH #151), and — since GH #174 trimmed `hf-hub`'s default features —
//! no tokio in the `b2` binary's dependency tree either.

use crate::sse;
use crate::{LlmConfig, LlmError};
use b2_core::llm::{ChatRequest, Completion, LlmProvider, Role};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Read};
use std::ops::ControlFlow;
use std::time::Duration;

/// How long to wait for the TCP/TLS handshake. Generous for a cloud endpoint,
/// still fast enough that a dead localhost port fails immediately (it refuses
/// rather than hangs).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a single socket read may block. This is a *gap* budget, not a
/// generation budget: tokens keep arriving, so what it actually bounds is a
/// server that accepted the request and then went silent. Wide enough that a
/// large model's first token on a cold CPU is never mistaken for a hang.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Overall deadline for the [`OpenAiCompatProvider::probe`] round-trip. Short by
/// design: the probe exists to answer "is anything there?" *before* the human
/// waits on an answer, so it must never itself become the wait.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// How many times a chat request may be *sent* before giving up — one retry, and
/// only while no response exists (see the loop in
/// [`OpenAiCompatProvider::stream`]).
const SEND_ATTEMPTS: usize = 2;

/// How much of an error response body to keep when the server didn't send a
/// structured message — enough to be diagnostic, bounded so a stray HTML error
/// page can't become the log.
const MAX_ERROR_BODY: usize = 500;

/// The largest streamed answer this crate will read. Far past any real
/// completion (a 16 MiB answer is not an answer), and small enough that a server
/// sending bytes without end cannot exhaust memory — nothing else bounds it.
const MAX_STREAM_BYTES: u64 = 16 * 1024 * 1024;

/// B2's real chat provider: any OpenAI-compatible endpoint, streamed.
///
/// Construct it, optionally [`probe`](Self::probe) it (the adapters do, so an
/// unreachable server is an error *before* the human types a question), then
/// hand it to `Vault::ask` as a `&dyn LlmProvider`.
pub struct OpenAiCompatProvider {
    config: LlmConfig,
    agent: ureq::Agent,
}

impl OpenAiCompatProvider {
    /// Wire a provider to `config`. Cheap and infallible — it opens no
    /// connection, so a bad URL or a stopped daemon surfaces at
    /// [`probe`](Self::probe) or at the first call, never from a constructor
    /// nobody expects to fail.
    pub fn new(config: LlmConfig) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .build();
        Self { config, agent }
    }

    /// The configuration this provider speaks to — what an adapter names in its
    /// "can't reach the model server at …" message.
    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    /// Fail fast, before a human waits: `GET {base}/models`.
    ///
    /// This is the `b2 init` posture applied to the second seam — "never a
    /// surprise mid-command" (index-engine.md §6). It answers two questions,
    /// and deliberately no others:
    ///
    /// 1. **Is anything listening?** Only a *transport* failure counts as
    ///    unreachable. Any HTTP answer — 404, 401, 405 — means a server is
    ///    there, so an endpoint that simply doesn't implement `/models` (some
    ///    compat servers don't) is never refused on that account.
    /// 2. **Does it serve the configured model?** Checked only when the
    ///    response *parsed* as a non-empty model list, and leniently
    ///    ([`model_listed`]) — the check may only ever catch a real mistake,
    ///    never invent one, because a false refusal here would block a working
    ///    setup.
    ///
    /// The cost is one round trip per process (sub-millisecond on localhost),
    /// paid to turn the most common two setup mistakes into sentences instead of
    /// a failed answer.
    pub fn probe(&self) -> Result<(), LlmError> {
        let url = self.config.endpoint("/models");
        let response = match self
            .request(self.agent.get(&url))
            .timeout(PROBE_TIMEOUT)
            .call()
        {
            Ok(r) => r,
            // The server answered — with a refusal, but it answered. Reachable.
            Err(ureq::Error::Status(status, _)) => {
                tracing::debug!(
                    target: "b2::llm",
                    status,
                    url,
                    "model list unavailable; skipping the model check"
                );
                return Ok(());
            }
            Err(ureq::Error::Transport(t)) => return Err(self.unreachable(&t)),
        };
        // `into_string` over `ureq`'s optional `json` feature: this crate already
        // owns serde_json, and the feature would pull a second copy of the same
        // decision into the dependency list.
        let listed: Vec<String> = match response
            .into_string()
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_str::<ModelList>(&b).map_err(|e| e.to_string()))
        {
            Ok(list) => list.data.into_iter().map(|m| m.id).collect(),
            // A 200 whose body isn't an OpenAI model list: still reachable, and
            // there is nothing to check the model against.
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

    /// One streamed chat completion, from request build to the last token.
    /// Separate from the trait method so the whole wire path keeps the typed
    /// [`LlmError`]; the trait boundary is where it collapses to a message.
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
        // One retry, in exactly one window. Connections are pooled — the probe's,
        // and the previous turn's in a `chat` session — and a server that closed
        // an idle one (or a proxy that did) fails the *send*, not the answer.
        // `ureq` won't retry that itself: its rule is idempotent-methods-only, and
        // a POST isn't one. But a request that never reached a server generated
        // nothing, so re-sending it is invisible — no double answer, no double
        // charge — and the alternative is a working setup that fails one call in
        // a while for a reason the user can do nothing about. Which failures those
        // are is [`worth_resending`]'s judgement, and the window closes the moment
        // a response exists: an error *inside* the stream is never retried,
        // because tokens have already been delivered.
        let mut attempt = 0;
        let response = loop {
            attempt += 1;
            let result = self
                .request(self.agent.post(&url))
                .set("content-type", "application/json")
                .set("accept", "text/event-stream")
                // Ask for no compression: `ureq` would otherwise advertise gzip,
                // and a server that honors it can hold tokens inside a
                // compression window — which turns streaming, the contract, back
                // into waiting.
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
        // `take` is the only bound on the stream: `into_reader` applies none (unlike
        // `into_string`), and the SSE reader grows a line until it meets a newline —
        // so a broken or hostile endpoint sending an endless run of bytes would
        // otherwise be free to exhaust memory. Hitting the cap reads as EOF, which
        // this crate already has an honest ending for: the partial text, marked
        // cancelled.
        let completion = sse::stream_completion(
            BufReader::new(response.into_reader()).take(MAX_STREAM_BYTES),
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

    /// Apply the per-call headers every request shares — today just the bearer
    /// token, and only when one is configured (a local runtime wants no auth).
    fn request(&self, request: ureq::Request) -> ureq::Request {
        match &self.config.api_key {
            Some(key) => request.set("authorization", &format!("Bearer {key}")),
            None => request,
        }
    }

    /// The transport failure, named by the endpoint the human configured — the
    /// input to E4's "is Ollama running?" message.
    fn unreachable(&self, detail: &ureq::Transport) -> LlmError {
        LlmError::Unreachable {
            endpoint: self.config.base_url.clone(),
            detail: detail.to_string(),
        }
    }
}

impl LlmProvider for OpenAiCompatProvider {
    /// The configured model id — display and logging only, like every
    /// provider's (chat carries no index identity; contrast M2).
    fn model_id(&self) -> &str {
        &self.config.model
    }

    /// Stream a completion, collapsing the typed [`LlmError`] into the seam's
    /// [`b2_core::Error::Llm`] message. The type isn't lost so much as *scoped*:
    /// `b2-core` stays free of this crate's types (the `Error::Embed`
    /// precedent), and the failures an adapter needs to act on — unreachable,
    /// missing model — are the ones it can already see through
    /// [`probe`](Self::probe), which it calls itself.
    fn complete(
        &self,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> b2_core::Result<Completion> {
        self.stream(req, on_token)
            .map_err(|e| b2_core::Error::Llm(e.to_string()))
    }
}

/// Is this transport failure one where re-sending the request is *invisible* —
/// that is, one where the server cannot already be generating an answer?
///
/// Two families qualify, and nothing else does:
///
/// - **Never connected** (`Dns`, `ConnectionFailed`): no server saw anything.
/// - **The connection was already dead** (`Io` over a closed/reset/broken
///   socket): the pooled-connection case this retry exists for — measured, it
///   arrives as `Io` + `ConnectionAborted`.
///
/// A **timeout** is the case that must *not* retry, and the reason this function
/// isn't just "any transport error": a server that took the request and went
/// quiet may be loading a model or already generating, so re-sending would ask
/// for the same expensive work twice. `ureq` reports it in the same `Io` kind as
/// a dead socket, so the underlying `io::ErrorKind` is what separates them.
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

/// Does the server's model list name this model? Lenient on purpose, in the one
/// direction that matters: Ollama accepts `llama3.2` and *lists*
/// `llama3.2:latest`, so an exact comparison would refuse the single most common
/// working configuration. Comparison is case-insensitive with an implicit
/// `:latest` tag on either side; everything else must match, so a genuinely
/// un-pulled model is still caught.
fn model_listed(model: &str, available: &[String]) -> bool {
    fn normalize(s: &str) -> String {
        // Lowercase *before* stripping: a server that lists `Llama3.2:LATEST`
        // must normalize to the same thing `llama3.2` does, or the check would
        // invent the mistake it exists to catch.
        let s = s.trim().to_lowercase();
        s.strip_suffix(":latest").unwrap_or(&s).to_string()
    }
    let want = normalize(model);
    available.iter().any(|a| normalize(a) == want)
}

/// An HTTP error response turned into a diagnosis: the provider's own
/// `error.message` when it sent one (every OpenAI-compatible server does for
/// the errors that matter — "model not found, try pulling it first"), else a
/// bounded slice of whatever it did send.
fn http_error(status: u16, response: ureq::Response) -> LlmError {
    let body = response.into_string().unwrap_or_default();
    let message = serde_json::from_str::<WireError>(&body)
        .ok()
        .map(|e| e.error.message())
        .unwrap_or_else(|| truncate(body.trim(), MAX_ERROR_BODY));
    LlmError::Http { status, message }
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
    /// Always `true` — streaming is the contract, not an option (GH #151).
    stream: bool,
}

impl<'a> WireRequest<'a> {
    /// Render a [`ChatRequest`] onto the wire: the assembled system message
    /// first (prompt + numbered passages — [`ChatRequest::system_message`] owns
    /// that rendering, so every provider numbers passages exactly as citation
    /// resolution counts them), then the turns, oldest first.
    fn from_chat(model: &'a str, req: &'a ChatRequest) -> Self {
        let mut messages = Vec::with_capacity(req.turns.len() + 1);
        messages.push(WireMessage {
            role: "system",
            content: req.system_message(),
        });
        messages.extend(req.turns.iter().map(|t| WireMessage {
            role: match t.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            },
            content: t.content.clone(),
        }));
        Self {
            model,
            messages,
            stream: true,
        }
    }
}

#[derive(Debug, Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: String,
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

/// The `error` field two ways, because servers disagree: OpenAI (and Ollama's
/// compat surface) send an object with a `message`; some send a bare string.
/// Both are the same fact, so both parse.
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
        // The configuration everyone actually types, against what Ollama lists.
        let available = vec!["llama3.2:latest".to_string(), "qwen3:8b".to_string()];
        assert!(model_listed("llama3.2", &available));
        assert!(model_listed("llama3.2:latest", &available));
        assert!(model_listed("Llama3.2", &available));
        assert!(model_listed("qwen3:8b", &available));
        // The tag is case-insensitive too: normalizing has to lowercase before it
        // strips, or a server that shouts its tag looks like a missing model.
        assert!(model_listed(
            "llama3.2",
            &["Llama3.2:LATEST".to_string(), "QWEN3:8B".to_string()]
        ));
        assert!(model_listed("qwen3:8b", &["QWEN3:8B".to_string()]));
    }

    #[test]
    fn model_list_match_still_catches_an_absent_model() {
        let available = vec!["llama3.2:latest".to_string()];
        // A different tag of the same model is a different model — it has to be
        // pulled, so the check must not wave it through.
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
