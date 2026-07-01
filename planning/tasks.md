---
title: "B2 — Tasks"
type: note
tags: [b2, tasks, planning]
created: 2026-06-28
updated: 2026-07-01
status: active
---

# B2 — Tasks

Working task queue for B2. Start at [README.md](../README.md) for the map; context lives in
[vision-and-scope.md](vision-and-scope.md) (motivations, principles, design philosophy, capability
areas, v1 scope, locked decisions).

## Done

- [x] **Motivations & problem** — folded into [vision-and-scope.md](vision-and-scope.md)
  ("Why I'm building it").
- [x] **Vision & scope** — [vision-and-scope.md](vision-and-scope.md), including v1 scope and the
  three locked decisions (2026-06-28: semantic is engine-gated, full CRUD in CLI, v1 discovery =
  links only).
- [x] **Data model** — [data-model.md](data-model.md): note + edge as the Markdown source of truth,
  `[[path|title]]` links keyed by `b2id`, inline typed relations, the three-tier model (Markdown /
  disposable index / durable `.b2/` event log), provenance + suggestion lifecycle, OKF compatibility,
  and a golden-vault fixture. All judgment calls resolved 2026-06-29: edge-provenance → event log
  (accepted edges stay pristine); `b2id` is B2's one always-allowed write; bare links = directed
  `references`; a 10-verb relation core + tolerated tail. Identity key in
  [index-engine.md](index-engine.md) realigned to `b2id`. **§0 revised 2026-06-30** (Decision 1–3): B2
  never authors the body; accepted edges live in frontmatter `relations:` (Format A); graph = union of
  body ∪ frontmatter ∪ log, inline-wins dedup.
- [x] **Language gate** — **Rust** (`crates/b2-core`), per the single-binary goal
  ([index-engine.md](index-engine.md) §7). rusqlite (bundled SQLite + FTS5) + `sqlite-vec`.
- [x] **Index-engine build, steps 0→5** — [specs/index-engine-build.md](specs/index-engine-build.md), all
  green against the golden-vault fixture: (0) DB substrate proving FTS5 + `sqlite-vec` coexist; (1)
  lossless parse/serialize, `b2id` stamping, `b2id ⇄ path` resolver; (2) `chunks` (+FTS5) + the typed
  `edges` graph + `neighbors` (minimal paragraph chunker; qmd heuristic deferred to a real-embedder eval);
  (3) `chunks_vec` + the embedder seam (deterministic fake; real model deferred); (4) the `.b2/` JSONL
  event log + replay (suggestions inert; drop→replay reproduces the queue; rejection tombstones); (5)
  hybrid retrieval — BM25 ⊕ vector → RRF (k=60) + the graph⨝vector join.
- [x] **Suggestion lifecycle, end-to-end** — generate → list → **accept** (append to frontmatter
  `relations:`, Markdown-first, re-project as `origin=frontmatter`) / reject (tombstone). Frontmatter
  `relations:` reader + inline-wins dedup. Survives drop→rebuild→replay; accepted edges stay pristine.
- [x] **`b2` CLI over a typed Core API** — the walking skeleton. A `b2_core::Vault` façade
  (`open`/`reindex`/`neighbors`/`search`; a note ref resolves by path **or** `b2id`) is now the one
  typed contract, and a `b2-cli` crate (binary `b2`) is a *dumb* adapter over it — parse args, call the
  façade, print — with a `--json` mode for agents. Index + log live in `<vault>/.b2/` (one portable
  folder). Ships the deterministic `FakeEmbedder`: `search`'s BM25 half is real, the vector half is not
  yet semantic (the CLI says so, never overstating). First real dogfooding moment — point B2 at a folder
  and explore its graph + search from the terminal. Façade + CLI-level tests (67 total).
- [x] **Real embedder + eval suite** — honest semantic `search` now ships. A new **`b2-embed`** crate
  holds the candle-backed **`LocalEmbedder`** behind the existing [`Embedder`](../crates/b2-core/src/embed.rs)
  seam (CLS-pool + L2-normalize, asymmetric `embed_query` prefix), so `b2-core` stays candle-free and the
  fast CI suite runs only the fake. `b2 init` downloads + **verifies** (loads + embeds a probe) the model
  into a shared XDG cache; `reindex`/`search` **fail fast** with "run `b2 init`" if absent. Config is a
  global TOML (`[embedder] model / source / cache_dir`), source overridable to a mirror/repo/local path.
  The `open()`-time drop is fixed: `open` never mutates the vector space; a model/dim mismatch **fails
  fast** on `search` and re-embeds only on `reindex`. Eval is a separate `--example eval` (out of CI)
  scoring precision/MRR over a hand-labelled set. **Decision change (2026-07-01):** EmbeddingGemma-300M is
  **gated** on HF (HTTP 401 without a token + license click — defeats a friction-free `b2 init`), so the
  default is the pre-authorized fallback **BAAI/bge-base-en-v1.5** (BERT, 768-dim, ungated), validated in
  the spike. Also fixed a real bug the eval surfaced: NL queries with punctuation crashed FTS5 —
  `keyword_search` now sanitizes to a safe `MATCH`. **73 tests** (all fake/deterministic in CI); the
  real model is exercised only by `b2 init` and the eval example.

## Next up — connection-discovery pipeline

> **Pick this up fresh.** The real embedder is done and on `main` — semantic `search` is honest, and the
> **graph⨝vector join is ready** ([search.rs](../crates/b2-core/src/search.rs) `graph_filtered_search`).
> The accept/reject engine ops are *built* (suggestion lifecycle slice), but **nothing generates
> suggestions yet** — this slice is what finally does. This is B2's reason to exist.

- **Connection-discovery pipeline** — candidate generation (run `graph_filtered_search` off each note as
  the anchor) → typed, **explained** suggestions → the review loop; then the `suggest` / `accept` /
  `reject` CLI commands (the accept op is *built* in the engine; nothing **generates** suggestions until
  this lands). Extend the **eval suite**'s scaffolded **suggestion-quality** half here (precision/recall
  over a hand-labelled candidate set), still out of CI.
- **Remaining CLI + kernel ops** — `b2 add` (note CRUD), `b2 mv` (the move + wikilink rewrite,
  [user-stories.md](user-stories.md) Story 1), `b2 explain`; plus a `reindex --dry-run` fast-follow (skip
  the `b2id` stamp-on-reindex, the one write B2 performs on the vault — [data-model.md](data-model.md) §1).

**Not in scope (keep discovery thin):** query expansion (qmd's 1.7B third model — off-by-default, later);
a reranker (a one-stage insertion after RRF, [index-engine.md](index-engine.md) §5); the actual
packaging/distribution build. **Unlocks (now available):** the qmd chunker upgrade — a real embedder can
finally score paragraph vs. qmd chunking (build spec §1.2); and ranking-quality tuning the eval can now
measure (e.g. the keyword-half stopword noise the first eval pass surfaced).

## Backlog (later, not now)

- Property tests for the invariants — round-trip, `full-reindex ≡ incremental`, rename-keeps-backlinks as
  property tests over generated vaults (golden-vault scenarios exist; property coverage is the gap).
- qmd chunker upgrade — replace the minimal paragraph chunker once a real embedder + eval can score it
  (build spec §1.2).
- GUI — deferred per the headless-first approach ([vision-and-scope.md](vision-and-scope.md)).
