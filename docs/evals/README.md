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

**What the gate deliberately does *not* watch** is the discovery floor's `member_z`. While
`member_z ≤ leader_z`, a negative anchor is clean **iff its leader is cut** — so the member bar
cannot dirty (or clean) a negative anchor, and the one gated discovery number is blind to it by
construction. What a loose member bar costs lands as *stranger tails on positive anchors' lists*,
and what a tight one costs is labelled mates never served at all
([#187](https://github.com/AlteredCraft/B2/issues/187)). Both are **reported, not gated**: every
run prints a `floor calibration` block — the three z populations the two constants answer to, the
re-derived admissible window for each, and the member bar's trade curve — and records it in the
row as `discovery_z`. Reported rather than gated because the member window is currently *empty*
(below), and a permanently-red gate is the advisory-but-exit-0 hole inverted: it trains the same
skimming.

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
  family came with the metric it needs — **per-mate ranks** (every labelled mate scored on its own,
  not just an anchor's first hit), because the issue's "ceiling problem" was real: `similar`'s
  per-anchor hit@1/hit@3/MRR@5 read **1.000 even with the family in place**, since each anchor's
  easy mate still lands and hides whatever happened to the hard one. Per-mate reads
  **hit@1 0.43 / hit@3 0.79 / MRR@5 0.595** on the same run — off the ceiling, with headroom in
  both directions. What it then exposed is worse than the demotion this repo expected: of 14
  labelled mates, **3 never surface on the shipped surface at all** — `tire-pressure-and-knots.md`
  (best-passage rank 2 for `bicycle.md`), `radio-and-sleep-debt.md` (rank 3 for `insomnia.md`) and
  the long-known `phishing.md` (rank 4 for `encryption.md`) all fall under the floor's `member_z`.
  The one multi-topic mate that does survive, `pour-over-and-pottery.md`, is demoted from
  best-passage rank 1 to centroid rank 3. Nothing was tuned in response — the eval measures, and
  [#182](https://github.com/AlteredCraft/B2/issues/182) is where the response is decided; it now
  has numbers rather than an eyeball. First draft of
  the negative used off-topic halves (furniture refinishing, then genealogy) that leaked past the
  discovery floor for `stain-removal.md` and `git-cheatsheet.md` respectively — caught by the
  two-direction audit and by hand-checking `b2 similar` against a real built vault, not by the
  aggregate score, which cannot see a single dirty negative anchor either. Growing the corpus past
  63 chunks also ended its candidate-width blindness (GH #141) as a side effect — see this file's
  "two halves" section above.
- **The journal-shaped note cannot land while the #150 floor ships**
  ([#189](https://github.com/AlteredCraft/B2/issues/189), measured 2026-08-17, and the first
  corpus edit this harness has *rejected*): the issue's ≥6-section journal positive
  (`week-log.md` — seven hobby sections, one lava-field gem labelled the mate of `volcano.md`,
  ~6:1 dilution) was built, measured, and **reverted**, not for any wording it contained but for
  its geometry. Averaging that many mutually-unrelated sections collapses the normalized centroid
  toward the corpus's shared direction, which makes it the top-z candidate for **loner anchors on
  content it does not contain**: with the note in place `git-cheatsheet.md` served it at z +2.00
  and `throat-singing.md` at +2.45 — above every labelled mate's z anywhere in the corpus — and
  the negatives gate went red. Rewriting sections moved pair cosines, never the verdict; a
  synthetic probe (averaging k of the corpus's *existing* chunk vectors — no prose at all)
  crosses a loner's bar from **k = 4** and tops the loners' lists at k ≥ 5. The distributions
  #187 asked about do not merely overlap on this shape — they **invert**, so no `member_z`
  rescues the suppressed mates without serving strangers. The issue's optional negative sibling
  fared no better: two journal-shaped notes were each other's strongest pair in the whole corpus
  (best-passage cosine 0.762, above the strongest *labelled* pair at 0.667) because the
  first-person practice-log register is itself a topic to the model, so its "nothing relates"
  label would be arguable — the watercolor rule, applied before landing this time. Note text,
  labels, and probe live in #189 for the day the floor can carry them; `just eval` stays green
  because the edit was reverted, not because the engine passed it.
- **There is no `member_z` — the inversion is already in the shipped corpus, not only under the
  journal shape** ([#187](https://github.com/AlteredCraft/B2/issues/187), measured 2026-08-17 by
  the `floor calibration` block this run added). #189 showed the member/stranger distributions
  invert once a ≥6-section note exists; dumping every candidate's ungated stage-1 z on the corpus
  **as it ships** shows they already do. Labelled mates run **+0.804 … +2.862**; strangers on
  positive anchors reach **+1.618** — so the admissible member window `(max stranger, min mate]`
  is **empty**, and the overlap is 0.8 z wide rather than marginal. The trade curve prices what
  #150's window could only assert: at the shipped `member_z = 1.85` discovery serves **11 of 14**
  labelled mates and **0** strangers; rescuing `radio-and-sleep-debt.md` (+1.547) or
  `tire-pressure-and-knots.md` (+1.445) costs exactly **one** stranger
  (`volcano.md → pour-over-and-pottery.md`, +1.618), while rescuing `phishing.md` (+0.804) costs
  **18**. The leader gate, by contrast, still has a window — but a **narrow** one, `(+1.787,
  +1.880]`, both edges set by multi-topic notes: its floor is `throat-singing.md`'s top candidate
  `pour-over-and-pottery.md` (a #183 family note surfacing on a loner — the journal mechanism in
  miniature) and its ceiling is `encryption.md`'s own leader, which sits **0.03** above the
  shipped 1.85 and would take that anchor's entire list with it. Nothing was tuned: the numbers
  say a constant cannot do this job, which is the measured case for the shape change (a floor
  that can see a candidate's best passage, not only its centroid) or the pair-scorer escalation.

The deliberately open threads: the **phishing inversion** — a real relation the model still
ranks a hair under two strangers — and, since #189, the **journal-shape inversion** above are
the standing evidence for the pair-scorer escalation named in
[`index-engine.md §3`](../design/index-engine.md), promoted only if real-vault dogfooding
demands it — with the #189 result the first evidence that arrives from geometry rather than a
single unlucky pair, and #187's window dump the first that needs no new corpus note at all.

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
