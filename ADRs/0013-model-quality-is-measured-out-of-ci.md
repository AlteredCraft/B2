# ADR-0013 — Model quality is measured out of CI, by a labelled harness

- **Status:** Accepted · 2026-07-13
- **Refs:** invariants E2 · `docs/evals/README.md` (the process rules) · GH #44, #141, #187

## Context

`cargo test` can prove `reindex` is idempotent; only a human-labelled corpus can say that "how do
leaves turn light into food" should rank `photosynthesis.md` first. Putting that in CI would make
the gate slow, networked, and flaky.

## Decision

- **Quality never enters CI.** Real-model work lives behind `b2 init` and the harness under
  `crates/b2-embed/evals/`, run on demand (`make eval`, `make eval-sweep`, `make eval-stemmer`,
  `make eval-chat`, `make eval-metal`).
- Two halves that rule together: `make eval` scores **quality** (it can say *better*) on labelled
  corpora; `make stability` is a model-free rank probe on a ~200-note fixture that scores
  **movement** (it can only say *different*) — and sees candidate-width changes a small corpus is
  structurally blind to.
- **Constants in code, measurements in the harness.** A shipped threshold's justification is
  **re-derived on every run** by the harness rather than quoted in a comment, because a number
  frozen into a comment goes stale the first time the corpus grows a shape it never saw.
- **A distributional constant owes a transfer check** (`make calibrate VAULT=<vault>`) on a real vault
  before it ships: a constant read off one corpus's distribution describes that corpus.
- `#[ignore]` is forbidden — a check that genuinely needs the real model belongs in the harness,
  where it actually runs.

## Consequences

- Exit-gate assertions are set a margin to the *safe* side of the day's reading, so headroom absorbs
  corpus drift rather than run noise (repeat runs are bit-identical).
- Corpus edits ship as their own commit plus a two-direction token audit; a paired per-query
  win/loss list is the primary A/B readout. Numbers live in `results.jsonl` (gitignored, local).
