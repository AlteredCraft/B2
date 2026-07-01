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
- [x] **`b2` CLI over a typed Core API** — the walking skeleton. A `b2_core::Vault` façade
  (`open`/`reindex`/`neighbors`/`search`; a note ref resolves by path **or** `b2id`) is now the one
  typed contract, and a `b2-cli` crate (binary `b2`) is a *dumb* adapter over it — parse args, call the
  façade, print — with a `--json` mode for agents. Index + log live in `<vault>/.b2/` (one portable
  folder). Ships the deterministic `FakeEmbedder`: `search`'s BM25 half is real, the vector half is not
  yet semantic (the CLI says so, never overstating). First real dogfooding moment — point B2 at a folder
  and explore its graph + search from the terminal. Façade + CLI-level tests (67 total).

## Next up — real embedder + eval suite

> **Pick this up fresh.** The walking skeleton is done: the `b2` CLI drives a typed `b2_core::Vault`
> façade against a real vault, but the shipped embedder is the deterministic *fake* — so the vector
> half of `search` (and, later, discovery candidate generation) is not yet semantic. This is the
> deferred half of build-spec steps 3 & 5.

**Goal:** drop a local model into the embedder seam ([`embed::Embedder`](../crates/b2-core/src/embed.rs);
[index-engine.md](index-engine.md) §6), replacing the `FakeEmbedder` the façade currently constructs —
one localized swap, no schema or flow change (a model id/dim change re-embeds via
`ensure_embedding_space`). Then the **eval suite** that scores semantic + suggestion quality against a
hand-labeled set, kept **separate** from the deterministic plumbing tests so model quality never flakes
CI ([vision-and-scope.md](vision-and-scope.md), testability stack point 5).

**Unlocks / knock-on:** the qmd chunker upgrade (a real embedder can finally score it, build spec §1.2),
and the CLI's `search` can stop caveating its vector half.

### After that (ordered)

- **Connection-discovery pipeline** — candidate generation (the graph⨝vector join is ready) → typed,
  explained suggestions → the review loop; then the `suggest` / `accept` / `reject` CLI commands (the
  accept op is *built* in the engine; nothing **generates** suggestions until this lands).
- **Remaining CLI + kernel ops** — `b2 add` (note CRUD), `b2 mv` (the move + wikilink rewrite,
  [user-stories.md](user-stories.md) Story 1), `b2 explain`; plus a `reindex --dry-run` fast-follow (skip
  the `b2id` stamp-on-reindex, the one write B2 performs on the vault — [data-model.md](data-model.md) §1).

## Backlog (later, not now)

- Property tests for the invariants — round-trip, `full-reindex ≡ incremental`, rename-keeps-backlinks as
  property tests over generated vaults (golden-vault scenarios exist; property coverage is the gap).
- qmd chunker upgrade — replace the minimal paragraph chunker once a real embedder + eval can score it
  (build spec §1.2).
- GUI — deferred per the headless-first approach ([vision-and-scope.md](vision-and-scope.md)).
