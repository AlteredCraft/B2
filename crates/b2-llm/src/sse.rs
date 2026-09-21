//! The SSE reader — the wire quirks this crate exists to own.
//!
//! Server-Sent Events is a line protocol: `field: value` lines, a blank line ending each
//! event, `:`-leading comments. OpenAI-compatible servers use exactly one field — `data:` —
//! carrying a JSON chunk per event, ending with the literal `data: [DONE]`. What varies
//! between servers, and what the tests below pin, is everything around that: keep-alive
//! comments during a long prefill, CRLF, a `data:` with no space, a spec-legal multi-line
//! `data:`, an error object arriving *inside* a 200, and a stream that just stops.
//!
//! Two endings are deliberately different, because they are different facts:
//!
//! - A stream that ends **without** `[DONE]` is truncated, not broken: the tokens that
//!   arrived are real, so the completion comes back `cancelled: true` and the human sees a
//!   partial answer rather than a failure with nothing to show.
//! - A **garbled** frame — a `data:` payload that isn't JSON — is an error. Skipping it
//!   would turn a protocol mismatch into a quietly truncated answer, the one outcome
//!   nobody can debug.
//!
//! A **tool call** arrives on the same stream as `delta.tool_calls`, and servers disagree
//! about how: OpenAI splits one call across frames (the id and name first, then the
//! arguments a few characters at a time, all keyed by `index`), Ollama sends each call
//! whole in one frame, and some send no `id` at all. [`ToolCallParts`] assembles all three
//! into the seam's [`ToolCall`]; the arguments stay JSON *text*, because they are model
//! output and parsing them is the judgement of whoever runs the tool.

use crate::provider::ErrorDetail;
use crate::LlmError;
use b2_core::llm::{Completion, ToolCall};
use serde::Deserialize;
use std::io::BufRead;
use std::ops::ControlFlow;

/// Read an SSE response to its end, delivering each content delta to `on_token` as it
/// arrives. Returns the accumulated text plus how the stream ended. `on_token` steers it:
/// [`ControlFlow::Break`] stops the loop at once and returns, which drops the reader — and
/// with it the connection — at the next scope exit. That is the whole of cancellation.
pub(crate) fn stream_completion<R: BufRead>(
    mut reader: R,
    max_tool_calls: usize,
    on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> Result<Completion, LlmError> {
    let mut text = String::new();
    let mut calls = ToolCallParts::new(max_tool_calls);
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
            match dispatch(&data, &mut text, &mut calls, on_token)? {
                Step::Done => return Ok(completed(text, calls)),
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
            // Tool calls from a stream that was cut off are not run: half a call's
            // arguments is not a call.
            return Ok(if finished {
                completed(text, calls)
            } else {
                cancelled(text)
            });
        }

        let field = line.trim_end_matches(['\n', '\r']);
        if field.is_empty() {
            // End of event: dispatch what was accumulated, then start the next.
            match dispatch(&data, &mut text, &mut calls, on_token)? {
                Step::Done => return Ok(completed(text, calls)),
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
    calls: &mut ToolCallParts,
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
        for part in choice.delta.tool_calls {
            calls.absorb(part)?;
        }
        if choice.finish_reason.is_some() {
            step = Step::Finished;
        }
    }
    Ok(step)
}

fn completed(text: String, calls: ToolCallParts) -> Completion {
    Completion {
        text,
        cancelled: false,
        tool_calls: calls.finish(),
    }
}

fn cancelled(text: String) -> Completion {
    Completion {
        text,
        cancelled: true,
        tool_calls: Vec::new(),
    }
}

/// Tool calls under assembly, slotted by the wire's `index`. A delta with an index fills
/// (or extends) that slot; a delta without one is a whole call and takes the next slot.
#[derive(Debug)]
struct ToolCallParts {
    slots: Vec<(Option<String>, String, String)>, // (id, name, arguments)
    /// The most slots this reply may fill ([`crate::LlmConfig::max_tool_calls`]).
    max: usize,
}

impl ToolCallParts {
    fn new(max: usize) -> Self {
        Self {
            slots: Vec::new(),
            max,
        }
    }

    fn absorb(&mut self, part: ToolCallDelta) -> Result<(), LlmError> {
        let mut at = part.index.unwrap_or(self.slots.len());
        // A *different* id on a slot that already names a call is a new call, whatever
        // the index says: some servers send every whole call as `index: 0`, and merging
        // them would run one tool named after two.
        if let (Some(id), Some((Some(held), name, _))) = (&part.id, self.slots.get(at)) {
            if held != id && !name.is_empty() {
                at = self.slots.len();
            }
        }
        // The cap is absolute, not a per-frame jump: `MAX_STREAM_BYTES` bounds the bytes
        // read, not the table a sparse `index` can make of them, and a run of modest
        // jumps adds up to the same allocation as one large one. Refused loudly, never
        // trimmed — see [`LlmError::TooManyToolCalls`].
        if at >= self.max {
            tracing::warn!(
                target: "b2::llm",
                index = at,
                limit = self.max,
                "a model reply exceeded the tool-call cap; failing the call"
            );
            return Err(LlmError::TooManyToolCalls { limit: self.max });
        }
        if at >= self.slots.len() {
            // A sparse index below the cap just leaves empty slots, which `finish` drops.
            self.slots.resize_with(at + 1, Default::default);
        }
        let Some(slot) = self.slots.get_mut(at) else {
            return Ok(());
        };
        if part.id.is_some() {
            slot.0 = part.id;
        }
        if let Some(function) = part.function {
            if let Some(name) = function.name {
                slot.1.push_str(&name);
            }
            match function.arguments {
                Some(serde_json::Value::String(text)) => slot.2.push_str(&text),
                // Arguments sent as an object rather than as JSON text (Ollama's native
                // shape, and some `/v1` shims): re-serialize so the seam stays text.
                Some(other) if !other.is_null() => slot.2.push_str(&other.to_string()),
                _ => {}
            }
        }
        Ok(())
    }

    /// The assembled calls, in index order. A slot that never got a name is not a call;
    /// a call the server gave no id gets a positional one, since the id is only ever
    /// echoed back beside its result.
    fn finish(self) -> Vec<ToolCall> {
        self.slots
            .into_iter()
            .enumerate()
            .filter(|(_, (_, name, _))| !name.is_empty())
            .map(|(i, (id, name, arguments))| ToolCall {
                id: id
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| format!("call_{i}")),
                name,
                arguments,
            })
            .collect()
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
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

/// One fragment of a tool call. Everything optional: which fields a given frame carries
/// is exactly what varies between servers.
#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Read a canned stream, collecting the tokens as an adapter would.
    fn read(canned: &str) -> (Result<Completion, LlmError>, Vec<String>) {
        let mut tokens = Vec::new();
        let result = stream_completion(Cursor::new(canned.as_bytes()), 64, &mut |t| {
            tokens.push(t.to_string());
            ControlFlow::Continue(())
        });
        (result, tokens)
    }

    /// [`read`] under a chosen tool-call cap.
    fn read_capped(canned: &str, max_tool_calls: usize) -> Result<Completion, LlmError> {
        stream_completion(Cursor::new(canned.as_bytes()), max_tool_calls, &mut |_| {
            ControlFlow::Continue(())
        })
    }

    /// One whole tool call in a frame, at `index` when given.
    fn call_frame(index: Option<usize>, name: &str) -> String {
        let index = index.map(|i| format!("\"index\":{i},")).unwrap_or_default();
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{{index}\"function\":{{\"name\":\"{name}\",\"arguments\":\"{{}}\"}}}}]}}}}]}}\n\n"
        )
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
        let completion = stream_completion(Cursor::new(canned.as_bytes()), 64, &mut |t| {
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
    fn a_tool_call_split_across_frames_is_assembled_by_index() {
        // OpenAI's shape: id + name first, then the arguments in pieces; two calls
        // interleaved by `index`.
        let canned = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"b2_read\",\"arguments\":\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_b\",\"function\":{\"name\":\"b2_neighbors\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"note\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.md\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let (result, tokens) = read(canned);
        let completion = result.unwrap();
        assert!(tokens.is_empty(), "a tool call is not answer text");
        assert!(!completion.cancelled);
        assert_eq!(
            completion.tool_calls,
            vec![
                ToolCall {
                    id: "call_a".into(),
                    name: "b2_read".into(),
                    arguments: "{\"note\":\"a.md\"}".into(),
                },
                ToolCall {
                    id: "call_b".into(),
                    name: "b2_neighbors".into(),
                    arguments: "{}".into(),
                },
            ]
        );
    }

    #[test]
    fn a_whole_call_in_one_frame_with_no_id_and_object_arguments_still_parses() {
        // The lenient end: no `index`, no `id`, arguments as an object rather than text.
        let canned = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"name\":\"b2_similar\",\"arguments\":{\"note\":\"a.md\"}}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let completion = read(canned).0.unwrap();
        assert_eq!(completion.tool_calls.len(), 1);
        let call = &completion.tool_calls[0];
        assert_eq!(
            (call.id.as_str(), call.name.as_str()),
            ("call_0", "b2_similar")
        );
        assert_eq!(call.arguments, "{\"note\":\"a.md\"}");
    }

    #[test]
    fn two_whole_calls_sharing_an_index_stay_two_calls() {
        let frame = |id: &str, name: &str| {
            format!(
                "data: {{\"choices\":[{{\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"{id}\",\"function\":{{\"name\":\"{name}\",\"arguments\":\"{{}}\"}}}}]}}}}]}}\n\n"
            )
        };
        let canned = format!(
            "{}{}data: [DONE]\n\n",
            frame("c1", "b2_similar"),
            frame("c2", "b2_neighbors")
        );
        let names: Vec<String> = read(&canned)
            .0
            .unwrap()
            .tool_calls
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["b2_similar", "b2_neighbors"]);
    }

    #[test]
    fn a_sparse_index_past_the_cap_fails_the_call_instead_of_growing_the_table() {
        // The memory bound `MAX_STREAM_BYTES` cannot give: a ~100-byte frame naming a
        // huge `index` would otherwise allocate a slot for every index below it. Any
        // index at or past the cap is refused outright — one frame, or a run of
        // modest jumps that adds up to the same thing.
        let err = read_capped(&call_frame(Some(1_000_000), "b2_read"), 64).unwrap_err();
        assert!(
            matches!(err, LlmError::TooManyToolCalls { limit: 64 }),
            "{err:?}"
        );

        let creeping: String = (1..=7)
            .map(|i| call_frame(Some(i * 10), "b2_read"))
            .collect();
        let err = read_capped(&creeping, 64).unwrap_err();
        assert!(
            matches!(err, LlmError::TooManyToolCalls { limit: 64 }),
            "{err:?}"
        );
        // The error says what to do about it.
        assert!(err.to_string().contains("64"), "{err}");
        assert!(err.to_string().contains(crate::ENV_MAX_TOOL_CALLS), "{err}");
    }

    #[test]
    fn the_cap_is_the_configured_one_and_exactly_that_many_calls_still_pass() {
        let done = "data: [DONE]\n\n";
        let calls = |n: usize| -> String {
            (0..n)
                .map(|_| call_frame(None, "b2_similar"))
                .collect::<String>()
                + done
        };
        // At the cap: fine. One past it: refused — under the default-sized cap and
        // under a small configured one alike.
        assert_eq!(read_capped(&calls(64), 64).unwrap().tool_calls.len(), 64);
        assert!(matches!(
            read_capped(&calls(65), 64).unwrap_err(),
            LlmError::TooManyToolCalls { limit: 64 }
        ));
        assert_eq!(read_capped(&calls(2), 2).unwrap().tool_calls.len(), 2);
        assert!(matches!(
            read_capped(&calls(3), 2).unwrap_err(),
            LlmError::TooManyToolCalls { limit: 2 }
        ));
    }

    #[test]
    fn a_stream_cut_off_mid_call_runs_no_tool() {
        // Half a call's arguments is not a call: a truncated stream reports the partial
        // text as cancelled, and offers nothing to execute.
        let canned = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"b2_read\",\"arguments\":\"{\\\"no\"}}]}}]}\n\n";
        let completion = read(canned).0.unwrap();
        assert!(completion.cancelled);
        assert!(completion.tool_calls.is_empty());
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
