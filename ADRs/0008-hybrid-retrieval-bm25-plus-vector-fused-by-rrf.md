# ADR-0008 — Retrieval is BM25 ⊕ vector KNN, fused by Reciprocal Rank Fusion

- **Status:** Accepted · 2026-07
- **Refs:** `index-engine.md` §4 · flow ② in `search.rs`

## Context

Lexical search misses paraphrase; vector search misses exact terms, names, and identifiers. A
personal vault needs both, and score-level blending would require calibrating two incomparable scales.

## Decision

- Search fuses BM25 over `chunks_fts` with an exact in-process vector KNN over `embeddings`, using
  **RRF (k=60)** over ranks, resolved from chunks up to notes.
- Raw natural-language queries are sanitized into a safe FTS5 `MATCH` expression (punctuation is
  FTS5 syntax and would otherwise crash the parse).
- On a projected-but-unembedded vault, search degrades to BM25-only rather than failing.

## Consequences

- RRF discards absolute signals, which is why a query-level evidence reading had to be carried
  alongside the fused order rather than derived from it (ADR-0015).
- Candidate width is a real tuning axis invisible to a small labelled corpus; the model-free rank
  probe exists for it (ADR-0013).
