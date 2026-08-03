# Test fixtures

Two committed vaults, for two different jobs. This file lives *outside* both — B2 ingests every
`.md` under a vault root as a note, so a vault directory holds only note/resource content.

## `golden-vault/`

The small, hand-authored vault the **deterministic integration tests** copy into a tempdir and
assert against (fixed `b2id`s in `crates/b2-core/tests/common/mod.rs`; shape per
`docs/design/data-model.md §8`). Model-free — the suite runs the `FakeEmbedder`. Change it only
with the tests.

## `test-vault/`

A **synthetic, procedurally-generated** vault (~200 notes / ~790 chunks across 10 topics),
sized to make embedding a meaningful workload. It is for **out-of-CI throughput and
retrieval-quality experiments** that need volume, *not* the deterministic suite:

- **Embed device A/B** (CPU vs Metal GPU, GH #40) — `just compare-device` runs it and reports
  chunks/s + speedup. On this workload Metal measured **~7× faster** than CPU. The script never
  mutates the fixture: it works on a throwaway copy in the system tempdir (same isolation the
  integration tests use for `golden-vault`).
- **Retrieval-quality sanity** — the notes are drawn from per-topic sentence pools, so they form
  real semantic clusters (vector-search, distributed-systems, rust, pkm, transformers, databases,
  productivity, gardening, coffee, hiking) cross-linked by body `[[wikilinks]]` (~2,300 edges) and
  a few typed frontmatter `b2_relations:`. Good for eyeballing `b2 search` / `b2 similar`. It is
  **not** the hand-labelled retrieval eval set (the eval harness under `crates/b2-embed/evals/`) — a scale
  fixture, not a graded benchmark.
- **Rank stability** (GH #141) — `just stability` measures how much the top of the ranking moves when
  the retrieval pool widens under it, and diffs the shipped top-10 against the committed snapshot in
  `crates/b2-embed/evals/stability-baseline.json`. This is the job the *scale* is load-bearing for: the
  labelled corpus (26 chunks) is no bigger than the 150-candidate pool retrieval reaches, so candidate width
  cannot move anything there, while ~780 chunks make the pool bind. The probe runs the fake embedder on
  a throwaway copy, so the numbers are deterministic and the fixture is never touched — but they are
  only as stable as this vault: **editing `test-vault/` invalidates the blessed baseline**, so re-bless
  (`just stability-bless`) in the same commit that changes the fixture.

The prose is templated (sentences recombined from the pools), not human-authored — realistic
enough to cluster and embed like real notes, not meant to be read. Every note carries a `b2id`,
so an ad-hoc `b2 reindex -C fixtures/test-vault` neither stamps nor rewrites anything; the
disposable `.b2/` index it builds is gitignored.
