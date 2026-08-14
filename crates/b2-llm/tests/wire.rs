//! The wire path end to end, over a real socket: request building, the HTTP
//! round trip, and the streamed response read back through [`LlmProvider`].
//!
//! The unit tests in `src/sse.rs` feed the parser canned bytes; these prove the
//! part canned bytes can't — that the request B2 actually sends is the one an
//! OpenAI-compatible server expects, and that the answer survives the socket.
//! The "server" is a scripted [`TcpListener`] on loopback, so this needs **no
//! model, no network, and no new dependency**, and it stays deterministic: the
//! script says how many connections to accept and exactly what to write back.

use b2_core::chat::build_request;
use b2_core::llm::{ContextPassage, LlmProvider};
use b2_llm::{LlmConfig, OpenAiCompatProvider};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::ops::ControlFlow;
use std::thread::JoinHandle;

/// What the scripted server does with one connection.
enum Reply {
    /// Accept, read the request, then close without answering — a pooled
    /// connection the server had already given up on.
    Close,
    /// Write these bytes verbatim (status line, headers, body), then close.
    Raw(String),
}

/// Start a server that handles exactly `script.len()` connections, one per entry.
/// Returns the base URL to configure and a handle yielding the requests it saw.
fn serve(script: Vec<Reply>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in script {
            let (mut socket, _) = listener.accept().expect("accept");
            seen.push(read_request(&mut socket));
            if let Reply::Raw(text) = reply {
                // A failed write means the client hung up first, which some of
                // these cases are *about* — never a reason to fail the thread.
                let _ = socket.write_all(text.as_bytes());
                let _ = socket.flush();
            }
        }
        seen
    });
    (format!("http://127.0.0.1:{port}/v1"), handle)
}

/// Read one HTTP request (head + `content-length` body) as text.
fn read_request(socket: &mut std::net::TcpStream) -> String {
    let mut reader = BufReader::new(socket.try_clone().expect("clone socket"));
    let mut head = String::new();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).expect("read head") == 0 {
            break;
        }
        if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
        let done = line == "\r\n" || line == "\n";
        head.push_str(&line);
        if done {
            break;
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).expect("read body");
    head + &String::from_utf8_lossy(&body)
}

/// An SSE response, framed as a server that streams and then closes.
fn sse_response(frames: &str) -> Reply {
    Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{frames}"
    ))
}

fn provider(base_url: &str) -> OpenAiCompatProvider {
    OpenAiCompatProvider::new(LlmConfig {
        base_url: base_url.to_string(),
        model: "test-model".to_string(),
        api_key: None,
    })
}

/// Collect a completion, streaming every token.
fn complete(p: &OpenAiCompatProvider, tokens: &mut Vec<String>) -> b2_core::Result<String> {
    let req = build_request(
        "what is memory?",
        &[],
        vec![ContextPassage {
            path: "concepts/memory.md".into(),
            heading_path: None,
            text: "The brain encodes, stores, and retrieves information.".into(),
        }],
    );
    let completion = p.complete(&req, &mut |t| {
        tokens.push(t.to_string());
        ControlFlow::Continue(())
    })?;
    assert!(!completion.cancelled, "a [DONE] stream is a whole answer");
    Ok(completion.text)
}

/// The happy path: what B2 sends is a streaming chat completion carrying the
/// grounded prompt with its numbered passages, and what comes back is the
/// answer, token by token.
#[test]
fn a_streamed_answer_makes_the_round_trip() {
    let (url, server) = serve(vec![sse_response(
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
         data: {\"choices\":[{\"delta\":{\"content\":\"Memory \"}}]}\n\n\
         : keep-alive\n\n\
         data: {\"choices\":[{\"delta\":{\"content\":\"is [1].\"}}]}\n\n\
         data: [DONE]\n\n",
    )]);

    let mut tokens = Vec::new();
    let text = complete(&provider(&url), &mut tokens).expect("the answer streams");
    assert_eq!(tokens, ["Memory ", "is [1]."]);
    assert_eq!(text, "Memory is [1].");

    let request = server.join().expect("server thread").remove(0);
    assert!(
        request.starts_with("POST /v1/chat/completions"),
        "the one endpoint shape: {request}"
    );
    assert!(request.contains("accept: text/event-stream"), "{request}");
    assert!(
        request.contains("accept-encoding: identity"),
        "compression would buffer the stream: {request}"
    );
    assert!(
        !request.to_lowercase().contains("authorization:"),
        "a local runtime is sent no key: {request}"
    );
    assert!(request.contains("\"stream\":true"), "{request}");
    assert!(
        request.contains("[1] concepts/memory.md"),
        "the grounded prompt carries its numbered passages: {request}"
    );
}

/// A connection the server had already closed costs a retry, not the answer.
/// `ureq` won't retry a POST itself (non-idempotent), so this is B2's own
/// one-shot resend — safe precisely because no response existed to have produced
/// anything. The scripted server proves it happened: two connections, one answer.
#[test]
fn a_request_that_meets_a_dead_connection_is_sent_once_more() {
    let (url, server) = serve(vec![
        Reply::Close,
        sse_response(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Memory [1].\"}}]}\n\n\
             data: [DONE]\n\n",
        ),
    ]);

    let mut tokens = Vec::new();
    let text = complete(&provider(&url), &mut tokens).expect("the retry carries the answer");
    assert_eq!(text, "Memory [1].");
    assert_eq!(
        server.join().expect("server thread").len(),
        2,
        "the request was delivered twice — once into the void, once for real"
    );
}

/// An HTTP refusal carries the server's own explanation ("model not found, try
/// pulling it first" is the one every Ollama user meets), so `B2_DEBUG` shows the
/// fix even though the user-facing line stays generic.
#[test]
fn an_http_refusal_keeps_the_servers_explanation() {
    let body = r#"{"error":{"message":"model \"test-model\" not found, try pulling it first"}}"#;
    let (url, server) = serve(vec![Reply::Raw(format!(
        "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ))]);

    let mut tokens = Vec::new();
    let err = complete(&provider(&url), &mut tokens).expect_err("a 404 is a failed call");
    let detail = err.to_string();
    assert!(detail.contains("404"), "{detail}");
    assert!(detail.contains("try pulling it first"), "{detail}");
    assert!(tokens.is_empty(), "nothing was streamed");
    server.join().expect("server thread");
}

/// Nothing listening is [`b2_llm::LlmError::Unreachable`] — the typed error the
/// adapters turn into "is Ollama running?" (E4). Probing a port that was just
/// released is the cheapest honest way to have nothing listening.
#[test]
fn probing_a_dead_endpoint_reports_it_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let err = provider(&format!("http://127.0.0.1:{port}/v1"))
        .probe()
        .expect_err("nothing is listening");
    assert!(
        matches!(err, b2_llm::LlmError::Unreachable { .. }),
        "got {err:?}"
    );
}

/// A server that answers `GET /models` with something other than a model list —
/// a 404, an HTML page — is still a *reachable* server, and the probe must not
/// refuse it: not every OpenAI-compatible endpoint implements that path.
#[test]
fn probing_tolerates_a_server_that_serves_no_model_list() {
    let (url, server) = serve(vec![Reply::Raw(
        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string(),
    )]);
    provider(&url)
        .probe()
        .expect("an HTTP answer means something is listening");
    server.join().expect("server thread");
}

/// When it *does* serve a model list, the configured model has to be in it —
/// caught here rather than as a 404 halfway through the first question.
#[test]
fn probing_catches_a_model_the_server_does_not_serve() {
    let body = r#"{"object":"list","data":[{"id":"llama3.2:latest"}]}"#;
    let (url, server) = serve(vec![Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ))]);

    let err = provider(&url).probe().expect_err("test-model isn't served");
    match err {
        b2_llm::LlmError::ModelMissing { available, .. } => {
            assert_eq!(available, ["llama3.2:latest"], "the list is shown as-is")
        }
        other => panic!("expected a missing-model error, got {other:?}"),
    }
    server.join().expect("server thread");
}
