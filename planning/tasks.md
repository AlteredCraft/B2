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
- [x] **Relator seam** — the classify/explain step of connection discovery now sits behind a swappable
  **`Relator`** trait ([relate.rs](../crates/b2-core/src/relate.rs)), mirroring
  [`Embedder`](../crates/b2-core/src/embed.rs): `relate(anchor, candidate) -> Result<Option<Proposal>>`,
  **pairwise**, with `Ok(None)` as a first-class **decline** — candidate generation over-produces, and the
  relator is the precision gate that prunes. `Proposal { edge_type, explanation, confidence }` maps 1:1 onto
  [`generate_suggestion`](../crates/b2-core/src/suggest.rs) (relator owns type/explanation/confidence + `by`
  via `model_id`; candidate-gen owns src/dst/`source`). Ships the deterministic **`FakeRelator`**
  (content-addressed on the `b2id` pair like `FakeEmbedder`; always emits a **core** verb, declines 1-in-4
  to exercise the prune path) so the pipeline is provable with no LLM. The real LLM-backed relator is
  deferred to its own crate (the `LocalEmbedder`/`b2-embed` precedent), keeping `b2-core` model-free. 5
  relator tests; **78** workspace tests green.
- [x] **Connection-discovery ① + candidate generation** — the first discovery stage now exists. **① resolved
  2026-07-01**, mirrored to [index-engine.md](index-engine.md) §3 + [docs/architecture.html](../docs/architecture.html)
  (new Connection-discovery section + relator seam): a candidate is the graph's *complement* — **near ∖
  connected** — not the intersection ([`graph_filtered_search`](../crates/b2-core/src/search.rs) is a
  scoped-traversal primitive, the wrong tool). Mechanism: per anchor chunk, KNN its **stored** `chunks_vec`
  vector (vector-only, **no re-embed**, passage↔passage — `embed_query`'s asymmetric prefix is the wrong
  side); score each other note by its **best** chunk-pair (**max-sim**); subtract
  [`reachable_within`](../crates/b2-core/src/graph.rs)`(anchor, 1)` (distance is **exclusion-only** — 2-hop
  triadic-closure candidates survive unboosted; distance-weighting is a backlog eval experiment); rank →
  top-N. Anchor text is **per-chunk**, not whole-note. Built
  [`discover::candidates`](../crates/b2-core/src/discover.rs) (+ db readers `chunks_for_note` / `chunk_vector`,
  `embed::unpack_f32`); 7 discover tests, **85** workspace tests green.
- [x] **Connection-discovery ② — the generate pipeline, wired end-to-end** — the glue that finally turns the
  three built pieces into suggestions now exists:
  [`discover::generate_for_anchor`](../crates/b2-core/src/discover.rs) + a
  [`generate_all`](../crates/b2-core/src/discover.rs) over the vault. Per anchor:
  [`candidates`](../crates/b2-core/src/discover.rs) → assemble the relator's borrowed inputs (anchor +
  per-candidate [`NoteCtx`](../crates/b2-core/src/relate.rs) / [`Candidate`](../crates/b2-core/src/relate.rs),
  `evidence_chunk` = [`db::chunk_text`](../crates/b2-core/src/db.rs), `signal="semantic:maxsim"` → the
  suggestion's `source`) → [`Relator::relate`](../crates/b2-core/src/relate.rs) → on `Some`, **validate
  [`relation::is_core`](../crates/b2-core/src/relation.rs)** (a real relator's verb is checked, not trusted —
  a non-core proposal is dropped + counted, never persisted) →
  [`suggest::generate_suggestion`](../crates/b2-core/src/suggest.rs) (`by="agent:<model_id>"`). Deterministic
  + idempotent like the rest of the core: `created`/`IdGen` passed in, anchors iterated in **sorted b2id
  order** ([`db::all_note_ids`](../crates/b2-core/src/db.rs)), and `generate_suggestion`'s `edge_exists` guard
  means a re-run proposes nothing new — every candidate lands in exactly one of
  `{generated, declined, non_core, existing}` ([`GenerateOutcome`](../crates/b2-core/src/discover.rs)).
  **Sub-decision resolved:** `NoteCtx.text` is the note's chunks joined
  ([`db::note_text`](../crates/b2-core/src/db.rs)) — the body as the index already holds it, cheapest-correct
  (a real relator reads it; `FakeRelator` ignores it, content-addressed on b2ids). Runs fully on
  `FakeRelator`, no LLM. **7 pipeline tests** (purpose-built relator stubs drive fire-core / decline /
  tail-verb exactly; `FakeRelator` proves the seam runs through; determinism across rebuild; idempotent
  re-run; queue survives drop→rebuild→replay); **92** workspace tests green.

- [x] **Connection-discovery ③ — the CLI + façade surface** — `suggest` / `accept` / `reject` now ship, so
  the review queue is reachable from the terminal. Four ops on the [`Vault`](../crates/b2-core/src/vault.rs)
  façade (`generate_suggestions` wrapping [`discover::generate_all`](../crates/b2-core/src/discover.rs) on the
  `FakeRelator`; `list_suggestions` resolving both ends to path+title as `SuggestionView`;
  `accept_suggestion` / `reject_suggestion`), and the `b2 suggest` (generate-then-list, idempotent) /
  `b2 accept <id>` / `b2 reject <id>` commands with `--json`. Wiring decisions: `suggest` needs **no model**
  (candidate-gen reads stored vectors, the relator is a stub) so it opens with the fake like `neighbors`;
  `accept` re-projects (re-embeds) the source note so it loads the **same embedder the index was built with**
  (real model, like `reindex`); `reject` touches no vectors. Timestamps come from **SQLite** (the
  `indexed_at` clock) via a façade `now()`, keeping `b2-core` wall-clock-free (engine ops still take
  `created`/`decided`). Honest to the user: `suggest` prints a loud **stub-relator caveat** + a generation
  summary on stderr (stdout stays pure results); a bad `accept`/`reject` id is a clean nonzero exit
  (`CliError::SuggestionNotFound`), no internals leaked. **6 CLI tests** (generate+list human/JSON,
  empty-before-reindex, accept writes the frontmatter link + leaves the queue, reject tombstones,
  accept/reject JSON shapes, unknown-id fails cleanly); **98** workspace tests green.

## Next up — make the suggestions real (the LLM relator), then kernel CRUD

> **Pick this up fresh.** The discovery pipeline is **end-to-end and reachable** — `b2 suggest` / `accept` /
> `reject` work — but the intelligence is a **stub**: [`FakeRelator`](../crates/b2-core/src/relate.rs) hashes
> the note pair, it never reads the notes (the CLI says so loudly). Two fronts remain: make the suggestions
> *real* (the LLM relator behind the existing seam), and the note-authoring kernel ops (`add` / `mv`) B2 still
> can't do.

- **The real relator** — the LLM-backed relator in its **own crate** (the `b2-embed` / `LocalEmbedder`
  precedent — keep `b2-core` model-free), dropped in behind the existing
  [`Relator`](../crates/b2-core/src/relate.rs) seam. `Vault::generate_suggestions` swaps the fake for it
  (mirror the embedder's `open_with_embedder` injection — the façade already reads `NoteCtx.text` from
  [`db::note_text`](../crates/b2-core/src/db.rs)), and the CLI's stub-relator caveat comes off. Then the
  **suggestion-quality eval** — extend the eval suite's scaffolded half (precision/recall over a
  hand-labelled candidate set), out of CI, exactly as the retrieval eval needs a real embedder.
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
- Distance-weighting for candidate ranking — v1 ranks candidates by semantic max-sim alone (graph distance
  is exclusion-only, ① resolved 2026-07-01). Once the suggestion-quality eval exists (②), measure whether
  boosting graph-*close* (triadic closure) or graph-*distant* (serendipity/bridging) candidates lifts
  accept-precision — and only add the knob if the eval says so.
- GUI — deferred per the headless-first approach ([vision-and-scope.md](vision-and-scope.md)).
