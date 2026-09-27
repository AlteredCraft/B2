//! The wire path end to end over a real socket: the request B2 sends, the HTTP round trip,
//! and the streamed answer. The server is a scripted loopback [`TcpListener`], so this is
//! deterministic and needs no model or network.

use b2_core::chat::build_request;
use b2_core::llm::{ContextPassage, LlmProvider};
use b2_llm::{LlmConfig, OpenAiCompatProvider};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::ops::ControlFlow;
use std::thread::JoinHandle;

/// What the scripted server does with one connection.
enum Reply {
    /// Close without answering: a pooled connection the server gave up on.
    Close,
    /// Write these bytes verbatim (status line, headers, body), then close.
    Raw(String),
}

/// A server handling one connection per script entry. Returns the base URL and a handle
/// yielding the requests it saw.
fn serve(script: Vec<Reply>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in script {
            let (mut socket, _) = listener.accept().expect("accept");
            seen.push(read_request(&mut socket));
            if let Reply::Raw(text) = reply {
                // The client hanging up first is what some cases are about.
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

/// A dead pooled connection costs a retry, not the answer. `ureq` won't retry a POST, so
/// this is B2's one-shot resend, safe because no response existed.
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

/// A refusal keeps the server's own explanation, so `B2_DEBUG` shows the fix.
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

/// Nothing listening is [`b2_llm::LlmError::Unreachable`], which adapters turn into
/// "is Ollama running?" (E4).
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

/// A 200 that isn't a model list is still a reachable endpoint, with nothing to check
/// the model against. The probe's tolerance stops at a 2xx.
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

/// A wrong base URL (`/v1X`) must not probe as connected. A refusal can't be told from an
/// unimplemented route, so the probe reports it and [`b2_llm::refusal_message`] gives the fix.
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
            // The sentence names it, and the adapter no longer has the config.
            assert_eq!(endpoint, &url, "the refusal names what was asked");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    // `Display` is the `B2_DEBUG` line, so the server's words must be in it.
    let debug = err.to_string();
    assert!(debug.contains("404 page not found"), "{debug}");
    assert!(debug.contains(&url), "{debug}");
    server.join().expect("server thread");
}

/// A cloud endpoint with no key: the path is right, the credential isn't.
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

/// A served model list must include the configured model, caught before the first question.
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

/// Hitting the configured cap is typed (`Error::ToolCallLimit`), so a caller that degrades
/// on "no tool support" can tell it apart and refuse to hide it.
#[test]
fn a_reply_past_the_configured_tool_call_cap_fails_with_a_typed_error() {
    let call = |i: usize| {
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":{i},\"id\":\"c{i}\",\"function\":{{\"name\":\"b2_read\",\"arguments\":\"{{}}\"}}}}]}}}}]}}\n\n"
        )
    };
    let three: String = (0..3).map(call).collect::<String>() + "data: [DONE]\n\n";
    let (url, server) = serve(vec![sse_response(&three), sse_response(&three)]);
    let capped = |max_tool_calls: usize| {
        OpenAiCompatProvider::new(LlmConfig {
            base_url: url.clone(),
            model: "test-model".to_string(),
            max_tool_calls,
            ..LlmConfig::default()
        })
    };
    let req = build_request("q", &[], Vec::new());
    let run = |p: &OpenAiCompatProvider| p.complete(&req, &mut |_| ControlFlow::Continue(()));

    let err = run(&capped(2)).unwrap_err();
    assert!(
        matches!(err, b2_core::Error::ToolCallLimit { limit: 2 }),
        "{err:?}"
    );
    assert_eq!(run(&capped(3)).expect("within the cap").tool_calls.len(), 3);
    server.join().expect("server thread");
}
