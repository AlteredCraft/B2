---
b2id: 01KWSRM3SWVAVZ66CK3H2Z362K
title: "B2 — Eval Strategy: measuring model quality"
type: note
tags: [b2, evals, testing, embedder, model-quality, spec]
created: 2026-07-03
updated: 2026-07-04
status: draft
---

# B2 — Eval Strategy: measuring model quality

> **How B2 measures the quality of its one AI seam — the [`Embedder`](../../crates/b2-core/src/embed.rs)
> — without letting a real model into the CI suite.** The [build spec](completed/index-engine-build.md) covers the
> deterministic engine; this doc covers the *non-deterministic* half: a hand-labelled eval that scores
> model output, lives **out of CI**, and is run on demand. It owns the eval philosophy, the labelled-set
> format, the metrics, and how to read and grow it. It does **not** own the seam itself
> ([embed.rs](../../crates/b2-core/src/embed.rs)) or the real backend
> ([`LocalEmbedder`](../../crates/b2-embed/src/lib.rs)).
>
> **Changed 2026-07-04.** B2 previously had a second AI seam and a second eval — the LLM `Relator` and a
> suggestion-quality eval (`b2-relate`'s `suggest-eval`). The relator was **cut** (connection discovery is
> now `b2 similar` + `b2 link`, no LLM in the loop; [vision-and-scope.md](../vision-and-scope.md)
> "Decisions locked (2026-07-04)"), so that eval, its labelled `pairs.json`, and the whole `b2-relate`
> crate are gone. The **semantic-retrieval eval below is now the sole out-of-CI model-quality pass.**

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

| | **Semantic-retrieval** |
|---|---|
| Crate | `b2-embed` |
| Run | `cargo run -p b2-embed --example eval` |
| Seam under test | [`Embedder`](../../crates/b2-core/src/embed.rs) (`LocalEmbedder`) |
| Question | does hybrid search rank the right note first? |
| Data | [`evals/corpus/`](../../crates/b2-embed/evals) + `queries.json` |
| Metrics | precision@1, precision@3, MRR@10 |
| Floor | precision@1 ≥ 0.75 |

A small hand-labelled corpus, a JSON label set whose queries deliberately probe the hard cases (semantic,
not lexical), a real-model run, and a printed score with a soft floor.

## 2. Semantic-retrieval eval (`b2-embed`)

Shipped with the real embedder. It builds a throwaway vault from
[`evals/corpus/`](../../crates/b2-embed/evals), reindexes it through the **real** `LocalEmbedder` and the
full hybrid pipeline (BM25 ⊕ vector → RRF), then scores each labelled query by the rank of its relevant
note. Queries in `queries.json` are written to **avoid the target's keywords** (synonyms / paraphrase), so
a passing score is genuine semantic lift, not lexical overlap. Reported: precision@1, precision@3, MRR@10;
floor `p@1 ≥ 0.75`. See [`examples/eval.rs`](../../crates/b2-embed/examples/eval.rs).

## 3. Growing the set & the tuning loop

- **Tune from numbers, not vibes.** Run → read the misses → change **one** thing (a query label; the
  default embedding model; a chunker parameter) → re-run. The floor guards against regressions.
- **Grow the labelled set.** The seed `queries.json` is a start; add queries that probe paraphrase,
  multi-hop meaning, and the keyword/stopword noise the first eval pass surfaced.
- **What this eval unlocks.** With a real embedder scoring retrieval, two deferred items can finally be
  *measured* rather than guessed ([tasks.md](../tasks.md) backlog): the **qmd chunker upgrade** (paragraph
  vs. qmd chunking, [build spec](completed/index-engine-build.md) §1.2), and ranking-quality tuning of the hybrid
  pipeline. Since `b2 similar` reuses the same stored vectors, better retrieval quality lifts
  candidate-surfacing quality directly.
