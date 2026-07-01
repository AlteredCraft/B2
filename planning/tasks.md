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

> **Pick this up fresh — start a new session here.** The walking skeleton is done and on `main`
> (commit `feat(cli): b2 CLI over a typed Vault core API`, 67 tests). The `b2` CLI drives a typed
> `b2_core::Vault` façade against a real vault — smoke-tested on a ~1000-note copy of the primary vault
> (indexed in ~10s, search fast). But the shipped embedder is the deterministic **fake**, so `search`'s
> vector half — and, later, discovery candidate generation — is **not yet semantic**. This is the
> deferred quality half of build-spec steps 3 & 5, and the one place the architecture meets real
> friction ([index-engine.md](index-engine.md) §6). **The gating decisions are now locked (below) — go
> straight to the build order.**

**Decisions locked (2026-06-30).** Mirrored in [index-engine.md](index-engine.md) §6/§8 and
[vision-and-scope.md](vision-and-scope.md):
- **Runtime = `candle` + `hf-hub`.** Pure-Rust inference compiled *into* the binary — no external ONNX
  Runtime lib to ship (best fit for the single-binary goal, principle #5); `hf-hub` is the download seam.
- **Model = EmbeddingGemma-300M @ dim 768.** 768 becomes the `chunks_vec` `FLOAT[N]` type; fine for
  brute-force KNN at vault scale (§8). Apply its prompt prefixes — `title:… | text:` for documents,
  `task:… | query:` for the query (§5). *Fallback:* if EmbeddingGemma is fiddly in candle, a known-good
  candle embedding model (bge / gte / nomic-embed) — confirm in the spike before committing.
- **Not bundled → explicit `b2 init` download.** The binary never carries the model and never
  surprise-downloads mid-command. `b2 init` fetches + verifies the model; `reindex`/`search` **fail fast**
  with "run `b2 init`" if the files are absent (the fail-fast config rule).
- **Model source is configurable.** Default = an HF repo id; overridable to a mirror (`HF_ENDPOINT`), a
  different repo, or a **local path** (fully-offline install). Lives in a global TOML at
  `$XDG_CONFIG_HOME/b2/config.toml` → `[embedder] model / source / cache_dir`.
- **Model cached in a shared XDG dir** (e.g. `~/.local/share/b2/models/`), **not** per-vault `.b2/` —
  it's a machine-level runtime dep, one copy per machine. (The vault still records which model built its
  vectors in `meta`, which is how a swap triggers a re-embed.)
- **Fix the `open()`-time drop.** `ensure_embedding_space` currently runs in `Vault::open` and would
  silently drop `chunks_vec` on a model/dim mismatch — so a config change could wipe vectors on the next
  `search`. Move it: `open` never mutates; a read that sees `meta.model ≠ config.model` **fails fast**
  ("index built with model X; run `b2 reindex`"); re-embed happens only on `reindex`.

**Build order (spike/test-first, like every slice before it):**
1. **Spike** — prove `candle` + `hf-hub` can download EmbeddingGemma-300M, embed a string, and KNN it at
   the expected dim. Cheapest way to de-risk the one uncertain part; pick the fallback model here if needed.
2. **Real `Embedder` impl** behind the existing seam ([`embed::Embedder`](../crates/b2-core/src/embed.rs)):
   tokenizer + mean-pool + normalize + the prompt prefixes. Plus the TOML config loader.
3. **`b2 init`** — download + verify into the XDG cache; friendly progress; idempotent (skip if present).
4. **Wire as the Vault/CLI default** — one point: `crates/b2-core/src/vault.rs` (`EMBED_DIM = 64` +
   `FakeEmbedder::new(…)` → the real impl, likely `Box<dyn Embedder>`, at dim 768). Apply the `open()`
   fail-fast fix. **Keep `FakeEmbedder` for all existing tests** (determinism — the real model must never
   enter the fast CI suite, testability points 4–5). Then relax the CLI `search` semantic caveat.
5. **Eval suite** — a separate, occasional pass scoring **semantic + suggestion quality**
   (precision/recall) against a small hand-labelled set, kept **out of the deterministic CI tests so
   model quality never flakes CI** ([vision-and-scope.md](vision-and-scope.md), testability point 5).

**Not in scope (keep it thin):** query expansion (qmd's 1.7B third model — off-by-default, later); a
reranker; the actual packaging/distribution build (strategy is decided here; ship the installer later).
**Unlocks:** the qmd chunker upgrade (a real embedder can finally score paragraph vs. qmd chunking —
build spec §1.2) and honest semantic `search`.

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
