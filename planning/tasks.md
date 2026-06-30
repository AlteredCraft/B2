---
title: "B2 — Tasks"
type: note
tags: [b2, tasks, planning]
created: 2026-06-28
updated: 2026-06-30
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

## Next up

The index engine + suggestion lifecycle are built and green (test-first throughout). Natural next steps,
roughly in order:

- **Core API surface** — a typed facade over the `b2-core` modules (the contract the CLI and tests call;
  testability-stack point 1). Today the "API" is the module functions directly.
- **CLI** — `b2 add / search / link / suggest / neighbors / reindex / explain` — the first real adapter
  and the single-binary artifact (vision-and-scope, headless-first).
- **Real embedder + eval suite** — drop a local model into the embedder seam, then the eval that scores
  semantic + suggestion quality (the deferred half of steps 3 & 5; also unlocks the qmd chunker upgrade).
- **Connection-discovery pipeline** — candidate generation (the graph⨝vector join is ready) → typed,
  explained suggestions → the review loop.

## Backlog (later, not now)

- Move operation (`b2 mv`) — the kernel rename/move + inbound wikilink-path rewrite (the lone body
  write), per [user-stories.md](user-stories.md) Story 1. Not yet built.
- Property tests for the invariants — round-trip, `full-reindex ≡ incremental`, rename-keeps-backlinks as
  property tests over generated vaults (golden-vault scenarios exist; property coverage is the gap).
- qmd chunker upgrade — replace the minimal paragraph chunker once a real embedder + eval can score it
  (build spec §1.2).
- GUI — deferred per the headless-first approach ([vision-and-scope.md](vision-and-scope.md)).
