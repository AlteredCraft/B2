---
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
  the whole pipeline runs locally. But it is a **search engine**, and B2 is not — B2 is a **typed,
  provenanced graph with hybrid retrieval over it**. qmd has no notion of typed edges, suggestion
  lifecycle, or provenance, which are the reasons B2 exists ([vision-and-scope.md](vision-and-scope.md),
  capability areas 3, 5, 6).
- SQLite gives us **one embedded store for all four concerns at once** — full-text (FTS5), vectors
  (`sqlite-vec`), the typed graph (plain relational tables), and provenance/review state (more tables) —
  with transactional consistency across them. That single-store property is worth more to B2 than
  anything we'd inherit by depending on qmd.
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
| **Typed graph** (`id→id` edges with a relation type) | ❌ none | ⭐ core (areas 3, 5) |
| **Provenance** (`by` human/agent, `source`, `confidence`) | ❌ none | ⭐ core (area 6) |
| **Suggestion lifecycle** (suggested → accepted/rejected, *inert until accepted*) | ❌ none | ⭐ core (area 6) |
| **`id`-keyed identity** surviving move/rename | ❌ path-keyed, cache is disposable | ⭐ core (user-stories 1–2) |
| **Markdown as source of truth** (index is rebuildable/derived) | ~ index *is* the artifact | ⭐ non-negotiable (principle #1) |
| Distribution | npm package, Node runtime | ⭐ single binary (principle #5) |

The decisive point: B2's index is a **derived projection of the Markdown vault**, and it must hold the
**typed graph + provenance + review state** *next to* the search indexes so retrieval and connection
discovery share one transactional store. qmd models none of the graph/provenance layer and treats its
DB as a throwaway cache. Wrapping qmd would mean maintaining a second store for everything that makes
B2 *B2*, and reconciling two sources of truth. Rebuilding the ~300 lines of retrieval glue that we
actually want is cheaper than that integration tax — and qmd's MIT license + public design make the
rebuild low-risk.

**What we borrow wholesale:** the chunking heuristic, the RRF formula and k, the position-aware blend,
the EmbeddingGemma prompt format (`task: ... | query:` / `title: ... | text:`), the JSON/`--explain`
agent-output discipline, and the MCP surface idea. **What we discard:** the npm/Node packaging and the
"DB is the product" framing.

## 3. The SQLite architecture for B2

One database file, four concerns, transactionally consistent. Everything below `derived/` is
**rebuildable from the Markdown** — drop the file, re-scan the vault, get back an identical index
(the locked `full-reindex ≡ incremental-update` invariant).

```
b2.sqlite
├── SOURCE-OF-TRUTH MIRROR (cheap to rebuild, lets us diff vs. disk)
│   ├── notes(id PK, path, title, type, frontmatter_json, body_hash, mtime, …)
│   └── note_bodies(note_id, content)            -- optional cache of file text
│
├── DERIVED: SEARCH
│   ├── chunks(id, note_id, seq, char_start, char_end, token_count, text)
│   ├── chunks_fts                                -- FTS5 over chunk text (BM25)
│   └── chunks_vec                                -- sqlite-vec vec0(embedding float[768])
│
├── DERIVED: TYPED GRAPH
│   └── edges(id, src_id, dst_id, type, origin,   -- origin: inline|frontmatter|suggested
│             status, explanation, …)             -- status: active|suggested|rejected
│
├── PROVENANCE / REVIEW
│   └── edge_provenance(edge_id, by, source, confidence, created, decided)
│
└── CACHES (disposable)
    └── llm_cache(key, value, created)            -- query expansion, rerank scores
```

Why this shape fits B2 specifically:

- **Edges key on `id`, never path** — directly implements the link-identity decision
  ([user-stories.md](user-stories.md)). A move rewrites `notes.path` and inbound `[[path|title]]` text;
  every row in `edges` is untouched because it never referenced the path. "Rename keeps every backlink
  resolving" becomes a foreign-key truth, not a fix-up pass.
- **Suggestions are just `edges` rows with `status='suggested'`** — *inert until accepted* is a `WHERE
  status='active'` clause. Accepting a suggestion is one `UPDATE` (plus writing the Markdown). The
  review layer is data, not a side-system.
- **Hybrid retrieval and graph queries compose in one query** — e.g. "semantic-nearest chunks whose
  note is within 2 typed hops of note X" is a join across `chunks_vec`, `chunks`, and `edges`. This is
  the substrate **connection discovery** (candidate generation) runs on, and it's the thing a
  qmd-as-dependency design could never give us cleanly.
- **Provenance travels with the edge** — every machine-derived or agent-suggested connection carries
  `by=agent:<model>`, `source`, `confidence`; human edits carry `by=human`. Area 6 falls out of the
  schema.
- **Deterministic seams for tests** — a fake embedder (deterministic vectors) writes to `chunks_vec`;
  a scripted relator writes `status='suggested'` rows. The whole pipeline is assertable with no live
  model (testability stack, points 4–5).

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

**Recommendation:** make the **embedder a seam** (we need it for tests regardless), ship a **local
model as the default** (option 1 or 2), and decide model-download-on-first-run vs. bundled-in-binary as
a packaging detail later. Crucially, **none of this blocks the engine work**: build the SQLite store +
FTS5 + `sqlite-vec` + the typed graph now against the deterministic fake embedder; drop the real
embedder into the seam when the packaging path is chosen.

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

## 8. Risks & open questions

- **`sqlite-vec` is pre-v1.** Mitigate: pin a version, wrap vector ops behind our own small interface so
  a future swap (or ANN upgrade) is contained.
- **Embedding model size vs. single binary** (§6) — the genuine open question; decide bundle vs.
  first-run download during packaging.
- **Embedding dimension & model lock-in.** Changing the embed model means re-embedding the whole vault
  (qmd's `embed -f`). Store the model id + dim in the DB; treat a model change as a full re-embed.
  Pick the default dim (e.g. 768) deliberately since it sets the `vec0` column type.
- **Loadable-extension friction.** `sqlite-vec` must be loaded at runtime (qmd notes macOS needs a
  SQLite that allows extensions). Static-linking it into our binary removes this for end users.
- **Chunk vs. note granularity for the graph.** Search is chunk-level; the typed graph is note-level.
  Keep `chunks.note_id` as the join and resolve search hits up to notes for graph operations — already
  reflected in §3.

## 9. Recommendation & next steps

1. **Adopt SQLite as the B2 index engine** (FTS5 + `sqlite-vec`), single file, schema per §3. Use qmd as
   a design reference under its MIT license; do **not** depend on it.
2. **Record the engine-gated outcome:** semantic search is **in v1** (brute-force KNN; quantization
   reserved for scale). Update [vision-and-scope.md](vision-and-scope.md) / [tasks.md](tasks.md) to flip
   semantic from "engine-gated" to "in v1".
3. **Reranker = explicit fast-follow** behind a post-fusion seam; query expansion = later/optional.
4. **Make the embedder a seam now**; build the store + indexes + typed graph against the **deterministic
   fake embedder**, so engine work proceeds before the single-binary embedding decision is made.
5. **Sequence:** this evaluation is gated on the data model ([tasks.md](tasks.md)). Finish
   `data-model.md` (frontmatter schema, typed-relation encoding, edge model) — the §3 schema above is
   the concrete target it should satisfy — then build the engine against the golden-vault fixtures.

> Net: qmd answers "can a great hybrid search engine run locally on Markdown?" — yes, and here's how.
> B2's question is one layer up: "can that retrieval live inside a typed, provenanced, agent-operated
> graph I fully own, in a single binary?" SQLite is the substrate that makes *that* one store. We take
> qmd's pipeline and build the graph it was never trying to be.
