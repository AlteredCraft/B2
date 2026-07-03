//! Request construction and response parsing for the Claude relator — the pure,
//! network-free half, so it is unit-testable in CI (the fast suite) without a key.
//!
//! Structured output is achieved by **forcing a single tool call**: the request
//! declares one `classify_relation` tool whose `input_schema` pins `relation` to the
//! closed core verb set, and sets `tool_choice` to force it. A successful response
//! therefore carries exactly one `tool_use` block we parse into a [`Proposal`] (or a
//! decline). No free-text JSON to coax or repair.

use b2_core::relate::{Candidate, NoteCtx, Proposal};
use b2_core::relation;
use serde_json::{json, Value};

/// The forced tool's name — matched when parsing the response.
const TOOL_NAME: &str = "classify_relation";

/// Upper bound on each note's body sent to the model, in chars. Notes in a PKM are
/// usually short; this only guards against a pathological outlier inflating cost.
/// Bounded — not silent about mechanism: an over-long note is clipped with a marker.
const MAX_NOTE_CHARS: usize = 8000;

/// A one-line, directed (anchor → candidate) gloss per core verb, to steer the model
/// toward the right verb and direction. Falls back to the bare verb for anything not
/// listed (keeps this in sync-by-tolerance with [`relation::CORE`]).
fn gloss(verb: &str) -> &'static str {
    match verb {
        "references" => "the anchor mentions or points to the candidate",
        "relates" => "the two notes are topically related, no stronger link (symmetric)",
        "elaborates" => "the anchor expands on or details the candidate",
        "supports" => "the anchor gives evidence or argument for the candidate",
        "refutes" => "the anchor argues against the candidate",
        "contradicts" => "the two notes make incompatible claims (symmetric)",
        "example-of" => "the anchor is a concrete instance of the candidate",
        "part-of" => "the anchor is a component of the candidate",
        "supersedes" => "the anchor replaces or obsoletes the candidate",
        "derived-from" => "the anchor is derived from or based on the candidate",
        _ => "",
    }
}

/// The closed core verb set, as `&str`s, for the tool schema's `enum`.
fn core_verbs() -> Vec<&'static str> {
    relation::CORE.iter().map(|c| c.verb).collect()
}

/// The system prompt: the task, the closed verb vocabulary with directed glosses, and
/// the decline-by-default stance (candidate generation over-produces).
fn system_prompt() -> String {
    let verbs: String = relation::CORE
        .iter()
        .map(|c| format!("  - {}: {}\n", c.verb, gloss(c.verb)))
        .collect();
    format!(
        "You classify whether a directed, typed connection exists FROM an ANCHOR note \
TO a CANDIDATE note in a personal knowledge vault.\n\n\
Candidate pairs are surfaced by semantic similarity and deliberately over-produce, so \
most pairs are merely similar, not genuinely connected. Decline (connected = false) \
unless there is a specific, typed relationship a careful author would record by hand. \
Being merely about the same topic is not enough — that is at most `relates`, and even \
`relates` should be reserved for a real thematic link, not incidental overlap.\n\n\
If connected, pick exactly ONE relation verb describing the anchor → candidate \
direction, from this closed set:\n{verbs}\n\
Direction matters: `relates` and `contradicts` are symmetric; the others are \
directional — choose the direction that fits the anchor pointing at the candidate.\n\n\
For the explanation, write ONE concrete sentence citing what in the notes justifies \
the link — it is shown to the user and stored verbatim if they accept. Set confidence \
in [0.0, 1.0].\n\n\
Call the {TOOL_NAME} tool with your verdict."
    )
}

/// The forced tool definition. `relation`'s `enum` is the closed core set, so the
/// model can only emit a core verb (the pipeline re-validates, never trusting it).
fn tool_definition() -> Value {
    json!({
        "name": TOOL_NAME,
        "description": "Record whether a directed, typed connection exists from the anchor note to the candidate note.",
        "input_schema": {
            "type": "object",
            "properties": {
                "connected": {
                    "type": "boolean",
                    "description": "true only if a specific typed relationship exists; false to decline."
                },
                "relation": {
                    "type": "string",
                    "enum": core_verbs(),
                    "description": "The relation verb (anchor → candidate). Required when connected is true."
                },
                "explanation": {
                    "type": "string",
                    "description": "One concrete sentence citing the justification. Required when connected is true."
                },
                "confidence": {
                    "type": "number",
                    "description": "0.0–1.0 confidence in the proposed connection."
                }
            },
            "required": ["connected"]
        }
    })
}

/// Clip `text` to [`MAX_NOTE_CHARS`], appending a visible marker when truncated so
/// the model (and any human reading the request) knows the note was cut.
fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_NOTE_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_NOTE_CHARS).collect();
    format!("{head}\n…[note truncated]")
}

/// The user turn: anchor, candidate, and the evidence chunk that surfaced the pair.
fn user_content(anchor: &NoteCtx, candidate: &Candidate) -> String {
    let atitle = anchor.title.unwrap_or("(untitled)");
    let ctitle = candidate.note.title.unwrap_or("(untitled)");
    format!(
        "ANCHOR NOTE\nTitle: {atitle}\n{}\n\n\
CANDIDATE NOTE\nTitle: {ctitle}\n{}\n\n\
EVIDENCE (the passage from the candidate that surfaced this pair):\n{}",
        clip(anchor.text),
        clip(candidate.note.text),
        clip(candidate.evidence_chunk),
    )
}

/// Build the full `POST /v1/messages` request body for one candidate pair.
pub(crate) fn build_request(
    model: &str,
    max_tokens: u32,
    anchor: &NoteCtx,
    candidate: &Candidate,
) -> Value {
    json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system_prompt(),
        "tools": [tool_definition()],
        "tool_choice": { "type": "tool", "name": TOOL_NAME },
        "messages": [
            { "role": "user", "content": user_content(anchor, candidate) }
        ]
    })
}

/// Parse a Messages-API response body into a verdict.
///
/// `Ok(None)` is a **decline** — either the model returned `connected = false`, or it
/// fired but gave no usable verb (degraded to a decline rather than aborting a whole
/// vault run over one odd response). `Err(msg)` means the response had no parseable
/// `classify_relation` tool call at all (e.g. a refusal or malformed body), which the
/// caller surfaces as [`b2_core::Error::Relator`].
pub(crate) fn interpret(body: &Value) -> std::result::Result<Option<Proposal>, String> {
    let input = body
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|block| {
            block.get("type").and_then(Value::as_str) == Some("tool_use")
                && block.get("name").and_then(Value::as_str) == Some(TOOL_NAME)
        })
        .and_then(|block| block.get("input"))
        .ok_or_else(|| "model returned no classification".to_string())?;

    let connected = input
        .get("connected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !connected {
        return Ok(None);
    }

    // Connected but no usable verb → degrade to a decline (robust over a batch).
    let verb = input
        .get("relation")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(verb) = verb else {
        return Ok(None);
    };

    let explanation = input
        .get("explanation")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("proposed `{verb}` connection"));

    let confidence = input
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);

    Ok(Some(Proposal {
        edge_type: verb.to_string(),
        explanation,
        confidence,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor() -> NoteCtx<'static> {
        NoteCtx {
            b2id: "A",
            title: Some("Anchor"),
            text: "anchor body",
        }
    }

    fn candidate() -> Candidate<'static> {
        Candidate {
            note: NoteCtx {
                b2id: "B",
                title: Some("Cand"),
                text: "candidate body",
            },
            evidence_chunk: "evidence",
            signal: "semantic:maxsim",
            score: 0.9,
        }
    }

    #[test]
    fn request_forces_the_tool_and_pins_the_verbs() {
        let req = build_request("claude-opus-4-8", 1024, &anchor(), &candidate());
        assert_eq!(req["model"], "claude-opus-4-8");
        assert_eq!(req["tool_choice"]["type"], "tool");
        assert_eq!(req["tool_choice"]["name"], TOOL_NAME);
        // The schema's enum is exactly the closed core set.
        let enum_vals = req["tools"][0]["input_schema"]["properties"]["relation"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(enum_vals.len(), relation::CORE.len());
        assert!(enum_vals.iter().any(|v| v == "references"));
        // The evidence chunk reaches the user turn.
        assert!(req["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("evidence"));
    }

    #[test]
    fn interprets_a_fired_proposal() {
        let body = json!({
            "content": [
                { "type": "text", "text": "thinking..." },
                {
                    "type": "tool_use",
                    "name": TOOL_NAME,
                    "input": {
                        "connected": true,
                        "relation": "supports",
                        "explanation": "The anchor's argument backs the candidate's claim.",
                        "confidence": 0.8
                    }
                }
            ]
        });
        let p = interpret(&body).unwrap().unwrap();
        assert_eq!(p.edge_type, "supports");
        assert_eq!(p.confidence, 0.8);
        assert!(p.explanation.contains("backs"));
    }

    #[test]
    fn interprets_a_decline() {
        let body = json!({
            "content": [{
                "type": "tool_use",
                "name": TOOL_NAME,
                "input": { "connected": false }
            }]
        });
        assert_eq!(interpret(&body).unwrap(), None);
    }

    #[test]
    fn connected_without_a_verb_degrades_to_decline() {
        let body = json!({
            "content": [{
                "type": "tool_use",
                "name": TOOL_NAME,
                "input": { "connected": true, "relation": "  " }
            }]
        });
        assert_eq!(interpret(&body).unwrap(), None);
    }

    #[test]
    fn confidence_is_clamped_and_defaulted() {
        let over = json!({"content":[{"type":"tool_use","name":TOOL_NAME,
            "input":{"connected":true,"relation":"relates","confidence":5.0}}]});
        assert_eq!(interpret(&over).unwrap().unwrap().confidence, 1.0);
        let missing = json!({"content":[{"type":"tool_use","name":TOOL_NAME,
            "input":{"connected":true,"relation":"relates"}}]});
        assert_eq!(interpret(&missing).unwrap().unwrap().confidence, 0.5);
    }

    #[test]
    fn no_tool_call_is_an_error() {
        let body = json!({ "content": [{ "type": "text", "text": "I can't help." }] });
        assert!(interpret(&body).is_err());
    }
}
