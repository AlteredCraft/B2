# The eval harness — overview

How B2 measures the one thing `cargo test` cannot: whether retrieval and discovery are any
*good*. A unit test can prove `reindex` is idempotent; only a human-labelled corpus can say that
"how do leaves turn light into food" should rank `photosynthesis.md` first. This directory is the
harness's home: this overview, and [`runlog.md`](runlog.md) — the **binding lab notebook** where
every decision is recorded against the run that settled it. Read the runlog's Orientation before
touching the corpus, the labels, or the metrics; its process rules are the harness's law.

Everything here runs **out of CI, on demand** — the repo rule is that `cargo test` stays fast,
deterministic, and model-free, so model quality can never flake CI
([`CLAUDE.md`](../../CLAUDE.md), Conventions).

## The two halves, and why neither is enough alone

| Command | What it measures | Model | Deterministic |
|---|---|---|---|
| `just eval` | BM25 / vector-only / hybrid note & passage ranks, semantic lift, fusion demotions, discovery ranks + **suppression under the quality floor**, cosine piles | real bge | no |
| `just eval-sweep` | the same, per `ChunkConfig` variant — the chunker A/B ([#44](https://github.com/AlteredCraft/B2/issues/44)'s gate, seven variants) | real bge | no |
| `just eval-stemmer` | the same, under the unstemmed `unicode61` ablation beside the shipped `porter unicode61` ([#157](https://github.com/AlteredCraft/B2/issues/157)'s instrument) | real bge | no |
| `just stability` | top-10 drift vs a blessed baseline as candidate pools widen ([#141](https://github.com/AlteredCraft/B2/issues/141)) | fake | yes |
| `just eval-metal` | `just eval` on the Apple-Silicon GPU (model id gains `@metal` — a different vector space) | real bge | no |

`just eval` scores **quality** — it can say *better*. `just stability` scores **movement** — it
can only say *different*, but it sees what the labelled corpus is structurally blind to: the
26-note corpus fits inside the candidate pools retrieval reaches, so a **candidate-width** change
prints bit-identical numbers here while genuinely reordering a real vault. Every run that is
blind that way says so (`pool_blind` in the row, a `[warn]` on stdout). The worked example is
[#140](https://github.com/AlteredCraft/B2/issues/140)/[#142](https://github.com/AlteredCraft/B2/issues/142):
the eval saw nothing, the probe saw 10 of 10 probes change, and the change was reverted — a probe
can say *different*, only labels can say *better*. Run both.

## The files (ground truth)

| Path | Role |
|---|---|
| [`crates/b2-embed/examples/eval.rs`](../../crates/b2-embed/examples/eval.rs) | the harness itself — builds a throwaway vault from the corpus each run, scores everything through the real `Vault` pipeline, appends one JSON row per config |
| [`crates/b2-embed/examples/stability.rs`](../../crates/b2-embed/examples/stability.rs) | the model-free rank-stability probe (`fixtures/test-vault`, ~200 notes — big enough for the pools to bind) |
| [`crates/b2-embed/evals/corpus/`](../../crates/b2-embed/evals/corpus/) | the hand-written 26-note vault: topic clusters, six long multi-chunk notes, four unambiguous loners, the stemmer-adversarial block |
| [`crates/b2-embed/evals/queries.json`](../../crates/b2-embed/evals/queries.json) | retrieval labels — 41 queries; a verbatim `passage` adds chunk-level scoring (n=20) |
| [`crates/b2-embed/evals/similar.json`](../../crates/b2-embed/evals/similar.json) | discovery labels — positive anchors with expected mates; **empty `expected` = a negative anchor** whose correct answer is *nothing* |
| `crates/b2-embed/evals/results.jsonl` | append-only run log (gitignored, local) — every number ever cited traces to a row here |
| [`../../justfile`](../../justfile) | the `model` group holds every recipe above |
| [`runlog.md`](runlog.md) | the decision log — what changed, why, which rows settled it; **append-only, newest last** |

## What the exit code enforces

`just eval` exits `0` only when the default config clears **both floors**
([`eval.rs`](../../crates/b2-embed/examples/eval.rs), the gate at the end of `run()`):

- hybrid note hit@1 ≥ `FLOOR_HIT1` (0.75), and
- **every negative anchor comes back clean** under the discovery floor (`neg_clean == neg_n`) —
  one stranger served where a label says "nothing" is a regression.

Exit `2` = a floor failed; `1` = the run itself broke. Two instrument checks print before any
score and gate everything after them: the model id (never average CPU and `@metal` rows) and
`batch ≡ single` embedding faithfulness.

## The verdicts this harness has ruled (all in the runlog, each traceable to a row)

- **`porter unicode61` FTS stemming** (schema v5, [#157](https://github.com/AlteredCraft/B2/issues/157)):
  7–0 BM25 / 3–0 hybrid on the paired win/loss readout, precision probes unmoved. The retired
  `unicode61` arm stays measurable via `just eval-stemmer`.
- **`ChunkConfig::default()` held** ([#44](https://github.com/AlteredCraft/B2/issues/44)): a
  seven-variant sweep; the scoreboard's best rows were impeached (512-token truncation, a
  measured ±3–4-of-20 boundary-luck noise floor), and the default kept. Retrial is one
  `just eval-sweep` away.
- **The discovery quality floor** ([#150](https://github.com/AlteredCraft/B2/issues/150)):
  per-anchor z-scores over the stage-1 centroid population
  ([`discover.rs`](../../crates/b2-core/src/discover.rs), `DiscoveryFloor`) — model-relative by
  construction, calibrated from every labelled anchor's full ranked list, transfer-checked on a
  228-essay single-author vault. Suppression went 0/4 → 4/4 clean and is now in the exit gate.
- **RRF fused-score ties break on the dense signal's rank**
  ([#156](https://github.com/AlteredCraft/B2/issues/156)) — a policy the eval decided, not walk order.
- **Corpus and labels agree everywhere**: an arguable negative was replaced rather than argued
  (watercolor → throat-singing), and a claimed relation the notes never expressed was written
  into their text (the encryption pair). Principle on record: when a label and a note disagree,
  fix whichever one is lying.

The one deliberately open thread: the **phishing inversion** — a real relation the model still
ranks a hair under two strangers — is the standing evidence for the pair-scorer escalation named
in [`index-engine.md §3`](../design/index-engine.md), promoted only if real-vault dogfooding
demands it.

## Running it

```console
just init          # provision bge-base-en-v1.5 (one time)
just eval          # ~40s warm; appends a row, exits non-zero on a floor regression
just eval-sweep    # + the seven-variant chunker A/B
just stability     # model-free, deterministic; `just stability-bless` only after an INTENDED change
```

Editing anything here? The corpus is frozen except through a runlog entry, every content edit
runs the two-direction token audit as a script, and a paired per-query win/loss list — never the
aggregate — is the readout any A/B is judged on. The full rules: [`runlog.md`](runlog.md) →
Orientation → Process rules.
