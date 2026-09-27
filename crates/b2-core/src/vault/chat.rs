//! Flow ④ on the façade: grounded chat ([`Vault::ask`]) and the tool-using Why? turn
//! ([`Vault::why_similar`], ADR-0022). Prompts and tool schemas live in [`crate::chat`].
//! Chat is a reader: nothing model-derived is stored.

use super::{AnswerView, Citation, ToolUseView, Vault};
use crate::chat;
use crate::db;
use crate::discover;
use crate::error::{Error, Result};
use crate::llm::{ChatRequest, ChatTurn, ContextPassage, LlmProvider, ToolCall, ToolExchange};
use crate::snippet::snippet;
use std::ops::ControlFlow;

impl Vault {
    /// Flow ④, grounded chat: condense → retrieve → stream → cite. The provider is
    /// injected per call: unlike the embedder it carries no index identity (ADR-0007).
    ///
    /// A failed condense falls back to the raw question. `on_token` returning `Break`
    /// cancels. A hallucinated `[n]` resolves to nothing; the text is never rewritten.
    /// A failed answer call is [`Error::Llm`](crate::Error::Llm).
    pub fn ask(
        &self,
        llm: &dyn LlmProvider,
        question: &str,
        history: &[ChatTurn],
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<AnswerView> {
        let _op = tracing::debug_span!(
            target: "b2::vault",
            "ask",
            question,
            multi_turn = !history.is_empty()
        )
        .entered();
        let query = if history.is_empty() {
            question.to_string()
        } else {
            chat::condense_query(llm, question, history)
        };
        let passages: Vec<ContextPassage> = self
            .search_chunks(&query, chat::ASK_PASSAGES)?
            .into_iter()
            .map(|c| ContextPassage {
                path: c.path,
                heading_path: c.heading_path,
                text: c.text,
            })
            .collect();
        tracing::debug!(
            target: "b2::chat",
            passages = passages.len(),
            "retrieved grounding passages"
        );
        let req = chat::build_request(question, history, passages);
        self.stream_answer(llm, &req, on_token)
    }

    /// Why? on a Similar & unlinked row: a grounded, cited explanation of why
    /// `candidate_ref` surfaced for `anchor_ref`. A tool-using turn (ADR-0022) over
    /// [`chat::why_tools`]; every argument defaults to this pair, so `{}` is valid.
    ///
    /// - Round 1 is the lookup round and is never streamed. No tool call, or a failed
    ///   one, degrades to [`chat::build_why_request`]. If the pair lookup was skipped,
    ///   B2 makes it itself (`seeded`): the row was ranked on those pairs.
    /// - At most [`chat::MAX_TOOL_ROUNDS`] rounds of [`chat::MAX_CALLS_PER_ROUND`] calls;
    ///   the last round offers no tools.
    /// - The row's rank and strength at `limit` ride in the prompt.
    ///
    /// Tool calls are untrusted: a bad one gets an `error:` result, never a failed turn.
    /// Passages are numbered once on one ledger, which `[n]` resolves against.
    pub fn why_similar(
        &self,
        llm: &dyn LlmProvider,
        anchor_ref: &str,
        candidate_ref: &str,
        limit: usize,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<AnswerView> {
        let _op = tracing::debug_span!(
            target: "b2::vault",
            "why_similar",
            anchor = anchor_ref,
            candidate = candidate_ref,
            limit
        )
        .entered();
        let anchor = self.resolve_ref(anchor_ref)?;
        let candidate = self.resolve_ref(candidate_ref)?;

        // Same call and limit as the surface, so the same rank and z.
        let served = self.similar(&anchor, limit)?;
        let row = served.iter().position(|s| s.path == candidate);
        let mut desk = ToolDesk {
            limit,
            chunk_ids: Vec::new(),
            passages: Vec::new(),
        };
        let mut facts = chat::WhyFacts {
            anchor_title: db::note_title(&self.conn, &anchor)?,
            candidate_title: db::note_title(&self.conn, &candidate)?,
            rank: row.map(|i| (i + 1, served.len())),
            z: row.and_then(|i| served.get(i)).and_then(|s| s.z),
            linked: self.neighbor_paths(&anchor)?.contains(&candidate),
            shared_neighbors: self
                .shared_neighbors(&anchor, &candidate)?
                .into_iter()
                .collect(),
            // The handoff's half, filled only if the turn degrades to it.
            embedded: true,
            pairs: Vec::new(),
            anchor_path: anchor,
            candidate_path: candidate,
        };

        let mut req =
            chat::build_why_agent_request(&facts, chat::why_tools(), Vec::new(), Vec::new());
        let mut tools_used: Vec<ToolUseView> = Vec::new();
        let mut answer = String::new();
        let mut cancelled = false;
        for round in 1..=chat::MAX_TOOL_ROUNDS {
            if round == chat::MAX_TOOL_ROUNDS {
                req.tools.clear(); // nothing left to do but answer
            }
            // Round 1's text is never shown: an answer there is from nothing.
            let completion = if round == 1 {
                llm.complete(&req, &mut |_| ControlFlow::Continue(()))
            } else {
                llm.complete(&req, on_token)
            };
            let completion = match completion {
                Ok(c) if round == 1 && c.tool_calls.is_empty() => {
                    tracing::debug!(
                        target: "b2::chat",
                        "the model called no tool; handing the evidence over instead"
                    );
                    return self.why_handoff(llm, &mut facts, desk, tools_used, on_token);
                }
                Ok(c) => c,
                // Never degraded: past the cap is a broken or hostile server.
                Err(e @ Error::ToolCallLimit { .. }) => return Err(e),
                // Most often a model with no tool support; a down server fails the
                // handoff too.
                Err(e) if round == 1 => {
                    tracing::debug!(
                        target: "b2::chat",
                        error = %e,
                        "the tool round failed; handing the evidence over without tools"
                    );
                    return self.why_handoff(llm, &mut facts, desk, tools_used, on_token);
                }
                Err(e) => return Err(llm_error(e)),
            };
            if round > 1 {
                answer.push_str(&completion.text);
            }
            if completion.cancelled {
                cancelled = true;
                break;
            }
            if completion.tool_calls.is_empty() || req.tools.is_empty() {
                break;
            }
            for call in completion
                .tool_calls
                .into_iter()
                .take(chat::MAX_CALLS_PER_ROUND)
            {
                let result = self.run_tool(&mut desk, &facts, &call);
                tracing::debug!(
                    target: "b2::chat",
                    tool = call.name,
                    failed = result.starts_with("error:"),
                    "ran a tool for the model"
                );
                tools_used.push(ToolUseView {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                    seeded: false,
                });
                req.exchanges.push(ToolExchange { call, result });
            }
            // The row was ranked on this pair's matches, so B2 seeds them if skipped.
            if round == 1 && desk.passages.is_empty() {
                let seeded = self.seed_pairs(&mut desk, &facts)?;
                tools_used.push(ToolUseView {
                    name: seeded.call.name.clone(),
                    arguments: seeded.call.arguments.clone(),
                    seeded: true,
                });
                req.exchanges.push(ToolExchange {
                    call: seeded.call,
                    result: seeded.result,
                });
            }
            req.passages.clone_from(&desk.passages);
        }
        Ok(AnswerView {
            citations: cite(&answer, &desk.passages),
            answer,
            cancelled,
            tools: tools_used,
        })
    }

    /// B2's own `b2_passage_pairs` call for the turn's pair, as the exchange it is.
    fn seed_pairs(&self, desk: &mut ToolDesk, facts: &chat::WhyFacts) -> Result<SeededLookup> {
        let call = ToolCall {
            id: "b2_seed_1".to_string(),
            name: chat::TOOL_PASSAGE_PAIRS.to_string(),
            arguments: serde_json::json!({
                "note": facts.anchor_path,
                "candidate": facts.candidate_path,
            })
            .to_string(),
        };
        let (result, pairs) =
            self.tool_passage_pairs(desk, &facts.anchor_path, &facts.candidate_path)?;
        Ok(SeededLookup {
            call,
            result,
            pairs,
        })
    }

    /// The why-turn's degrade: B2 makes the pair lookup and sends one plain grounded
    /// request.
    fn why_handoff(
        &self,
        llm: &dyn LlmProvider,
        facts: &mut chat::WhyFacts,
        mut desk: ToolDesk,
        mut tools_used: Vec<ToolUseView>,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<AnswerView> {
        let seeded = self.seed_pairs(&mut desk, facts)?;
        tools_used.push(ToolUseView {
            name: seeded.call.name,
            arguments: seeded.call.arguments,
            seeded: true,
        });
        facts.embedded = !seeded.pairs.is_empty();
        facts.pairs = seeded.pairs;
        let req = chat::build_why_request(facts, desk.passages);
        let mut view = self.stream_answer(llm, &req, on_token)?;
        view.tools = tools_used;
        Ok(view)
    }

    /// Run one tool call and render its result as text. Infallible: the call is model
    /// output, so every failure becomes an `error:` result the model reads.
    fn run_tool(&self, desk: &mut ToolDesk, facts: &chat::WhyFacts, call: &ToolCall) -> String {
        let args: serde_json::Value = if call.arguments.trim().is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str(&call.arguments) {
                Ok(v @ serde_json::Value::Object(_)) => v,
                _ => return "error: the arguments were not a JSON object".to_string(),
            }
        };
        let text_arg = |key: &str| args.get(key).and_then(|v| v.as_str()).map(str::to_string);
        let note = text_arg("note");
        let ran = match call.name.as_str() {
            chat::TOOL_PASSAGE_PAIRS => {
                let note = note.unwrap_or_else(|| facts.anchor_path.clone());
                let candidate =
                    text_arg("candidate").unwrap_or_else(|| facts.candidate_path.clone());
                self.resolve_ref(&note).and_then(|n| {
                    let c = self.resolve_ref(&candidate)?;
                    Ok(self.tool_passage_pairs(desk, &n, &c)?.0)
                })
            }
            chat::TOOL_SIMILAR => {
                let note = note.unwrap_or_else(|| facts.anchor_path.clone());
                self.tool_similar(&note, desk.limit)
            }
            chat::TOOL_NEIGHBORS => {
                let note = note.unwrap_or_else(|| facts.anchor_path.clone());
                self.tool_neighbors(&note)
            }
            chat::TOOL_READ => {
                // The anchor is already on screen; default to the suggestion.
                let note = note.unwrap_or_else(|| facts.candidate_path.clone());
                let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
                self.tool_read(desk, &note, offset as usize)
            }
            other => return format!("error: unknown tool `{other}`"),
        };
        ran.unwrap_or_else(|e| match e {
            Error::NoteNotFound(n) => format!("error: note not found: {n}"),
            // Internals stay out of the model's context as they stay out of the UI.
            _ => "error: B2 could not run that lookup".to_string(),
        })
    }

    /// `b2_passage_pairs`: the matched pairs between two notes and any passages new to the
    /// ledger. Also returns the pairs in ledger numbering.
    fn tool_passage_pairs(
        &self,
        desk: &mut ToolDesk,
        note: &str,
        candidate: &str,
    ) -> Result<(String, Vec<chat::MarkedPair>)> {
        let mut pairs = Vec::new();
        let mut fresh = Vec::new();
        for pair in discover::passage_pairs(&self.conn, note, candidate, chat::WHY_PAIRS)? {
            let a = self.ledger_marker(desk, pair.anchor_chunk_id, note, &mut fresh)?;
            let c = self.ledger_marker(desk, pair.candidate_chunk_id, candidate, &mut fresh)?;
            // A miss is a torn read (C1); skip the pair.
            if let (Some(a), Some(c)) = (a, c) {
                pairs.push((a, c, pair.score));
            }
        }
        if pairs.is_empty() {
            return Ok((
                format!(
                    "No matched passages: {note} or {candidate} has no stored vectors to \
                     compare yet. A reindex fills them."
                ),
                pairs,
            ));
        }
        let mut out: Vec<String> = pairs
            .iter()
            .enumerate()
            .map(|(i, (a, c, score))| chat::pair_line(i + 1, *a, *c, *score))
            .collect();
        out.push(String::new());
        out.extend(self.fresh_blocks(desk, &fresh));
        Ok((out.join("\n"), pairs))
    }

    /// `b2_similar`: the ranked list for a note, as `similar` serves it.
    fn tool_similar(&self, note: &str, limit: usize) -> Result<String> {
        let rows = self.similar(note, limit)?;
        if rows.is_empty() {
            return Ok(format!(
                "B2 has no similar unlinked notes to suggest for {note}."
            ));
        }
        Ok(rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let strength =
                    r.z.map(|z| format!(", strength z = {z:.2}"))
                        .unwrap_or_default();
                format!("#{} {}{strength} — {}", i + 1, r.path, r.evidence)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// `b2_neighbors`: a note's direct links, as `neighbors` serves them.
    fn tool_neighbors(&self, note: &str) -> Result<String> {
        let rows = self.neighbors(note)?;
        if rows.is_empty() {
            return Ok(format!("{note} is not linked to any note."));
        }
        Ok(rows
            .iter()
            .map(|n| {
                let why = n
                    .explanation
                    .as_deref()
                    .map(|e| format!(" — {e}"))
                    .unwrap_or_default();
                format!("{} {} ({}){why}", n.label, n.path, n.direction)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// `b2_read`: up to [`chat::READ_PASSAGES`] of a note's passages from `offset`, each
    /// numbered on the ledger so the model can cite what it read.
    fn tool_read(&self, desk: &mut ToolDesk, note: &str, offset: usize) -> Result<String> {
        let path = self.resolve_ref(note)?;
        let ids = db::note_chunk_ids(&self.conn, &path)?;
        let mut fresh = Vec::new();
        let mut markers = Vec::new();
        for id in ids.iter().skip(offset).take(chat::READ_PASSAGES) {
            if let Some(m) = self.ledger_marker(desk, *id, &path, &mut fresh)? {
                markers.push(m);
            }
        }
        if markers.is_empty() {
            return Ok(format!("{path} has no passages at offset {offset}."));
        }
        let mut out = vec![format!(
            "{path}: passages {}–{} of {}, numbered {}.",
            offset + 1,
            offset + markers.len(),
            ids.len(),
            markers
                .iter()
                .map(|m| format!("[{m}]"))
                .collect::<Vec<_>>()
                .join(" ")
        )];
        out.push(String::new());
        out.extend(self.fresh_blocks(desk, &fresh));
        Ok(out.join("\n"))
    }

    /// The ledger number of a chunk, handing it over (and recording it in `fresh`) the
    /// first time it is seen. `None` when the chunk's row is gone.
    fn ledger_marker(
        &self,
        desk: &mut ToolDesk,
        chunk_id: i64,
        path: &str,
        fresh: &mut Vec<usize>,
    ) -> Result<Option<usize>> {
        if let Some(i) = desk.chunk_ids.iter().position(|id| *id == chunk_id) {
            return Ok(Some(i + 1));
        }
        let Some((heading_path, text)) = db::chunk_detail(&self.conn, chunk_id)? else {
            return Ok(None);
        };
        desk.chunk_ids.push(chunk_id);
        desk.passages.push(ContextPassage {
            path: path.to_string(),
            heading_path,
            text,
        });
        fresh.push(desk.passages.len());
        Ok(Some(desk.passages.len()))
    }

    /// The text of passages handed over for the first time.
    fn fresh_blocks(&self, desk: &ToolDesk, fresh: &[usize]) -> Vec<String> {
        fresh
            .iter()
            .filter_map(|m| {
                Some(chat::passage_block(
                    *m,
                    desk.passages.get(m.checked_sub(1)?)?,
                ))
            })
            .collect()
    }

    /// Stream the completion, then resolve `[n]` against the request's passages.
    fn stream_answer(
        &self,
        llm: &dyn LlmProvider,
        req: &ChatRequest,
        on_token: &mut dyn FnMut(&str) -> ControlFlow<()>,
    ) -> Result<AnswerView> {
        let completion = llm.complete(req, on_token).map_err(llm_error)?;
        Ok(AnswerView {
            citations: cite(&completion.text, &req.passages),
            answer: completion.text,
            cancelled: completion.cancelled,
            tools: Vec::new(),
        })
    }
}

/// A tool-using turn's state: the surface's list length and the passage ledger, every
/// passage handed to the model numbered once in first-seen order.
struct ToolDesk {
    limit: usize,
    /// Parallel to `passages`: the chunk behind each, so a passage is never numbered twice.
    chunk_ids: Vec<i64>,
    passages: Vec<ContextPassage>,
}

/// B2's own pair lookup, as the model would have made it.
struct SeededLookup {
    call: ToolCall,
    result: String,
    pairs: Vec<chat::MarkedPair>,
}

/// Normalize a provider failure to [`Error::Llm`], which adapters match for the
/// "can't reach the model server" message.
fn llm_error(e: Error) -> Error {
    match e {
        Error::Llm(_) | Error::ToolCallLimit { .. } => e,
        other => Error::Llm(other.to_string()),
    }
}

/// Resolve an answer's distinct `[n]` markers against the passages it was grounded in.
fn cite(answer: &str, passages: &[ContextPassage]) -> Vec<Citation> {
    chat::cited_markers(answer, passages.len())
        .into_iter()
        .filter_map(|marker| {
            // 1-based marker; a broken invariant degrades to a missing citation.
            let p = passages.get(marker.checked_sub(1)?)?;
            Some(Citation {
                marker,
                path: p.path.clone(),
                excerpt: snippet(&p.text),
            })
        })
        .collect()
}
