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
        ..LlmConfig::default()
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

/// A **200** whose body isn't a model list — an HTML page, an empty object — is
/// still a reachable chat endpoint: it answered on the right path, and there is
/// simply nothing to check the model against. This is the whole of the probe's
/// tolerance, and it is bounded by *evidence* (a 2xx) rather than by hope.
#[test]
fn probing_tolerates_a_server_that_serves_no_model_list() {
    let body = "<html>not a model list</html>";
    let (url, server) = serve(vec![Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ))]);
    provider(&url)
        .probe()
        .expect("an answer on the right path is a reachable endpoint");
    server.join().expect("server thread");
}

/// The bug this replaced a tolerance to catch: `…:11434/v1X` instead of `/v1`.
///
/// The old probe read *any* HTTP answer as "something is listening, it just may
/// not implement `/models`", so a wrong base URL came back **Connected** and the
/// configuration failed at the first question instead — the exact surprise the
/// probe exists to prevent. A refusal cannot be told from an unimplemented route
/// on this evidence, so the probe reports what it saw and lets
/// [`b2_llm::refusal_message`] give the fix.
#[test]
fn probing_refuses_a_url_that_answers_but_isnt_a_chat_api() {
    let body = "404 page not found";
    let (url, server) = serve(vec![Reply::Raw(format!(
        "HTTP/1.1 404 Not Found\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ))]);
    let err = provider(&url)
        .probe()
        .expect_err("a refused probe is not a connection");
    match &err {
        b2_llm::LlmError::Refused {
            status, endpoint, ..
        } => {
            assert_eq!(*status, 404);
            // The endpoint rides along because the sentence names it, and by the
            // time an adapter prints one it no longer has the config in hand.
            assert_eq!(endpoint, &url, "the refusal names what was asked");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    // `Display` *is* the `B2_DEBUG` line (b2-cli's `user_message` prints
    // `err.to_string()` and nothing else), so the server's own words have to be in
    // it — the user-facing sentence deliberately drops them, and a field missing
    // from the format is a field no adapter can reach.
    let debug = err.to_string();
    assert!(debug.contains("404 page not found"), "{debug}");
    assert!(debug.contains(&url), "{debug}");
    server.join().expect("server thread");
}

/// A cloud endpoint with no key: the path is right, the credential isn't. Also
/// previously *Connected*, and also a failure deferred to the first question.
#[test]
fn probing_catches_an_endpoint_that_refuses_the_key() {
    let body = r#"{"error":{"message":"invalid api key"}}"#;
    let (url, server) = serve(vec![Reply::Raw(format!(
        "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ))]);
    let err = provider(&url).probe().expect_err("401 is not a connection");
    assert!(
        matches!(err, b2_llm::LlmError::Refused { status: 401, .. }),
        "got {err:?}"
    );
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
