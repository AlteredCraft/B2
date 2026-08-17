# The eval harness — overview

How B2 measures the one thing `cargo test` cannot: whether retrieval and discovery are any
*good*. A unit test can prove `reindex` is idempotent; only a human-labelled corpus can say that
"how do leaves turn light into food" should rank `photosynthesis.md` first. This directory is the
harness's home: this overview and the [process rules](#process-rules) below, which are the
harness's law — read them before touching the corpus, the labels, or the metrics. **Decision
history lives in git and in [GitHub Issues](https://github.com/AlteredCraft/B2/issues)**: every
verdict below names the issue that drove it, and the commit that shipped it is the record of what
changed and why.

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
can only say *different*, but it sees what a labelled corpus can be structurally blind to:
candidate-width truncation applies to **chunk** candidate lists, and while a corpus's chunks fit
inside the narrower pool retrieval reaches (`chunk_candidate_pool(10) = 60`; the note view reaches
150), a **candidate-width** change prints bit-identical numbers there while genuinely reordering a
real vault. Blindness holds exactly while corpus chunks ≤ that pool, and every run that is blind
that way says so (`pool_blind` in the row, a `[warn]` on stdout). The worked example is
[#140](https://github.com/AlteredCraft/B2/issues/140)/[#142](https://github.com/AlteredCraft/B2/issues/142):
the eval saw nothing, the probe saw 10 of 10 probes change, and the change was reverted — a probe
can say *different*, only labels can say *better*. Run both. The eval corpus is no longer inside
that pool itself: [#183](https://github.com/AlteredCraft/B2/issues/183)'s multi-topic family
brought it to 30 notes / 63 chunks, three past the 60-chunk threshold, so `just eval` now measures
the shipped `K=10` default's own passage-view truncation too — `just stability` on
`fixtures/test-vault` (~780 chunks) remains the instrument for candidate-width at a scale where
the effect is unambiguous rather than a three-chunk edge case.

## The files (ground truth)

| Path | Role |
|---|---|
| [`crates/b2-embed/examples/eval.rs`](../../crates/b2-embed/examples/eval.rs) | the harness itself — builds a throwaway vault from the corpus each run, scores everything through the real `Vault` pipeline, appends one JSON row per config |
| [`crates/b2-embed/examples/stability.rs`](../../crates/b2-embed/examples/stability.rs) | the model-free rank-stability probe (`fixtures/test-vault`, ~200 notes — big enough for the pools to bind) |
| [`crates/b2-embed/evals/corpus/`](../../crates/b2-embed/evals/corpus/) | the hand-written 30-note vault: topic clusters, six long multi-chunk notes, five unambiguous loners, the stemmer-adversarial block, and the [#183](https://github.com/AlteredCraft/B2/issues/183) multi-topic family (four notes stitching an on-topic half to a genuinely unrelated one, the shape that makes centroid-vs-best-passage discovery ranking disagree) |
| [`crates/b2-embed/evals/queries.json`](../../crates/b2-embed/evals/queries.json) | retrieval labels — 41 queries; a verbatim `passage` adds chunk-level scoring (n=20) |
| [`crates/b2-embed/evals/similar.json`](../../crates/b2-embed/evals/similar.json) | discovery labels — positive anchors with expected mates; **empty `expected` = a negative anchor** whose correct answer is *nothing* |
| `crates/b2-embed/evals/results.jsonl` | append-only run log (gitignored, local) — every number ever cited traces to a row here |
| [`../../justfile`](../../justfile) | the `model` group holds every recipe above |

## What the exit code enforces

`just eval` exits `0` only when the default config clears **both floors**
([`eval.rs`](../../crates/b2-embed/examples/eval.rs), the gate at the end of `run()`):

- hybrid note hit@1 ≥ `FLOOR_HIT1` (0.75), and
- **every negative anchor comes back clean** under the discovery floor (`neg_clean == neg_n`) —
  one stranger served where a label says "nothing" is a regression.

Exit `2` = a floor failed; `1` = the run itself broke. Two instrument checks print before any
score and gate everything after them: the model id (never average CPU and `@metal` rows) and
`batch ≡ single` embedding faithfulness.

## The verdicts this harness has ruled (each traceable to its issue and its commit)

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
- **A multi-topic note family** ([#183](https://github.com/AlteredCraft/B2/issues/183)) closed the
  gap left by every prior note being single-subject, where a centroid and its best chunk agree by
  construction: `pour-over-and-pottery.md` (on-topic half first), `radio-and-sleep-debt.md`
  (on-topic half second — chunk order provably doesn't matter), `tire-pressure-and-knots.md`
  (off-topic half ~3× the on-topic half — the centroid dragged furthest), and
  `running-and-aquarium.md`, a negative whose two halves are both unrelated to every anchor. The
  positive family measurably worked: `tire-pressure-and-knots.md` raised the related-cosine pile's
  floor down to 0.554 (from 0.580), the hardest mate in the corpus by construction. `similar`'s own
  hit@1/hit@3/MRR@5 stayed pinned at 1.000 exactly as the issue predicted — a hit only needs *one*
  of an anchor's mates, so a specific hard mate's placement is invisible to that aggregate — and per
  process rule 4 the piles are the readout that actually moved, which is standing evidence that a
  ceilinged discrete metric here needs a continuous companion, not a replacement. First draft of
  the negative used off-topic halves (furniture refinishing, then genealogy) that leaked past the
  discovery floor for `stain-removal.md` and `git-cheatsheet.md` respectively — caught by the
  two-direction audit and by hand-checking `b2 similar` against a real built vault, not by the
  aggregate score, which cannot see a single dirty negative anchor either. Growing the corpus past
  63 chunks also ended its candidate-width blindness (GH #141) as a side effect — see this file's
  "two halves" section above.

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

## Process rules

Adopted 2026-08-10; each traces to a measured mistake or a named risk, and each is binding on
anyone editing the corpus, the labels, or the metrics.

1. **A paired per-query win/loss list is the primary readout of any A/B; the aggregate is a smoke
   alarm.** At n≈40, every aggregate point is 1–2 queries — "hit@1 +0.05" and "these two flipped"
   are the same fact, but only the second can be argued with against the labels. The sweep prints
   the diff (`Δ vs default`) automatically.
2. **A corpus edit is a change to the instrument, so it ships as its own commit** whose message
   says what changed and why, and every edit runs the **two-direction token audit** before it
   lands: no existing query's content tokens may newly land in the edited/added note, and no new
   query's content tokens may split evenly toward a rival (the `insomnia.md` steal, and the
   `recover`+`mistake` near-miss, are the precedents). The audit is a ten-line script; run it,
   don't eyeball it.
3. **The same person authoring notes, queries, and fixes is a ratchet toward measuring what the
   engine already does.** Mitigations in order of cheapness: rule 2's audit; sourcing future
   queries from outside the corpus author's head (from note titles alone, or another person);
   dogfooding on a real vault before trusting any threshold.
4. **A bit-identical or unmoved metric is a claim to verify, never proof of "no effect"** —
   compare a continuous quantity (the piles) before believing a discrete one. (Standing rule from
   the `prepend-heading-path` trace; the sweep diff prints its own reminder.)
