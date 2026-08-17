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
| [`crates/b2-embed/evals/corpus/`](../../crates/b2-embed/evals/corpus/) | the hand-written 31-note vault: topic clusters, six long multi-chunk notes, five unambiguous loners, the stemmer-adversarial block, the [#183](https://github.com/AlteredCraft/B2/issues/183) multi-topic family (four notes stitching an on-topic half to a genuinely unrelated one, the shape that makes centroid-vs-best-passage discovery ranking disagree), and — since [#192](https://github.com/AlteredCraft/B2/issues/192) landed [#189](https://github.com/AlteredCraft/B2/issues/189)'s note — `week-log.md`, the journal-shaped dilution extreme (seven unrelated sections, one lava-field gem) |
| [`crates/b2-embed/evals/queries.json`](../../crates/b2-embed/evals/queries.json) | retrieval labels — 41 queries; a verbatim `passage` adds chunk-level scoring (n=20) |
| [`crates/b2-embed/evals/similar.json`](../../crates/b2-embed/evals/similar.json) | discovery labels — positive anchors with expected mates; **empty `expected` = a negative anchor** whose correct answer is *nothing* |
| `crates/b2-embed/evals/results.jsonl` | append-only run log (gitignored, local) — every number ever cited traces to a row here |
| [`../../justfile`](../../justfile) | the `model` group holds every recipe above |

## What the exit code enforces

`just eval` exits `0` only when the default config clears **all four assertions**
([`eval.rs`](../../crates/b2-embed/examples/eval.rs), the gate at the end of `run()`):

| Assertion | Constant | Watches |
|---|---|---|
| hybrid note hit@1 ≥ 0.75 | `FLOOR_HIT1` | retrieval |
| every negative anchor comes back clean (`neg_clean == neg_n`) | — | the floor's `leader_z` |
| per-mate discovery MRR@5 ≥ 0.50 | `FLOOR_MATE_MRR` | discovery **rank** |
| labelled mates suppressed ≤ 2 | `MAX_MATES_SUPPRESSED` | discovery **existence** |

One stranger served where a label says "nothing" is a regression; so is a labelled mate that stops
being served at all. Exit `2` = an assertion failed; `1` = the run itself broke. Two instrument
checks print before any score and gate everything after them: the model id (never average CPU and
`@metal` rows) and `batch ≡ single` embedding faithfulness.

The bottom two landed with [#188](https://github.com/AlteredCraft/B2/issues/188), and **how they
are placed is the point**. Both sit *below* the reading they were set from, never at it — a gate
pinned to today's number fails on the first legitimate corpus edit, and the cheapest way to clear
a red per-mate number is to **edit a label**, which is the one habit this harness must never
train (process rule 2's concern, sharper here: per-mate is label-sensitive by construction —
adding a mate to an anchor changes `n` and moves the aggregate whether or not the engine did
anything). Sizing came from repeated runs rather than intuition, and the measurement was that
there is nothing to size against: **five consecutive runs on an unchanged corpus, model and build
produced bit-identical rows** — every rank, every z, every cosine — so the run-to-run noise floor
that [#188](https://github.com/AlteredCraft/B2/issues/188) asked for is *zero*, and the headroom
these floors need is for corpus drift, not for run noise. At n = 15 mates, one mate lost from
rank 1 costs 1/15 ≈ 0.067 of MRR@5, and the floor sits about two such losses under the shipped
0.633. Suppression is asserted separately because averaging hides the difference between a mate
sliding 2 → 3 and a mate disappearing; its allowance is today's one named residue
(`phishing.md`) plus a slot, so a *second* unexplained disappearance is what trips it.

The **saturating per-anchor metric** (`similar` hit@1/hit@3/MRR@5 — first mate found, then stop)
is no longer printed: it read 1.000 across every change it was meant to judge, and a line that
cannot move trains skimming. It is still recorded — `results.jsonl`'s `"similar"` key is
untouched, so rows stay comparable back to the first run — and the sweep's per-variant column now
carries per-mate MRR@5 instead, for the same reason.

**What the gate deliberately does *not* watch** is the discovery floor's `member_z`. While
`member_z ≤ leader_z`, a negative anchor is clean **iff its leader is cut** — so the member bar
cannot dirty (or clean) a negative anchor, and the negatives assertion is blind to it by
construction. What a loose member bar costs lands as *stranger tails on positive anchors' lists*,
and what a tight one costs is labelled mates never served at all
([#187](https://github.com/AlteredCraft/B2/issues/187)). The second of those is now gated (the
suppression assertion above). The first is **counted but deliberately not gated**: every run
prints a `strangers` line — unlabelled notes served on positive anchors at the ranks' own depth,
each named `anchor → path` — and records it as `similar_strangers`. It reads **0 cards** as of
[#192](https://github.com/AlteredCraft/B2/issues/192), which is exactly why gating it would be a
trap: a floor at today's value forbids the next corpus edit from surfacing anything unlabelled,
and the cheapest way to shrink the count is to *label the stranger*, which moves the per-mate
metric too. An unlabelled note served is not proof of junk — the labels are not exhaustive — so
it is a smoke alarm shipped with the list you argue against it with (process rule 1's posture).
The floor's two constants are reported the same way: every run prints a `floor calibration`
block — the three z populations they answer to, the re-derived admissible window for each, and
the member bar's trade curve — recorded in the row as `discovery_z`. Reported rather than gated
because the member window is *empty* by one pair (below), and a permanently-red gate is the
advisory-but-exit-0 hole inverted: it trains the same skimming.

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
  labels, and probe were preserved on #189 for the day the floor could carry them — which came
  with [#192](https://github.com/AlteredCraft/B2/issues/192), below.
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
- **The floor is judged after stage 2, and the journal note landed**
  ([#192](https://github.com/AlteredCraft/B2/issues/192), measured and shipped 2026-08-17). The
  same ungated dump, read in the stage-2 best-passage unit, separates what the centroid unit
  could not: with `week-log.md` in the corpus, labelled mates span **+1.253 … +2.866** against
  strangers-on-positives topping at **+1.367**, and the leader window is **open** at
  `(+1.919, +2.004]` where the stage-1 one had inverted outright (neg leaders to +2.447 vs pos
  from +1.849). `discover::candidates` now truncates the shortlist, scores it, and judges the
  floor on the best-passage z — both gates — with the defaults re-read as window midpoints
  (`leader_z 1.96`; `member_z 1.49`, midpoint of the widest range the trade curve prices at zero
  strangers, the member window proper being empty by exactly one row). At them: **14/15 labelled
  mates, 0 strangers, 5/5 negatives clean** — `week-log.md` lands with its gem served to
  `volcano.md` at z +2.238 and the note itself cut on both loner anchors it used to top (+1.655,
  +1.919, both under the gate), the geometry read correctly in **both** directions at once. The
  per-mate metric moved exactly as designed: 0.40/0.93/**0.633** shipped (n = 15), with the Δ to
  the unfloored arm now pure suppression (−0.017 = the one suppressed mate) since the two orders
  agree by construction. What remains suppressed is `phishing.md` at +1.253, under three
  strangers — the pair-level residue in the judge unit, its rescue priced by the trade curve, the
  pair-scorer's standing evidence. The instrument moved with the rule (the calibration block
  reads the judge unit, keeps the engine replay check, and adds a statistic recheck — the dump's
  z recomputed from the served scores; rows carry `"unit": "stage2-best-passage"`), and the
  reorder was priced on `fixtures/test-vault`: a floored `similar` went ~1.3 ms → ~7.5 ms per
  call (debug build), converging on the unfloored path's unchanged cost — still O(shortlist).

- **The buried gem is served, and the badge was the last thing still reading the retired unit**
  ([#182](https://github.com/AlteredCraft/B2/issues/182), closed 2026-08-17). The issue asked how
  to surface a note whose one matching passage is hidden behind a whole-note gist that ranks it
  down or cuts it; #192's reorder answered the engine half — of the four options the issue
  listed, three assumed a *card* to mark or reorder, and the measured failure was that the card
  did not exist. What the harness then caught was the half nobody had re-read: the desktop's
  strength band (`ui/src/strength.ts`) still carried the **centroid** z's landmarks
  (`●●● ≥ 3.0`, `●●○ ≥ 2.3`) after the unit under them changed. In the stage-2 unit nothing in
  the corpus reaches 3.0, so the top band was dead and the strongest human-confirmed relation in
  the vault (`bicycle.md ↔ bike-maintenance.md`, z +2.87) painted the same two dots as a middling
  one — a buried gem, once surfaced, arriving pre-graded as unremarkable. The bands were re-read
  in the judged unit off the floor's own bars and the labelled-mate population (`●○○` under the
  leader gate; `●●○` at or above it; `●●●` at or above the mate population's upper quartile, +2.529 → a bar of 2.52),
  and the comment now cites this harness rather than freezing the window — #187's lesson applied
  to the UI side of the same number.
- **Discovery rank is in the exit gate** ([#188](https://github.com/AlteredCraft/B2/issues/188),
  2026-08-17) — the two new assertions above, the strangers instrument beside them, and the
  saturating per-anchor line retired to JSON. The finding worth keeping: **this harness is
  bit-reproducible run to run** (five runs, identical rows — ranks, z's and cosines alike), and
  the figures #192 recorded came back unchanged on a different machine's CPU build, so "noise
  floor" here means corpus and label drift rather than run variance. That is what both new
  floors are sized for.

The deliberately open thread: the **phishing inversion** — a real relation the model ranks
under three stranger pairs even in the best-passage unit (+1.253 vs strangers to +1.367), the
one labelled mate #192's floor still cannot serve at an acceptable price. It is the standing
evidence for the pair-scorer escalation named in
[`index-engine.md §3`](../design/index-engine.md), promoted only if real-vault dogfooding
demands it. The journal-shape inversion that used to sit beside it was resolved by #192's
reorder — geometry the centroid unit could never survive, carried by the passage unit — which
also means the remaining residue really is pair-level, not shape-level.

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
   lands, and — since the gate reads discovery rank ([#188](https://github.com/AlteredCraft/B2/issues/188))
   — **a red gate is never an argument for editing a label**: per-mate MRR@5 and the strangers
   count both move when the labels move, so the only honest response to either going red is to
   argue about the *notes*: no existing query's content tokens may newly land in the edited/added note, and no new
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
