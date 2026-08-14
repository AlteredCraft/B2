//! The SSE reader — the wire quirks this crate exists to own (GH #151: "the
//! costs we own knowingly").
//!
//! Server-Sent Events is a line protocol: `field: value` lines, a blank line
//! ending each event, and `:`-leading lines that are comments. OpenAI-compatible
//! servers use exactly one field — `data:` — carrying a JSON chunk per event,
//! and end with the literal `data: [DONE]`. What varies between servers, and
//! what the tests below pin, is everything around that: keep-alive comments
//! during a long prefill, `\r\n` line endings, a `data:` with no space after the
//! colon, a spec-legal multi-line `data:`, an error object arriving *inside* a
//! 200 response, and a stream that just stops.
//!
//! Two endings are deliberately different, because they are different facts:
//!
//! - A stream that ends **without** `[DONE]` (or a `finish_reason`) is
//!   truncated, not broken: the tokens that arrived are real, so the completion
//!   comes back `cancelled: true` — the seam's own words for "the provider's own
//!   early stop", surfaced to the human as a partial answer rather than as a
//!   failure with nothing to show.
//! - A frame that is **garbled** — a `data:` payload that isn't JSON — is an
//!   error. Silently skipping it would turn a protocol mismatch (talking to
//!   something that isn't an OpenAI-compatible endpoint) into a quietly
//!   truncated answer, which is the one outcome nobody can debug.

use crate::provider::ErrorDetail;
use crate::LlmError;
use b2_core::llm::Completion;
use serde::Deserialize;
use std::io::BufRead;
use std::ops::ControlFlow;

/// Read an SSE response to its end, delivering each content delta to `on_token`
/// as it arrives.
///
/// Returns the accumulated text plus how the stream ended. `on_token` steers it:
/// [`ControlFlow::Break`] stops the loop at once and returns, which drops the
/// reader — and with it the connection — at the next scope exit. That is the
/// whole of cancellation (GH #151: "cancellation is just returning early from a
/// blocking read loop").
pub(crate) fn stream_completion<R: BufRead>(
    mut reader: R,
    on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> Result<Completion, LlmError> {
    let mut text = String::new();
    // The current event's `data:` payload — accumulated across lines, since the
    // spec allows an event to carry several and joins them with newlines.
    let mut data = String::new();
    let mut line = String::new();
    // Whether the server declared the answer finished (a `finish_reason`), which
    // is what separates "ended" from "cut off" when no `[DONE]` follows.
    let mut finished = false;

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| LlmError::Stream(format!("could not read the stream: {e}")))?;
        if read == 0 {
            // EOF. A pending event has no blank line to close it, so dispatch it
            // before deciding how the stream ended.
            match dispatch(&data, &mut text, on_token)? {
                Step::Done => return Ok(completed(text)),
                Step::Cancelled => return Ok(cancelled(text)),
                Step::Finished => finished = true,
                Step::Continue => {}
            }
            if !finished {
                tracing::debug!(
                    target: "b2::llm",
                    chars = text.len(),
                    "the model stream ended without [DONE]; reporting a partial answer"
                );
            }
            return Ok(Completion {
                text,
                cancelled: !finished,
            });
        }

        let field = line.trim_end_matches(['\n', '\r']);
        if field.is_empty() {
            // End of event: dispatch what was accumulated, then start the next.
            match dispatch(&data, &mut text, on_token)? {
                Step::Done => return Ok(completed(text)),
                Step::Cancelled => return Ok(cancelled(text)),
                Step::Finished => finished = true,
                Step::Continue => {}
            }
            data.clear();
            continue;
        }
        if field.starts_with(':') {
            // A comment — the keep-alive a server sends while it thinks. Nothing
            // to deliver, but it *is* the signal that the connection is alive.
            continue;
        }
        if let Some(value) = field.strip_prefix("data:") {
            // One optional leading space belongs to the framing, not the value.
            let value = value.strip_prefix(' ').unwrap_or(value);
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value);
        }
        // Every other field (`event:`, `id:`, `retry:`) is framing this wire
        // shape doesn't use. Ignored, not refused: a server is free to send it.
    }
}

/// What one dispatched event means for the read loop.
enum Step {
    /// Nothing that ends the stream (deltas delivered, or nothing to deliver).
    Continue,
    /// The server declared the answer complete (`finish_reason`), but hasn't
    /// sent `[DONE]` yet — so an EOF from here is a clean ending, not a cut one.
    Finished,
    /// `[DONE]`: the stream is over.
    Done,
    /// The caller's callback asked to stop.
    Cancelled,
}

/// Interpret one complete event payload: deliver its content deltas, and report
/// anything that ends the stream.
fn dispatch(
    payload: &str,
    text: &mut String,
    on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> Result<Step, LlmError> {
    let payload = payload.trim();
    if payload.is_empty() {
        // A blank line with no data before it — SSE's own no-op.
        return Ok(Step::Continue);
    }
    if payload == "[DONE]" {
        return Ok(Step::Done);
    }
    let chunk: StreamChunk = serde_json::from_str(payload).map_err(|e| {
        LlmError::Stream(format!(
            "a stream frame was not JSON ({e}) — is the configured URL an OpenAI-compatible endpoint?"
        ))
    })?;
    // An error object inside a 200 response: the server accepted the request,
    // then failed — a model unloaded mid-answer, a context overflow, a rate
    // limit. It arrives as data, so it can only be caught here.
    if let Some(error) = chunk.error {
        return Err(LlmError::Provider(error.message()));
    }
    let mut step = Step::Continue;
    for choice in chunk.choices {
        if let Some(content) = choice.delta.content {
            if !content.is_empty() {
                text.push_str(&content);
                if on_token(&content).is_break() {
                    return Ok(Step::Cancelled);
                }
            }
        }
        if choice.finish_reason.is_some() {
            step = Step::Finished;
        }
    }
    Ok(step)
}

fn completed(text: String) -> Completion {
    Completion {
        text,
        cancelled: false,
    }
}

fn cancelled(text: String) -> Completion {
    Completion {
        text,
        cancelled: true,
    }
}

/// One `data:` chunk of a streamed chat completion. Only the fields B2 acts on
/// are read; `#[serde(default)]` throughout, since which of them a given server
/// sends on a given frame is not something to be strict about.
#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    /// The mid-stream error frame (see [`dispatch`]) — the same shape servers
    /// send as an error *body*, which is why the type is shared.
    #[serde(default)]
    error: Option<ErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Delta,
    /// `"stop"`, `"length"`, … — present on the last frame of an answer.
    #[serde(default)]
    finish_reason: Option<String>,
}

/// The incremental payload. A frame may carry role-only (the first), content, or
/// neither (the last) — all three are ordinary.
#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Read a canned stream, collecting the tokens as an adapter would.
    fn read(canned: &str) -> (Result<Completion, LlmError>, Vec<String>) {
        let mut tokens = Vec::new();
        let result = stream_completion(Cursor::new(canned.as_bytes()), &mut |t| {
            tokens.push(t.to_string());
            ControlFlow::Continue(())
        });
        (result, tokens)
    }

    /// A content frame, as every OpenAI-compatible server sends it.
    fn frame(content: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}}}}]}}\n\n",
            serde_json::to_string(content).unwrap()
        )
    }

    #[test]
    fn deltas_stream_in_order_and_accumulate() {
        let canned = format!(
            "{}{}{}data: [DONE]\n\n",
            frame("Grounded"),
            frame(" in"),
            frame(" [1].")
        );
        let (result, tokens) = read(&canned);
        let completion = result.expect("a well-formed stream reads");
        assert_eq!(tokens, ["Grounded", " in", " [1]."]);
        assert_eq!(completion.text, "Grounded in [1].");
        assert!(!completion.cancelled, "[DONE] is a clean ending");
    }

    #[test]
    fn keep_alive_comments_and_blank_lines_are_not_tokens() {
        // What a server sends while it prefills a long prompt: comments, and the
        // blank lines that frame them. None of it is answer text.
        let canned = format!(
            ": ping\n\n: ping\n\n{}\n\ndata: [DONE]\n\n",
            frame("Grounded").trim_end()
        );
        let (result, tokens) = read(&canned);
        assert_eq!(tokens, ["Grounded"]);
        assert_eq!(result.expect("comments are skipped").text, "Grounded");
    }

    #[test]
    fn crlf_endings_and_a_spaceless_data_field_parse() {
        // Two framing variations that are equally legal and appear in the wild.
        let canned =
            "data:{\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\ndata:[DONE]\r\n\r\n";
        let (result, tokens) = read(canned);
        assert_eq!(tokens, ["hi"]);
        assert!(!result.expect("CRLF framing reads").cancelled);
    }

    #[test]
    fn a_multi_line_data_field_is_joined_as_the_spec_says() {
        // SSE allows an event's payload to span `data:` lines, joined with
        // newlines — which is still one JSON object to us.
        let canned =
            "data: {\"choices\":[{\"delta\":\ndata: {\"content\":\"split\"}}]}\n\ndata: [DONE]\n\n";
        let (result, tokens) = read(canned);
        assert_eq!(tokens, ["split"]);
        assert_eq!(result.expect("multi-line data parses").text, "split");
    }

    #[test]
    fn a_mid_stream_error_frame_fails_the_call() {
        // A 200 that goes wrong afterwards — the quirk a hand-rolled client owns.
        let canned = format!(
            "{}data: {{\"error\":{{\"message\":\"context window exceeded\"}}}}\n\n",
            frame("Groun")
        );
        let (result, tokens) = read(&canned);
        assert_eq!(tokens, ["Groun"], "tokens before the error still arrived");
        match result {
            Err(LlmError::Provider(msg)) => assert!(msg.contains("context window exceeded")),
            other => panic!("expected a provider error, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_string_error_frame_fails_the_call_too() {
        let canned = "data: {\"error\":\"model unloaded\"}\n\n";
        let (result, _) = read(canned);
        match result {
            Err(LlmError::Provider(msg)) => assert_eq!(msg, "model unloaded"),
            other => panic!("expected a provider error, got {other:?}"),
        }
    }

    #[test]
    fn a_truncated_stream_returns_the_partial_answer_as_cancelled() {
        // The server died, or the connection dropped: no [DONE], no
        // finish_reason. The tokens that arrived are real, so they are returned
        // — honestly marked as not the whole answer.
        let canned = format!("{}{}", frame("Grounded"), frame(" in"));
        let (result, tokens) = read(&canned);
        let completion = result.expect("a truncated stream is not an error");
        assert_eq!(tokens, ["Grounded", " in"]);
        assert_eq!(completion.text, "Grounded in");
        assert!(
            completion.cancelled,
            "a stream that stopped early must not pass as a whole answer"
        );
    }

    #[test]
    fn a_finish_reason_ends_the_answer_even_without_done() {
        // Some servers close the connection right after the final frame instead
        // of sending [DONE]. That is a complete answer, not a truncated one.
        let canned = format!(
            "{}data: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n",
            frame("Grounded")
        );
        let (result, tokens) = read(&canned);
        let completion = result.expect("a finished stream reads");
        assert_eq!(tokens, ["Grounded"]);
        assert!(!completion.cancelled);
    }

    #[test]
    fn a_last_event_without_its_blank_line_is_still_dispatched() {
        // EOF closes the final event even when the server didn't.
        let canned = "data: [DONE]";
        let (result, _) = read(canned);
        assert!(!result.expect("EOF closes the last event").cancelled);
    }

    #[test]
    fn a_garbled_frame_is_an_error_not_a_silent_truncation() {
        let canned = format!("{}data: not json at all\n\n", frame("Grounded"));
        let (result, tokens) = read(&canned);
        assert_eq!(tokens, ["Grounded"]);
        match result {
            Err(LlmError::Stream(msg)) => assert!(msg.contains("not JSON")),
            other => panic!("expected a stream error, got {other:?}"),
        }
    }

    #[test]
    fn breaking_the_callback_stops_at_that_token() {
        // Cooperative cancellation at token granularity: the token that broke is
        // part of the answer (it was delivered), and nothing after it is read.
        let canned = format!(
            "{}{}{}data: [DONE]\n\n",
            frame("one"),
            frame(" two"),
            frame(" three")
        );
        let mut tokens = Vec::new();
        let completion = stream_completion(Cursor::new(canned.as_bytes()), &mut |t| {
            tokens.push(t.to_string());
            if tokens.len() == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .expect("a cancelled stream is not an error");
        assert_eq!(tokens, ["one", " two"]);
        assert_eq!(completion.text, "one two");
        assert!(completion.cancelled);
    }

    #[test]
    fn role_only_and_empty_deltas_deliver_nothing() {
        // The opening frame of every OpenAI-shaped stream carries a role and no
        // content; an empty-string delta happens too. Neither is a token, and an
        // adapter that rendered them would flicker.
        let canned = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n\
                      data: [DONE]\n\n";
        let (result, tokens) = read(canned);
        assert!(tokens.is_empty(), "no token was delivered");
        assert_eq!(result.expect("empty deltas read").text, "");
    }
}
