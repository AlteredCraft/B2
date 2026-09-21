//! Flow ④ — grounded chat: the core logic of `Vault::ask`, condense -> retrieve ->
//! assemble -> stream -> cite. This module owns what must be deterministic and testable
//! against [`FakeLlm`] — prompt assembly, the condensation degrade, citation-marker
//! parsing — while the façade owns retrieval and resolving markers into display views,
//! mirroring the `discover.rs`/`vault.rs` split.
//!
//! Chat is a **reader** whose output is never stored: no response caching, no `meta` rows,
//! session-only history. Model output is untrusted content (ADR-0016), enforced where it
//! renders.
//!
//! [`FakeLlm`]: crate::llm::FakeLlm

use crate::llm::{
    ChatRequest, ChatTurn, ContextPassage, LlmProvider, RequestKind, ToolExchange, ToolSpec,
};
use std::collections::BTreeSet;
use std::ops::ControlFlow;

/// How many passages retrieval hands the model — the "~8–12" of the spec
/// (GH #153). Retrieval goes through `Vault::search_chunks`, so the passage
/// view's pool discipline applies unchanged: a 10-ask reaches
/// `chunk_candidate_pool(10) = 60` candidates per signal (GH #141/#142) —
/// chat introduces no new candidate width.
pub const ASK_PASSAGES: usize = 10;

/// The grounded system prompt (flow ④ step 2). Prompt assembly is core logic:
/// authored here, rendered with the numbered passages by
/// [`ChatRequest::system_message`], asserted by the engine suite. The quoted
/// no-evidence sentence is [`crate::llm::NO_EVIDENCE_ANSWER`] verbatim (the
/// suite pins the containment), so the instruction and the fake that obeys it
/// can never drift apart.
pub const GROUNDED_SYSTEM_PROMPT: &str = "You are B2, answering questions about the user's own \
notes. Answer ONLY from the numbered passages below. Cite each claim with the supporting \
passage's [n] marker. If the passages do not support an answer, say \"I don't find that in \
your notes.\" — never answer from general knowledge.";

/// The condensation prompt (flow ④ step 0): rewrite a follow-up into a
/// standalone retrieval query ("tell me more about that" → a query that names
/// *that*). The reply is used as search input, never shown to the human.
pub const CONDENSE_SYSTEM_PROMPT: &str = "Rewrite the user's latest message as one standalone \
search query that names its subject explicitly, using the earlier conversation to fill in \
any pronouns or references. Reply with the query text only — no preamble, no quotes.";

/// Step 0 — condense a follow-up into a standalone retrieval query. Multi-turn
/// only; the caller skips this entirely for a single-turn ask. **This step can
/// never break chat**: on provider failure, a cancelled stream, or a blank
/// reply, it degrades to the raw question text — a worse retrieval query, not
/// an error. Tokens are consumed internally (the condensed query is search
/// input, not answer text), so nothing streams up from this call.
pub fn condense_query(llm: &dyn LlmProvider, question: &str, history: &[ChatTurn]) -> String {
    let req = ChatRequest {
        kind: RequestKind::Condense,
        system: CONDENSE_SYSTEM_PROMPT.to_string(),
        turns: turns_with_question(history, question),
        passages: Vec::new(),
        tools: Vec::new(),
        exchanges: Vec::new(),
    };
    match llm.complete(&req, &mut |_| ControlFlow::Continue(())) {
        Ok(c) if !c.cancelled && !c.text.trim().is_empty() => c.text.trim().to_string(),
        Ok(_) => {
            tracing::debug!(
                target: "b2::chat",
                "condensation returned nothing usable; degrading to the raw question"
            );
            question.to_string()
        }
        Err(e) => {
            tracing::debug!(
                target: "b2::chat",
                error = %e,
                "condensation failed; degrading to the raw question"
            );
            question.to_string()
        }
    }
}

/// Step 2 — assemble the grounded chat request: the system prompt, the
/// conversation ending in the current question, and the numbered passages.
pub fn build_request(
    question: &str,
    history: &[ChatTurn],
    passages: Vec<ContextPassage>,
) -> ChatRequest {
    ChatRequest {
        kind: RequestKind::Chat,
        system: GROUNDED_SYSTEM_PROMPT.to_string(),
        turns: turns_with_question(history, question),
        passages,
        tools: Vec::new(),
        exchanges: Vec::new(),
    }
}

/// How many passage pairs a "why was this suggested?" explanation is grounded in. The
/// first is the pair the card's score came from; the next two show whether the
/// resemblance is one passage or a pattern. Up to `2 × WHY_PAIRS` passages, well under
/// [`ASK_PASSAGES`], so the explanation costs less context than an ordinary ask.
pub const WHY_PAIRS: usize = 3;

/// How many shared neighbours the facts block names before it counts the rest.
const WHY_SHARED_NAMED: usize = 5;

/// The system prompt for explaining one *Similar & unlinked* row (`Vault::why_similar`).
/// Grounded chat with a narrower subject, so it keeps grounded chat's two rules — cite by
/// `[n]`, never reach for general knowledge — and the same no-evidence sentence
/// ([`crate::llm::NO_EVIDENCE_ANSWER`], pinned by the suite). The facts block
/// [`build_why_request`] appends is what B2's own discovery reads found for the pair;
/// the model's job is the part no statistic can do, saying what the matched passages
/// have in common in the human's terms.
pub const WHY_SYSTEM_PROMPT: &str = "You are B2, explaining why one of the user's notes was \
suggested as similar to the note they are reading. B2 suggests a note when one of its passages \
sits close to a passage of the open note in embedding space and the two notes are not linked \
yet. Below are the facts B2's discovery tools found for this pair, then the matched passages, \
numbered. In a few sentences of your own — do not repeat the facts or the passages back — \
say what the matched passages have in common — the shared \
subject, claim or vocabulary — and what kind of connection the user might consider. Base this \
ONLY on the facts and the numbered passages, and cite each claim with the supporting \
passage's [n] marker, written exactly as the bracketed number — [1], [2] — never as \
\"passage 1\". Similarity is a suggestion, not a verdict: if the passages share little, \
say so plainly. If there are no passages, say \"I don't find that in your notes.\" — never \
answer from general knowledge.";

/// One matched passage pair over a request's 1-based passage numbering:
/// `(anchor marker, candidate marker, score)`.
pub type MarkedPair = (usize, usize, f64);

/// What B2's discovery reads found for one (anchor, suggested note) pair — the facts half
/// of a "why was this suggested?" request, gathered by the façade and rendered here so
/// the wording is core logic the suite can assert. Owned: built once per click and handed
/// straight to [`build_why_request`].
#[derive(Debug, Clone, PartialEq)]
pub struct WhyFacts {
    /// The note being read — the list's anchor.
    pub anchor_path: String,
    pub anchor_title: Option<String>,
    /// The suggested note the human asked about.
    pub candidate_path: String,
    pub candidate_title: Option<String>,
    /// `(rank, served)`: the 1-based position `similar` served the note at, out of how
    /// many rows it served. `None` when the note is not in that list (already linked, or
    /// beyond the list's length) — no rank is claimed for a row that would not be shown.
    pub rank: Option<(usize, usize)>,
    /// The row's strength z, when the list was graded (`SimilarView::z`).
    pub z: Option<f64>,
    /// Whether the two notes are already directly linked.
    pub linked: bool,
    /// Notes directly linked to **both** — the triadic-closure signal discovery keeps in
    /// the pool but does not rank on.
    pub shared_neighbors: Vec<String>,
    /// The matched pairs, nearest first, as `(anchor marker, candidate marker, score)`
    /// over the request's 1-based passage numbering.
    pub pairs: Vec<MarkedPair>,
    /// Whether both notes had stored vectors to compare at all.
    pub embedded: bool,
}

/// Assemble the "why was this suggested?" request: [`WHY_SYSTEM_PROMPT`], the rendered
/// facts, one user turn naming both notes, and the matched passages. A
/// [`RequestKind::Chat`] request — it is answered from numbered passages exactly as a
/// grounded ask is, so every provider (and the fake) serves it unchanged. It carries no
/// history: the evidence for one row is self-contained, and a follow-up question goes
/// through the ordinary `ask` with the explanation as its context.
pub fn build_why_request(facts: &WhyFacts, passages: Vec<ContextPassage>) -> ChatRequest {
    ChatRequest {
        kind: RequestKind::Chat,
        system: format!("{WHY_SYSTEM_PROMPT}\n\n{}", why_facts_block(facts, true)),
        turns: vec![why_turn(facts)],
        passages,
        tools: Vec::new(),
        exchanges: Vec::new(),
    }
}

/// The one user turn of a why-request, naming both notes by path (the identity the
/// tools take).
fn why_turn(facts: &WhyFacts) -> ChatTurn {
    ChatTurn::user(&format!(
        "Why was {} suggested as similar to {}?",
        facts.candidate_path, facts.anchor_path
    ))
}

// --- the tool-using turn ---------------------------------------------------------
//
// `Vault::why_similar` is an *agent* turn: the model is offered B2's read-only tools and
// makes the lookups itself. Three things keep that honest on a small local model. The
// first round is the lookup round: its text is never shown, a model that calls nothing
// there is handed the evidence instead, and one that skips the pair lookup has it made for
// it. The loop is bounded, and its last round offers no tools. And a model with no tool
// support at all degrades to [`build_why_request`], the same evidence in one plain request.

/// The matched passage pairs between two notes — what `b2 similar` ranked the row on.
pub const TOOL_PASSAGE_PAIRS: &str = "b2_passage_pairs";
/// The ranked *Similar & unlinked* list for a note.
pub const TOOL_SIMILAR: &str = "b2_similar";
/// A note's linked neighbours, with relation and direction.
pub const TOOL_NEIGHBORS: &str = "b2_neighbors";
/// A note's passages, in order.
pub const TOOL_READ: &str = "b2_read";

/// Model calls per why-turn, the final answer included. The last round offers no tools,
/// so a model that would look things up forever still has to answer.
pub const MAX_TOOL_ROUNDS: usize = 4;

/// Tool calls honoured per round; any beyond this are dropped unanswered-for. A bound on
/// what one malformed or runaway completion can make B2 do.
pub const MAX_CALLS_PER_ROUND: usize = 4;

/// Passages one `b2_read` call returns. Enough to read a note's opening sections without
/// one call filling a small model's context.
pub const READ_PASSAGES: usize = 4;

/// The system prompt of the tool-using why-turn. [`WHY_SYSTEM_PROMPT`]'s rules — cite by
/// `[n]`, nothing from general knowledge, the same no-evidence sentence — plus what the
/// tools are for. The first lookup's result is already in the conversation when the model
/// first sees it.
pub const WHY_AGENT_SYSTEM_PROMPT: &str = "You are B2, explaining why one of the user's notes \
was suggested as similar to the note they are reading. B2 suggests a note when one of its \
passages sits close to a passage of the open note in embedding space and the two notes are not \
linked yet. You have B2's read-only tools, and nothing has been looked up yet. First call \
b2_passage_pairs to get the passages the suggestion was ranked on, and b2_neighbors to see \
how the open note is already connected. Call b2_read on either note if the matched passages \
alone do not explain the resemblance; b2_similar shows the rest of the suggestion list. Every \
tool defaults to this pair, so empty arguments are fine. Then answer. Tool results number \
the passages they return as [1], [2], and so on. In a few sentences of your own — do not repeat the facts or the \
passages back — say what the matched passages have in common — the shared subject, claim or \
vocabulary — and what kind of connection the user might consider. Base this ONLY on the facts \
below and on tool results, and cite each claim with the supporting passage's [n] marker, \
written exactly as the bracketed number — [1], [2] — never as \"passage 1\". Similarity is a \
suggestion, not a verdict: if the passages share little, say so plainly. If no tool returned \
any passage, say \"I don't find that in your notes.\" — never answer from general knowledge.";

/// The tools a why-turn offers — each one read-only `Vault` op, described for the model.
/// Every argument is optional and defaults to this turn's pair — `note` to the open note,
/// `candidate` (and `b2_read`'s `note`, since the open note is already on screen) to the
/// suggested one — so `{}` is always a valid call, which is what a small model sends.
pub fn why_tools() -> Vec<ToolSpec> {
    let note = |what: &str| {
        serde_json::json!({
            "type": "string",
            "description": format!("Vault-relative path of {what}. Defaults to the open note."),
        })
    };
    vec![
        ToolSpec {
            name: TOOL_PASSAGE_PAIRS.to_string(),
            description: "The passages of two notes that sit closest to each other in \
                embedding space, nearest pair first, with the passage text. This is what B2 \
                ranks a similar-note suggestion on."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "note": note("the first note"),
                    "candidate": {
                        "type": "string",
                        "description": "Vault-relative path of the second note. Defaults to the suggested note.",
                    },
                },
            }),
        },
        ToolSpec {
            name: TOOL_SIMILAR.to_string(),
            description: "The ranked list of notes B2 suggests as similar to a note and not \
                yet linked to it: rank, strength, and a one-line excerpt of the matching passage."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "note": note("the note") },
            }),
        },
        ToolSpec {
            name: TOOL_NEIGHBORS.to_string(),
            description: "The notes a note is already linked to, in either direction, with \
                the relation and any explanation the user wrote."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "note": note("the note") },
            }),
        },
        ToolSpec {
            name: TOOL_READ.to_string(),
            description: "Read a note: its passages in order, numbered so they can be cited."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "note": {
                        "type": "string",
                        "description": "Vault-relative path of the note to read. Defaults to the suggested note.",
                    },
                    "offset": {
                        "type": "integer",
                        "description": "How many passages to skip, to read further into a long note.",
                    },
                },
            }),
        },
    ]
}

/// Assemble one round of the tool-using why-turn. `passages` is the citation ledger so
/// far; the pair lines are left out of the facts because the seeded `b2_passage_pairs`
/// result carries them, beside the passages they number.
pub fn build_why_agent_request(
    facts: &WhyFacts,
    tools: Vec<ToolSpec>,
    exchanges: Vec<ToolExchange>,
    passages: Vec<ContextPassage>,
) -> ChatRequest {
    ChatRequest {
        kind: RequestKind::Agent,
        system: format!(
            "{WHY_AGENT_SYSTEM_PROMPT}\n\n{}",
            why_facts_block(facts, false)
        ),
        turns: vec![why_turn(facts)],
        passages,
        tools,
        exchanges,
    }
}

/// One matched pair as a line of text — shared by the handoff's facts block and the
/// `b2_passage_pairs` tool result, so the two read the same.
pub fn pair_line(
    ordinal: usize,
    anchor_marker: usize,
    candidate_marker: usize,
    score: f64,
) -> String {
    format!(
        "Matched pair {ordinal}: passages [{anchor_marker}] and [{candidate_marker}], distance {:.3}{}.",
        -score,
        if ordinal == 1 {
            " — the nearest pair, the one the suggestion was ranked on"
        } else {
            ""
        }
    )
}

/// One numbered passage as tool-result text — [`ChatRequest::system_message`]'s own
/// layout, so a passage looks the same whichever way it reached the model.
pub fn passage_block(marker: usize, p: &ContextPassage) -> String {
    let heading = match &p.heading_path {
        Some(h) => format!(" — {h}"),
        None => String::new(),
    };
    format!("[{marker}] {}{heading}\n{}\n", p.path, p.text)
}

/// The facts block, one line per finding, each naming the B2 read it came from so the
/// model (and a human reading a debug log) can tell a measured fact from an inference.
fn why_facts_block(f: &WhyFacts, handoff: bool) -> String {
    let named = |path: &str, title: &Option<String>| match title {
        Some(t) => format!("{path} (\"{t}\")"),
        None => path.to_string(),
    };
    let mut lines = vec![
        "Facts from B2's discovery tools:".to_string(),
        format!("- Open note: {}", named(&f.anchor_path, &f.anchor_title)),
        format!(
            "- Suggested note: {}",
            named(&f.candidate_path, &f.candidate_title)
        ),
    ];
    if let Some((rank, served)) = f.rank {
        let strength = match f.z {
            Some(z) => format!(
                " (strength z = {z:.2}: how far it stands above this note's other candidates)"
            ),
            None => String::new(),
        };
        lines.push(format!(
            "- `b2 similar`: ranked #{rank} of {served} unlinked candidates{strength}."
        ));
    }
    // The graph facts are the handoff's only: a tool-using turn reads them itself, with
    // `b2_neighbors`, and a fact the model looked up is one it is likelier to use.
    if !handoff {
        return lines.join("\n");
    }
    lines.push(if f.linked {
        "- `b2 neighbors`: the two notes are already directly linked, so `b2 similar` would \
         no longer list this one."
            .to_string()
    } else {
        "- `b2 neighbors`: the two notes have no direct link, which is why it is listed as \
         unlinked."
            .to_string()
    });
    lines.push(if f.shared_neighbors.is_empty() {
        "- `b2 neighbors`: they share no linked neighbours.".to_string()
    } else {
        // Named up to a handful: a hub-heavy vault can share dozens, and a long list is
        // what a small model parrots back instead of answering.
        let named: Vec<&str> = f
            .shared_neighbors
            .iter()
            .take(WHY_SHARED_NAMED)
            .map(String::as_str)
            .collect();
        let more = f.shared_neighbors.len() - named.len();
        format!(
            "- `b2 neighbors`: they share {} linked neighbour(s). Both link to or from: {}{}.",
            f.shared_neighbors.len(),
            named.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        )
    });
    if !f.embedded {
        lines.push(
            "- One or both notes have no stored vectors to compare yet, so there are no \
             matched passages; a reindex fills them."
                .to_string(),
        );
    }
    for (i, (a, c, score)) in f.pairs.iter().enumerate() {
        lines.push(format!("- {}", pair_line(i + 1, *a, *c, *score)));
    }
    lines.join("\n")
}

/// The conversation as the provider sees it: the adapter-held history, oldest
/// first, with the current question appended as the final user turn.
fn turns_with_question(history: &[ChatTurn], question: &str) -> Vec<ChatTurn> {
    let mut turns = history.to_vec();
    turns.push(ChatTurn::user(question));
    turns
}

/// Step 4 (the parse half) — the distinct `[n]` markers in an answer that
/// actually name a passage: 1-based, `n <= passage_count`, ascending. A
/// hallucinated marker (`[0]`, `[99]` over ten passages, digits too long to
/// parse) simply doesn't resolve — it stays in the answer text verbatim
/// (model output is never rewritten) and contributes no citation. The façade
/// maps each surviving marker onto its passage for the display view.
pub fn cited_markers(text: &str, passage_count: usize) -> Vec<usize> {
    let mut found = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // A byte-wise scan is UTF-8-safe: '[', ']' and ASCII digits never occur
        // as continuation bytes, so a multi-byte character can't fake a marker.
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b']' {
                if let Ok(n) = text[i + 1..j].parse::<usize>() {
                    if (1..=passage_count).contains(&n) {
                        found.insert(n);
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    found.into_iter().collect()
}
