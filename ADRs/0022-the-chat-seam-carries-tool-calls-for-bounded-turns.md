# ADR-0022 — The chat seam carries tool calls; a tool-using turn is bounded and degrades

- **Status:** Accepted · 2026-09-20
- **Refs:** invariants M1, E2, E5, S4 · ADR-0005, ADR-0011, ADR-0012 · GH #210

## Context

Grounded chat (flow ④) is one-shot: B2 retrieves, the model answers. "Why was this note suggested?"
is a different question — its evidence is not a search result but the output of B2's own reads
(`similar`'s matched passages, the graph, the notes themselves), and which of them matter depends on
what the first ones show. The requirement was that the chat model reach that evidence **through B2
tools**, not be handed a fixed bundle.

GH #210 argues against a loop inside `b2-core`, on two grounds: small local models execute tool loops
worst, and a loop in the caller keeps the engine dumb. Both were measured here rather than assumed.
On Ollama, `llama3.2:3b` and `gemma4` both emit well-formed tool calls for these tools. And a model
handed a lookup it did not make stops looking: with the pair evidence pre-seeded into the
conversation, neither model called anything further in any run.

## Decision

- **`LlmProvider` carries tool calls.** `ChatRequest` gains `tools` (name, description, JSON-Schema
  parameters) and `exchanges` (each call with B2's result, replayed after the turns); `Completion`
  gains `tool_calls`; `RequestKind::Agent` marks the turn. Still one trait method, still sync
  (ADR-0011). A request with no tools is byte-for-byte the request it was, so `ask` is unchanged.
- **A tool is one read on the `Vault` façade**, run by the façade for the model. No tool writes (W1),
  embeds a query, or reaches outside the vault. Arguments are model output, so they are untrusted: a
  malformed or unknown call gets an `error:` *result*, never a failed turn.
- **The loop lives in `b2-core`, and it is bounded.** At most `MAX_TOOL_ROUNDS` model calls and
  `MAX_CALLS_PER_ROUND` tool calls each; the last round offers no tools. The loop is in the core, not
  an adapter, for ADR-0012's reason: two adapters running their own loops would be two behaviours.
- **The model makes the lookups; B2 guarantees the grounding.** Round 1's text is never streamed. A
  model that calls nothing there, or a provider that refuses tools, degrades to the same evidence in
  one plain grounded request. A model that calls tools but skips the pair lookup has it appended,
  marked `seeded`.
- **Passages are numbered on one ledger per turn**, across every tool result, and `[n]` resolves
  against it — the citation contract of flow ④, unchanged.
- **The answer says which tools ran** (`AnswerView.tools`, `seeded` distinguishing B2's own call), so
  a surface never overstates what the model did.

## Consequences

- `Vault::why_similar` is the one tool-using turn. `ask` stays one-shot until #210's experiment says
  otherwise; nothing here prejudges it, and its loop-in-the-caller shape (an MCP or `serve` adapter)
  remains open — these tool definitions are what it would expose.
- A cancel during the hidden lookup round lands at the first visible token. Accepted: the round is
  short, and the alternative is showing text the turn may discard.
- The degrade costs one refused request against a model with no tool support, on every why-turn.
- `FakeLlm` scripts an agent turn off the request's *structure* (call every tool that needs no
  arguments once, then answer), so the engine, CLI and desktop suites cover the loop model-free (E2).
  How well a given model uses the tools is a quality question, which is ADR-0013's harness.
