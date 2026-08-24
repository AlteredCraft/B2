# ADR-0005 — AI lives behind two enumerated seams; the core is model-free

- **Status:** Accepted · 2026-07 (chat seam 2026-08-14)
- **Refs:** invariants M1, M3, M5, E2 · GH #151, #153, #154

## Context

The Bitter-Lesson tenet: build for tomorrow's model. Machinery that compensates for today's model
becomes the thing blocking a better one — and any model dependency in the engine makes the test
suite slow, non-deterministic, and network-bound.

## Decision

- Exactly two seams: **`Embedder`** (text → vector) and **`LlmProvider`** (grounded chat — streamed,
  cooperatively cancellable). A reranker, if one lands, is the next enumerated seam, not an exception.
- `b2-core` is **model-free** (never links candle) and is built and tested against deterministic
  fakes (`FakeEmbedder`, `FakeLlm`); the real model drops in with no schema or flow change.
- Model-compensating machinery (per-pair adjudication, query expansion, heavy orchestration) is
  deferred or default-off.
- One embedding space in v1: every vault member funnels to text through the same model.
- **Note content never leaves the machine unbidden.** The default chat endpoint is local; a cloud one
  exists only by explicit configuration, so the consent moment is the configuration moment.

## Consequences

- Chat carries **no index identity** (contrast ADR-0007): nothing it produces is stored, history is
  session-only, so swapping chat models never touches the index.
- Model quality cannot be tested in CI, which is what the eval harness exists for (ADR-0013).
