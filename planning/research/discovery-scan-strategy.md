---
title: "B2 — Research: making discovery O(fast) — the #38 scan-strategy decision"
type: note
tags: [b2, research, index-engine, sqlite-vec, discovery, performance, quantization, ann]
created: 2026-07-12
status: draft
---

# B2 — Research: making discovery O(fast) — the #38 scan-strategy decision

> Deep analysis for [#38](https://github.com/AlteredCraft/B2/issues/38): note-open discovery
> (`b2 similar`, the desktop **Similar & Connections** pane) is still ~4.4 s (release) on the
> primary vault after the [#37](https://github.com/AlteredCraft/B2/issues/37) fix. Goal: relation
> resolution for a given document feels **instant**, and stays instant at **10× the current
> document population** (~1k docs → ~10k docs). Constraints: local-first on a laptop, no
> app-managed result cache (OS/library-internal caches are fine), background one-time heavy
> lifts acceptable, maintained derived structures acceptable if low-effort. Full refactor was
> on the table; the conclusion is it isn't needed.

## TL;DR / recommendation

**Own the flat scan: store vectors in a plain SQLite table, score them in-process, and drop
`vec0` from the read path entirely. Then make discovery two-stage (note-centroid prefilter →
exact chunk rescore) so the heavy pass is O(notes), not O(chunks).** No new store, no ANN
library, no managed cache.

1. **Now (fixes #38 and #36 structurally):** replace `chunks_vec` (a `vec0` virtual table) with
   a plain `embeddings(chunk_id INTEGER PRIMARY KEY, vector BLOB)` table; scan it with one
   sequential statement; fix the arithmetic shape (reused scratch buffer, unrolled accumulators);
   add `PRAGMA mmap_size` + a bigger `cache_size` in `db::open`. Expected: **~4.4 s → ~0.2–0.3 s
   warm** at today's scale, bit-identical ranking, one SQL log line per open instead of ~38.6K,
   and the `sqlite-vec` dependency can be removed outright.
2. **For 10× (small; can ship with 1):** a per-note **centroid** column maintained by the same
   embed pass — discovery scans ~N_notes centroids (10k × 768 ≈ 30 MB at 10×, ~5 ms), shortlists
   the top ~200 notes, and runs today's exact max-sim only over the shortlist's chunks
   (~10–20 ms). Scaling becomes essentially flat in vault size.
3. **Multiplier already on the roadmap:** the qmd chunker upgrade
   ([#19](https://github.com/AlteredCraft/B2/issues/19)) shrinks ~38 paragraph-chunks/doc to a
   handful of ~900-token chunks — a ~5–40× reduction in vector count that makes every option
   cheaper (and max-sim less noisy). Do it for quality; bank the perf.
4. **Held in reserve, in order:** int8/binary **quantization** (shrinks the constant, needs the
   eval), **ANN** (usearch/LanceDB — wrong scale, non-deterministic, standing maintenance;
   both planning docs already ranked it last), **precomputed similar-notes table** (a managed
   cache with an O(N²)-shaped invalidation problem — the explicit last resort).

## 1. Problem decomposition — which "relations" are slow

"Resolve the relations of a given document" is two different queries:

- **Authored relations** (backlinks + outbound, `Vault::neighbors`/`explain`) — indexed lookups
  on the materialized `edges` table. Already O(link-degree), microseconds. **Not the problem.**
- **Latent relations** (`b2 similar` / `discover::candidates` — "semantically near ∖ already
  connected") — an **exact brute-force max-sim scan of every stored chunk vector on every
  note-open**. This is the whole problem, and (post-#37) it is the only remaining O(vault)
  read in the open-a-note path. `hybrid_search` and `graph_filtered_search` pay the same
  per-row tax on their `vec0` scans, so whatever fixes discovery fixes them for free.

## 2. What we've learned so far (the #35→#36→#37 trail)

The issue history localizes the cost precisely; this is worth restating because it *rules out*
most exotic fixes:

- **#37:** the A× whole-space rescan (one KNN scan per anchor chunk) and the per-hit N+1 were
  the first-order bug — fixing the *access pattern* took 51 s → 4.4 s with zero algorithm change.
- **#36 + #38:** what remains is dominated by **how the bytes are read, not how many FLOPs are
  done**. Every row read from `chunks_vec` makes `sqlite-vec` probe its `chunks_vec_rowids`
  shadow table (~38.6K internal single-row statements per open — the O(vault) log lines), and
  the 132 MB DB is walked through SQLite's default ~2 MB page cache with no mmap (the ~3.2 s
  that stays *system* time even warm).
- **Arithmetic shape, measured** (microbench at N=38,610 × 768-dim, A=12, this container's CPU,
  `rustc -O`): the current shape — per-row `unpack_f32` `Vec` allocation + iterator-`sum()`
  `l2_sq` (float non-associativity blocks autovectorization) — costs **~530 ms**; a reused
  scratch buffer + 8-accumulator unrolled loop costs **~75 ms** (10×: ~5.2 s vs ~0.74 s). The
  index-engine.md §4 prediction ("tens of ms") was about the FLOPs and remains right; the
  implementation shape multiplies it ~7×.
- **Structural fact:** `sqlite-vec`'s shipped search is brute force — no ANN path — and
  discovery already scores in-process. So on the scan path `vec0` provides **storage only**,
  at the price of a per-row vtab round-trip. B2 uses none of what it would charge for.
- **Structural fact #2:** the shipped chunker is the *minimal paragraph splitter* (`chunk.rs`),
  not the planned ~900-token qmd heuristic (#19). ~1k docs → 38.6k chunks (~38/doc) means the
  vector population is inflated ~5–40× over the design target. #38's cost is partly a chunking
  artifact.

Consequence: **before changing the algorithm, the read path and arithmetic shape are worth
~10–20×.** That reframes the option space — heuristics are for the 10× headroom, not for today's
4.4 s.

## 3. Budget at target scale

| | today (~1k docs) | 10× (~10k docs, current chunker) | 10× + #19 chunker |
|---|---|---|---|
| chunks | ~38.6k | ~386k | ~30–80k |
| f32 vectors on disk | ~118 MB | ~1.2 GB | ~90–240 MB |
| flat scan, fixed shape (measured/extrapolated) | ~75 ms + read | ~740 ms + read | ~60–160 ms + read |
| two-stage (centroid → rescore ~200 notes) | ~10 ms | **~15–25 ms** | ~10 ms |

Brute force with a sane memory layout is comfortably "instant" today and *borderline* at 10×
under the current chunker; the centroid stage (or #19 alone) restores a wide margin. This
matches industry practice: FAISS's own guidance is flat/exact search until high-hundreds-of-
thousands of vectors, and ANN only when exactness or memory must be traded for scale. A
personal vault never leaves the flat regime — *if* the flat scan isn't paying a per-row
virtual-table tax.

## 4. Options considered

### A. Own the flat scan (plain table + in-process top-k + pragmas) — **do now**

Store embeddings in an ordinary table (`embeddings(chunk_id INTEGER PRIMARY KEY, vector BLOB)`,
same packed-LE-f32 blob), keyed/lifecycled exactly as `chunks_vec` is today (created at embed
time at the model's dim; dropped on model swap; `meta` discipline unchanged). All four vector
consumers — `for_each_stored_vector`, `vector_search`, `vector_search_all`, and #36's
missing-vector anti-join — become one sequential B-tree scan (plus, for search, an in-process
top-k heap). Add `PRAGMA mmap_size` (≥ DB size, e.g. 1–2 GB — it's a cap, not an allocation)
and a bigger `page_size`/`cache_size` so the scan streams from the OS page cache — the one
cache we're happy to use because nobody manages it. Fix `l2_sq`/`unpack_f32` shape as measured.

- **Pros:** simplest possible change; results bit-identical (same vectors, same metric, same
  order); kills the O(vault) log lines and #36's probe storm *structurally*; deletes a pre-v1
  dependency (`sqlite-vec`) instead of adding one; keeps the single-store `chunks ⨝ edges ⨝
  embeddings` join property that decided the qmd and Chroma/Lance evaluations; fully
  deterministic; no index to maintain beyond what embed already maintains.
- **Cons:** we own ~50 lines of scan/top-k code and the (already-shared) blob format; still
  O(chunks) per open — at 10× under the current chunker that's ~0.8–1 s warm, i.e. A alone
  doesn't hit the 10× "instant" bar; forfeits a hypothetical future sqlite-vec ANN mode (which
  the planning docs already said not to design for).

### B. Two-stage discovery: note-centroid prefilter → exact chunk rescore — **the 10× lever**

At embed time, also store one **summary vector per note** (mean of its chunk vectors,
L2-renormalized; summation in `seq` order so it's deterministic). Discovery becomes: (1) scan
~N_notes centroids against the anchor's centroid, excluding the anchor + 1-hop neighbors;
(2) take a generous shortlist (~10–20× the display limit, e.g. 200–300 notes); (3) run today's
exact max-sim over only the shortlist's chunks; surface evidence chunks as now. This is IVF's
coarse-quantizer idea using B2's *natural* partition (the note) instead of learned clusters —
no training, no ANN library, no recall cliff.

- **Pros:** the heavy pass drops from O(chunks) to O(notes) — ~30 MB / ~5 ms at 10×, flat
  effectively forever (100k notes ≈ 300 MB ≈ still sub-100 ms); maintenance is one UPDATE per
  note inside the existing embed pass (the same lifecycle as the vectors themselves — a derived
  projection in the disposable index, **not** an app-managed result cache: nothing to
  invalidate that embed doesn't already touch); deterministic; the note-level vector is also a
  natural substrate for #20 (distance weighting) later.
- **Cons:** no longer exhaustively exact — a note whose centroid is far away but which contains
  one very close paragraph can miss the shortlist. Mitigations: discovery is *by design*
  permissive/recall-oriented with a human precision gate, the shortlist is cheap to widen, and
  the retrieval eval (eval-strategy.md) can pin shortlist size against exact-scan ground truth.
  Slightly more code than A (two stages instead of one).

### C. Quantization (int8 / binary + f32 rescore) — **reserve**

`index-engine.md` §4 names it the first lever before ANN. Once we own the scan (A), an extra
int8 (4×) or binary (32×, Hamming/popcount, rescore top-k in f32) column is easy to add and is
standard practice for bge-family embeddings.

- **Pros:** shrinks bytes-read and FLOPs by 4–32× (1.2 GB → 300 MB / 37 MB at 10×); composes
  with A and B; well-trodden (FAISS SQ8, binary-quantized bge retains ~95 %+ recall with
  rescoring).
- **Cons:** measurable quality risk → gated on the eval harness; more code than B for a smaller
  win than B at this scale; reduces the constant, not the O(chunks) growth. Only worth it if,
  after A+B(+#19), cold-start I/O or memory footprint still bites.

### D. Real ANN (HNSW via usearch/hnsw_rs, IVF, or LanceDB vector tier) — **rejected**

- **Pros:** sub-linear queries; the industry default at 10M+ vectors.
- **Cons:** solves a scale B2 doesn't reach at 10× (§3); HNSW graphs are build-order-dependent
  (breaks the determinism requirement); a second index artifact to build/persist/heal alongside
  the disposable-projection invariant; dependency weight vs. the single binary; both
  `index-engine.md` §4 and `research/vector-store-alternatives.md` already ranked ANN last and
  re-affirmed staying on SQLite. Nothing in #38's evidence (read-path overhead, not FLOPs)
  argues for it.

### E. Materialized similar-notes table (precompute discovery per note) — **last resort, as specified**

Background-compute top-k candidates per note at embed time; note-open reads a row.

- **Pros:** O(1) note-open, trivially instant.
- **Cons:** this is exactly the app-managed cache the requirements deprioritize: one note's new
  embedding can perturb *every* other note's candidate list, so correctness needs either O(N²)
  recompute sweeps or push-based incremental maintenance — a standing effort/complexity bill.
  B delivers ~tens-of-ms opens without any of that; E only becomes interesting if the target
  were single-digit ms on a vastly larger corpus.

## 5. Sequencing & migration

1. **A** — new `embeddings` table + in-process top-k + pragmas + arithmetic shape; delete
   `vec0`/`sqlite-vec`. Bump `schema_version`: the existing migration gate drops derived tables
   and the next `reindex` rebuilds — and thanks to the projection/embedding split the app stays
   usable (BM25 + graph) while the background embed refills vectors. That *is* the acceptable
   "one-time heavy lift, slightly diminished state". (If re-embedding ~38.6k chunks is
   undesirable, a one-shot copy-across from `chunks_vec` before dropping it avoids even that.)
   Closes #38's acceptance criteria at today's scale and #36 as a side effect.
2. **B** — centroid column + two-stage `discover::candidates`, shortlist size pinned by the
   eval against exact ground truth. Buys the 10× (and 100×) headroom.
3. **#19** — the chunker upgrade, on its own quality-driven schedule; it multiplies both wins.
4. **C** then **D** stay shelved until the eval + measured latency say otherwise; **E** stays
   the last resort.

Regression guardrails, matching the #37 pattern: extend `discover_query_count.rs` to assert the
whole-space pass emits O(1) SQL statements (not O(chunks)), and add a scale smoke-test with a
synthetic few-hundred-note vault under the fake embedder.
