# Test fixtures

Two committed vaults, for two different jobs. This file lives *outside* both — B2 ingests every
`.md` under a vault root as a note, so a vault directory holds only note/resource content.

## `golden-vault/`

The small, hand-authored vault the **deterministic integration tests** copy into a tempdir and
assert against (fixed note *paths* — the identity, L1 — in `crates/b2-core/tests/common/mod.rs`;
shape per `docs/design/data-model.md §8`). Model-free — the suite runs the `FakeEmbedder`. Change
it only with the tests.

The copy-to-a-tempdir step is now belt and braces rather than load-bearing: it existed because
ingest could *write* to a note it met without a `b2id`, and since [GH #170] nothing does (W1). It
stays because a test that mutates a committed fixture is a bad idea whatever the current write
posture is, and CI's `git diff --exit-code` step would catch it either way.

[GH #170]: https://github.com/AlteredCraft/B2/issues/170

## `test-vault/`

A **synthetic, procedurally-generated** vault (~200 notes / ~790 chunks across 10 topics),
sized to make embedding a meaningful workload. It is for **out-of-CI throughput and
retrieval-quality experiments** that need volume, *not* the deterministic suite:

- **Embed device A/B** (CPU vs Metal GPU, GH #40) — `make compare-device` runs it and reports
  chunks/s + speedup. On this workload Metal measured **~7× faster** than CPU. The script never
  mutates the fixture: it works on a throwaway copy in the system tempdir (same isolation the
  integration tests use for `golden-vault`).
- **Retrieval-quality sanity** — the notes are drawn from per-topic sentence pools, so they form
  real semantic clusters (vector-search, distributed-systems, rust, pkm, transformers, databases,
  productivity, gardening, coffee, hiking) cross-linked by body `[[wikilinks]]` (~2,300 edges) and
  a few typed frontmatter `b2_relations:`. Good for eyeballing `b2 search` / `b2 similar`. It is
  **not** the hand-labelled retrieval eval set (the eval harness under `crates/b2-embed/evals/`) — a scale
  fixture, not a graded benchmark.
- **Rank stability** (GH #141) — `make stability` measures how much the top of the ranking moves when
  the retrieval pool widens under it, and diffs the shipped top-10 against the committed snapshot in
  `crates/b2-embed/evals/stability-baseline.json`. This is the job the *scale* is load-bearing for: the
  labelled corpus (29 chunks) is no bigger than the 150-candidate pool retrieval reaches, so candidate width
  cannot move anything there, while ~780 chunks make the pool bind. The probe runs the fake embedder on
  a throwaway copy, so the numbers are deterministic and the fixture is never touched — but they are
  only as stable as this vault: **editing `test-vault/` invalidates the blessed baseline**, so re-bless
  (`make stability-bless`) in the same commit that changes the fixture.

The prose is templated (sentences recombined from the pools), not human-authored — realistic
enough to cluster and embed like real notes, not meant to be read. An ad-hoc
`b2 reindex -C fixtures/test-vault` rewrites nothing — indexing writes nothing to a vault at all
(W1) — and the disposable `.b2/` index it builds is gitignored.
