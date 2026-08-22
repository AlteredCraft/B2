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

> **Not the audience for this page?** [**Search & similarity — the explainer**](../search-and-similarity.html)
> is the plain-language tour of everything these metrics score — chunks, embeddings, hybrid search, the
> two-stage discovery engine, and the quality floor — written for people *using* B2 rather than measuring
> it, with a glossary and diagrams.

## The two halves, and why neither is enough alone

| Command | What it measures | Model | Deterministic |
|---|---|---|---|
| `just eval` | BM25 / vector-only / hybrid note & passage ranks, semantic lift, fusion demotions, discovery per-mate ranks on the always-served surface + the **dense fixture's zero-empty-panes and rank assertions** ([#197](https://github.com/AlteredCraft/B2/issues/197)), strangers, cosine piles, the z calibration dump, and the **search evidence calibration** — negative queries' BM25/cosine/served readings ([#201](https://github.com/AlteredCraft/B2/issues/201), Phase A) | real bge | no |
| `just eval-sweep` | the same, per `ChunkConfig` variant — the chunker A/B ([#44](https://github.com/AlteredCraft/B2/issues/44)'s gate, seven variants) | real bge | no |
| `just eval-stemmer` | the same, under the unstemmed `unicode61` ablation beside the shipped `porter unicode61` ([#157](https://github.com/AlteredCraft/B2/issues/157)'s instrument) | real bge | no |
| `just stability` | top-10 drift vs a blessed baseline as candidate pools widen ([#141](https://github.com/AlteredCraft/B2/issues/141)) | fake | yes |
| `just calibrate <vault>` | discovery calibration on **any built vault**, no labels: per-anchor pool cosines, leader z, what a replayed z gate would serve vs always-serve, strength-band histogram ([#196](https://github.com/AlteredCraft/B2/issues/196)/[#197](https://github.com/AlteredCraft/B2/issues/197) Phase 0a), and the replayed **mutual-k reciprocity fold** — the leading disclosure-boundary candidate, priced per anchor before anything ships ([#200](https://github.com/AlteredCraft/B2/issues/200), Phase A; `--mutual-k`) | stored vectors (pure read) | yes, per vault |
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
| [`crates/b2-embed/examples/calibrate.rs`](../../crates/b2-embed/examples/calibrate.rs) | the real-vault calibration instrument ([#197](https://github.com/AlteredCraft/B2/issues/197) Phase 0a): [#196](https://github.com/AlteredCraft/B2/issues/196)'s hand arithmetic as a command — per-anchor pool distributions, replayed-gate vs always-serve, bands; process rule 5's transfer check |
| [`crates/b2-embed/evals/corpus/`](../../crates/b2-embed/evals/corpus/) | the hand-written 31-note vault: topic clusters, six long multi-chunk notes, five unambiguous loners, the stemmer-adversarial block, the [#183](https://github.com/AlteredCraft/B2/issues/183) multi-topic family (four notes stitching an on-topic half to a genuinely unrelated one, the shape that makes centroid-vs-best-passage discovery ranking disagree), and — since [#192](https://github.com/AlteredCraft/B2/issues/192) landed [#189](https://github.com/AlteredCraft/B2/issues/189)'s note — `week-log.md`, the journal-shaped dilution extreme (seven unrelated sections, one lava-field gem) |
| [`crates/b2-embed/evals/queries.json`](../../crates/b2-embed/evals/queries.json) | retrieval labels — 41 positive queries (a verbatim `passage` adds chunk-level scoring, n=20) + 5 **negative queries**: empty `relevant` = the labelled answer is *no matches*, the query-side sibling of the negative anchors ([#201](https://github.com/AlteredCraft/B2/issues/201)); excluded from every rank aggregate |
| [`crates/b2-embed/evals/similar.json`](../../crates/b2-embed/evals/similar.json) | discovery labels — positive anchors with expected mates; **empty `expected` = a negative anchor** whose correct answer is *nothing* |
| [`crates/b2-embed/evals/corpus-dense/`](../../crates/b2-embed/evals/corpus-dense/) | the **dense single-domain fixture** ([#196](https://github.com/AlteredCraft/B2/issues/196)/[#197](https://github.com/AlteredCraft/B2/issues/197) Phase 0b): fifteen beekeeping notes, all genuinely inter-related, **no loner** — the vault-level geometry the orthogonal corpus is structurally incapable of expressing; scored in its own throwaway vault, its own `results.jsonl` row (`"corpus": "dense"`), never averaged with the orthogonal rows |
| [`crates/b2-embed/evals/similar-dense.json`](../../crates/b2-embed/evals/similar-dense.json) | the dense fixture's labels — **rankings only** (expected mates per anchor, per-mate scored); no negative anchors, because in this corpus "nothing relates" is false of every note |
| `crates/b2-embed/evals/results.jsonl` | append-only run log (gitignored, local) — every number ever cited traces to a row here |
| [`../../justfile`](../../justfile) | the `model` group holds every recipe above |

## What the exit code enforces

`just eval` exits `0` only when the default config clears **all the assertions**
([`eval.rs`](../../crates/b2-embed/examples/eval.rs), the gate at the end of `run()`) — the set
re-derived by [#197](https://github.com/AlteredCraft/B2/issues/197) for the always-served surface:

| Assertion | Constant | Direction | Watches |
|---|---|---|---|
| hybrid note hit@1 ≥ 0.75 | `FLOOR_HIT1` | floor, **below** the 0.95 reading | retrieval (untouched by #197) |
| per-mate discovery MRR@5 ≥ 0.52 | `FLOOR_MATE_MRR` | floor, **below** the 0.650 reading | discovery **rank**, orthogonal corpus |
| labelled mates suppressed = 0 | `MAX_MATES_SUPPRESSED` | ceiling, **at** the structural 0 | the **tripwire**: nonzero means an existence gate is back in the path |
| dense fixture: zero empty panes | — | absolute | [#197](https://github.com/AlteredCraft/B2/issues/197)'s ruling made mechanical |
| dense fixture: per-mate MRR@5 ≥ 0.32 | `FLOOR_DENSE_MATE_MRR` | floor, **below** the 0.467 reading | discovery **rank** on the single-domain geometry ([#196](https://github.com/AlteredCraft/B2/issues/196)) |

Exit `2` = an assertion failed; `1` = the run itself broke. Two instrument checks print before
any score and gate everything after them: the model id (never average CPU and `@metal` rows) and
`batch ≡ single` embedding faithfulness.

**How the gates are placed is the point.** A rank floor never sits *at* the reading it was set
from: a gate pinned to today's number fails on the first legitimate corpus edit, and the cheapest
way to clear a red per-mate number is to **edit a label**, which is the one habit this harness
must never train (process rule 2's concern, sharper here: per-mate is label-sensitive by
construction — adding a mate to an anchor changes `n` and moves the aggregate whether or not the
engine did anything; and on the dense fixture, where everything relates, relabelling toward the
model's order would *always* look plausible). Sizing is measured, per
[#188](https://github.com/AlteredCraft/B2/issues/188)'s method re-run for the #197 readings:
**five consecutive always-serve runs on an unchanged corpus, model and build produce
bit-identical rows** — every rank, every z, every cosine, on both corpora — so the run-to-run
noise floor is *zero* and the headroom is for corpus drift. Each MRR floor sits about two
lost-from-rank-1 mates under its reading (orthogonal: 0.650 − 2/15 → 0.52; dense: 0.467 − 2/14 →
0.32; the orthogonal reading moved 0.633 → 0.650 when the gate retired — exactly the returned
phishing mate at rank 4 — and the floor moved with it, by the same method that set it; the dense
reading moved 0.502 → 0.467 when a one-word grammar fix in `hive-inspection.md` slid one mate a
rank — the worked example of the corpus drift these margins exist for, and of why a floor never
sits at its reading). The two
exceptions run the other way on purpose: **suppression is asserted at zero with no headroom**,
because under always-serve both discovery passes read one surface and nothing can be
reachable-but-unserved — the assertion is no longer a budget but a tripwire, and the only event
that can trip it is a Phase-2 existence signal suppressing a labelled mate again, which is
exactly what must never ship unmeasured twice; and the **dense pane assertion is absolute**,
because a vault where everything relates may never read as "nothing relates".

**What retired**: the negatives' suppression assertion (`neg_clean == neg_n`,
[#150](https://github.com/AlteredCraft/B2/issues/150)). Under always-serve a loner anchor serves
its ranked nearest — that is the ruling, not a regression — so the assertion would have pinned
the retired behavior. The five negative anchors stay labelled: their cards' *bands* are the
readout now (#197's A2 — as of the re-derivation run, every negative leader paints the weakest
band, `●○○`, z +1.458 … +1.919, all under the 1.96 landmark), printed per leader in the
calibration block and recorded in `discovery_z`.

The **saturating per-anchor metric** (`similar` hit@1/hit@3/MRR@5 — first mate found, then stop)
is no longer printed: it read 1.000 across every change it was meant to judge, and a line that
cannot move trains skimming. It is still recorded — `results.jsonl`'s `"similar"` key is
untouched, so rows stay comparable back to the first run. The sweep's per-variant column carries
per-mate MRR@5 for the same reason, and its `neg clean` column — 0/5 forever under always-serve —
was replaced by the strangers count, which can move.

**What the gate deliberately does *not* watch** is the strangers count: unlabelled notes served
on positive anchors at the ranks' own depth, each named `anchor → path`, recorded as
`similar_strangers`. It reads **15 cards on 6/6 positive anchors** under always-serve (it read 0
under the retired floor — that difference *is* the visible cost of serving the ranked list, and
[#197](https://github.com/AlteredCraft/B2/issues/197) ruled it worth paying: the labels are not
exhaustive, and a served unlabelled note is not proof of junk). Gating it would be a trap — a
ceiling at today's value forbids the next corpus edit from surfacing anything unlabelled, and the
cheapest way to shrink the count is to *label the stranger*, which moves the per-mate metric too.
It is a smoke alarm shipped with the list you argue against it with (process rule 1's posture).
The z dump is reported the same way: every run prints a `discovery z calibration` block — the
populations an existence bar *would* answer to, the re-derived would-be window for each (the
member window is **empty** — inverted — on the corpus's own numbers,
[#187](https://github.com/AlteredCraft/B2/issues/187); the leader window is open on this corpus
and was measured failing on a real vault, [#196](https://github.com/AlteredCraft/B2/issues/196) —
which is why openness on one corpus proves nothing without process rule 5's transfer check), and
the negatives' band readout — recorded in the row as `discovery_z`. That block is the first
reading any Phase-2 bake-off candidate answers to; `just calibrate` on real vaults is the second.

The **fold bake-off** ([#200](https://github.com/AlteredCraft/B2/issues/200), Phase B) is
reported the same way, and for the standing reason: nothing folds today, so the exit gate has
nothing to watch. Its judged quantity — **labelled mates served within `limit` but below the
fold** — reads a structural 0 under always-serve exactly as suppression does, and it is kept
printed rather than gated because the block's job is to price the *next* candidate rule, not to
re-assert the incumbent. If a fold ever ships, that number and the swept window are what the
gate takes ([#202](https://github.com/AlteredCraft/B2/issues/202) moves the assertions in the
same change as the surfaces, per #182's rule). Recorded as `discovery_fold` in both corpora's
rows, carrying the per-anchor folds, the whole `k` sweep, and each candidate's `recip_rank` —
the anchor's rank in *its* list — so any depth is re-derivable from a row without re-running the
model.

The **search evidence calibration** ([#201](https://github.com/AlteredCraft/B2/issues/201),
Phase A) is reported the same way — per labelled query, the OR-sanitized BM25 hit count and best
score, the dense top-1 cosine, and what always-serve serves today, positives and negatives apart,
with the would-be pure-cosine query window re-derived each run (its caveat printed with it: D2's
rule is lexical OR semantic evidence, so that window overstates the keep set) — recorded as
`search_evidence` in the row, gating nothing: the query-level bar is #201's to earn, and it owes
process rule 5's transfer check like any distributional constant.

## The verdicts this harness has ruled (each traceable to its issue and its commit)

- **`porter unicode61` FTS stemming** (schema v5, [#157](https://github.com/AlteredCraft/B2/issues/157)):
  7–0 BM25 / 3–0 hybrid on the paired win/loss readout, precision probes unmoved. The retired
  `unicode61` arm stays measurable via `just eval-stemmer`.
- **`ChunkConfig::default()` held** ([#44](https://github.com/AlteredCraft/B2/issues/44)): a
  seven-variant sweep; the scoreboard's best rows were impeached (512-token truncation, a
  measured ±3–4-of-20 boundary-luck noise floor), and the default kept. Retrial is one
  `just eval-sweep` away.
- **The discovery quality floor** ([#150](https://github.com/AlteredCraft/B2/issues/150);
  **superseded by [#197](https://github.com/AlteredCraft/B2/issues/197)**, below): per-anchor
  z-scores over the stage-1 centroid population — model-relative by construction, calibrated from
  every labelled anchor's full ranked list, transfer-checked on a 228-essay single-author vault.
  Suppression went 0/4 → 4/4 clean and entered the exit gate. (#197's review later found that
  transfer check was read through the assumption it should have been testing: keeping 99–100% of
  a single-author vault's candidates was plausibly the *correct* answer, recorded as evidence
  against absolute floors.)
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

- **The existence gate itself was the defect, and it retired — discovery serves the ranked list**
  ([#196](https://github.com/AlteredCraft/B2/issues/196) measured,
  [#197](https://github.com/AlteredCraft/B2/issues/197) ruled, 2026-08-18; invariants.md **D1**).
  The first real vault dogfooded against the #192 constants was single-domain — 17 notes, three
  same-subject articles deliberately related — and the leader gate emptied **16 of 17** panes
  while the ranking underneath was correct throughout (the articles were each other's top
  candidates at cosine ~0.79). The z rule is a single-population outlier test, valid only when
  related notes are rare outliers in a dominant unrelated tail; a single-domain vault has no such
  tail, so the rule reads *everything is related* as *nothing is*. The member bar was **deleted**
  (its admissible window had been empty on the corpus's own numbers since
  [#187](https://github.com/AlteredCraft/B2/issues/187)), the leader gate retired from the
  default path, and the harness moved with the rule: this README's exit-gate table above is the
  re-derivation (per-mate 0.633 → 0.650 = the returned phishing mate at rank 4; suppression a
  structural-zero tripwire; the negatives' assertion retired for their band readout — all five
  loner leaders paint `●○○`; strangers 0 → 15 cards, the priced and accepted cost). Two
  instruments landed **before** the fix, per the sequencing rule: `just calibrate` (0a — #196's
  hand arithmetic as a command, and process rule 5's mechanism) and the dense single-domain
  fixture (0b — its zero-empty-panes assertion is the ruling made mechanical, and its first
  reading, per-mate MRR@5 0.502 at n = 14, priced its own floor). Whether any existence signal
  returns is Phase 2's evidence-gated bake-off — mutual-kNN/reciprocal-rank leading, "no gate at
  all" admissible — with continuity in population size an entry requirement (the n = 12
  statistics threshold now moves banding only, never membership).

- **Result count is evidence, not layout — the disclosure work opened, instruments first**
  ([#200](https://github.com/AlteredCraft/B2/issues/200)/[#201](https://github.com/AlteredCraft/B2/issues/201)/[#202](https://github.com/AlteredCraft/B2/issues/202),
  2026-08-22; invariants.md **D1 redrafted, D2 added**). Real-vault dogfooding measured
  always-serve's cost: a pane that always fills to `limit` trains distrust of every card, and a
  nonsense search query serves 10 confident-looking results — zero being unrepresentable in
  Flow ② as built (the vector half always has k nearest; RRF keeps only ranks). D1 now splits
  ranking / reachability / **default disclosure**: an evidence-gated *prefix fold* may set what
  the default view vouches for, everything below it stays served and one gesture away, and
  reachability is untouchable; D2 makes a served search result a claim of evidence (`limit` is a
  quota nowhere). Phase A landed the instruments before any rule, per the #196/#197 sequencing
  precedent: five negative queries + the search evidence calibration in `just eval`, and the
  mutual-k reciprocity fold replay in `just calibrate` (per-anchor `fold_serves` beside the
  replayed gate and always-serve, per-candidate `reciprocal` in the JSON). Nothing gates yet —
  the bake-offs are #200 (discovery fold; "no fold at all" admissible) and #201 (search's
  query-level bar); #202 lands the winners on every surface and moves the exit-gate assertions
  in the same change.

- **The discovery fold bake-off ran, and no fold ships — mutual-k's admissible window is empty on
  both corpora** ([#200](https://github.com/AlteredCraft/B2/issues/200), measured and ruled
  2026-08-22; invariants.md **D1**). The run added a `discovery fold bake-off` block to both
  corpora's reports: every candidate disclosure rule judged on the *same* served lists, `k` swept
  1..15, and — the #187 idiom moved onto the disclosure axis — the rule's admissible window
  **re-derived each run** rather than quoted. What it reads:

  | | orthogonal (31 notes, median 30-candidate pool) | dense (15 notes, median 14-candidate pool) |
  |---|---|---|
  | smallest `k` folding **no** labelled mate, darkening **no** pane | **14** (= 0.47 of the pool) | **7** (= 0.50 of the pool) |
  | largest `k` emptying **every** loner's default view | **none at any depth** (4 of 5 at `k ≤ 11`) | — (no loner by construction) |
  | `k` at which the fold equals always-serve | — | **13** |

  So the two ends of the one knob never meet. At the depths where the fold still buys what it
  exists to buy — `k ≤ 11`, four of five loners folding to an honest empty — it hides between
  **1 and 13** of the corpus's 15 labelled mates, each named in the block: one even at its most
  generous (`k = 11`), three at `k = 9–10` (`insomnia → radio-and-sleep-debt`,
  `photosynthesis → houseplant-care`, `bicycle → tire-pressure-and-knots`), eleven at the pane's
  own depth `k = 5`. And on the dense fixture every `k ≤ 5` darkens 1–3 of 15 panes, which is
  D1's absolute and disqualification rather than a tuning note. **The decisive finding is the
  pair of safe depths**: 14 and 7 are the same *fraction* of their pools and different
  *constants*, so no constant `k` transfers between two corpora sixteen notes apart, let alone to
  a 10k-note vault — and a *fraction* cannot ship either, because a candidate's own list is
  itself truncated by the recall shortlist (`discover::SHORTLIST_MIN = 200`), so "the anchor is
  in this candidate's nearer half" stops being computable past a few hundred notes. Rank-based
  bought exactly what it promised (no cosine or z constant to transfer-check) and not the thing
  that mattered: the constant is a rank *in a population*, and the population is what changes.
  A third reading nails it — `just calibrate` replaying the same fold at 200-note scale
  (`fixtures/test-vault`, a median 180-candidate pool; a **geometry** bench, not a quality one,
  since its notes and links are procedurally generated): **`k = 10` discloses 36% of the cards on
  the orthogonal corpus, 91% on the dense fixture, and 98% here** (189 of 200 anchors showing
  their whole view — the rule gone vacuous), while `k = 5` darkens 7 of 200 panes on a vault where
  every note has nineteen same-topic siblings. One constant, three vaults, three different rules.
  What the candidate **did** prove is kept as evidence rather than thrown away — at `k ≤ 11` the
  loners fold empty 4 of 5 while (from `k ≥ 7`) every dense pane stays lit, which is the
  loner-versus-dense discrimination [#196](https://github.com/AlteredCraft/B2/issues/196) proved
  no *anchor-local* statistic can make. A pair-level signal can see it; this particular one
  cannot be aimed. Candidate 2, the **authored-edge reference bar**, is *unpriceable here and
  undecided*: its calibration population is the human's own committed edges and both corpora
  carry **zero** (measured and recorded per run as `authored_edges`, not assumed — the
  process-rule-2 token audit that keeps them orthogonal leaves nothing to link), and being a
  distributional constant it owes process rule 5's transfer check anyway. Its bench is a real
  vault with human-authored edges, judged by its owner. The one *mechanism* reading available —
  `just calibrate` on `fixtures/test-vault`, whose 1748 generated links are a shape, not a
  judgement — says the quantile choice is the whole rule: the authored population's cosines run
  0.475 / **0.628** / 0.901 / 0.979 (min / q1 / median / max) while the pane's top-5 sit at 0.94+,
  so a lower-quartile bar folds **nothing at all** (1000 of 1000 cards above it) and a median one
  would cut deep. That is the transfer question in one line, and no honest population in this
  repository can answer it. **The fourth bench — two or more real vaults, each judged against
  named per-anchor lists by the person whose vault it is — was not run**: this ruling therefore
  disqualifies candidate 1 on the benches that *can* disqualify it (a rule that darkens a dense
  vault's panes is inadmissible wherever it is measured) and leaves candidate 2 open rather than
  claiming it lost. The incumbent — **no fold** — clears
  every bench it can be judged on, and stays; the dogfood complaint that opened #200 is
  therefore still unpaid, and the honesty still rides on the band and the copy.

The deliberately open thread: the **phishing pair** — a real relation the model ranks under
three stranger pairs even in the best-passage unit (+1.253 vs strangers to +1.367). Under
always-serve it is *served*, at best-passage rank 4, so the residue is **ordering quality rather
than existence** — still the standing evidence for the pair-scorer escalation named in
[`index-engine.md §3`](../design/index-engine.md), promoted only if real-vault dogfooding
demands it. The journal-shape inversion that used to sit beside it was resolved by #192's
reorder; the single-domain inversion beside *that* by #197's ruling. Beside it now sits the
**dense-vault band compression** (#197's A6): on a vault where every candidate is close, the
within-list z compresses and the dots lose resolution — first reading taken on a real-embedded
build of the dense fixture itself (`just calibrate` on a vault built from `corpus-dense/`):
**0 ●●● / 6 ●●○ / 144 ●○○** across fifteen top-10 lists, leaders +1.16 … +2.43, with the
replayed retired gate darkening 9 of 15 anchors and serving 7 of 150 candidates on the same
vault. #200 was to decide the re-reference *if reciprocity shipped* (#182's rule: a change to the
judged statistic is a change to every surface that paints it) — it did not, so the judged
statistic did not move and the bands are correct as they stand. The compression itself is
unfixed and now has no mechanism riding on it: it stays open on its own.

## Running it

```console
just init             # provision bge-base-en-v1.5 (one time)
just eval             # ~1min warm (both corpora); appends rows, exits non-zero on a gate regression
just eval-sweep       # + the seven-variant chunker A/B
just stability        # model-free, deterministic; `just stability-bless` only after an INTENDED change
just calibrate ~/notes   # the real-vault transfer check (process rule 5) — any built vault, no labels
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
5. **A constant derived from a corpus's score *distribution* is invalid until transfer-checked on
   a real vault** (`just calibrate` is the check; adopted with
   [#197](https://github.com/AlteredCraft/B2/issues/197) — rule 3's dogfooding clause made
   mechanical, from a measured mistake made twice). Rank-derived readings transfer because the
   corpus's *orderings* are engineered to be checkable; its score **distributions** are an
   artifact of engineered orthogonality, so any threshold read off them — a cosine bar, a z
   window, a band landmark — describes the corpus, not a vault. The #150 floor was calibrated
   this way and survived its one transfer check only because the result was read through the
   assumption under test; the #192 re-derivation shipped with no real-vault check at all and
   failed on the first one it met (#196). A distributional constant now ships only with a
   `calibrate` reading from at least one real vault beside the corpus numbers.
