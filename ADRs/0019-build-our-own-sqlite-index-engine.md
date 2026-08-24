# ADR-0019 — Build our own SQLite index engine; qmd is a reference, not a dependency

- **Status:** Accepted · 2026-06-29
- **Refs:** `index-engine.md` §1–§3 · [tobi/qmd](https://github.com/tobi/qmd)

## Context

[qmd](https://github.com/tobi/qmd) is an excellent local Markdown search engine — SQLite + FTS5 +
`sqlite-vec`, ~900-token Markdown-aware chunks, BM25 ⊕ vector ⊕ RRF ⊕ LLM rerank, local GGUF models,
MIT-licensed. It proves the pipeline runs on-device. The question was whether to depend on it.

## Decision

**Build the index engine; take qmd as a design reference.** The disagreement is **scope, not
quality**: qmd is a *search engine*, and B2 is a *typed graph with hybrid retrieval over it*. qmd
models no typed edges, no backlinks, and nothing that rewrites links on a move — which are the
reasons B2 exists. Wrapping it would mean a second store for everything that makes B2 B2, and two
sources of truth to reconcile; the retrieval glue actually wanted is ~300 lines.

- **Borrowed wholesale:** the chunking heuristic, the RRF formula and `k = 60`, the position-aware
  blend, the asymmetric query/document prompt discipline, the JSON/`--explain` agent-output
  discipline, and the MCP surface idea.
- **Discarded:** the npm/Node packaging and the "the DB is the product" framing.
- **SQLite is the substrate** because it gives one embedded store for every *queryable* concern at
  once — full-text (FTS5), vectors (plain tables, ADR-0006), and the typed graph — transactionally,
  so discovery's candidate generation joins all three in one query. That single-store property is
  worth more than anything inheritable from qmd.
- Because the engine **does** provide vector search, the engine-gated call resolves in favour of
  **semantic search in v1** — exact, in-process, no ANN.
- **The single-binary goal picked the language, not the engine.** SQLite and FTS5 are
  language-agnostic; B2 is a Rust workspace (`rusqlite` with bundled SQLite, `candle` for the
  embedder) because a Node runtime is the least aligned with shipping one binary — another reason
  not to inherit qmd's runtime by depending on it.

## Consequences

- The one genuinely hard part is **not the engine** — it is producing embeddings inside a single
  binary, which is orthogonal to choosing SQLite (ADR-0020).
- A reranker is a clean fast-follow behind a post-fusion seam, not a redesign; retrieval quality is
  good without it.
- Net: qmd answers "can great hybrid search run locally over Markdown?". B2's question is one layer
  up — "can that retrieval live inside a typed, traversable, agent-operated graph I fully own, in a
  single binary?"
