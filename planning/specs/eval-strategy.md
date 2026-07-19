---
b2id: 01KWSRM3SWVAVZ66CK3H2Z362K
title: "B2 — Eval Strategy: measuring model quality"
type: note
tags: [b2, evals, testing, embedder, model-quality, spec]
created: 2026-07-03
updated: 2026-07-19
status: active
---

# B2 — Eval Strategy: measuring model quality

> **How B2 measures the quality of its one AI seam — the [`Embedder`](../../crates/b2-core/src/embed.rs)
> — without letting a real model into the CI suite.** The [build spec](completed/index-engine-build.md) covers the
> deterministic engine; this doc covers the *non-deterministic* half: a hand-labelled eval that scores
> model output, lives **out of CI**, and is run on demand. It owns the eval philosophy, the labelled-set
> formats, the metrics, and how to run, read, and grow it. It does **not** own the seam itself
> ([embed.rs](../../crates/b2-core/src/embed.rs)) or the real backend
> ([`LocalEmbedder`](../../crates/b2-embed/src/lib.rs)).
>
> **Changed 2026-07-04.** B2 previously had a second AI seam and a second eval — the LLM `Relator` and a
> suggestion-quality eval (`b2-relate`'s `suggest-eval`). The relator was **cut** (connection discovery is
> now `b2 similar` + `b2 link`, no LLM in the loop; [vision-and-scope.md](../vision-and-scope.md)
> "Decisions locked (2026-07-04)"), so that eval, its labelled `pairs.json`, and the whole `b2-relate`
> crate are gone.
>
> **Changed 2026-07-19.** The eval grew from one score to four. The original harness measured only
> *note-rank retrieval* over 6 easily-separable notes — a gate the qmd-chunker change was invisible to
> ([qmd-chunker.md](completed/qmd-chunker.md) §7's caveat, tracked as
> [#44](https://github.com/AlteredCraft/B2/issues/44)). One run now also scores the **BM25-only
> baseline** (so semantic lift is a measured delta, not an assumption), **passage-level ranks** (so
> chunking changes move the score), and **`b2 similar` discovery** (the product's headline feature,
> previously covered only by inference). The corpus grew to 20 notes arranged in confusable topic
> clusters plus three long, sectioned notes; `--sweep` A/Bs `ChunkConfig` levers in one process; and
> every run appends to a results log.

## 0. Why evals are examples, not tests

The core invariant of the test suite is that **`cargo test` is fast, deterministic, and model-free**
([vision-and-scope.md](../vision-and-scope.md) testability point 5; [CLAUDE.md](../../CLAUDE.md)). Model
output is neither fast nor deterministic — a real embedder needs a downloaded model and can drift
run-to-run. Letting it into CI would make the suite slow and flaky and would gate every commit on model
behavior.

So model quality is measured by a **Cargo example, never a `#[test]`**. An example is compiled but only
run on demand (`cargo run --example …`), so it never runs in `cargo test` and can never flake the suite.
The eval builds its own throwaway inputs, drives the **real** backend, prints a score table, and exits
non-zero below a soft reference floor so it can double as a manual quality gate. The deterministic fake
([`FakeEmbedder`](../../crates/b2-core/src/embed.rs)) stays the CI default; the eval is one of the few
places a real model is exercised, alongside `b2 init`.

## 1. The eval at a glance

| | **Semantic-retrieval + discovery** |
|---|---|
| Crate | `b2-embed` |
| Run | `cargo run -p b2-embed --example eval` (`just eval`); add `-- --sweep` for the chunker A/B (`just eval-sweep`) |
| Seam under test | [`Embedder`](../../crates/b2-core/src/embed.rs) (`LocalEmbedder`) through the full `Vault` pipeline |
| Questions | does hybrid search rank the right note first — and by how much more than keywords alone? does the right *passage* rank? does `similar` surface the notes a human would connect? |
| Data | [`evals/corpus/`](../../crates/b2-embed/evals) (20 notes) + `queries.json` (25 labelled queries) + `similar.json` (6 labelled anchors) |
| Metrics | hit@1, hit@3, MRR@10 — for BM25-only and hybrid, at note and passage level; hit@1/3 + MRR@5 for `similar` |
| Floor | default-config **hybrid note hit@1 ≥ 0.75** (exit code 2 below it) |
| Record | every scored run appends one JSON line to `evals/results.jsonl` (gitignored, machine-local) |

## 2. What one run measures

The harness ([`examples/eval.rs`](../../crates/b2-embed/examples/eval.rs)) copies the corpus into a
throwaway vault and drives the **real** production pipeline through the `Vault` façade, in two phases
that exist because the engine itself split them ([projection-embedding-split.md](completed/projection-embedding-split.md)):

1. **`project` only → the BM25 baseline.** With no vector space yet, `search` runs keyword-only, so
   scoring every query here yields the ablation *for free* — same vault, same index, paused between the
   passes. The labelled queries avoid their target's keywords, so this is the floor the model must clear.
2. **`embed` → the semantic scores.** The same queries re-score through hybrid retrieval (BM25 ⊕ vector
   → RRF). Three things are read off this phase:
   - **Note rank** (`Vault::search`) — the headline retrieval metric, reported next to the baseline so
     the **semantic lift** (hybrid − BM25 hit@1) is a printed number, not a belief.
   - **Passage rank** (`Vault::search_chunks`) — for queries labelled with a `passage` phrase: the rank
     of the first top-K *chunk* that belongs to a relevant note **and** contains the phrase. Note-rank
     is blind to sub-note retrieval — which is precisely what chunking levers move — so this is the
     metric that makes the chunker gate sensitive (§5).
   - **Discovery** (`Vault::similar`) — for each labelled anchor, the rank of the first expected
     cluster-mate among the top 5 candidates. `similar` shares the stored vectors with search but is a
     different task (note-to-note, centroid-shortlisted — [#38]'s two-stage scan), so it gets its own
     labels rather than being assumed covered.

The embed pass is also **timed and chunk-counted** (via the progress callback), so a chunking change
reports its cost axis (chunk count, embed seconds) next to its quality axis.

`--sweep` then re-runs phase 2 under variant `ChunkConfig`s — `Vault::set_chunk_config` →
`project(force)` → `embed` → re-score — all in one process with one model load. This is the seam
[`chunk.rs`](../../crates/b2-core/src/chunk.rs) promised the "Step-3 eval" (spec §3 D5): a chunker A/B
is a loop over configs, not a recompile per cell.

## 3. The labelled sets & the corpus design

Three files under [`crates/b2-embed/evals/`](../../crates/b2-embed/evals):

- **`corpus/*.md`** — 20 hand-written notes, designed rather than accumulated:
  - **Confusable clusters.** Six topic clusters of 2–3 notes each (coffee: espresso / french-press /
    roasting; sleep: insomnia / hygiene / dreaming; plants; security; geology; cycling). Real vaults
    cluster, and the hard ranking problem is *within* a topic, not across unrelated ones — a corpus of
    mutually-alien notes lets any embedder score perfectly. Distractors keep the labels honest: when
    writing a cluster note, avoid the sibling labels' key phrases (e.g. `sleep-hygiene.md` never
    mentions stimulants, which label the caffeine→insomnia query).
  - **Long, sectioned notes.** Three multi-section notes (`fermentation.md`, `backpacking-gear.md`,
    `personal-finance.md`) big enough to cut into several chunks under the default config, each hiding
    labelled content the way real notes do: **a fact buried in a table row**, **a heading-less
    subsection**, and **a deep paraphrase target** — the three failure modes #44 requires the gate to
    see before it can arbitrate chunking.
- **`queries.json`** — 25 queries. Each avoids the target's keywords (synonyms/paraphrase) so a pass is
  semantic lift, not lexical overlap; `relevant` lists the path(s) that should rank first. An optional
  **`passage`** field is a short **verbatim** phrase from the target passage — those queries are also
  scored at chunk level. Keep passages short, contiguous, and unique in the corpus (containment is the
  match rule), and pick phrases that don't depend on chunk boundaries or on `heading_path` (which is
  itself a lever under test).
- **`similar.json`** — 6 anchors with their expected cluster-mates. The corpus is deliberately
  **unlinked**, so no candidate is excluded by discovery's 1-hop-neighbor rule and the labels stay pure.

## 4. Reading the numbers

- **hit@k, not precision@k.** Each query has essentially one relevant target, so precision@k and
  recall@k collapse into "was it in the top k" — the output names it honestly. MRR@10 is the
  tie-breaker metric between runs whose hit rates match.
- **The floor is a regression guard, not a target.** Default-config hybrid note hit@1 ≥ 0.75, chosen
  low enough that model swaps and chunker experiments can be *compared* without the gate blocking every
  probe. The floor deliberately ignores the other scores — they are for reading, not gating (yet).
- **The lift is the seam's value.** If hybrid ≈ BM25-only on a keyword-avoiding query set, the model is
  not paying for its inference cost. (Reference run, 2026-07-19, CPU: BM25 0.76 → hybrid 0.88 hit@1;
  passage-level 0.33 → 0.50 hit@1 with hit@3 going 0.50 → 1.00; `similar` a clean 1.00 — the discovery
  labels are currently easy, which makes them a pure regression guard.)
- **Read the misses — they are findings.** The 2026-07-19 run's misses are instructive: queries like
  *"pedalling a bike up a steep hill"* rank poorly **because of RRF noise, not the model** — the
  forgiving OR-of-terms FTS5 query matches stopwords ("a", "up") across the whole corpus, and on a
  small corpus a chunk scoring mid-list in *both* signals outranks a vector-only #1. That is a genuine
  pipeline observation (a stopword filter or fusion weighting experiment, measurable right here), not a
  labelling error to paper over.
- **`results.jsonl` is the memory.** One JSON line per scored configuration: timestamp, git SHA, model
  id (which encodes the compute device — an `@metal` build logs as a different model, GH #40), chunk
  config, corpus size, embed cost, all aggregates, and per-query ranks. Append-only and gitignored
  (scores are machine-local), same convention as `B2_LOG_FILE` — pipe into jq/DuckDB to diff runs:
  `jq -r '[.ts, .git, .config.label, .note.hybrid.hit1, .chunk.hybrid.mrr] | @tsv' results.jsonl`.

## 5. The chunker gate (GH #44)

The qmd chunker shipped implementation-first ([qmd-chunker.md](completed/qmd-chunker.md) Steps 1–2);
its §7 quality gate — *qmd must not regress retrieval vs. the old paragraph chunker, and should cut
chunk count and embed time* — stayed open as #44 because the old eval could not see chunking at all
(single-chunk notes, note-rank only). The sensitivity work #44 front-loads is now in place: passage
probes into tables, heading-less subsections, and deep sections (§3), scored at chunk level (§2).

To run an A/B over chunker levers:

```bash
just eval-sweep     # = cargo run -p b2-embed --example eval -- --sweep
```

The sweep prints one row per config (chunks, embed seconds, note hit@1/MRR, chunk hit@1/MRR, similar
hit@3) and logs each as its own `results.jsonl` line. Editing the variant list in `eval.rs` is the
intended workflow — it is a lab bench, not a product surface. Decision rule per §7: **keep a chunking
change if the note metrics hold and the chunk metrics hold-or-improve; retune or revert on a
regression.** (First sweep, 2026-07-19: `target-250` — smaller chunks — *costs* quality here, note
hit@1 0.88 → 0.80 and chunk hit@1 0.50 → 0.33, evidence the gate now has teeth;
`prepend-heading-path` (D3) moved only chunk MRR — more passage probes into *heading-titled* deep
sections would sharpen that particular A/B.)

## 6. Growing the set & the tuning loop

- **Tune from numbers, not vibes.** Run → read the misses → change **one** thing (a query label; a
  chunker lever; the default model; a fusion parameter) → re-run → diff the two `results.jsonl` lines.
  The floor guards against outright regressions while you probe.
- **Grow labels along failure modes, not volume.** A new query earns its place by probing something the
  set is currently blind to: more heading-titled deep sections (sharpen D3), near-duplicate cluster
  pairs (ranking under real confusion), multi-hop meaning, longer conversational queries. Same for
  `similar.json`: today's labels are all easy cluster-mates — anchors with *competing* plausible mates
  would make discovery's MRR informative rather than saturated.
- **Keep labels achievable and clean.** Every query should be answerable by a human shown the corpus;
  every passage phrase verbatim and unique; every distractor written without the sibling's label
  phrases. A label the pipeline *can't* satisfy measures the label, not the model.
- **What this eval unlocks.** With lift, passage rank, and discovery all measured: the #44 gate (§5),
  distance-weighting for `b2 similar` ([#20](https://github.com/AlteredCraft/B2/issues/20) — its A/B
  reads straight off the `similar` metrics), the smaller/faster embedder spike
  ([#17](https://github.com/AlteredCraft/B2/issues/17) — swap the model, compare lines), CPU↔Metal
  quality parity (GH #40 — `just eval-metal` logs under the `@metal` model id), and fusion experiments
  (the stopword/RRF finding in §4 is sitting there waiting to be tried).

[#38]: https://github.com/AlteredCraft/B2/issues/38
