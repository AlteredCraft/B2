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

## Next up — wire the discovery pipeline (② → ③)

> **Pick this up fresh.** ① is resolved and **candidate generation is built**
> ([`discover::candidates`](../crates/b2-core/src/discover.rs)); the **`Relator` seam** exists
> ([relate.rs](../crates/b2-core/src/relate.rs); `FakeRelator` for tests); the **accept / reject / list
> engine ops** exist ([suggest.rs](../crates/b2-core/src/suggest.rs)). The one missing piece is the **glue
> that turns candidates into suggestions**, then the CLI. **Nothing generates suggestions yet** — ② is the
> slice that finally does. This is B2's reason to exist.

- **② Wire the generate pipeline** — the orchestration that runs the three built pieces end-to-end. Home: a
  new fn in [discover.rs](../crates/b2-core/src/discover.rs) (e.g. `generate_for_anchor` + a `generate_all`
  over the vault), **deterministic** like the rest of the core — timestamp (`created`) and ids (`IdGen`)
  passed in, notes iterated in **sorted b2id order** so suggestion ids are assertable under `FixedId`. Per
  note as anchor:
  1. [`discover::candidates`](../crates/b2-core/src/discover.rs)`(conn, anchor, top_n)` → the `CandidateNote`s
     (already built).
  2. Assemble the relator's inputs: anchor [`relate::NoteCtx`](../crates/b2-core/src/relate.rs)`{ b2id, title,
     text }` and, per candidate, a `relate::Candidate { note, evidence_chunk, signal, score }` —
     `evidence_chunk` = [`db::chunk_text`](../crates/b2-core/src/db.rs)`(cand.evidence_chunk_id)`; `signal` =
     the candidate-gen provenance string (e.g. `"semantic:maxsim"`), which flows to the suggestion's `source`.
  3. [`Relator::relate`](../crates/b2-core/src/relate.rs)`(&anchor, &cand)?` → on `Some(proposal)`,
     **validate [`relation::is_core`](../crates/b2-core/src/relation.rs)`(&proposal.edge_type)`** (the gate
     deferred from the seam slice — a real relator's output isn't trusted blindly; skip + count a non-core
     verb). `None` is a decline → drop.
  4. [`suggest::generate_suggestion`](../crates/b2-core/src/suggest.rs)`(conn, sink, idgen, anchor,
     cand.note_b2id, proposal.edge_type, Some(explanation), by = "agent:<relator.model_id()>", Some(signal),
     Some(proposal.confidence), created)`. It already guards on `edge_exists`, so re-running never re-proposes
     an active/pending/rejected pair — **the whole op is idempotent.**

  **Settle one small sub-decision first:** how the anchor's / candidate's `NoteCtx.text` is assembled for a
  *real* relator — join the note's chunks (`chunks_for_note` → `chunk_text`), add a `db::note_text` helper,
  or pass only the evidence chunk. Irrelevant to `FakeRelator` (content-addressed on b2ids), so pick the
  cheapest correct option and note it. Also likely needs a `db::all_note_ids` reader (list every note b2id,
  sorted). Runs fully on `FakeRelator`, provable with no LLM. **Tests:** suggestions appear for complement
  candidates; the `is_core` gate drops a non-core proposal (needs a tail-verb *stub* relator, since
  `FakeRelator` only ever emits core); a decline yields no suggestion; determinism under `FixedId`; and the
  queue survives drop→rebuild→replay. Then extend the **eval suite**'s scaffolded **suggestion-quality** half
  (precision/recall over a hand-labelled candidate set), still out of CI.
- **③ CLI + façade** — surface `suggest` (generate + list) / `accept` / `reject` on the
  [`Vault`](../crates/b2-core/src/vault.rs) façade (add ops as the commands need them — keep the surface
  minimal; `list_suggestions` / `accept_suggestion` / `reject_suggestion` already exist in
  [suggest.rs](../crates/b2-core/src/suggest.rs), and ② adds the generate op). Then the `b2 suggest` /
  `accept` / `reject` commands with `--json` for agents, like the others.
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
