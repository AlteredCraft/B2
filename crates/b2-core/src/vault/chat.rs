//! Flow ④ on the façade: grounded chat ([`Vault::ask`]) and the one tool-using turn,
//! **Why?** on a *Similar & unlinked* card ([`Vault::why_similar`], ADR-0022). The prompt
//! text and tool schemas live in [`crate::chat`]; this is the orchestration over the
//! façade's own reads. Chat is a reader: nothing model-derived is stored.

use super::{AnswerView, Citation, ToolUseView, Vault};
use crate::chat;
use crate::db;
use crate::discover;
use crate::error::{Error, Result};
use crate::llm::{ChatRequest, ChatTurn, ContextPassage, LlmProvider, ToolCall, ToolExchange};
use crate::snippet::snippet;
use std::ops::ControlFlow;

impl Vault {
    /// Flow ④ — grounded chat over the vault: condense → retrieve → assemble →
    /// stream → cite, orchestrated here over the core logic in [`crate::chat`]. The
    /// provider is injected **per call**: chat is its sole consumer and, unlike the
    /// embedder, it carries no index identity (contrast ADR-0007), so nothing about
    /// it belongs on the open vault.
    ///
    /// - **Condense** (multi-turn only): one provider call rewrites the follow-up
    ///   into a standalone retrieval query; on failure it degrades to the raw
    ///   question, so that step can never break chat.
    /// - **Retrieve**: [`search_chunks`](Self::search_chunks) at
    ///   [`chat::ASK_PASSAGES`], holding `search`'s posture — chat is a reader.
    /// - **Stream**: tokens flow up through `on_token` as they arrive; returning
    ///   `ControlFlow::Break(())` cancels at token granularity and the result reports
    ///   it ([`AnswerView::cancelled`]).
    /// - **Cite**: distinct `[n]` markers resolve to `(path, excerpt)`; a
    ///   hallucinated marker resolves to nothing, and the text is never rewritten.
    ///
    /// Nothing model-derived is stored anywhere, and history is the caller's,
    /// session-only. Errors: retrieval as `search` raises it; a failed *answer* call
    /// as [`Error::Llm`](crate::Error::Llm).
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

    /// **Why was this suggested?** — the chat answer behind one *Similar & unlinked*
    /// row: explain, grounded and cited, why `candidate_ref` surfaced for `anchor_ref`.
    ///
    /// A **tool-using** turn (ADR-0022). The model is offered B2's read-only tools
    /// ([`chat::why_tools`]: `b2_passage_pairs`, `b2_similar`, `b2_neighbors`, `b2_read`)
    /// and makes the lookups itself; each call is one read on this façade, run here and
    /// replayed to the model with its result. Every argument defaults to this turn's
    /// pair, so `{}` is always a valid call. Three things bound that:
    ///
    /// - **Round 1 is the lookup round.** Its text is never streamed: a model that
    ///   answers there answered from nothing. If it calls no tool — or the call fails,
    ///   most often a model with no tool support — the turn degrades to
    ///   [`chat::build_why_request`]: B2 makes the pair lookup and hands the evidence over
    ///   in one plain grounded request. If it calls tools but skips the pair lookup, B2
    ///   appends that call itself (`seeded`), because the matched pairs are what the row
    ///   was ranked on and no explanation is written without them.
    /// - **The loop is bounded** at [`chat::MAX_TOOL_ROUNDS`] model calls and
    ///   [`chat::MAX_CALLS_PER_ROUND`] tool calls each, and the last round offers no
    ///   tools, so it can only answer.
    /// - **The row's position rides in the prompt** — rank and strength from
    ///   [`similar`](Self::similar) at the `limit` the surface showed, so the explanation
    ///   describes the card that was clicked.
    ///
    /// One consequence of the hidden first round: a cancel lands when the first visible
    /// token arrives, not during the lookups.
    ///
    /// A tool call is model output, so it is untrusted: an unknown tool, malformed
    /// arguments or an unknown note is answered with an `error:` *result* the model can
    /// read, never a failed turn. Every passage a tool hands over is numbered once on one
    /// ledger, and the answer's `[n]` markers resolve against it. Streaming, cancellation
    /// and the [`Error::Llm`] normalization are [`ask`](Self::ask)'s — chat is a reader
    /// here too, and nothing is stored. [`Error::NoteNotFound`] for an unknown ref on
    /// either side.
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

        // The row as the surface showed it: same call, same limit, so the same rank + z.
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
            // Round 1 is the lookup round, and its text is never shown: a model that
            // answers there has answered from nothing, and one that calls tools says at
            // most "let me check". From round 2 on, tokens stream as they arrive.
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
                // Never degraded: a reply past the tool-call cap is a broken or hostile
                // server, and a quiet handoff would hide it.
                Err(e @ Error::ToolCallLimit { .. }) => return Err(e),
                // The lookup round failed: most often a model with no tool support.
                // If the server is simply down, the handoff fails the same way, honestly.
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
            // Whatever the model chose to look up, the matched pairs for *this* pair are
            // what the row was ranked on. If the lookup round skipped them, B2 makes that
            // call itself, so no explanation is written without them on the desk.
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

    /// The degrade of a tool-using why-turn: B2 makes the pair lookup itself and hands
    /// the evidence over in one plain grounded request — for a model with no tool
    /// support, and for one that was offered tools and used none.
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

    /// Run one tool call for the model and render its result as text. Infallible by
    /// design: the call is model output, so every failure — an unknown tool, arguments
    /// that aren't a JSON object, a note that doesn't exist, an index error — becomes an
    /// `error:` result the model reads, and the turn carries on.
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
                // The open note is already on the human's screen; the one worth reading
                // by default is the suggestion.
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

    /// `b2_passage_pairs`: the matched pairs between two notes, then each passage they
    /// name that the ledger had not handed over yet. Also returns the pairs over the
    /// ledger's numbering, which the seeded call's facts need.
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
            // A miss means a torn read against a concurrent reindex; skip the pair rather
            // than name a passage with no text.
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

    /// The text of the passages a tool call handed over for the first time. A passage
    /// already on the ledger is named by its marker only — the model has its text.
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

    /// The shared tail of [`ask`](Self::ask) and the handoff half of
    /// [`why_similar`](Self::why_similar): stream the completion, then resolve the
    /// answer's `[n]` markers against the request's own passages.
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

/// The state one tool-using turn carries between calls: the list length the surface
/// showed, and the **passage ledger** — every passage a tool has handed the model,
/// numbered once in first-seen order, which is what `[n]` resolves against. Whose turn it
/// is (what `{}` arguments default to) rides on the turn's [`chat::WhyFacts`].
struct ToolDesk {
    limit: usize,
    /// Parallel to `passages`: the chunk behind each, so a passage is never numbered twice.
    chunk_ids: Vec<i64>,
    passages: Vec<ContextPassage>,
}

/// B2's own pair lookup: the call as the model would have made it, its result text, and
/// the pairs over the ledger's numbering.
struct SeededLookup {
    call: ToolCall,
    result: String,
    pairs: Vec<chat::MarkedPair>,
}

/// Normalize a provider failure: the trait returns the crate-wide `Result`, but every
/// failure of a model call is a failed model call, and adapters match [`Error::Llm`] for
/// the "can't reach the model server" message. Enforced here, not hoped for.
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
            // 1-based marker to 0-based passage; skip rather than index, so even a
            // broken invariant degrades to a missing citation.
            let p = passages.get(marker.checked_sub(1)?)?;
            Some(Citation {
                marker,
                path: p.path.clone(),
                excerpt: snippet(&p.text),
            })
        })
        .collect()
}
