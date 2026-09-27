//! The SSE reader: `data:` JSON chunks ending in `data: [DONE]`, plus the quirks servers
//! vary on (keep-alive comments, CRLF, multi-line `data:`, an error inside a 200).
//!
//! A stream that ends without `[DONE]` is truncated, not broken: the tokens are real, so it
//! returns `cancelled: true`. A garbled frame is an error, since skipping it would turn a
//! protocol mismatch into a silently truncated answer.
//!
//! Tool calls arrive split across frames by `index` (OpenAI) or whole (Ollama), sometimes
//! without an `id`; [`ToolCallParts`] assembles them. Arguments stay JSON text: parsing
//! model output is the tool runner's job.

use crate::provider::ErrorDetail;
use crate::LlmError;
use b2_core::llm::{Completion, ToolCall};
use serde::Deserialize;
use std::io::BufRead;
use std::ops::ControlFlow;

/// Read an SSE response to its end, delivering each content delta to `on_token`. A
/// [`ControlFlow::Break`] returns at once, dropping the reader and the connection: that is
/// the whole of cancellation.
pub(crate) fn stream_completion<R: BufRead>(
    mut reader: R,
    max_tool_calls: usize,
    on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> Result<Completion, LlmError> {
    let mut text = String::new();
    let mut calls = ToolCallParts::new(max_tool_calls);
    // The current event's `data:` payload; an event may span several lines.
    let mut data = String::new();
    let mut line = String::new();
    // A `finish_reason` separates "ended" from "cut off" when no `[DONE]` follows.
    let mut finished = false;

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| LlmError::Stream(format!("could not read the stream: {e}")))?;
        // EOF closes a pending event as a blank line would.
        let eof = read == 0;
        let field = line.trim_end_matches(['\n', '\r']);
        if eof || field.is_empty() {
            match dispatch(&data, &mut text, &mut calls, on_token)? {
                Step::Done => return Ok(completed(text, calls)),
                Step::Cancelled => return Ok(cancelled(text)),
                Step::Finished => finished = true,
                Step::Continue => {}
            }
            if eof {
                if !finished {
                    tracing::debug!(
                        target: "b2::llm",
                        chars = text.len(),
                        "the model stream ended without [DONE]; reporting a partial answer"
                    );
                }
                // A cut-off stream's tool calls are not run.
                return Ok(if finished {
                    completed(text, calls)
                } else {
                    cancelled(text)
                });
            }
            data.clear();
            continue;
        }
        if field.starts_with(':') {
            // A keep-alive comment.
            continue;
        }
        if let Some(value) = field.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value);
        }
        // Other fields (`event:`, `id:`, `retry:`) are ignored, not refused.
    }
}

/// What one dispatched event means for the read loop.
enum Step {
    /// Nothing that ends the stream (deltas delivered, or nothing to deliver).
    Continue,
    /// A `finish_reason` arrived: an EOF from here is a clean ending.
    Finished,
    /// `[DONE]`: the stream is over.
    Done,
    /// The caller's callback asked to stop.
    Cancelled,
}

/// Interpret one complete event payload.
fn dispatch(
    payload: &str,
    text: &mut String,
    calls: &mut ToolCallParts,
    on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> Result<Step, LlmError> {
    let payload = payload.trim();
    if payload.is_empty() {
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
    // An error inside a 200 (model unloaded, context overflow): only catchable here.
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

/// One tool call under assembly.
#[derive(Debug, Default)]
struct PartialCall {
    id: Option<String>,
    name: String,
    /// The arguments as JSON text, concatenated fragment by fragment.
    arguments: String,
}

/// Tool calls under assembly, slotted by the wire's `index`. A delta with an index fills
/// (or extends) that slot; a delta without one is a whole call and takes the next slot.
#[derive(Debug)]
struct ToolCallParts {
    slots: Vec<PartialCall>,
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
        // A different id on a named slot is a new call: some servers send every whole
        // call as `index: 0`.
        if let (Some(id), Some(slot)) = (&part.id, self.slots.get(at)) {
            if slot.id.as_ref().is_some_and(|held| held != id) && !slot.name.is_empty() {
                at = self.slots.len();
            }
        }
        // An absolute cap: `MAX_STREAM_BYTES` doesn't bound the table a sparse `index`
        // can allocate. Refused, never trimmed.
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
            // Empty slots from a sparse index are dropped by `finish`.
            self.slots.resize_with(at + 1, Default::default);
        }
        let Some(slot) = self.slots.get_mut(at) else {
            return Ok(());
        };
        if part.id.is_some() {
            slot.id = part.id;
        }
        if let Some(function) = part.function {
            if let Some(name) = function.name {
                slot.name.push_str(&name);
            }
            match function.arguments {
                Some(serde_json::Value::String(text)) => slot.arguments.push_str(&text),
                // Arguments sent as an object (Ollama): re-serialize so the seam stays text.
                Some(other) if !other.is_null() => slot.arguments.push_str(&other.to_string()),
                _ => {}
            }
        }
        Ok(())
    }

    /// The assembled calls, in index order. Unnamed slots are dropped; a missing id gets a
    /// positional one (it is only echoed back beside the result).
    fn finish(self) -> Vec<ToolCall> {
        self.slots
            .into_iter()
            .enumerate()
            .filter(|(_, call)| !call.name.is_empty())
            .map(|(i, call)| ToolCall {
                id: call
                    .id
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| format!("call_{i}")),
                name: call.name,
                arguments: call.arguments,
            })
            .collect()
    }
}

/// One `data:` chunk of a streamed chat completion; lenient, since servers vary.
#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    /// The mid-stream error frame, the same shape as an error body.
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

/// The incremental payload; may carry content, tool calls, or neither.
#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

/// One fragment of a tool call; which fields arrive varies between servers.
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
        let canned =
            "data:{\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\ndata:[DONE]\r\n\r\n";
        let (result, tokens) = read(canned);
        assert_eq!(tokens, ["hi"]);
        assert!(!result.expect("CRLF framing reads").cancelled);
    }

    #[test]
    fn a_multi_line_data_field_is_joined_as_the_spec_says() {
        let canned =
            "data: {\"choices\":[{\"delta\":\ndata: {\"content\":\"split\"}}]}\n\ndata: [DONE]\n\n";
        let (result, tokens) = read(canned);
        assert_eq!(tokens, ["split"]);
        assert_eq!(result.expect("multi-line data parses").text, "split");
    }

    #[test]
    fn a_mid_stream_error_frame_fails_the_call() {
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
        // The token that broke was delivered, so it is part of the answer.
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
        // OpenAI's shape: two calls interleaved by `index`.
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
        // A ~100-byte frame naming a huge `index` would otherwise allocate every slot
        // below it.
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
        let canned = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"b2_read\",\"arguments\":\"{\\\"no\"}}]}}]}\n\n";
        let completion = read(canned).0.unwrap();
        assert!(completion.cancelled);
        assert!(completion.tool_calls.is_empty());
    }

    #[test]
    fn role_only_and_empty_deltas_deliver_nothing() {
        // The opening role-only frame and an empty delta are not tokens.
        let canned = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n\
                      data: [DONE]\n\n";
        let (result, tokens) = read(canned);
        assert!(tokens.is_empty(), "no token was delivered");
        assert_eq!(result.expect("empty deltas read").text, "");
    }
}
