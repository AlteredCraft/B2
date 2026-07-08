---
title: "B2 — Research: ChromaDB / LanceDB as an index-store replacement"
type: note
tags: [b2, research, index-engine, sqlite, sqlite-vec, chromadb, lancedb, vector-store, reranker]
created: 2026-07-08
status: draft
---

# B2 — Research: ChromaDB / LanceDB as an index-store replacement

> Evaluates whether **ChromaDB** or **LanceDB** should replace B2's current index store
> (**SQLite: FTS5 + `sqlite-vec` + the typed `edges` graph**), given the known limitations of the
> SQLite path, the planned **reranker** ([index-engine.md](../index-engine.md) §5), and the hard
> constraints of **free-and-commercial-OK open source** that **runs fully locally**. Companion to
> [index-engine.md](../index-engine.md) (the SQLite-vs-qmd decision this revisits) and
> [vision-and-scope.md](../vision-and-scope.md) (principle #5: single binary, local-first).

## TL;DR / recommendation

**Stay on SQLite (FTS5 + `sqlite-vec`). Neither ChromaDB nor LanceDB justifies a migration.**

- **ChromaDB — eliminate.** Apache-2.0 and runs locally, *but its embedded/in-process mode is
  Python/TypeScript only*. Every Rust path (`chromadb`, `chromadb-rs`) is an **HTTP client to a
  separately-running Chroma server**. Chroma's 2025 Rust-core rewrite is *internal to Chroma*, not an
  embeddable Rust library. Bundling/managing a server process directly violates B2's single-binary,
  no-install-ritual goal ([vision-and-scope.md](../vision-and-scope.md) principle #5). It's out on
  architecture before feature comparison even matters.
- **LanceDB — the only serious contender, but still no.** Apache-2.0, genuinely **Rust-native and
  embedded/serverless** (reads/writes a local directory, no server), with BM25 full-text, hybrid + RRF,
  ANN indexes, and built-in rerankers in the OSS library. It aligns with B2's stack far better than
  Chroma. **The blocker is architectural, not feature-level:** LanceDB is a *vector/multimodal store,
  not a graph store*. It cannot hold B2's typed `edges` graph **next to** the vectors in one
  transactional store — and that single-store property is the entire reason B2 chose SQLite over qmd
  ([index-engine.md](../index-engine.md) §2). Adopting Lance means running **two stores** (Lance for
  vectors, SQLite for the graph) and turning the `chunks_vec ⨝ chunks ⨝ edges` join — the substrate
  for `b2 similar` and `graph_filtered_search` — into a cross-store join in application code.
- **The reranker does not favor either platform.** A reranker is a post-fusion, model-side pure
  function `(query, candidates) → scores` ([index-engine.md](../index-engine.md) §5). It sits *above*
  the store. LanceDB ships reranker plumbing, but B2 wants a **local candle cross-encoder** (same seam
  pattern as the `Embedder`), which Lance's Rust SDK would make you implement yourself anyway — and
  Chroma's rerankers aren't reachable from an embedded Rust context at all. **Don't switch stores for
  the reranker; it's a one-stage insertion regardless of store.**
- **The limitation Lance would fix isn't a limitation B2 has.** `sqlite-vec` is brute-force KNN, and
  a personal vault (≈10k notes → 50–100k chunks) is nowhere near where ANN matters — KNN there is
  single-digit-to-low-tens-of-ms ([index-engine.md](../index-engine.md) §4). The recent
  `4096-k KNN cap` fix (`da6787e`) was a **query-shaping bug, not a scaling wall**. Quantization and
  partitioning in `sqlite-vec` are the in-place levers before ANN is ever needed.

---

## 1. The decision frame: B2 is a graph, not a search engine

This is the same lens [index-engine.md](../index-engine.md) §2 applied to qmd, and it decides this
question too. B2's index deliberately holds **every queryable concern in one transactional SQLite
file**: full-text (FTS5/BM25), vectors (`sqlite-vec`), **and the typed `edges` graph** — so retrieval
and connection-discovery compose in a single query:

- `graph_filtered_search` — "semantic-nearest chunks whose note is within *k* typed hops of anchor X"
  = `chunks_vec ⨝ chunks ⨝ edges`.
- `b2 similar` — its complement, "near ∖ already-connected" — KNN over stored vectors **minus** the
  anchor's 1-hop graph neighbors (tasks.md ①).

Both are joins *across the vector index and the graph in the same store*. **Neither ChromaDB nor
LanceDB is a graph database.** Move the vectors into either one and the graph must stay in SQLite (or
move to a third store), so:

1. You run **two stores** and lose the transactional single-store consistency
   ([index-engine.md](../index-engine.md) §2–3).
2. The vector⨝graph join stops being one SQL statement and becomes a **cross-store join in Rust**
   (KNN in Lance → collect note ids → filter against the SQLite graph → re-join). More code, more
   surface for the `index = projection of (Markdown)` invariant to drift.
3. You gain nothing the graph needs — these engines optimize the *vector* half B2 already has covered.

That is the crux. The rest is detail.

## 2. Constraint gate (all three pass the license/local test)

| Requirement | SQLite (FTS5 + `sqlite-vec`) | ChromaDB | LanceDB |
|---|---|---|---|
| **License — free + commercial OK** | ✅ SQLite public-domain; `sqlite-vec` dual MIT/Apache-2.0 | ✅ Apache-2.0 | ✅ Apache-2.0 |
| **Runs fully locally / offline** | ✅ embedded, statically linked | ✅ (server, or embedded in Py/TS) | ✅ embedded, serverless |

License and "runs locally" are **not** the differentiators — all three clear them. The differentiators
are **embeddability in a Rust single binary** and **fit with the typed-graph architecture**.

## 3. ChromaDB — verdict: poor fit, eliminate

- **Architecture / Rust embeddability — the blocker.** Chroma's embedded, in-process mode
  (`PersistentClient`, "ship Chroma bundled with your product") is **Python/TypeScript only**. The Rust
  crates (`chromadb`, `chromadb-rs`) are **HTTP clients that default to `http://localhost:8000` and
  require a running Chroma server** (typically `docker run -p 8000:8000 chromadb/chroma`). Chroma's
  2025 Rust-core rewrite (≈4× faster writes/queries, morsel-driven engine) is *inside* Chroma's own
  server — it is **not** exposed as an embeddable Rust library. So for B2's Rust CLI, ChromaDB means
  bundling and lifecycle-managing a server process: a direct hit to principle #5 (single binary, no
  install ritual, [vision-and-scope.md](../vision-and-scope.md)).
- **Search features (moot given the above).** Modern Chroma is actually strong here: first-class
  **BM25** and **SPLADE** sparse vectors, dense+sparse **hybrid search fused with RRF** out of the box,
  full-text & regex, and reranking. Feature-competitive with B2's pipeline — but unreachable from an
  embedded Rust context.
- **No typed graph** (§1).
- **Net:** architecturally incompatible with B2's Rust + single-binary + local-first constraints.
  Would only reconsider if B2 abandoned the single-binary goal or moved to a Python/TS runtime.

## 4. LanceDB — verdict: the only real contender, but not worth migrating

**Where it genuinely fits B2 (better than Chroma):**

- **Rust-native & embedded.** Lance's core is Rust; `lancedb` is a first-class **embedded, serverless**
  Rust crate that reads/writes a **local directory** with no server. This *does* fit B2's stack.
- **Apache-2.0**, 100% OSS, runs locally — clears the constraint gate.
- **BM25 full-text in the OSS embedded lib** (Lance-native FTS), plus **vector search, SQL filtering,
  hybrid search, RRF, and built-in rerankers** (linear combination, RRF, Cohere, ColBERT,
  cross-encoder — reranking currently scoped to hybrid search).
- **Real ANN indexes** (IVF-PQ / IVF-HNSW families) *plus* brute-force — so it scales past
  `sqlite-vec`'s brute-force-only ceiling.

**Why it still loses for B2:**

- **No typed graph — the deal-breaker (§1).** You'd keep SQLite for `edges` and run two stores; the
  vector⨝graph join that powers `b2 similar` / `graph_filtered_search` becomes cross-store glue.
- **Solves a problem B2 doesn't have.** Lance's headline advantage is ANN at scale. At personal-vault
  scale, `sqlite-vec` brute-force KNN is already interactive-fast
  ([index-engine.md](../index-engine.md) §4); the `4096-k` cap (`da6787e`) was a query-shaping bug,
  now fixed — not evidence of a wall. Int8/binary quantization and partitioning in `sqlite-vec` are
  the in-place levers if scale ever bites, all before reaching for ANN.
- **Determinism tax.** B2's core is **model-free and deterministic** — fixed ids, a fake embedder,
  golden-vault fixtures ([CLAUDE.md](../../CLAUDE.md), "Determinism is a hard requirement"). Lance's
  ANN indexing and its own ranking add a heavier, less-transparent moving part; to preserve
  reproducible tests you'd pin brute-force/flat search — forfeiting the very ANN edge that was the
  reason to adopt it.
- **Dependency weight vs. single binary.** `rusqlite` statically links one bundled C library. Lance
  drags in the **Arrow + Lance columnar format + object-store** ecosystem — a much larger dependency
  tree and binary, tensioning principle #5.
- **Migration churn for a working system.** Rewrites `search.rs` (RRF fusion), `discover.rs`
  (near ∖ connected), and `db.rs`; and the deterministic test suite + golden-vault fixtures would need
  reworking. Large, risky change to a personal tool that already ships and is dogfooded.

## 5. The reranker question, specifically

The prompt asks whether these platforms **simplify the planned reranker**. Answer: **no — it's
orthogonal to the store.**

- Per [index-engine.md](../index-engine.md) §5, the reranker is a **post-fusion** insertion: retrieve
  (BM25 + vector) → RRF fuse → **cross-encoder rerank of top ~30** → position-aware blend. It is a
  pure function `(query, candidates) → scores` behind a swappable seam — the *same* seam discipline as
  the `Embedder`. It "changes *ordering*, not the store, the schema, or the candidate set."
- **LanceDB** ships reranker plumbing, which is real convenience *in Python*. But B2 wants a **local
  candle-backed cross-encoder** (offline, single-binary, testable via recorded-score fixtures) — not
  Lance's cloud rerankers (e.g. Cohere). In Lance's **Rust** SDK you'd wire a local cross-encoder
  yourself regardless, so Lance saves B2 almost nothing on the reranker it would actually ship.
- **ChromaDB's** reranking isn't reachable from an embedded Rust context at all (§3).
- **Conclusion:** the reranker is a one-stage seam B2 already has the pattern for. It is **not** a
  reason to change stores. Build it as a candle cross-encoder behind the post-fusion seam, store-agnostic.

## 6. What would change this call

- **B2 drops the single-binary / Rust constraint** (e.g. a Python/TS runtime becomes acceptable) →
  ChromaDB's embedded mode becomes viable and its hybrid+rerank story is attractive.
- **Vault scale explodes** into multi-hundred-k+ chunks where brute-force KNN stops being interactive,
  *and* `sqlite-vec` quantization/partitioning prove insufficient → revisit **LanceDB** for the vector
  tier specifically — and even then keep SQLite for the typed graph (§1).
- **The typed graph is cut** and B2 becomes pure hybrid search over Markdown (i.e. qmd) → LanceDB
  becomes a clean, Rust-native substrate. But that would be abandoning B2's reason to exist
  ([index-engine.md](../index-engine.md) §2).

## Sources

- ChromaDB — [Introduction](https://docs.trychroma.com/docs/overview/introduction.md),
  [Client-Server mode](https://docs.trychroma.com/docs/run-chroma/client-server),
  [Chroma BM25](https://docs.trychroma.com/integrations/embedding-models/chroma-bm25),
  [`chromadb-rs` (HTTP client)](https://github.com/Anush008/chromadb-rs),
  [product page](https://www.trychroma.com/products/chromadb).
- LanceDB — [Quickstart](https://docs.lancedb.com/quickstart),
  [GitHub (Apache-2.0, OSS embedded)](https://github.com/lancedb/lancedb),
  [Full-Text Search](https://docs.lancedb.com/search/full-text-search),
  [Rust crate](https://docs.rs/lancedb/latest/lancedb/),
  [reranker comparison example](https://github.com/CortexReach/memory-lancedb-pro).
- B2 — [index-engine.md](../index-engine.md) (§2 single-store rationale, §4 `sqlite-vec` scaling, §5
  reranker), [vision-and-scope.md](../vision-and-scope.md) (principle #5), [tasks.md](../tasks.md) (① discovery).
