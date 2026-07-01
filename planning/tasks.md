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

## Next up — thin vertical slice: the `b2` CLI over a typed Core API

> **Pick this up fresh.** The engine (`crates/b2-core`, steps 0→5 + accept, 51 tests) exists only as a
> library exercised by integration tests that call module functions directly. There is **no adapter** —
> you cannot yet point B2 at a real vault and use it. This slice fixes that, and in doing so forces the
> **Core API surface** (testability-stack point 1: "one typed core API; CLI and tests are clients") into
> existence. Keep it *thin and vertical*: the smallest end-to-end that makes B2 a real, daily-usable
> artifact against a real vault — not a broad command surface.

**Goal (the slice):** *point B2 at a folder of Markdown and explore its graph + search from the terminal.*
Fully deterministic (no live model), so it stays test-first like everything before it.

**In scope:**
- **Core API façade** — a small typed entry point (e.g. `b2_core::Vault`) that owns the open connection,
  the `JsonlSink`, and the embedder, exposing *only what this slice needs*: `open(vault_root)`,
  `reindex()`, `neighbors(note_ref)`, `search(query, limit)`. `note_ref` resolves by path **or** `b2id`.
  This is the contract; the CLI and tests are its only clients. Do **not** pre-build a sprawling API —
  add operations when a command needs them.
- **`b2-cli` crate** — a new workspace member; a *dumb* adapter (no logic, just parse args → call the
  façade → print). Commands: `b2 reindex [vault]`, `b2 neighbors <note>`, `b2 search <query>`.
  Human-readable output plus a `--json` mode for agents (vision-and-scope: the agent drives the CLI).
- **Index + log location:** `<vault>/.b2/` — `b2.sqlite` beside `log/events.jsonl`, so a vault is one
  portable folder ([data-model.md](data-model.md) §4).
- **Embedder:** ship the deterministic `FakeEmbedder` as the default for now (no model-download ritual,
  keeps the slice deterministic). Document that `search`'s **BM25 half is real**; the **vector half is
  not yet semantic** until the real embedder lands — do not overstate it in CLI output.
- **Tests:** CLI-level — run a command against a copied fixture vault and assert output (the "run a
  command against a fixture, assert the output" surface [vision-and-scope.md](vision-and-scope.md) names),
  plus façade tests. Reuse the golden vault; add a slightly larger fixture if useful.

**Explicitly out of scope (deliberately deferred so the slice stays thin):**
- `b2 suggest` / `b2 link --accept` / `b2 reject` — the accept operation is *built* in the engine, but
  nothing **generates** suggestions until the discovery pipeline exists, so these commands would be empty
  in real use. Add them alongside discovery.
- `b2 add` (note CRUD), `b2 mv` (move + wikilink rewrite), `b2 explain` — follow-on commands.
- Real embedder, semantic-quality eval, the qmd chunker upgrade.

**Why this slice first:** it is the product's walking skeleton — the engine's is done, but nothing yet
*uses* it. It delivers the first real dogfooding moment (point B2 at a copy of the real vault), it seeds
the single-binary artifact (vision-and-scope, headless-first), and it makes the Core API real without
speculation. The `b2id` stamp-on-reindex is the one write it performs on the real vault — by design
([data-model.md](data-model.md) §1); consider a `--dry-run` as a fast follow, not part of this slice.

### After the slice (ordered)

- **Real embedder + eval suite** — drop a local model into the embedder seam, then the eval that scores
  semantic + suggestion quality (the deferred half of steps 3 & 5; also unlocks the qmd chunker upgrade).
- **Connection-discovery pipeline** — candidate generation (the graph⨝vector join is ready) → typed,
  explained suggestions → the review loop; then the `suggest`/`accept`/`reject` CLI commands.
- **Remaining CLI + kernel ops** — `b2 add`, `b2 mv` (the move + wikilink rewrite, [user-stories.md](user-stories.md)
  Story 1), `b2 explain`.

## Backlog (later, not now)

- Property tests for the invariants — round-trip, `full-reindex ≡ incremental`, rename-keeps-backlinks as
  property tests over generated vaults (golden-vault scenarios exist; property coverage is the gap).
- qmd chunker upgrade — replace the minimal paragraph chunker once a real embedder + eval can score it
  (build spec §1.2).
- GUI — deferred per the headless-first approach ([vision-and-scope.md](vision-and-scope.md)).
