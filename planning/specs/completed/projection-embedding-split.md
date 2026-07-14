---
title: "B2 — Decoupled projection & embedding (keyword-first index)"
type: note
tags: [b2, ui, desktop, tauri, reindex, indexing, embedding, projection, keyword-first, spec]
created: 2026-07-07
status: implemented
---

# B2 — Decoupled projection & embedding (keyword-first index)

> **The build spec for splitting the fused `reindex` into a model-free *projection* pass and a
> real-model *embedding* pass, so the desktop paints the file tree and answers keyword search the
> instant projection finishes — while embedding streams behind it as its own metered, cancellable
> task.** Today `reindex` fuses both behind one façade op and one model load; the desktop refreshes
> its tree only after the *whole* thing (embedding included) resolves, even though the notes were
> written to the index in the first, fast phase. This doc closes that gap.
>
> **This is the execution of [async-indexing.md](async-indexing.md) §7** ("Progressive enhancement —
> keyword-first"). That doc proved the load-bearing fact this one builds on: projection writes
> notes + chunks + FTS + edges *before* any embedding, autocommitting per statement, so a
> projected-but-unembedded index is already **consistent** — keyword search and the graph are
> complete, only vectors are missing (async-indexing.md §1/§5). This doc makes that intermediate
> state a **first-class, separately-invokable** thing rather than a mid-`reindex` accident.
>
> **This doc owns:** the `project` / `embed` façade split and the DB-derived "what still needs
> embedding" set; the small `search` change that makes a projected (unembedded) vault genuinely
> usable; the desktop's two-command orchestration (the *Shape A* decision below); and the build order.
>
> **It does not own:** the engine invariant or the reindex algorithm
> ([index-engine.md](../../index-engine.md), [index-engine-build.md](index-engine-build.md)); the async
> **progress + cancel** plumbing this reuses wholesale ([async-indexing.md](async-indexing.md) §3/§4);
> the thin-adapter charter ([desktop-ui-mvp.md](desktop-ui-mvp.md),
> [`b2-desktop/CLAUDE.md`](../../../crates/b2-desktop/CLAUDE.md)). **Auto-index-on-open**, **embed
> ordering**, a **CLI background embed**, and a **faster embedder** stay deferred (§9 / async-indexing
> §7–§8).

## 0. Scope & ground rules

Dogfooding a ~1000-note vault (the same run that motivated [async-indexing.md](async-indexing.md))
surfaced the next gap: even with async progress + cancel, the **first cold index still gates the whole
app on embedding**. The tree doesn't populate — and keyword search stays dead — until every vector is
computed, because `reindex` is one fused op and the desktop only refreshes *after* it resolves.

This doc adds **exactly one capability** — projection and embedding become two separately-invokable
passes — and holds every existing decision fixed:

- **The core stays model-free, synchronous, and deterministic** (root `CLAUDE.md`). Projection needs
  **no embedder at all** (it never touches `chunks_vec`); embedding keeps the one deterministic
  cancel checkpoint async-indexing.md §3 already added. No `async`/threads/wall-clock enter `b2-core`.
- **The host stays a dumb adapter** ([`b2-desktop/CLAUDE.md`](../../../crates/b2-desktop/CLAUDE.md)). The
  two passes become two textbook-dumb commands; the *sequencing* (project → refresh tree → embed)
  lives in the **frontend controller**, where UI flow belongs — not as branching engine logic in Rust.
- **The invariant `index = projection of (Markdown)` is untouched.** A projected-but-unembedded index
  is a *smaller* projection, never a wrong one — exactly the consistency async-indexing.md §5 already
  guarantees, now reached deliberately instead of only via cancel.

**In scope:** the `project`/`embed` façade split, the DB-derived pending set, the keyword-first
`search` fallback, and the desktop *Shape A* wiring.
**Out of scope (later, §9):** auto-index-on-open; relevance-ordered embedding; a CLI background embed;
a smaller/faster embedder.

**Handoff — start here.** An agent implementing this **begins at §8 Step 1**, written as a
self-contained, model-free brief (files + current-state anchors, the ordered moves, the tests, and a
definition of done). Steps 2–3 are sketched and get the same treatment when reached. The naming is
locked (§3): **`project`** (the model-free pass) + **`embed`** (the vectors), with **`reindex`**
remaining their composition — use those exact names.

## 1. The problem, grounded in the code

| Layer | What exists today | The gap |
|---|---|---|
| **Core** (`b2-core`) | [`ingest_vault_with_progress`](../../../crates/b2-core/src/ingest.rs) runs Phase 1 (project notes/chunks/FTS) → Phase 1b (embed) → Phase 2 (edges) in **one call**, handing the pending vectors between 1 and 1b as an **in-memory `staged` Vec**. `ensure_embedding_space` is called at the top, so the op needs the embedder's `dim` up front. | Projection and embedding are **fused**: you cannot get the (fast, model-free) keyword+graph index without also paying for (slow, model-bound) embedding in the same call. |
| **Façade** (`Vault`) | One [`reindex_with_progress(force, on_progress)`](../../../crates/b2-core/src/vault.rs); the desktop `reindex` command opens the **real model** first, then calls it. | The single op opens the model before any projection runs, and returns only when *everything* is done. There is no "just project" entry point. |
| **Search** (`Vault::search`) | Returns an **empty list** when `chunks_vec` doesn't exist yet (`vault.rs:388`) — because [`hybrid_search`](../../../crates/b2-core/src/search.rs) unconditionally calls `db::vector_search`, which needs the table. | A projected-but-unembedded vault answers **no** search at all — even though BM25 over `chunks_fts` is fully ready. "Usable while embedding" is impossible. |
| **Desktop** (`ui/` + host) | [`doReindex`](../../../ui/src/main.ts) calls `loadNotes()` only **after** the reindex Promise resolves; the Promise resolves only after embedding. | The tree can't paint early, so the whole point of the fast first phase is thrown away. |

**Root cause, in one line:** the two phases are fused because pending vectors are handed off in memory,
and the desktop refreshes the tree only after the fused op returns — so the already-written keyword +
graph index is invisible until embedding finishes.

## 2. The enabling insight — pending is DB-derivable

The reason the phases *can* be split cleanly (no in-memory handoff) is one fact about the schema:

> **"What still needs embedding" is exactly the chunks in `chunks` with no row in `chunks_vec`.**
> [`replace_chunks`](../../../crates/b2-core/src/db.rs) already clears a note's stale vectors when its
> body changes; [`note_fully_embedded`](../../../crates/b2-core/src/db.rs) already expresses the per-note
> version of this query. So the embed pass doesn't need projection to *tell* it what to embed — it can
> **query** the DB for it.

Consequences:

- **Projection needs no embedder.** It writes only `notes`/`chunks`/`chunks_fts`/`edges` — never
  `chunks_vec` — so it needs neither the model nor its `dim`. Creating the embedding space
  (`ensure_embedding_space`, the only place `dim` is needed) moves to the embed pass, where it belongs.
- **`force` is a projection concern, not an embed concern.** Today `force` re-chunks every note (fresh
  chunk ids → cleared vectors) via `replace_chunks`. So `project(force)` re-chunks → clears vectors →
  `embed()` simply refills whatever lacks a vector. The embed pass therefore needs **no `force`
  parameter** — it is purely "fill missing vectors."
- **Determinate progress from t=0.** The embed pass can count the notes with missing vectors in one
  cheap query before it starts, so its progress bar is determinate immediately — resolving
  async-indexing.md §9's "indeterminate during Phase 1" open question without a double scan.

## 3. Decisions locked (2026-07-07)

| Concern | Locked choice | Rejected — and why |
|---|---|---|
| **The split** | **Two façade ops**: `Vault::project()` (Phase 1 + Phase 2, model-free) and `Vault::embed(on_progress)` (fill missing vectors, real model). `reindex_with_progress` **composes** them, so the CLI and every existing test are unchanged. | **A milestone event inside one fused op** (async-indexing's channel gains a "projection done" variant) — keeps the phases fused, keeps the model on the first-paint path, and churns the progress type. Recorded as *Shape B* and not taken (§6). |
| **Naming** | **`project`** (the model-free pass — notes/chunks/FTS/edges) + **`embed`** (the vectors); **`reindex`** stays the composed verb. Extends the existing `project_note_and_chunks` / `project_edges` and `embed_pending` names in `ingest.rs`, so it's a widening of the vocabulary, not a new coinage. *(The invariant's "index = projection of Markdown" still means the* full *index — `project` + `embed` together; the pass is named for the row-projection it already performs in the code, and the doc-comment says so to head off the overload.)* | **`index` / `embed`** — "index" already names the whole SQLite store (FTS + vec + graph), so it can't stand for just the keyword half. **`reindex_keyword` / `reindex_embeddings`** — verbose, and welds the two names to "reindex". |
| **Pending set** | **DB-derived** — chunks with no `chunks_vec` row (§2). The embed pass queries it; nothing is handed between passes in memory. | **In-memory `staged` handoff** (today's shape) — the very thing that couples the two phases; can't survive as two separate calls. |
| **Projection's embedder** | **None.** `project()` takes no embedder and never touches the embedding space. | **Pass the real model's identity** so projection can pre-create `chunks_vec` — needs `dim`, reintroduces the model dependency, and buys nothing (space creation is embed's job). |
| **`force`** | Lives on **`project(force)`** (re-chunk everything → clear vectors); `embed()` has no `force`. | A `force` on `embed` — would need `embed` to overwrite live vectors (an `INSERT OR REPLACE` on `chunks_vec`), duplicating what re-chunk already does. |
| **Keyword-first `search`** | When `chunks_vec` is **absent**, `search` runs **BM25-only** (no query embedding, no model) instead of returning empty. The model-mismatch fail-fast stays for when the space *does* exist. | **Keep `search` gated on the space** — leaves a projected vault unusable for search, defeating "work with it while it embeds." |
| **Desktop orchestration** | ***Shape A* — two commands, the frontend sequences them.** `project` (fake vault, model-free) then `embed` (real model, carrying today's single-in-flight guard + cancel). `doReindex` awaits `project`, calls `loadNotes()`, then awaits `embed`. | ***Shape B* — one command + milestone.** Its only edge (one guarded slot spanning both phases) is free in A anyway (§5.3); its cost (model load before projection, or a two-vault command body that smuggles orchestration into the host) is real. Details in the transcript that produced this doc. |
| **CLI** | **Unchanged** — one `b2 reindex` = project + embed, one live progress line, byte-identical output. A CLI keyword-first / background-embed split is a separate effort (async-indexing.md §8). | Splitting the CLI command too — no CLI user need yet; the stateless one-process model wants the different answer async-indexing.md §8 already records. |

## 4. The core seam — `project` and `embed`

Evolve `b2-core`'s ingest into two entry points plus a composing wrapper. All three are model-free to
*call* except `embed`, which takes the embedder it actually uses.

- **`ingest::project_vault(conn, root, idgen, force) -> ProjectOutcome`** — Phase 1 (project every
  note + its chunks + FTS; stamp missing `b2id`s) then Phase 2 (edges), exactly as today **minus the
  embed phase and minus `ensure_embedding_space`**. The per-note re-chunk decision becomes purely
  `force || stored body_hash ≠ new hash || note is new` — it **no longer consults vector state**
  (`note_fully_embedded`), because "unchanged body but missing vectors" is now the embed pass's job,
  not a reason to re-chunk. Returns per-note `indexed`/`stamped` counts.
- **`ingest::embed_vault(conn, embedder, on_progress) -> EmbedOutcome`** — `ensure_embedding_space`
  (creates `chunks_vec` at the model's `dim`; a model swap drops + resets it, so *all* chunks then
  count as missing) → query the **chunks with no vector**, grouped by note in `(path, seq)` order →
  batch-embed via the existing [`embed_pending`](../../../crates/b2-core/src/ingest.rs) machinery, firing
  `ReindexProgress` per batch and honoring the `ControlFlow::Break` cancel checkpoint unchanged. No
  `force`. Returns `embedded`/`cancelled`.
- **`ingest::ingest_vault_with_progress(...) = project_vault(force) then embed_vault(on_progress)`** —
  a thin composition, so `Vault::reindex_with_progress` and the existing `cancel.rs` / `embed.rs` tests
  keep calling one function and stay green. The composed run is byte-identical to today's from a clean
  index; the sole intentional divergence is a resume-after-partial run, where `project` leaves an
  unchanged-body note's chunks in place rather than regenerating their rowids (observably identical —
  see §7.1). Note the composed order is `project` (Phase 1 **and** Phase 2) then `embed`, so edges are
  now projected *before* embedding; they're independent, so this reordering changes nothing.

New DB helper: **`db::chunks_missing_vectors(conn) -> Vec<(note_b2id, path, chunk_id, text)>`** — the
`chunks LEFT JOIN chunks_vec WHERE v.chunk_id IS NULL`, joined to `notes` for `path`, ordered
`(path, seq)` so the embed pass reproduces today's per-note batching + progress deterministically.
(Generalizes the per-note `note_fully_embedded` query already in `db.rs`.)

Façade surface (mirrors the ops one-to-one; the composing wrapper stays for the CLI):

```
Vault::project(force) -> ProjectReport { indexed, stamped }          // model-free
Vault::embed(on_progress) -> EmbedReport { embedded, cancelled }     // real model
Vault::reindex_with_progress(force, on_progress) -> ReindexReport    // = project + embed (unchanged)
```

**Determinism preserved.** Projection is pure (no model). Embedding keeps the deterministic
batch-boundary cancel check. A composed `reindex` that is never cancelled produces the same bytes as
before; the fake embedder keeps the split unit-testable.

## 5. The keyword-first `search` fallback

One small change makes a projected (unembedded) vault genuinely usable:

- In `Vault::search`, when `db::embedding_space_exists` is **false**, run **BM25-only** — call
  `search::keyword_search` (which touches only `chunks_fts`, no model, no query embedding) and resolve
  its chunk hits to notes — instead of the current early `return Ok(Vec::new())`. When the space *does*
  exist, behavior is unchanged: the model-mismatch fail-fast, then full hybrid RRF (the vector half is
  naturally partial while embedding is mid-flight, and RRF already tolerates that).
- **Honesty (async-indexing.md §7).** `vault_info.semantic` already tells the UI whether semantic
  ranking is live; a projected-but-unembedded vault should read as "keyword-only for now" rather than
  silently under-ranking. Surfacing a "semantic: N/M embedded" signal is a follow-on (§9), not required
  for this slice — the honest empty state + the existing `semantic` flag suffice to ship.

## 6. The host & frontend — Shape A

Two dumb commands replace the single `reindex` command; the guard/cancel machinery
([`AppState`](../../../crates/b2-desktop/src/main.rs), unchanged) attaches to the embed one.

- **`project(state) -> ProjectReport`** — opens the **fake** vault (`open_vault(state, false)` — no
  model load), calls `vault.project(false)`, returns. Fast; nothing to stream.
- **`embed(state, on_event: Channel<ReindexProgress>) -> EmbedReport`** — opens the **real** vault
  (`open_vault(state, true)`), claims the single-in-flight slot, arms the cancel flag, and calls
  `vault.embed(on_progress)` with today's forward-progress-and-consult-cancel closure. This *is*
  today's `reindex_impl`, minus the projection it no longer does. `cancel_reindex` and
  `cancel_and_wait_for_reindex` (vault-switch) point at this command unchanged.
- **Frontend `doReindex()`** becomes linear:

  ```
  state.reindexing = true; render()
  await api.project()          // fast — keyword + graph index complete
  await loadNotes()            // the tree paints HERE; keyword search is live
  const r = await api.embed(onProgress)   // determinate progress bar + Cancel
  ...refresh open note / discovery as today...
  ```

**Why leaving `project` outside the slot is safe (§5.3 of the decision):** the single-in-flight slot
guards only `embed`. A second Reindex can't fire (`state.reindexing` disables the button across both
awaits), and a vault switch during the short `project` window is harmless because `project` opened its
`Vault` over the root it **captured at dispatch** (`open_vault` clones `current_root()` then) — so a
late-finishing `project` writes the *old* vault's own `.b2/`, idempotently, and never touches the new
one. `doReindex` already bails from UI updates when `vaultRoot` changed mid-run. The slot therefore
only ever needs to protect the long, vector-writing `embed` pass — exactly what it protects today.

**Still dumb.** Each command is "deserialize → one façade call → serialize." The project→embed
sequencing is three lines of frontend UI flow, not engine logic — no branch or rule crosses into Rust.

## 7. Reliability & correctness invariants

Inherits async-indexing.md §5 wholesale (partial index consistent; `incremental ≡ eventual full`;
cooperative cancel; concurrent reads safe under WAL; generic errors), and adds:

1. **`project + embed` is observably equivalent to today's fused reindex — and the existing suite proves
   it.** `reindex` stays defined as the composition, so the current `embed.rs` / `cancel.rs` tests
   (incremental counts, `force`, cumulative progress, cancel-and-resume) are the regression guard and
   must stay green. From a clean index the composed run is byte-identical (chunk ids are assigned fresh
   in file order). The **one intentional divergence** is the resume-after-partial case: `project` no
   longer re-chunks an unchanged-body note merely because it lacks vectors (that is `embed`'s job now),
   so the note keeps its chunk rows instead of getting fresh rowids. The result is identical in every
   *observable* way (notes, chunk text, FTS, text→vector, edges) — only internal rowids differ, and
   nothing durable or exposed depends on them (`cancel.rs` already asserts counts, not ids). Tests for
   the split assert observable equivalence, never rowid equality.
2. **Convergence after any interruption.** Because the pending set is *derived* (`chunks_missing_vectors`),
   any stop point — a cancelled embed, a crash between `project` and `embed`, a second `project` before
   `embed` — heals on the next `embed`: it embeds exactly the chunks still lacking a vector. No state to
   corrupt, nothing recomputed that's already done.
3. **A projected vault is a usable vault.** After `project` and before any `embed`: file tree lists
   (`list_notes`), notes open (`read`), keyword search answers (§5), the graph resolves
   (`neighbors`/`explain`). Only `similar` and the semantic half of `search` wait for vectors — and
   they degrade to empty/keyword honestly, never error.

## 8. Build order

Each step is a provable increment; the desktop wiring (Step 3) is the only one that touches Tauri.
**Implementers start at Step 1** — it is specified in full below and is entirely within `b2-core`
(model-free, no host/UI/search changes). It *is* the architectural decoupling; Steps 2–3 build on it.

### Step 1 — the core split (start here)

**Goal.** Factor the fused [`ingest_vault_with_progress`](../../../crates/b2-core/src/ingest.rs) into a
model-free `project_vault` and a model-bound `embed_vault`, expose them as `Vault::project` /
`Vault::embed`, and keep `reindex` as their composition. Nothing user-visible changes; this is a pure
refactor that *separates* the two passes and derives the pending set from the DB (§2). Entirely in
`b2-core`, entirely model-free to test (fake embedder).

**Files & current-state anchors.**
- [`crates/b2-core/src/ingest.rs`](../../../crates/b2-core/src/ingest.rs) — `ingest_vault_with_progress`
  is the fused fn to split; `project_note_and_chunks` computes today's in-memory `pending`;
  `embed_pending` is the batched + `ControlFlow`-cancel loop to **reuse verbatim**; `would_reembed` is
  kept **only** for the dry-run (do not reuse it in `project`).
- [`crates/b2-core/src/db.rs`](../../../crates/b2-core/src/db.rs) — add `chunks_missing_vectors`;
  `note_fully_embedded` (line ~408) is its per-note ancestor; `ensure_embedding_space` /
  `embedding_space_exists` / `replace_chunks` / `set_chunk_vector` are unchanged.
- [`crates/b2-core/src/vault.rs`](../../../crates/b2-core/src/vault.rs) — add `Vault::project` /
  `Vault::embed`; `reindex_with_progress` stays (now composes them).
- Tests: [`tests/embed.rs`](../../../crates/b2-core/tests/embed.rs) and
  [`tests/cancel.rs`](../../../crates/b2-core/tests/cancel.rs) must stay green; add the new tests below.

**The moves (in order).**
1. `db::chunks_missing_vectors(conn) -> Vec<(String /*note_b2id*/, String /*path*/, i64 /*chunk_id*/, String /*text*/)>`
   — `chunks LEFT JOIN chunks_vec WHERE v.chunk_id IS NULL`, joined to `notes` for `path`, ordered
   `(path, seq)`. This is the DB-derived pending set that replaces the in-memory `staged` hand-off.
2. `ingest::project_vault(conn, root, idgen, force) -> ProjectOutcome` — Phase 1 (project every note +
   chunks + FTS, stamp missing `b2id`s) then Phase 2 (edges). **Re-chunk predicate = `force || stored
   body_hash ≠ new hash || note is new`, read only from `notes`.** Do **not** call `would_reembed` /
   `note_fully_embedded` and do **not** call `ensure_embedding_space` here — that is what keeps
   `project` free of `chunks_vec` (and thus of the model/`dim`). Takes no embedder; returns no pending.
3. `ingest::embed_vault(conn, embedder, on_progress) -> EmbedOutcome` — `ensure_embedding_space`
   (creates `chunks_vec`; a model swap drops + resets it, so all chunks then read as missing) → group
   `chunks_missing_vectors` by note → the existing per-note `embed_pending` loop, firing
   `ReindexProgress` and honoring `ControlFlow::Break` unchanged. `notes_to_embed` = distinct notes with
   missing vectors, counted up front → determinate progress from t=0. **No `force`** (§2/§3).
4. `ingest_vault_with_progress = project_vault(force) then embed_vault(on_progress)`; merge the two
   outcomes (`indexed` + `stamped` from project, `embedded` + `cancelled` from embed) so `ReindexReport`
   is unchanged. `ingest_vault` (no-op progress) and `Vault::reindex` keep their signatures.
5. Façade: `Vault::project(force) -> ProjectReport { indexed, stamped }` and
   `Vault::embed(on_progress) -> EmbedReport { embedded, cancelled }`.

**The one behavioral subtlety** (§7.1): the composed order is now project(Phase 1 + Phase 2) → embed, so
edges precede embedding (independent — immaterial), and a resume-after-partial run preserves an
unchanged-body note's chunk rowids instead of regenerating them. Assert **observable** equivalence
(counts, chunk text, text→vector, edges), never rowid equality.

**New tests (fake embedder, model-free).**
- `project_only_builds_keyword_graph_index_with_no_vectors` — project the golden vault (no embed):
  `chunks > 0`, `chunks_fts == chunks`, `edges > 0`, and `embedding_space_exists` is **false** (no
  `chunks_vec` created).
- `embed_fills_exactly_the_missing_vectors` — after project, `embed` → `chunks_vec == chunks`; a second
  `embed` fills 0.
- `project_then_embed_matches_reindex` — on a fresh golden copy, project+embed yields the same
  note / chunk-text / edge counts and the same text→vector as `Vault::reindex()` on a sibling copy.

**Definition of done.** `cargo test -p b2-core` green (existing suite + the three new tests); no
candle/model/tokenizers dep added to `b2-core`; `project_vault` provably issues no query against
`chunks_vec`.

**Out of scope for Step 1 — do not touch.** `Vault::search` (Step 2); the desktop host + `ui/`
(Step 3); `ingest_file` (the single-note `link`/`add`/`mv` path — leave as-is, it still embeds inline);
`plan_reindex` / `would_reembed` (the dry-run — unchanged; its `would_embed` still predicts the composed
run's embed set correctly, since a body-changed *or* vector-missing note both end up embedded).

### Step 2 — keyword-first search (sketch)

`Vault::search` gains a BM25-only fallback when the vector space is absent (§5). Test: project a vault
(no embed), assert keyword search returns hits and `similar` is empty — no error. Gets the full
Step-1-style brief when it's picked up.

### Step 3 — desktop Shape A (sketch)

Split the host `reindex` command into `project` + `embed` (guard/cancel on `embed`); `api.ts` gains
`project()` and `embed()`; `doReindex` sequences project → `loadNotes` → embed (§6). Thin host tests for
the two commands + the unguarded-project safety note. Manual dogfood: cold-index the ~1000-note vault,
confirm the tree + keyword search are live within seconds, embedding meters behind, Cancel still works.

*Later (follow-on, §9): auto-index-on-open (#25); the "semantic N/M" signal (#26); embed ordering (#27).*

Step 1 alone is the whole architectural decoupling (and de-risks the rest); Steps 2–3 turn it into the
UX the desktop wants.

## 9. Open questions / deferred

- **Auto-index-on-open** (async-indexing.md §7). Detect an unindexed/partly-indexed vault on open and
  offer/start `project` (then `embed`) immediately, so a first-run vault is keyword-usable in seconds
  without a manual click. A first-run UX call taken when this lands; not this slice. **Tracked: #25.**
- **"Semantic N/M embedded" signal.** ~~Extend `vault_info` (or a light `embed_status` read) so search
  results can flag "keyword-only for now" precisely, not just via the binary `semantic` flag (§5).~~
  **Shipped 2026-07-14 (#26):** `Vault::embed_status` reads `db::embed_progress` — a model-free
  `(embedded, total)` count over the projection; `vault_info` carries the fraction alongside `semantic`,
  and the desktop search caveat reads "keyword-only for now (N/M embedded)" while a projected vault embeds
  (and "keyword-first (N/M embedded)" once partial), instead of the binary flag alone. **Tracked: #26.**
- **Relevance-ordered embedding** — embed the open note + its neighbors first, so discovery lights up
  for what the user is looking at soonest. Pure ordering; orthogonal to the split. **Tracked: #27.**
- **CLI keyword-first / background embed** — the stateless CLI wants the different answer
  async-indexing.md §8 records; unchanged here. **Tracked: #16.**

## 10. Docs to mirror (doc-driven follow-ups)

Per the design-docs-are-source-of-truth discipline:

- [async-indexing.md](async-indexing.md) — §7 ("Progressive enhancement — keyword-first") is **being
  executed here**; add a pointer noting this doc owns the projection/embedding split it anticipated.
- [tasks.md](../../tasks.md) — promote "decouple embedding from indexing" to an active work item tracking
  Steps 1→3, pointing here.
- [index-engine.md](../../index-engine.md) / [index-engine-build.md](index-engine-build.md) — Flow ① is
  described as one pass; note that projection and embedding are now separately invokable (the fused
  `reindex` is their composition), with no change to the invariant or the two-phase link resolution.
- [desktop-ui-mvp.md](desktop-ui-mvp.md) — the `reindex` surface is now project-then-embed; add a
  pointer once Step 3 ships.
