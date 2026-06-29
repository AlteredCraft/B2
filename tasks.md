---
title: "B2 — Tasks"
type: note
tags: [b2, tasks, planning]
created: 2026-06-28
updated: 2026-06-29
status: active
---

# B2 — Tasks

Working task queue for B2. Context lives in [notes.md](notes.md) (motivations, principles,
headless-first approach) and [vision-and-scope.md](vision-and-scope.md) (vision, capability areas,
v1 scope, locked decisions).

## Done

- [x] **Motivations & problem** — [notes.md](notes.md).
- [x] **Vision & scope** — [vision-and-scope.md](vision-and-scope.md), including v1 scope and the
  three locked decisions (2026-06-28: semantic is engine-gated, full CRUD in CLI, v1 discovery =
  links only).
- [x] **Data model** — [data-model.md](data-model.md): note + edge as the Markdown source of truth,
  `[[path|title]]` links keyed by `b2id`, inline typed relations, the three-tier model (Markdown /
  disposable index / durable `.b2/` event log), provenance + suggestion lifecycle, OKF compatibility,
  and a golden-vault fixture. All judgment calls resolved 2026-06-29: edge-provenance → event log
  (accepted edges stay pristine); `b2id` is B2's one always-allowed write; bare links = directed
  `references`; a 10-verb relation core + tolerated tail. Identity key in
  [index-engine.md](index-engine.md) realigned to `b2id`.

## Next up — Index-engine build

Unblocked now the data model is locked. The evaluation is already drafted in
[index-engine.md](index-engine.md): **build our own SQLite store** (FTS5 + `sqlite-vec`) rather than
depend on `qmd` — which also settles the engine-gated decision: **semantic search is in v1**
(brute-force KNN; see "Decisions locked" in [vision-and-scope.md](vision-and-scope.md)).
[index-engine.md](index-engine.md) is reconciled with the three-tier / event-log model (§3 = disposable
index + durable `.b2/` event log, `index = projection of (Markdown ∪ log)`).

**Immediate gate — pick the stack first.** The single-binary goal favours **Rust or Go**
([index-engine.md](index-engine.md) §7); the embedding-in-a-binary question (§6) can stay open behind the
embedder seam. Decide the language before writing engine code (this is the "Tech-stack / language
decision" from the backlog, pulled forward).

**Then build, in order (each step asserts the locked invariants + the golden-vault fixture,
[data-model.md](data-model.md) §8):**
1. **Vault parse/serialize** — lossless MD + YAML round-trip (`parse → serialize → parse` byte-identical);
   `b2id` stamp-on-ingest; the `path ↔ b2id` resolver.
2. **Markdown-derived tables** — `notes`, `chunks` (+ FTS5), and `edges` (inline `references` + typed
   relations) per [data-model.md](data-model.md) §2–3.
3. **`sqlite-vec` + embedder seam** — deterministic fake embedder first; real local model later (§6).
4. **The `.b2/` event log** — `append(event)` / `replay()` sink (JSONL); review-queue replay so
   `index = projection of (Markdown ∪ log)` holds.
5. **Hybrid retrieval** — BM25 + vector → RRF fusion (reranker is a fast-follow, §5).

## Backlog (later, not now)

- Core API surface — the typed contract every adapter calls.
- CLI command surface — `b2 add / search / link / suggest / neighbors / reindex / explain`.
- Connection-discovery pipeline — candidate generation → typed, explained suggestions → review loop.
- Test harness — golden vaults, property tests, deterministic AI seams.
