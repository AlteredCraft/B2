---
b2id: 01KWSRKASCAYXRPQ1AP5HE6R2N
title: "B2 — Index Engine: rebuild qmd on SQLite"
type: note
tags: [b2, index-engine, sqlite, fts5, sqlite-vec, search, reranker, architecture]
created: 2026-06-29
status: draft
---

# B2 — Index Engine: rebuild qmd on SQLite

> **Findings for the "Index-engine evaluation" task** ([tasks.md](tasks.md)). Evaluates the idea of
> rebuilding [tobi/qmd](https://github.com/tobi/qmd) on our own SQLite store (FTS5 + `sqlite-vec`,
> reranker as a fast follow) instead of adopting qmd as a dependency. Context:
> [vision-and-scope.md](vision-and-scope.md) (semantic search is **engine-gated**; single-binary;
> local-first) and the data model leans in [tasks.md](tasks.md).

## TL;DR / recommendation

**Build our own SQLite-backed index engine; take qmd as a design reference, not a dependency.**

- qmd is an excellent *blueprint* for hybrid retrieval (BM25 + vector + RRF + LLM rerank) and proves
  the whole pipeline runs locally. But it is a **search engine**, and B2 is not — B2 is a **typed graph
  with hybrid retrieval over it**. qmd has no notion of typed edges, backlinks, or `b2id`-stable identity,
  which are the reasons B2 exists ([vision-and-scope.md](vision-and-scope.md), capability areas 3, 5).
- SQLite gives us **one embedded store for every *queryable* concern at once** — full-text (FTS5),
  vectors (`sqlite-vec`), and the typed graph — with transactional consistency across them, so
  `b2 similar` candidate generation joins all three in a single query. That single-store property is
  worth more to B2 than anything we'd inherit by depending on qmd. The index is a pure **disposable
  cache**: `index = projection of (Markdown)` — drop it, reindex, get it back identical, with **no
  durable state outside your notes** (two tiers, [data-model.md](data-model.md); the review-layer /
  event-log tier was removed with the LLM relator, 2026-07-04).
- Because `sqlite-vec` **does** provide vector search, the locked **engine-gated** decision resolves in
  favour of **semantic search in v1**, not as a fast follow ([vision-and-scope.md](vision-and-scope.md),
  "Decisions locked 2026-06-28").
- The **reranker is a clean fast-follow**: a swappable seam after RRF fusion, exactly as the testability
  stack wants the AI parts isolated. Retrieval quality is good without it; it's pure upside later.
- The one genuinely hard part is **not the engine** — it's **producing embeddings inside a single
  binary** ([vision-and-scope.md](vision-and-scope.md), principle #5). qmd solves this with
  `node-llama-cpp` + GGUF + Node 22, a heavy stack that fights the single-binary goal. This is the real
  decision to make, and it is **orthogonal to choosing SQLite** (see §7).

---

## 1. What qmd actually is (the reference)

A local CLI search engine for Markdown, all on-device. The shape worth stealing:

- **Storage:** SQLite at `~/.cache/qmd/index.sqlite` with FTS5 + `sqlite-vec`. Tables: `collections`,
  `path_contexts`, `documents`, `documents_fts` (FTS5/BM25), `content_vectors` (chunk metadata),
  `vectors_vec` (`sqlite-vec` index), `llm_cache`.
- **Chunking:** ~900-token chunks, ~15% overlap, Markdown-aware break-point scoring (H1=100, H2=90,
  code-fence=80, … blank-line=20, list-item=5), with a 200-token backward scan and quadratic distance
  decay to pick the cleanest boundary. Optional tree-sitter AST chunking for code files.
- **Three search modes:** `search` (BM25 only), `vsearch` (vector only), `query` (hybrid).
- **Hybrid pipeline (`query`):** LLM query expansion (1–2 variants, original weighted 2×) → parallel
  BM25 + vector retrieval per variant → **RRF fusion** (`Σ 1/(k+rank+1)`, k=60) + small top-rank
  bonuses → keep top 30 → **LLM rerank** (cross-encoder, 0–1) → **position-aware blend** (top ranks
  trust retrieval more, deep ranks trust the reranker more).
- **Models (local GGUF via `node-llama-cpp`):** EmbeddingGemma-300M (embed, ~300 MB),
  Qwen3-Reranker-0.6B (rerank, ~640 MB), a fine-tuned 1.7B (query expansion, ~1.1 GB). ~2–3 GB VRAM
  with all three loaded.
- **Surfaces:** rich CLI, JSON/CSV/files output for agents, an **MCP server** (stdio + HTTP daemon),
  and a TypeScript SDK (`createStore`).
- **Stack:** TypeScript on Node 22+/Bun; MIT licensed.

It's a clean, well-thought-out design. The disagreement is **scope**, not quality.

## 2. Why we rebuild instead of depend on qmd

| Concern | qmd | B2's need |
|---|---|---|
| Full-text search | ✅ FTS5/BM25 | ✅ same |
| Semantic search | ✅ `sqlite-vec` | ✅ same |
| Rerank | ✅ cross-encoder | ✅ fast-follow |
| **Typed graph** (`b2id→b2id` edges with a relation type) | ❌ none | ⭐ core (areas 3, 5) |
| **Backlinks** (who points at X, typed, over the whole vault) | ❌ none | ⭐ core (area 5) |
| **`b2id`-keyed identity** surviving move/rename | ❌ path-keyed, cache is disposable | ⭐ core (user-stories 1–2) |
| **Markdown as source of truth** (index is rebuildable/derived) | ~ index *is* the artifact | ⭐ non-negotiable (principle #1) |
| Distribution | npm package, Node runtime | ⭐ single binary (principle #5) |

The decisive point: B2's index is a **derived projection of the vault** that holds the **typed graph**
*next to* the search indexes, so retrieval and connection discovery share one transactional store —
`index = projection of (Markdown)`, drop-and-rebuild at any time ([data-model.md](data-model.md)). qmd
models none of the graph layer — wrapping it would mean maintaining a second store for everything that
makes B2 *B2*, and reconciling two sources of truth. Rebuilding the ~300 lines of retrieval glue that we
actually want is cheaper than that integration tax — and qmd's MIT license + public design make the
rebuild low-risk.

**What we borrow wholesale:** the chunking heuristic, the RRF formula and k, the position-aware blend,
the asymmetric query/document prompt discipline (each model brings its own prefix — B2 ships bge's, §6;
not EmbeddingGemma's `task:…|query:` / `title:…|text:`), the JSON/`--explain`
agent-output discipline, and the MCP surface idea. **What we discard:** the npm/Node packaging and the
"DB is the product" framing.

## 3. The storage architecture (one disposable SQLite index)

One artifact, per the two-tier model ([data-model.md](data-model.md)) and realizing the **"volatile vault
over a disposable index"** tenet ([vision-and-scope.md](vision-and-scope.md#design-philosophy)): a
**disposable** SQLite index holding every queryable concern transactionally. The whole index is
**rebuildable from Markdown** — drop `b2.sqlite`, re-scan the vault, get back an identical index (the
locked `full-reindex ≡ incremental-update` invariant). Markdown is the single source of truth (notes +
every committed edge); the index is a cache of it, with **no durable state outside your notes**. *(Through
2026-06-30 there was a second artifact — a durable `.b2/` event log holding the suggestion queue +
rejection memory; the LLM-relator cut removed the only thing it was for, so the index now stands alone;
[data-model.md](data-model.md) §4.)*

> The precise DDL, the relations between these tables, the read/write data flows, and the build order
> are specified in **[specs/index-engine-build.md](specs/index-engine-build.md)**. The sketch below is
> the orientation; that doc is the buildable contract.

```
b2.sqlite — DISPOSABLE CACHE  (= projection of Markdown; drop & rebuild any time)
├── MIRROR OF MARKDOWN (source of truth for *knowledge*; lets us diff vs. disk)
│   ├── notes(b2id PK, path, title, type, frontmatter_json, body_hash, mtime, …)  -- b2id ← frontmatter
│   └── note_bodies(note_b2id, content)          -- optional cache of file text
│
├── DERIVED FROM MARKDOWN: SEARCH
│   ├── chunks(id, note_b2id, seq, char_start, char_end, token_count, text)
│   ├── chunks_fts                                -- FTS5 over chunk text (BM25)
│   └── chunks_vec                                -- sqlite-vec vec0(embedding float[768])
│
├── DERIVED FROM MARKDOWN: TYPED GRAPH
│   └── edges(id, src_id, dst_id, type, origin,   -- every row ← Markdown (body links + FM relations:)
│             explanation, …)                     -- origin ∈ {inline, frontmatter}; every edge active
│
└── CACHES (disposable)
    └── llm_cache(key, value, created)            -- reserved for a future reranker (fast-follow, §5)
```

Every table is derived from the Markdown; there is no third home. `index = projection of (Markdown)`.
*(Since 2026-07-07 the projection is built in two separately-invokable passes —
model-free `project` (notes/chunks/FTS/edges) then `embed` (vectors), with `reindex` their
composition — so keyword search + graph are usable before embedding completes;
[specs/projection-embedding-split.md](specs/projection-embedding-split.md). The invariant is untouched:
a projected-but-unembedded index is a smaller projection, never a wrong one.)*

Why this shape fits B2 specifically:

- **Edges key on `b2id`, never path** — directly implements the link-identity decision
  ([user-stories.md](user-stories.md), [data-model.md](data-model.md)). `notes.b2id` is the durable
  frontmatter identity (B2's one always-allowed write); `src_id`/`dst_id` and `note_b2id` all hold
  `b2id` values. A move rewrites `notes.path` and inbound `[[path|title]]` text; every row in `edges`
  is untouched because it never referenced the path. "Rename keeps every backlink resolving" becomes a
  foreign-key truth, not a fix-up pass.
- **Every `edges` row derives from Markdown** — body links (`origin=inline`) ∪ frontmatter `relations:`
  (`origin=frontmatter`), deduped inline-wins. There is **no `status` column and no suggestion queue**:
  an edge exists iff it is authored in the Markdown. Committing with **`b2 link`** appends a typed-link
  string to the source note's frontmatter `relations:` (Markdown first; never the body —
  [data-model.md](data-model.md) §0), then re-projects that note — a projection of an authored line, not
  an in-place index write.
- **Hybrid retrieval and graph queries compose in one query** — e.g. "semantic-nearest chunks whose
  note is within 2 typed hops of note X" is a join across `chunks_vec`, `chunks`, and `edges`. This is
  the substrate **`b2 similar`** (connection-discovery candidate generation) runs on, and it's the thing a
  qmd-as-dependency design could never give us cleanly.
- **Deterministic seams for tests** — a fake embedder (deterministic vectors) writes to `chunks_vec`, so
  the whole pipeline is assertable with no live model (testability stack, points 4–5). The embedder is the
  one AI seam left.

### Why materialize the graph at all — vs. resolving links at runtime

A note's *outbound* links (and their type + explanation) are parseable from that one file on demand, so
it's fair to ask why the index carries an `edges` table at all rather than resolving links at read time.
The answer separates two things the question tends to bundle. **Edge metadata is *not* the reason:** a
typed line `- supersedes [[path|title]] — because X` yields its verb and explanation to a runtime parse
just as well, *for that note's outbound edges*. **Inversion and composition are the reason.** Materializing
edges is what turns the following from full-vault scans (or impossibilities) into indexed lookups:

- **Backlinks / inversion.** "Who points at X" cannot be read from X — only from every *other* note.
  The runtime answer is O(vault) per query; the table makes it one lookup. This is also what services
  *"rename keeps every backlink resolving"* ([user-stories.md](user-stories.md), Story 1): the edges name
  the exact N inbound files to rewrite on a move instead of scanning the vault to find them (§8).
- **Typed multi-hop traversal.** "notes within 2 hops of X via `supports`/`refutes`" is a scan *per hop*
  at runtime; over `edges` it is one SQL traversal.
- **The graph⨝vector join.** "semantic-nearest chunks whose note is within k typed hops of X" is a single
  join `chunks_vec ⨝ chunks ⨝ edges`, not expressible as a per-note parse. It is a **scoped-traversal**
  primitive (search *within* an already-related neighborhood). **`b2 similar`'s candidate generation is its
  *complement*, not this join:** notes semantically near an anchor but *not* within 1 hop — the links you
  *haven't* made (resolved 2026-07-01, see [tasks.md](tasks.md) ①) — where the materialized graph supplies
  the "∖ already-connected" exclusion. Both stand on the same reason the graph and search indexes must live
  in **one** store (§2): area-5 discovery is the substrate this enables.

The reframe that keeps this cheap: **runtime outbound-parsing is the correctness *definition*
(`index = projection of Markdown`); the `edges` table is its *cache*, kept so the inverse and compositional
queries are fast.** It is therefore not a third subsystem beside FTS5 and `sqlite-vec` — it is one more
**disposable** table in the same store, populated by the **same parse pass** that already walks each body
for chunking. Strip it and B2 is vector + keyword search over Markdown — i.e. qmd (§2); the typed,
traversable graph is the value-add, not the search. The standing cost of carrying it is the
`b2id`-under-`[[path]]` write-amplification budgeted in §8.

FTS5 is built into SQLite (BM25 ranking included). `sqlite-vec` is a small loadable C extension we
statically link. Both are battle-tested at personal-vault scale.

## 4. Semantic search & the engine-gated decision → verdict

The locked decision: *"if the index engine provides vector/semantic search, it's in v1."*

`sqlite-vec` provides it. Therefore **semantic search is in v1.**

Reality check on `sqlite-vec` so we don't over-promise:

- **Maturity:** pre-v1, "expect breaking changes." Dual MIT/Apache-2.0. ~8k stars, the de-facto
  successor to `sqlite-vss`, broad bindings (Python/Node/Go/Rust/WASM). Acceptable for a personal tool;
  we pin a version and own the upgrade.
- **Search method:** in practice today it is **brute-force KNN** (full linear scan) over `vec0` virtual
  tables, with metadata/partition-key filtering. ANN (IVF, DiskANN) files exist in the repo but are
  **not a stable, shipped path** — do **not** design v1 assuming ANN.
- **Does brute force scale to B2?** Yes, comfortably. A personal vault of, say, 10k notes → ~50–100k
  chunks. Brute-force cosine over ~100k × 768-dim float32 vectors is on the order of **single-digit to
  low-tens of milliseconds** — well within an interactive CLI budget. We're nowhere near the regime
  where ANN matters. If we ever are (multi-hundred-k chunks), options are int8/binary quantization in
  `sqlite-vec` (built in) or partitioning by collection — both before reaching for ANN.

So: **semantic search ships in v1**, brute-force KNN, 768-dim float vectors, with quantization in our
back pocket. This is the headline consequence of choosing SQLite.

## 5. The reranker as a fast follow

Slot it exactly where qmd puts it: **after RRF fusion, before final ranking**, behind a swappable seam.

- v1: retrieve (BM25 + vector) → **RRF fusion** → return top-N. This is already good; RRF alone is a
  strong hybrid baseline.
- Fast follow: insert a **cross-encoder rerank** over the top ~30 candidates → position-aware blend.
  The reranker is a pure function `(query, candidates) → scores`; the seam is the same one the
  testability stack wants for AI parts (replay recorded scores as fixtures; quality measured in a
  separate eval suite, never in CI).
- This is why the reranker is genuinely deferrable with no architectural debt: it changes *ordering*,
  not the store, the schema, or the candidate set. "Eventually add a reranker" is a one-stage insertion,
  not a redesign.

Query expansion (qmd's third model, the fine-tuned 1.7B) is **optional and lowest priority** — it's the
heaviest model for the smallest, most variable win. Treat it as a later, off-by-default flag.

## 6. The real hard part: embeddings in a single binary

This is the only place the architecture meets real friction, and it's worth being honest that **it is
independent of the SQLite decision** — any engine that does semantic search needs vectors from
somewhere.

qmd's answer is `node-llama-cpp` + auto-downloaded GGUF models + Node 22/Bun, needing ~300 MB–3 GB of
model files and a JS runtime. That directly tensions B2's single-binary, no-install-ritual goal
([vision-and-scope.md](vision-and-scope.md), principle #5). Options, roughly in order of single-binary
friendliness:

1. **Bundle a small embedding model + a `llama.cpp`/GGUF runtime, statically linked.** Self-contained,
   fully offline, but the binary carries a few-hundred-MB model (or downloads it on first run — a
   one-time ritual, not a per-use one). EmbeddingGemma-300M or Qwen3-Embedding-0.6B are the candidates.
2. **`fastembed` / ONNX Runtime** with a small embedding model — mature, embeddable, good language
   bindings (Rust/Go/Python); similar size tradeoff, arguably cleaner than carrying a full LLM runtime
   just to embed.
3. **Pluggable embedder behind a seam, default local + optional remote API.** B2 already wants the
   embedder swappable (deterministic fake for tests). Ship local-by-default; allow an API embedder for
   users who opt in. Keeps the binary tiny; preserves local-first as the default.

**Recommendation:** make the **embedder a seam** (we need it for tests regardless — and a swappable
model seam *is* the **"build for tomorrow's model"** tenet in practice,
[vision-and-scope.md](vision-and-scope.md#design-philosophy)), ship a **local model as the default**
(option 1 or 2), and decide model-download-on-first-run vs. bundled-in-binary as a packaging detail
later. Crucially, **none of this blocks the engine work**: build the SQLite store +
FTS5 + `sqlite-vec` + the typed graph now against the deterministic fake embedder; drop the real
embedder into the seam when the packaging path is chosen.

**Decided (2026-06-30).** Runtime = **`candle` + `hf-hub`** (pure-Rust inference compiled into the
binary — no external ONNX Runtime to ship; `hf-hub` is the download seam). Model =
**EmbeddingGemma-300M @ dim 768** (fallback to a known-good candle embedding model if it proves fiddly).
**Not bundled** — an explicit **`b2 init`** downloads + verifies the model into a shared **XDG cache**
(`~/.local/share/b2/models/`), never a surprise mid-command download; `reindex`/`search` fail fast with
"run `b2 init`" if it's absent. **The model source is configurable** (default = an HF repo id;
overridable to a mirror, another repo, or a local path for offline installs) via a global TOML at
`$XDG_CONFIG_HOME/b2/config.toml`. Build/execution plan in [tasks.md](tasks.md) "Next up".

**Built (2026-07-01).** Shipped in the **`b2-embed`** crate (`LocalEmbedder` behind the `b2-core`
`Embedder` seam; candle + `hf-hub`; CLS-pool + L2-normalize; asymmetric query prefix). **Model default
changed to `BAAI/bge-base-en-v1.5`** (BERT-family, **768-dim**, ungated): EmbeddingGemma-300M is *gated*
on Hugging Face (HTTP 401 without a token + license acceptance), which defeats a friction-free `b2 init`
— so B2 ships the pre-authorized bge fallback by default, validated in the spike (cat↔feline 0.83; NL
queries retrieve by meaning, not keyword). EmbeddingGemma remains selectable via config for anyone who
provides a token. The dim is read authoritatively from the model's own `config.json` (`hidden_size`), so
config can't lie about it. `open()` no longer shapes/drops the vector space (the mismatch fails fast on
`search`, re-embeds on `reindex`); the fake embedder stays the CI default so model quality never enters
the fast suite. Eval is a `cargo run -p b2-embed --example eval` pass (precision/MRR), out of CI.

## 7. Tech-stack implications (noted, not decided)

The stack is still open ([vision-and-scope.md](vision-and-scope.md)). The index-engine choice nudges it:

- **SQLite + FTS5 + `sqlite-vec` are language-agnostic.** Strong embedded bindings exist for **Rust**
  (`rusqlite`), **Go** (`mattn/go-sqlite3` / `modernc.org/sqlite`), Python, and Node/Bun. The engine
  does not pick the language for us.
- **The single-binary goal favours Rust or Go** (static link SQLite + `sqlite-vec` + an ONNX/GGUF
  embedder into one artifact). qmd's TypeScript/Node path is the *least* aligned with principle #5,
  which is another reason not to inherit qmd's runtime by depending on it.
- This is a **separate decision** ([tasks.md](tasks.md) backlog) — flagged here only because "rebuild on
  SQLite" quietly closes off "just use qmd's Node stack" and tilts toward a compiled language.

## 8. Risks, open questions & operational burden

### Engine risks & open questions

- **`sqlite-vec` is pre-v1.** Mitigate: pin a version, wrap vector ops behind our own small interface so
  a future swap (or ANN upgrade) is contained.
- **Embedding model size vs. single binary** (§6) — **resolved (2026-06-30):** not bundled; an explicit
  `b2 init` downloads a configurable model (candle + hf-hub) into a shared XDG cache. The binary stays small.
- **Embedding dimension & model lock-in.** Changing the embed model means re-embedding the whole vault
  (qmd's `embed -f`). Store the model id + dim in the DB; treat a model change as a full re-embed.
  **Locked:** the `vec0` column type is **dim 768** — the default **`BAAI/bge-base-en-v1.5`** and the
  config-selectable EmbeddingGemma-300M are both 768-dim (§6, built 2026-07-01); a model/dim change is a
  full re-embed, detected via `meta` — fail fast on read, re-embed on `reindex`.
- **Loadable-extension friction.** `sqlite-vec` must be loaded at runtime (qmd notes macOS needs a
  SQLite that allows extensions). Static-linking it into our binary removes this for end users.
- **Chunk vs. note granularity for the graph.** Search is chunk-level; the typed graph is note-level.
  Keep `chunks.note_b2id` as the join and resolve search hits up to notes for graph operations — already
  reflected in §3.

### Operational burden — the bill for a `b2id`-keyed graph under `[[path|title]]` links

The graph buys B2 its reason to exist (typed, `b2id`-stable edges — §2), but the decision to
keep links written as human-clickable `[[path|title]]` while the graph keys on `b2id`
([user-stories.md](user-stories.md), "Link format & identity") has standing operational costs. These are
*the trade working as designed*, not defects — but they must be budgeted, tested, and watched.

- **Write amplification on move.** The inline `path` is a repairable convenience copy, so moving one note
  rewrites the inbound link text in **every** file that points at it — an N-file write, not a one-file
  write. It's bounded and mechanical (the `b2id`-keyed edges name exactly which files/links to touch,
  Markdown-first then index), but moving a heavily-linked note is proportional to its backlink count, not
  O(1). Watch the cost on hub notes; keep the rewrite transactional so a partial move never half-updates
  the vault.
- **Out-of-band moves degrade gracefully, not perfectly.** A `git mv`/Finder move + reindex re-reads the
  frontmatter `b2id` and re-establishes `b2id → newpath`, repairing dangling inbound links — **if** there is
  prior index continuity. A **cold reindex with no prior state** can only repair a dangling `[[oldpath]]`
  heuristically (e.g. via the alias); those links are **flagged for repair, not silently dropped**. This
  is the same failure surface as moving files with Obsidian closed — acceptable, but it means the index is
  load-bearing for full repair fidelity.
- **Derived-index consistency is a permanent invariant, not a one-time build.** The index is a derived
  projection of `Markdown` and must never drift from it. Three locked invariants are the tripwires
  ([vision-and-scope.md](vision-and-scope.md)): round-trip losslessness (`parse → serialize → parse`),
  `full-reindex ≡ incremental-update`, and `rename keeps every backlink resolving`. Every edit path
  (kernel `b2 mv`, link delete, out-of-band reindex) has to preserve all three or the graph silently
  diverges from the source of truth.
- **Committed edges are only ever authored, never inferred.** B2 writes an edge only on your command
  (`b2 link`, or a body link you write) — there is no agent proposing edges and no review queue to keep
  consistent. Editing the vault can strand a connection — e.g. deleting an authored `A→B` link
  ([user-stories.md](user-stories.md), Story 2) — but B2 only ever *surfaces* the consequence (an orphan
  flag in `b2 explain`), never silently rewrites an inbound file or an edge. Files are touched only when asked.

## 9. Recommendation & next steps

1. **Adopt SQLite as the B2 index engine** (FTS5 + `sqlite-vec`) per the §3 schema — one disposable
   index, `index = projection of (Markdown)`. Use qmd as a design reference under its MIT license; do
   **not** depend on it.
2. **Record the engine-gated outcome:** semantic search is **in v1** (brute-force KNN; quantization
   reserved for scale). Update [vision-and-scope.md](vision-and-scope.md) / [tasks.md](tasks.md) to flip
   semantic from "engine-gated" to "in v1".
3. **Reranker = explicit fast-follow** behind a post-fusion seam; query expansion = later/optional.
4. **Make the embedder a seam now**; build the store + indexes + typed graph against the **deterministic
   fake embedder**, so engine work proceeds before the single-binary embedding decision is made.
5. **Sequence:** the data model is **locked** ([data-model.md](data-model.md)) and this doc is now
   reconciled with its two-tier model, so the engine is unblocked — build the store + indexes + typed
   graph against the golden-vault fixtures ([data-model.md](data-model.md) §8).

> Net: qmd answers "can a great hybrid search engine run locally on Markdown?" — yes, and here's how.
> B2's question is one layer up: "can that retrieval live inside a typed, `b2id`-stable, agent-operated
> graph I fully own, in a single binary?" SQLite is the substrate that makes every queryable concern one
> disposable store, a pure projection of your Markdown. We take qmd's pipeline and build the graph it was
> never trying to be.
