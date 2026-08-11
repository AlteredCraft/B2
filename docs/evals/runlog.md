# Eval run log

The lab notebook for B2's out-of-CI eval harness. [`results.jsonl`](../../crates/b2-embed/evals/)
keeps every *number*; this file keeps every *decision* — what we changed, why, and which recorded
rows settled it. Entries are append-only, newest last, and each names the `results.jsonl` row it
came from so a claim here can always be traced back to raw data.

**If you are new to this harness, read [Orientation](#orientation) first.** If you just want the
history, skip to [the log](#the-log).

---

## Orientation

### Why an eval harness exists at all

B2's job is to surface *semantically* similar notes — the one part of the system whose correctness
cannot be asserted with a unit test. A test can prove `reindex` is idempotent; nothing in
`cargo test` can prove that "how do leaves turn light into food" should rank `photosynthesis.md`
first. That judgement is a human's, so it lives in hand-written label files, and the harness scores
the engine against them.

This is deliberately **outside CI**. The repo's standing rule is that `cargo test` stays fast,
deterministic, and model-free ([CLAUDE.md](../../CLAUDE.md), "Conventions"), so the real
[bge-base-en-v1.5](https://huggingface.co/BAAI/bge-base-en-v1.5) model never enters the test suite —
model quality can never flake CI. The harness runs on demand instead.

The design authority for everything scored here is [`docs/design/index-engine.md`](../design/index-engine.md)
(the *how*), with [`docs/design/invariants.md`](../design/invariants.md) winning any conflict.

### The two halves, and why neither is enough alone

| Command | What it measures | Model | Deterministic |
|---|---|---|---|
| [`just eval`](../../justfile) | BM25-vs-hybrid lift, note & passage ranks, discovery ranks + suppression, cosine piles | real bge | no |
| `just eval-sweep` | the same scores per `ChunkConfig` variant — the [#44](https://github.com/AlteredCraft/B2/issues/44) A/B | real bge | no |
| `just eval-stemmer` | the same scores under the unstemmed `unicode61` ablation beside the shipped `porter unicode61` — the [#157](https://github.com/AlteredCraft/B2/issues/157) instrument | real bge | no |
| `just stability` | top-10 drift vs a blessed baseline as candidate pools widen | fake | yes |
| `just eval-metal` | `just eval` on the Apple-Silicon GPU (model id gains `@metal`) | real bge | no |

`just eval` scores **quality** — it can say *better*. `just stability` scores **movement** — it can
only say *different*, but it can see changes the labelled corpus is structurally blind to.

The blindness is specific and worth understanding, because it is the trap this whole harness is
shaped around. Retrieval reaches a fixed pool of candidates per signal (60 for the passage view, 150
for the note view). If the corpus has fewer chunks than the pool, **no list is ever truncated**, so a
change to how *wide* the search casts cannot alter a single number — the eval prints bit-identical
output and looks like proof of "no change" when it is really proof of "cannot see". That is
[#141](https://github.com/AlteredCraft/B2/issues/141); `just eval` now prints a `[warn] … ≤
60-candidate pool` line whenever it applies, and records `pool_blind: true` in the row.

The worked example is on record: [#140](https://github.com/AlteredCraft/B2/issues/140) widened the
passage view to 3×, the eval saw nothing at all, the stability probe saw 10 of 10 probes' top-4
passages change — and [#142](https://github.com/AlteredCraft/B2/issues/142) reverted it, because a
probe can say *different* but only a labelled corpus can say *better*. Run both.

(One knob is exempt from the blindness: `RRF_K` re-weights the same lists rather than changing their
width, so even a small corpus does see it.)

### The files

| File | Role |
|---|---|
| [`crates/b2-embed/evals/corpus/`](../../crates/b2-embed/evals/corpus/) | the throwaway vault the eval builds fresh each run — topic clusters (coffee, sleep, plants, security, geology, cycling), deliberate loners, and the stemmer-adversarial block (#157: the universe/university Porter-collision pair + a code-fenced git note) |
| `evals/queries.json` | retrieval labels: query → relevant note(s); an optional verbatim `passage` turns on chunk-level scoring |
| `evals/similar.json` | discovery labels: anchor → cluster-mates; **empty `expected` = negative anchor** (a loner whose correct answer is "nothing") |
| `evals/results.jsonl` | append-only run log (gitignored) — one JSON line per scored config |
| `evals/stability-baseline.json` | the blessed top-10 snapshot the probe diffs against (committed) |

There is no build step: the eval copies the corpus into a fresh tempdir vault every run, so editing
a label or a note and re-running is the entire loop (~5s warm).

### How to read a run

Two lines print *before* any score, and both are gates rather than decoration:

- `[eval] model = BAAI/bge-base-en-v1.5 (dim 768)` — confirm the model id. On a Metal build it ends
  in `@metal`, which is a **different vector space**; rows logged under it must never be averaged
  with CPU rows. (Device is folded into model identity on purpose — see
  [#40](https://github.com/AlteredCraft/B2/issues/40) and `b2-embed/src/model.rs`.)
- `[eval] batch ≡ single: worst-row cosine 0.9999…` — the batching-faithfulness check. If this
  fails, every number after it is meaningless. This is the worked example of the repo's rule that a
  check needing the real model belongs in the eval harness, never in `cargo test` wearing an
  `#[ignore]`.

Then the per-query table, with four rank columns — `✓1` is a first-place hit, `·3` is rank 3,
`✗>10` is a miss beyond K=10:

```text
 bm25    vec hybrid  chunk  query                                     top hybrid hit
   ·2     ✓1     ✓1         how do leaves turn light into food        photosynthesis.md
 ✗>10     ✓1     ✓1     ·2  steeping coarse grounds then pushing …    french-press.md
```

A query BM25 misses but hybrid nails is the entire point of the AI seam. The aggregate of that is
the **semantic lift** line. The `vec` column (added 2026-08-10) is the **dense ablation** —
`Vault::search_vector_only`, the vector signal alone — and the `fusion` line beneath the aggregates
names every query hybrid ranks *worse* than that ablation would: RRF's consensus bias, counted and
named on every run instead of rediscovered by decomposing scores by hand
([#158](https://github.com/AlteredCraft/B2/issues/158)).

**Metric glossary.** `hit@1` = fraction of queries whose labelled answer ranked first. `hit@3` = …
ranked in the top three. `MRR@10` = mean reciprocal rank — average of `1/rank`, so rank 1 scores
1.0, rank 2 scores 0.5, a miss scores 0. MRR is the sensitive one: it moves when a result shifts
from rank 4 to rank 2, where hit@1 and hit@3 both stay flat.

**Exit codes.** `0` = ran and cleared the reference floor · `2` = ran but hybrid **note** hit@1 fell
below the soft floor of `FLOOR_HIT1 = 0.75` · `1` = the run itself failed. The discovery suppression
metric is deliberately *not* in the gate yet (see below).

### Red by design

The **negatives** line reads `0/N clean` on every run today, and that is not a regression. Discovery
currently truncates to `limit` with no quality floor: ask for 10 candidates and you get 10, even when
the honest answer is "nothing relates". The ruling that `limit` is a cap and not a promise is already
written in [`index-engine.md §3`](../design/index-engine.md); what is missing is the calibrated
cutoff. That is [#150](https://github.com/AlteredCraft/B2/issues/150). Until it ships, the metric is
a failing target the floor is built against — fix the engine, not the eval.

### Process rules

Adopted 2026-08-10, after the review below; each traces to a measured mistake or a named risk.

1. **A paired per-query win/loss list is the primary readout of any A/B; the aggregate is a smoke
   alarm.** At n≈40, every aggregate point is 1–2 queries — "hit@1 +0.05" and "these two flipped"
   are the same fact, but only the second can be argued with against the labels. The sweep prints
   the diff (`Δ vs default`) automatically.
2. **The corpus is frozen except through this file.** Every corpus edit gets a runlog entry, and
   every edit runs the **two-direction token audit** before it lands: no existing query's content
   tokens may newly land in the edited/added note, and no new query's content tokens may split
   evenly toward a rival (the `insomnia.md` steal, and the `recover`+`mistake` near-miss, are the
   precedents). The audit is a ten-line script; run it, don't eyeball it.
3. **The same person authoring notes, queries, and fixes is a ratchet toward measuring what the
   engine already does.** Mitigations in order of cheapness: rule 2's audit; sourcing future
   queries from outside the corpus author's head (from note titles alone, or another person);
   dogfooding on a real vault before trusting any threshold.
4. **A bit-identical or unmoved metric is a claim to verify, never proof of "no effect"** — compare
   a continuous quantity (the piles) before believing a discrete one. (Standing rule from the
   `prepend-heading-path` trace; the sweep diff now prints its own reminder.)

### Background reading

- **BM25** — the lexical half, via [SQLite FTS5](https://www.sqlite.org/fts5.html)'s built-in
  ranking. Matches words, not meaning.
- **Reciprocal Rank Fusion (k=60)** — how the lexical and vector rankings are combined without
  needing their scores to be commensurable ([Cormack et al., 2009](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf)).
- **bge-base-en-v1.5** — the embedding model ([card](https://huggingface.co/BAAI/bge-base-en-v1.5)).
  Asymmetric: queries get an instruction prefix, documents do not.
- **qmd chunker** — the Markdown-structure-aware splitter, named after
  [tobi/qmd](https://github.com/tobi/qmd). Implementation: `b2-core/src/chunk.rs`.
- **Flow ② hybrid search / Flow ③ discovery** — [`index-engine.md`](../design/index-engine.md) and
  the Architecture section of [CLAUDE.md](../../CLAUDE.md).

---

## The log

## 2026-08-10 — Baseline, and why the #44 gate is currently blind

**Run:** `just eval` · git [`33c2f8a`](https://github.com/AlteredCraft/B2/commit/33c2f8a) ·
`BAAI/bge-base-en-v1.5` (CPU, dim 768) · config `default` · `results.jsonl` row 6

First session working [the eval guide](https://github.com/AlteredCraft/B2/issues/152) end to end.
Goal for the session: establish a trustworthy baseline, then start
[#44](https://github.com/AlteredCraft/B2/issues/44) (the chunker gate).

### Step 1 — both pre-score gates clear

- Model id is the bare `BAAI/bge-base-en-v1.5`, **not** `@metal`. CPU vector space.
- `batch ≡ single: worst-row cosine 1.000000` — exact, so batching is faithful and the scores below
  are meaningful.

### Step 2 — the baseline numbers

| metric | bm25-only | hybrid |
|---|---|---|
| note hit@1 (n=25) | 0.72 | **0.84** (semantic lift +0.12) |
| note hit@3 | 0.84 | 0.88 |
| note MRR@10 | 0.778 | 0.876 |
| chunk hit@1 (n=6) | 0.33 | **0.33** |
| chunk hit@3 | 0.50 | 1.00 |
| chunk MRR@10 | 0.492 | 0.583 |

Discovery: positive anchors hit@1 = 1.00 (n=6) — the labelled cluster-mate is always first. Negatives
**0/3 clean**, 15 stranger cards surfaced where the label says "nothing" — red by design, as above.

Cosine piles (the raw material for #150's cutoff): related n=11 spanning [0.525 … 0.650], junk n=34
spanning [0.456 … **0.684**]. So `related-min − junk-max = −0.159` — **the piles overlap**, and a
single absolute floor would cut good candidates and keep bad ones. Note the shape of that overlap:
it is driven by the junk pile's *maximum*, the watercolor ↔ stain-removal pair, which scores above
every genuinely related pair in the corpus. Two loners that happen to share vocabulary about pigment
and fabric.

`pool_blind: true` — 29 chunks against a 60-candidate pool. Anything about candidate width is
invisible in this run by construction.

### Verified: the four note-level misses are real, not an instrument fault

The "top hybrid hit" column looked implausible enough to check before building anything on it —
`keeping messages secret from eavesdroppers` returning `sleep-hygiene.md` at rank 1 is not behaviour
you would expect from bge. Two candidate explanations, both ruled out by inspection:

1. **A mislabelled or off-by-one column.** No — it is genuinely `results.first().path` of the hybrid
   pass (`crates/b2-embed/examples/eval.rs:387`).
2. **The missing bge query prefix.** bge-v1.5 is an *asymmetric* model: queries are supposed to be
   embedded with `"Represent this sentence for searching relevant passages: "` prepended, and
   omitting it measurably degrades retrieval. This is a classic integration bug, so worth confirming
   in two places — the prefix must exist *and* be applied on the search path. It is defined at
   `crates/b2-embed/src/config.rs:83` and applied at `crates/b2-core/src/search.rs:152`, which calls
   `embed_query`, not `embed`. Correct.

So the misses are genuine model behaviour on a small corpus, and the harness reads true. The four:

| query | hybrid rank | top hit instead |
|---|---|---|
| `pedalling a bike up a steep hill` | ·7 | `french-press.md` |
| `a mountain that erupts and spews magma` | ·4 | `glaciers.md` |
| `keeping messages secret from eavesdroppers` | ✗>10 | `sleep-hygiene.md` |
| `my potted fern's leaves are going brown indoors` | ·2 | `photosynthesis.md` |

Worth revisiting later: three of the four target notes (`bicycle.md` 245 B, `volcano.md` 252 B,
`encryption.md` 285 B) are among the shortest in the corpus. A note with almost no text gives the
embedder almost no signal, which is a plausible common cause — and it is fixed by the same corpus
growth #44 needs.

### Finding: only 3 of 23 corpus notes are chunkable

This is the whole of #44's "the gate can lie to you" warning, reduced to one arithmetic step.

`ChunkConfig::default()` is `target_tokens: 450` × `chars_per_token: 4.0` = **1800 target chars per
chunk** (`crates/b2-core/src/chunk.rs:134`). Against the corpus:

| notes | bytes each | chunks |
|---|---|---|
| 20 notes (`espresso.md` … `watercolor-painting.md`) | 238–643 | 1 each = 20 |
| `personal-finance.md` | 3324 | ~3 |
| `backpacking-gear.md` | 3874 | ~3 |
| `fermentation.md` | 3945 | ~3 |
| | | **29** ✓ matches the run exactly |

Twenty notes sit an order of magnitude below a single chunk's target. **A chunker change is therefore
a no-op on 87% of the corpus by construction** — no value of `target_tokens`, `overlap_frac`, or
`prepend_heading_path` can split a 300-byte note into anything other than one chunk. The three long
notes are also precisely the three carrying all six passage labels, which is why chunk-level `n` is
pinned at 6.

That reframes Step 3A. The task is not primarily "add more queries" — queries cannot reach structure
that does not exist. It is **add corpus notes long enough to be multi-chunk**: >1800 chars, ideally
2–4 chunks so that *where the boundary falls* is itself under test, carrying the three structures
#44 names:

- content buried in a **table** with no heading nearby (qmd never cuts mid-table),
- a subsection with **no heading label** at all,
- a paraphrase that only resolves at the right **deep section** of a long note (where the heading
  breadcrumb, and so `prepend_heading_path`, is what makes it findable).

Queries follow the notes. A side benefit the guide calls out: growing the corpus past ~60 chunks
makes the passage view's candidate pool actually bind, which narrows the `pool_blind` warning and
gives #150 more pile data for free.

**Status:** baseline recorded and trusted. Corpus growth is the next action.

---

## 2026-08-10 — Step 3A/3B: corpus growth, and the gate opens its eyes

**Runs:** `just eval` then `just eval-sweep` · git `33c2f8a` (working tree) ·
`BAAI/bge-base-en-v1.5` (CPU) · `results.jsonl` rows 7–10

### What changed, and the one design decision behind it

Six notes — one per topic cluster — were expanded from ~450 bytes to ~5,500–6,400 bytes each:
`coffee-roasting.md`, `encryption.md`, `earthquakes.md`, `bike-maintenance.md`, `sleep-hygiene.md`,
`houseplant-care.md`. Each now carries the three structures #44 names: a data table sitting several
paragraphs below its heading, at least one subsection with no heading of its own, and a deep section
reachable only by paraphrase. Twelve new passage-labelled queries followed, two per note, one aimed
at the table and one at the unlabelled or deep material.

**Decision: expand existing notes rather than add new ones.** Adding notes would have been easier to
write, and it would have quietly corrupted #150. A brand-new coffee note becomes a *candidate* for
`espresso.md`'s `similar` call, and because `similar.json`'s `expected` list would not name it, the
eval would score it **junk** — inflating the junk pile with pairs a human would call obviously
related, on exactly the distribution #150 reads its cutoff from. Expanding keeps the corpus at 23
notes, keeps the candidate set identical, and leaves every existing label valid.

Two smaller rules were adopted while writing the labels, both worth keeping:

- **A passage must not duplicate a heading.** `prepend_heading_path` injects the breadcrumb into
  chunk text, so a passage that matches a heading would start matching chunks for a reason that has
  nothing to do with retrieval — a confound *inside* the very A/B the passages exist to measure. All
  twelve were checked against every heading in the corpus, and against each other for uniqueness.
- **Do not let a new note reach for another note's labelled query.** Sleep-stage and dream material
  was deliberately kept out of the expanded `sleep-hygiene.md` because `dreaming.md` owns
  *"why sleeping minds play vivid stories"*, and caffeine material was kept out because `insomnia.md`
  owns *"trouble sleeping because of caffeine"*. Both held: those two queries still rank ✓1.

Corpus: 19,501 → 52,557 bytes. Chunks: **29 → 50** at the default config.

### Result: the corpus grew the gate's resolution

| metric | baseline (29 chunks) | now (50 chunks) |
|---|---|---|
| note n | 25 | **37** |
| note hit@1 (hybrid) | 0.84 | **0.89** |
| note MRR@10 (hybrid) | 0.876 | **0.926** |
| note hit@1 (bm25) | 0.72 | 0.81 |
| semantic lift | +0.12 | +0.08 |
| **chunk n** | **6** | **18** |
| chunk hit@1 (hybrid) | 0.33 | **0.67** |
| chunk MRR@10 (hybrid) | 0.583 | **0.786** |

The chunk-level `n` tripling is the headline, and it matters more than the score. At n=6 the metric
could only move in steps of 0.167, so it was too coarse to register anything subtle; at n=18 the step
is 0.056. That is the difference between a gate and a coin-flip.

The **semantic lift falling** from +0.12 to +0.08 is not a regression and is worth understanding:
BM25 improved more than hybrid did (0.72 → 0.81), because longer notes simply give a lexical matcher
more surface to match. Lift is a *difference* between two rising numbers; both halves went up.

### Confirmed: the short-note hypothesis

The baseline entry flagged that three of the four note-level misses targeted the three shortest notes
in the corpus. `encryption.md` was expanded from 285 bytes to 5,982 — and *"keeping messages secret
from eavesdroppers"* went from **✗>10 to ✓1**. Same query, same model, same engine; the note simply
had enough text to be findable.

### But the same mechanism produced four new misses — a confound I introduced

Expanding six notes and not their cluster-mates created a **length imbalance inside each cluster**,
and the eval immediately reported it:

| query | before | after | now ranked #1 instead |
|---|---|---|---|
| how do leaves turn light into food | ✓1 | **·2** | `houseplant-care.md` (5,816 B vs `photosynthesis.md` 296 B) |
| i can't fall asleep at night | ✓1 | **·2** | `sleep-hygiene.md` (5,578 B vs `insomnia.md` 244 B) |
| pedalling a bike up a steep hill | ·7 | **·9** | `bike-maintenance.md` (6,131 B vs `bicycle.md` 245 B) |
| a mountain that erupts and spews magma | ·4 | **·7** | `earthquakes.md` (5,699 B vs `volcano.md` 252 B) |

Every one is the expanded note beating its own short cluster-mate. In each case the labelled answer
is still a near miss rather than a wrong topic, so the retrieval is not broken — but the corpus is
now partly measuring *which note is longer* instead of *which note is right*, and a metric with a
length artifact in it will eventually launder a real regression. This is a corpus defect, not an
engine defect, and the fix is to finish the job: bring the remaining cluster notes up to comparable
length. That also carries the corpus past the 60-chunk pool threshold.

One untouched note moved too: `fermentation.md`'s *"how warm should the crock stay for sour cabbage"*
fell from chunk rank ·3 to ·7. Nothing about that note changed — it is now competing against 50
chunks instead of 29. Expected, and a reminder that chunk-level ranks are only comparable within a
fixed corpus.

### Step 3B — the sweep, and what each variant says

```text
config                  chunks  embed_s   note h@1/MRR   chunk h@1/MRR   similar h@3   neg clean
default (reference)         50     10.5   0.89 / 0.926    0.67 / 0.786    1.00          0/3
prepend-heading-path        50     10.8   0.89 / 0.926    0.67 / 0.786    1.00          0/3
target-250                  89     10.2   0.95 / 0.953    0.67 / 0.766    1.00          0/3
```

**`target-250` moves the numbers, so the gate can see.** Halving the chunk target to 250 tokens
takes the corpus from 50 to 89 chunks and lifts note hit@1 from 0.89 to **0.95** (MRR 0.926 → 0.953)
while nudging chunk MRR *down* slightly, 0.786 → 0.766. A knob that moves note rank and chunk rank in
opposite directions is exactly the kind of trade-off #44 exists to adjudicate, and before the corpus
grew there was no chance of seeing it.

It also produced **the first `pool_blind: false` row in the log's history** — 89 chunks finally
exceeds the 60-candidate passage pool, so that row is not structurally blind to candidate width.

**`prepend-heading-path` printed bit-identical scores, and this was checked rather than assumed.**
Identical aggregates across a config change is the signature of a knob that is not wired through, so
it was traced end to end before being believed:

1. `chunk_body` does prepend — `chunk.rs:515`, and `tests/chunks.rs:445`
   (`prepend_heading_path_seeds_the_embedded_text`) asserts it.
2. The embed pass sends the **stored** chunk text to the model (`db.rs:1105` selects `c.text`;
   `ingest.rs:1037` feeds those strings to the embedder), so the prefix does reach the vectors.
3. Decisive check: the recorded **cosine piles differ** between the two rows — junk `0.5268 → 0.5155`,
   related `0.6298 → 0.6360`. Raw float distances, so the vectors genuinely changed.

Conclusion: the knob works, and on this corpus it is **rank-neutral**. MRR is computed from integer
ranks, so if the vectors shift without reordering anything, MRR is bit-identical by construction.
That is a legitimate #44 data point — the breadcrumb prefix is not currently paying for itself — and
it is *not* evidence of a broken lever. The general lesson: when two configs agree exactly, compare a
continuous quantity (the piles) before concluding anything, because the discrete metrics cannot tell
"no effect" apart from "not plumbed".

### Side effect for #150

The piles improved without anyone touching discovery. Related n=10 now spans [0.591 … 0.667] against
junk [0.490 … **0.684**], so `related-min − junk-max` narrowed from **−0.159 to −0.093**. The related
pile's floor rose (0.525 → 0.591) because longer notes make genuine cluster-mates look more alike;
the junk ceiling did not move at all, because 0.684 is still the watercolor ↔ stain-removal pair and
both of those notes are untouched loners. The overlap is now almost entirely that single pair.

Discovery positives stayed at hit@1 = 1.00 throughout, and negatives stayed 0/3 — unchanged, as
expected, since no floor exists yet.

**Status:** #44's gate demonstrably sees chunking (`target-250` moves it). Two things block the
verdict: the within-cluster length confound above, and a sweep that currently tries only two
variants. Next action: expand the remaining cluster notes to restore balance and clear 60 chunks.

> **Superseded by the next entry.** The proposal to expand the remaining short notes was challenged
> on the right grounds — a more relevant note should outrank a longer one, so uniform lengths would
> *hide* the effect rather than explain it. Investigation followed, and three of the four "length
> regressions" turned out not to be about length at all.

---

## 2026-08-10 — Diagnosing the four misses: two engine defects and one of my own

**Runs:** four `b2 search` probes on a real indexed copy of the corpus, plus `just eval` after one
corpus fix · `results.jsonl` row 11

### Why this entry exists

The previous entry proposed expanding every remaining short note so that no note could lose on
length. The objection: *regardless of length, the more relevant note should score highest —
`how do leaves turn light into food` should return `photosynthesis.md`, and lengthening everything
would hide the issue.* That is correct, and it is the stronger position. A vault full of uniform
5 KB essays is not what anyone's notes look like; real vaults are full of stubs sitting beside long
pieces, so **length robustness is a property worth measuring, not a confound worth erasing.**

So rather than author fourteen more notes, the four misses were diagnosed individually. Method: copy
the corpus to a scratch vault, `b2 reindex` it with the real model, and read the actual ranked output
with `b2 search --json` — full-precision scores, not the eval's rank columns.

### The verdict: only one of the four was ever about length

| # | query | rank | cause | whose bug |
|---|---|---|---|---|
| 1 | how do leaves turn light into food | ·2 | **RRF tie** broken by index walk order | engine |
| 2 | i can't fall asleep at night | ·2 | verbatim query phrase planted in a rival note | **mine** |
| 3 | pedalling a bike up a steep hill | ·9 | **no stemmer**: `pedalling` ≠ `pedals` | engine |
| 4 | a mountain that erupts and spews magma | ·7 | **no stemmer**: `erupts` ≠ `erupt` | engine |

#### 1. The photosynthesis "regression" is a coin flip

Full-precision note scores for that query:

```text
0.032266458495966696   houseplant-care.md
0.032266458495966696   photosynthesis.md
0.030776515151515152   sleep-hygiene.md
```

The top two are **bit-identical f64**. `photosynthesis.md` did not lose on relevance; it tied, and
`rrf_fuse` breaks ties with `.then(a.0.cmp(&b.0))` (`search.rs:49`) — ascending **chunk id**, which
is insertion order, which is the projection walk order. The note indexed first wins.

That is deterministic, which is why the eval is reproducible, and it is *semantically arbitrary*.
Exact ties are not rare here either: RRF with integer ranks produces a discrete lattice of possible
sums (`1/61 + 1/63` is reachable by several rank pairs), so collisions are structural rather than
freak events. A principled secondary key — the raw vector similarity, or the BM25 score — would break
these on evidence instead of on filesystem order. **Candidate issue.**

Note what this means for the earlier reading: dedup is *not* the problem. `Vault::search` does skip a
note already represented by a higher-scoring chunk (`vault.rs:1099`), so note rank is genuine
distinct-note rank. The residual length effect is only that a multi-chunk note gets more draws at the
max, which is inherent to passage retrieval — and it is *not* what decided this query.

#### 2. The insomnia miss was my own authoring error

`sleep-hygiene.md`, as I expanded it, contained the sentence *"…the conclusion that one
**"can't fall asleep"**…"* — the verbatim text of a labelled query targeting `insomnia.md`. I planted
the query's own words in a competing cluster-mate, which is precisely the failure mode I had been
checking the *new* passages against and never checked the *existing* queries against.

Rewritten to say "the capacity for rest has broken down" instead. The query returned to **✓1**
immediately, and note hit@1 rose 0.89 → **0.92** (MRR 0.926 → 0.939, lift +0.08 → **+0.11**).

Rule added, and it cuts both ways: **when you edit a corpus note, re-check it against every existing
query, not just the ones you are adding.** A new note can steal an old label silently.

#### 3–4. Two misses trace to a missing stemmer

`chunks_fts` is declared `tokenize = 'unicode61'` (`db.rs:469`) — no stemming — and
`index-engine.md` does not mention the choice anywhere, so it reads as an unexamined default rather
than a decision. Tested directly against the indexed corpus:

```text
MATCH pedals     → 1 chunk      MATCH pedalling  → 0 chunks
MATCH ascents    → 1 chunk      MATCH ascent     → 0 chunks
```

`bicycle.md` reads "driven by **pedals**" and "long **ascents** manageable". The query is
"**pedalling** a bike up a steep hill". The lexical half matches **neither term**, which is exactly
why BM25 scores ✗>10 there. Same shape for `volcano.md` and "**erupts**".

A porter-stemmed FTS table built over the identical chunk text recovers them:

| term | `unicode61` | `porter unicode61` |
|---|---|---|
| pedalling | 0 | **1** |
| ascent | 0 | **1** |
| erupts | 0 | **1** |
| erupting | 0 | **1** |
| roasting | 1 | **4** |
| roasted | 0 | **4** |

Every term driving the two remaining misses is recovered. (Porter is not magic — `leaf`/`leaves`
still fail to unify, since it stems them to different roots — but the gap is large and one-directional.)

This is a genuine retrieval defect the eval surfaced, it has nothing to do with note length, and
lengthening `bicycle.md` would have papered directly over it. **Candidate issue.**

#### Correction — the stemmer is only half the mechanism

The framing above ("these two misses trace to a missing stemmer") is incomplete, and the real
mechanism is more interesting. Decomposing the fused scores back into component ranks — RRF sums
`1/(60 + rank)` terms, so an exact score identifies the rank pair that produced it — gives:

| query | note | vector rank | BM25 rank | final |
|---|---|---|---|---|
| pedalling a bike up a steep hill | `bicycle.md` | **1** | 46 | **9th** |
| pedalling a bike up a steep hill | `bike-maintenance.md` | 2 | 3 | 1st |
| a mountain that erupts and spews magma | `volcano.md` | **1** | 41 | **7th** |
| a mountain that erupts and spews magma | `earthquakes.md` | 5 | 5 | 1st |

(The list placing each target at rank 1 must be the vector half: the eval scored BM25-only at ✗>10
for both queries, so BM25 cannot be the signal ranking them first.)

**The semantic half already gets both queries exactly right.** `bicycle.md` and `volcano.md` are each
the single nearest chunk in vector space. They lose because RRF rewards *consensus*, and a note that
is mediocre in both signals (5, 5) outscores one that is perfect in one and invisible in the other
(1, 41). Hybrid retrieval is here performing **worse than vector-only would**.

That splits the finding into two independent levers, and the stemmer is only the first:

- **Lexical recall.** Stemming would let BM25 match `pedalling`→`pedal` and `erupts`→`erupt`, moving
  those targets from rank 41–46 up into agreement with the vector half. Fixes the input.
- **Fusion policy.** RRF's consensus bias is a deliberate property, not an accident — but whether it
  is right for a *two*-signal hybrid, where one signal is known to be morphologically blind, is a
  separate question. `RRF_K` is the knob, and per `CLAUDE.md` it is one of the few this small corpus
  can actually see (it re-weights the same lists rather than changing their width). Fixes the
  weighting.

Either could rescue these two queries; they should be measured separately, and the eval can now tell
them apart.

### Where this leaves the corpus

The corpus stays as it is — six long notes beside seventeen short ones — because that mix is what
exposed both engine findings, and it is more faithful to a real vault than a uniform one would be.
The one change made was the authoring fix in #2.

Current state after that fix: note n=37, hybrid hit@1 **0.92**, MRR **0.939**, lift **+0.11**; chunk
n=18, hybrid hit@1 0.67, MRR 0.786; 50 chunks; discovery unchanged at 1.00 positive / 0/3 negatives;
piles unchanged (`related-min − junk-max = −0.093`).

The three surviving misses are now *labelled* rather than mysterious — each one is a standing test
for a specific engine defect, and each should flip to ✓1 when that defect is fixed. That is worth
more than a clean scoreboard.

**Status:** two candidate engine issues to open (RRF tie-break, FTS5 stemmer). #44's own verdict is
still pending a wider sweep.

---

## 2026-08-10 — Session close

Stopping point for the day. This section is the re-entry summary: what is true now, what is
uncommitted, and what the next person (or the next session) should pick up.

### Where the numbers stand

`results.jsonl` grew from 5 rows to **12**. The current default-config row is the one to compare
against:

| metric | session start | session end |
|---|---|---|
| corpus | 23 notes / 19.5 KB / **29 chunks** | 23 notes / 52.6 KB / **50 chunks** |
| note n | 25 | **37** |
| note hit@1 (hybrid) | 0.84 | **0.92** |
| note MRR@10 (hybrid) | 0.876 | **0.939** |
| semantic lift | +0.12 | +0.11 |
| chunk n | **6** | **18** |
| chunk hit@1 / MRR (hybrid) | 0.33 / 0.583 | **0.67 / 0.786** |
| discovery positive hit@1 | 1.00 | 1.00 |
| negatives clean | 0/3 | 0/3 (unchanged — no floor exists yet) |
| `related-min − junk-max` | −0.159 | **−0.093** |
| `pool_blind` (default config) | true | true (false at `target-250`, 89 chunks) |

Both Step 1 gates cleared on every run: bare CPU model id, `batch ≡ single` worst-row cosine
1.000000.

### The three findings, in order of how much they cost to act on

**1. RRF ties are broken by index walk order.** `rrf_fuse` sorts by score then `.then(a.0.cmp(&b.0))`
— ascending chunk id, i.e. whichever note the projection walked first (`search.rs:49`). Exact ties
are structural, not freak events: RRF over integer ranks lands on a discrete lattice of reachable
sums. Worked case: `photosynthesis.md` and `houseplant-care.md` tie at `0.032266458495966696`, and
the tie is *symmetric* — both decompose to ranks (1, 3), so one is vector-1/BM25-3 and the other the
mirror. A vector-similarity tie-break picks the labelled answer; a BM25 tie-break picks the wrong
one. So the secondary key is a real policy choice about which signal to trust in a photo finish, and
not arbitrary in the way chunk id is. Deterministic either way, so no reproducibility is lost.

**2. `chunks_fts` has no stemmer, and the choice is undocumented.** `tokenize = 'unicode61'`
(`db.rs:469`), unmentioned anywhere in `index-engine.md`. Measured on the indexed corpus:
`pedalling`→0 chunks vs `pedals`→1; `ascent`→0 vs `ascents`→1; `erupts`→0. A `porter unicode61`
table over identical chunk text recovers every one. This is a **hypothesis for improvement, not a
bug** — `unicode61` is FTS5's documented default and stemming is a genuine precision/recall
trade-off (Porter is English-only; a personal vault holds code, proper nouns, and possibly other
languages, and sparse retrieval arguably earns its keep by being literal where embeddings are weak).
The defect is that it was never written down or measured.

**3. The one that reframes both — RRF's consensus bias is demoting correct semantic hits.**
Decomposing fused scores into component ranks shows the dense half is *already right*:

| query | note | vector | BM25 | final |
|---|---|---|---|---|
| pedalling a bike up a steep hill | `bicycle.md` | **1** | 46 | 9th |
| pedalling a bike up a steep hill | `bike-maintenance.md` | 2 | 3 | **1st** |
| a mountain that erupts and spews magma | `volcano.md` | **1** | 41 | 7th |
| a mountain that erupts and spews magma | `earthquakes.md` | 5 | 5 | **1st** |

Mediocre-in-both beats perfect-in-one. On these two queries **hybrid retrieval performs worse than
vector-only would**. That is RRF behaving exactly as designed — it rewards agreement — but with only
two signals, one of them morphologically blind, the design assumption is worth re-examining. It also
means stemming is not the only lever and possibly not the cheapest: the dense half does not need
help *finding* these notes, it needs the lexical half to stop vetoing them.

### Method notes worth keeping

- **Bit-identical output across a config change is a claim to verify, never to accept.** The
  `prepend-heading-path` sweep row matched `default` exactly, which is the signature of an unwired
  knob. Tracing it (`chunk.rs:515` → `db.rs:1105` → `ingest.rs:1037`) and then comparing a
  *continuous* quantity — the cosine piles, which did differ — showed the vectors genuinely changed
  and no rank moved. Discrete metrics cannot distinguish "no effect" from "not plumbed"; a float can.
- **Decompose fused scores to read the mechanism.** An exact RRF score identifies the rank pair that
  produced it, which is how findings 1 and 3 were established. Cheap, and it turns "why did this
  rank badly" into an answerable question.
- **A scratch vault is the right diagnostic instrument.** `cp corpus → tmp; b2 reindex; b2 search
  --json` gives full-precision scores the eval's rank columns hide. Reusable for #150's real-vault
  dogfooding (Step 4C).
- **When you edit a corpus note, re-check it against every existing query.** The `insomnia.md` miss
  was self-inflicted: expanding `sleep-hygiene.md` planted the verbatim phrase `"can't fall asleep"`
  — a labelled query's own words — in a cluster rival. Fixing the sentence returned it to ✓1 and
  moved note hit@1 from 0.89 to 0.92. New notes steal old labels silently.
- **A corpus defect and an engine defect look identical in the scoreboard.** Of the four misses this
  session, one was a tie, one was mine, and two were the fusion/tokenizer interaction. Only
  per-query diagnosis told them apart.

### Deliberate non-decision

The proposal to expand every remaining short note was **dropped**, and the corpus deliberately keeps
its mixed lengths — six long notes beside seventeen short ones. Uniform lengths would have hidden
findings 1 and 3 rather than explaining them, and a real vault is a mix of stubs and long pieces
anyway. Length robustness is a property to measure, not a confound to erase. The three surviving
misses are now standing regression tests, each tied to a named defect.

### Next actions, cheapest first

1. **`RRF_K` sweep.** No code change beyond a variant entry in `examples/eval.rs`, no re-index, and
   `CLAUDE.md` confirms this corpus *can* see it. Does down-weighting consensus rescue `bicycle.md`
   and `volcano.md` without costing elsewhere?
2. **Vector-similarity tie-break** in `rrf_fuse`. Small change; measure hit@1 across all 37 queries.
   If the aggregate does not move, prefer documenting that ties occur over pretending to break them.
3. **Porter stemmer A/B.** Schema change plus re-index; same 37-query gate.
4. Only then: the #44 verdict, with a wider `variants` vec, and #150's Step 4A (more loner anchors).

Each of 1–3 is a candidate GitHub issue; none were opened this session.

### Repository state — nothing committed

Working tree at git `33c2f8a`, on `main`, **uncommitted**:

```text
 M crates/b2-embed/evals/corpus/bike-maintenance.md     expanded ~443 B → 6.1 KB
 M crates/b2-embed/evals/corpus/coffee-roasting.md      expanded ~458 B → 6.4 KB
 M crates/b2-embed/evals/corpus/earthquakes.md          expanded ~420 B → 5.7 KB
 M crates/b2-embed/evals/corpus/encryption.md           expanded ~285 B → 6.0 KB
 M crates/b2-embed/evals/corpus/houseplant-care.md      expanded ~461 B → 5.8 KB
 M crates/b2-embed/evals/corpus/sleep-hygiene.md        expanded ~448 B → 5.6 KB (+ the label fix)
 M crates/b2-embed/evals/queries.json                   25 → 37 queries, 6 → 18 passage-labelled
?? docs/evals/runlog.md                                 this file
```

`results.jsonl` is gitignored by design and holds all 12 rows locally. No engine code was modified
this session — every finding above is an observation, not a change.

---

## 2026-08-10 (evening) — Actioning the review: the engine's first fix, a third column, and the floor that didn't transfer

**Runs:** `just eval` ×3 (baseline replay, post-fix, post-corpus-growth), `just stability` +
`--bless`, and an out-of-repo distributional probe · branch `claude/eval-review-actions` ·
`results.jsonl` rows 13–15

The morning's log was reviewed end to end, with five risks named: aggregate deltas too coarse for
the n, the RRF_K sweep as an overfitting trap, a corpus structurally unable to vote against a
stemmer, #150 calibrated from piles that cannot transfer, and an author-circularity ratchet — plus
one broken promise (rows 7–12 recorded against an uncommitted tree) and one missing instrument (no
vector-only baseline). Everything below actions that review. Issues opened this session:
[#156](https://github.com/AlteredCraft/B2/issues/156) (tie-break),
[#157](https://github.com/AlteredCraft/B2/issues/157) (stemmer A/B),
[#158](https://github.com/AlteredCraft/B2/issues/158) (fusion characterization).

### Traceability restored, then the instrument re-verified

The six expanded notes + queries.json + this file were committed *first and unchanged*
(`6a4727b`) — the exact state rows 7–12 scored. Then, before measuring anything new, the working
changes were stashed and the eval re-run at that commit: **every number reproduced exactly** (note
hit@1 0.92, MRR 0.939, piles gap −0.093). CPU bge is deterministic; the instrument reads true.
Method rule: separate "is the instrument stable" from "did my change move it" — one stash apart.

### #156 — the tie-break fix moved exactly one query, the right one

`rrf_fuse`'s secondary key is now the candidate's rank in the dense list (absent below present),
id kept only as the final determinism key. Test-first
(`rrf_breaks_symmetric_ties_by_the_dense_lists_rank`, `…cross_signal…`), and the per-query diff of
rows 13→14 shows **exactly one** rank change in 37: *how do leaves turn light into food* ·2 → ✓1 —
the photosynthesis/houseplant photo finish, now decided on the signal that was right instead of the
walk order. Note hit@1 0.92 → **0.95**, MRR 0.939 → **0.953**, nothing else reordered. The
stability probe agreed from the other side: all 10 probes kept identical **note** top-10s; drift
was chunk-level tie-flips only (8/10 same set reordered, 2/10 one boundary swap) — blessed as the
new baseline, per the recipe's rule that a bless follows an *intended* ranking change.

### #158 — the vec-only column's first reading: fusion is not currently paying rent here

The eval now scores three signals per query (`bm25 / vec / hybrid`), and the aggregate block gains
a `fusion` line naming every query hybrid ranks worse than the dense ablation. First measurement
(n=37, row 14): **vec-only hit@1 0.97 / MRR 0.986** against hybrid's 0.95 / 0.953. The two known
demotions (`pedalling…` vec ✓1 → hybrid ·9, `erupts…` vec ✓1 → hybrid ·7) are now printed on every
run — and the column also caught the counter-case the forensic decomposition missed: *i can't fall
asleep at night* is vec ·2 → hybrid ✓1, fusion **rescuing** a query. 2 demotions vs 1 rescue is
the honest current score, which is exactly why #158 is characterization, not a verdict — and why
the RRF_K sweep was deliberately **not** run this session: any K rescuing two named queries on 23
notes is curve-fitting, and the stemmer (#157) may dissolve the disagreement at the input instead.

### #157 prerequisite — the corpus can now vote no

Three notes landed (row 15): `cosmos.md` + `campus-life.md` — the universe/university pair sharing
the Porter stem `univers`, so each one's query gains a lexical rival under a stemmed tokenizer and
not under `unicode61` — and `git-cheatsheet.md`, fenced commands with exact identifiers, two
code-literal passage queries (`reset --soft`, `reflog`), and the fourth negative anchor. These
queries deliberately break the avoid-the-target's-keywords rule and queries.json's description says
so: they measure lexical *precision under tokenizer change*, not semantic lift. Both audit
directions ran as scripts, not eyeballs; the one flag (query token pair `recover`+`mistake`
matching `encryption.md`) was fixed by rewording to "by accident". After: **all 4 new queries ✓1
on all three signals, and zero pre-existing queries moved rank.** New baseline: n=41, hybrid
hit@1 **0.95** / MRR **0.957**, vec-only **0.98 / 0.988**, chunk n=20 at 0.70 / 0.807, negatives
0/4 (20 cards), junk pile n=40 with ceiling unchanged at 0.684 (still the watercolor ↔
stain-removal pair), gap still −0.093. The A/B in #157 now has probes it can regress.

### #150 — the distributional probe: absolute floors do not transfer

228 Paul Graham essays (single author, one topic cloud — the density extreme *opposite* this
corpus) were indexed with the real model in a scratch vault and swept with `b2 similar --limit 10
--json`. The eval-calibrated floors are dead on arrival there: **junk-max 0.684 keeps 99% of all
2,280 surfaced candidates; related-min 0.591 keeps 100%.** Rank-1 medians 0.848, rank-10 medians
0.806 — the whole distribution lives above this corpus's junk ceiling, on a ~0.04 dynamic range.
The knee is real but shallow: 46% of anchors put their largest gap right after rank 1 (68% within
the top 2) at a median magnitude of just 0.0135, and a "within Δ of top-1" rule discriminates at
Δ≈0.02 and saturates by 0.04. Conclusion, posted to #150: calibrate from **per-anchor structure**
(gap or z-score), validate the rule against this corpus's *labelled* piles, use PG-scale data as
the transfer check — and remember a single-author vault may be a corpus where zero suppression is
genuinely correct, which only labelled negatives can distinguish.

### Housekeeping the gate surfaced

`just ci` failed at its audit stage on pre-existing advisories: **dompurify ≤3.4.12** (an XSS
advisory in the E5 trust-boundary sanitizer — "moderate" by label, load-bearing by role) and
nanoid. Both were in-range lockfile bumps; the full gate is green after. The audit stage doing
exactly what the CLAUDE.md design says it exists for is worth a line in the log it validated.

### Where the numbers stand now

| metric | session start (row 12) | session end (row 15) |
|---|---|---|
| corpus | 23 notes / 50 chunks | **26 notes / 53 chunks** |
| note n | 37 | **41** |
| note hit@1 / MRR (hybrid) | 0.92 / 0.939 | **0.95 / 0.957** |
| note hit@1 / MRR (vec-only) | — (no column) | **0.98 / 0.988** |
| fusion demotions / rescues | — (unmeasured) | 2 / 1, named per run |
| chunk n · hit@1 / MRR | 18 · 0.67 / 0.786 | **20 · 0.70 / 0.807** |
| negatives clean | 0/3 | 0/4 (floor still unbuilt — #150) |
| `related-min − junk-max` | −0.093 | −0.093 |
| standing engine issues | 0 open | #157, #158 (#156 fixed) |

The two surviving misses (`pedalling…`, `erupts…`) remain the standing regression tests for #157 —
now with four adversarial queries standing guard on the other side of the same trade.

### Next actions

1. **Run #157's A/B** (schema variant + reindex), judged by the paired win/loss readout, precision
   probes included. The two standing misses should flip; the four new probes must not.
2. **#44's wider sweep** — the gate sees chunking now; give it more than two variants.
3. **#150's rule design** — per-anchor gap/z-score, validated against the labelled piles, PG-scale
   transfer check after.
4. Accumulate vec-only rows before touching any fusion weight (#158).

---

## 2026-08-11 — PR #159 review: fifteen factual comments, and what honest prose does to a knife-edge tie

**Runs:** `just eval` ×2 (post-edit, post-cause-fix) · branch `claude/eval-review-actions` ·
`results.jsonl` rows 16–17

CodeRabbit reviewed the PR with 15 inline comments — all on corpus *content* (torque specs scoped
to the wrong fastener, S-waves vs surface waves, foreshocks being a retrospective label, hybrid-PQC
and AEAD overclaims, the reflog recovery guarantee, moisture probes, fluoride filters, universal
watering tables, sleep restriction being CBT-I with contraindications) — plus two label defects
(non-unique passages `Roth` and `reflog`; a physically false premise in the early-warning query)
and one style nitpick. Everything factual was accepted: the corpus should read like notes a
careful human would keep, and several fixes (reflog conditionality, AEAD scope) also make the
adversarial git/security material *more* faithful test content. The nitpick — moving the eval to
`anyhow` — was declined: the example only propagates-and-prints, `Box<dyn Error>` already does
that, and the repo rule is no new dependencies without concrete need.

### The instrument's verdict on the edits, per the process rules

Both audit directions ran (one real flag: the reworded alert query's first draft used
`shaking`/`arrives` — the note's own wave vocabulary — and its labelled chunk fell ·1→·4 as
sibling chunks outranked it; re-reworded to *"…with a few seconds to spare"*, whose distinctive
tokens all live in the labelled chunk: ·2). The passage-uniqueness scan now runs over all 20
labels (`Roth` → `untaxed growth and withdrawals`, `reflog` → `git reflog`; the other 18 were
already unique). Per-query diff vs row 15, every move diagnosed:

| query | vec | hybrid | chunk | cause |
|---|---|---|---|---|
| how do leaves turn light into food | ✓1 | ✓1 → **·2** | — | the bit-exact tie dissolved: content edits changed chunk lengths, so BM25 renormalized and `houseplant-care.md` now outscores outright |
| i can't fall asleep at night | ·2 | ✓1 → **·2** | — | same mechanism; the fusion *rescue* this column caught two days ago is gone |
| can an alert reach a phone… (reworded) | ✓1 | ✓1 | ✓1 → **·2** | query reword (see above) |
| protecting sore rubbed heels… | ✓1 | ✓1 | ·2 → **·3** | untouched note; corpus-wide IDF/length renormalization |

Current default row: **hybrid hit@1 0.90 / MRR 0.933** (vec-only 0.98 / 0.988, bm25 0.80), chunk
0.65 / 0.774, fusion demotions **3, rescues 0**. Discovery improved: the encryption edits moved
its vectors enough that `phishing.md` — a labelled expected mate — now *surfaces* (related n
10→11), setting the new related-min 0.571 and widening the recorded overlap to **−0.113**. The
floor gate (0.75) passes.

### The lesson worth the scoreboard drop

Yesterday's 0.95 was partly sitting on a **bit-exact RRF tie** that the dense tie-break resolved
the right way. Any honest edit anywhere near the competing notes could dissolve that tie — and
did. The number was real but *fragile*, and the drop to 0.90 is not a regression of the engine:
vector-only still ranks every one of these targets ✓1, and what changed is that photosynthesis
became the **third standing fusion demotion** instead of a photo finish. The durable rescue for
all three is #157/#158, not corpus surgery — and per process rule 2, no prose was re-sculpted to
resurrect the tie. The distinction that *is* legitimate, and was applied: qualifiers added for
factual accuracy had their **lexical footprint minimized at the cause** (`light` → `lighting` /
`grow lamp`, a shorter CBT-I clause) so a correction earns only the rank impact its meaning
requires, not what its wording happens to add.

**Status:** review addressed in full (15/15 factual accepted, 1 nitpick declined with reason);
labels tighter than before (unique passages, no false premises); three standing demotions now
guard #157/#158. Next actions unchanged from the previous entry.

---

## 2026-08-11 — #157's verdict: porter wins 7–0 / 3–0, and the shipped tokenizer switches (schema v5)

**Runs:** `just eval-stemmer` ×2 (the A/B under the then-shipped `unicode61`, then the confirmatory
mirror under the new default), `just stability` + `--bless`, and one scratch-vault decomposition ·
branch `claude/stemmer-ab` · `results.jsonl` rows 18–21

### The instrument, and why it needed no re-embed

The A/B's lever is new but small: `Vault::rebuild_fts(FtsTokenizer)` (`db.rs`) drops `chunks_fts`
and recreates it with the other tokenizer, repopulated from the untouched `chunks` content table —
same write-lock discipline as the migration and the vector-table rebuilds. The tokenizer only
touches the lexical half, so **nothing re-chunks and nothing re-embeds**: the same vault flips
between arms in milliseconds, and every rank move is the tokenizer's alone. The eval's `--stemmer`
flag scores BM25-only under both tokenizers *while the vault is still projected-but-unembedded*
(the honest lexical ablation — no hybrid fallback ambiguity), then hybrid under both after embed.
Two built-in checks held on every run: the dense ablation re-scored across the flip was
bit-identical (FTS cannot reach it, so movement there would mean a broken harness, not an engine
finding), and row 18 reproduced row 17's every aggregate before anything new was measured.

### The readout, per process rule 1 — paired win/loss, aggregate second

BM25-only, porter vs unicode61: **7 note ranks improved, 0 worsened.** Both standing stemmer
misses flip end to end (`pedalling…` ✗>10 → ✓1, `erupts…` ✗>10 → ✓1), and five more queries whose
inflection mismatches had been paying quiet rank tax surface with them (`i can't fall asleep at
night` ·2 → ✓1, `sore rubbed heels` ·2 → ✓1 with its chunk ·6 → ✓1, `kernels… begin to crack`
·3 → ✓1, `explosive force… tremor` ·2 → ✓1, `leaves… light into food` ·3 → ·2).

Hybrid, porter vs unicode61: **3 note ranks improved, 0 worsened** — precisely the three standing
fusion demotions, all to ✓1.

| metric | unicode61 (row 18) | porter (row 19) |
|---|---|---|
| bm25 note hit@1 / MRR | 0.80 / 0.870 | **0.95 / 0.976** |
| hybrid note hit@1 / MRR | 0.90 / 0.933 | **0.98 / 0.988** |
| vec-only (instrument check) | 0.98 / 0.988 | 0.98 / 0.988 — bit-identical |
| chunk hit@1 / MRR | 0.65 / 0.774 | 0.65 / 0.780 |
| fusion demotions | 3 | **0** |

**The precision probes did not move.** The corpus was grown last session specifically so it could
vote *no* — the `universe`/`university` Porter-collision pair and the two code-literal
`git-cheatsheet.md` queries all sit at ✓1 on all three signals under porter, unchanged. Chunk level
saw only within-note shuffles (crock ·7→·10, kernels ✓1→·2, against heels ·3→✓1 and thermostat
·3→✓1 on the lexical side; aggregate flat-to-up). The recall side of the trade was bought for
nothing the labels can see.

### What it did to #158: the demotions dissolved at the input

Under porter the eval prints, for the first time, **"fusion: no query ranks worse under hybrid than
under vector alone."** Hybrid rejoined the dense ablation at 0.98 / 0.988. This is the outcome
#157 flagged as possible — fixing lexical recall dissolved the fusion disagreement — and it means
the `RRF_K` sweep stays unrun with its overfitting trap unopened: there are no named queries left
to rescue.

The one remaining hybrid non-✓1 was decomposed rather than assumed (scratch vault, full-precision
`--json` scores): `i can't fall asleep at night` puts `sleep-hygiene.md` and `insomnia.md` at
**bit-identical** fused scores (`0.03252247488101534` = 1/61 + 1/62 — the mirrored (1,2)/(2,1)
photo finish), which the #156 dense tie-break decides toward `sleep-hygiene.md`, the dense rank-1.
The same policy that fixed the photosynthesis tie here lands against the label — and vec-only
itself ranks `insomnia.md` ·2, so *no* signal puts the label first. A photo finish on a knife-edge,
noted rather than re-litigated; the policy stands on its original grounds.

One aggregate moved down and should not be misread: **semantic lift fell +0.10 → +0.02.** Same
lesson as the corpus-growth entry — lift is a difference between two rising numbers, and stemming
made the lexical baseline much stronger (BM25 hit@1 0.80 → 0.95). Nothing about the model changed.

### The switch, and what guards it

#157's decision criteria were pre-registered and met (recall rescued, precision probes unmoved,
verdict documented), so the default switched in the same change: `chunks_fts` is now
`porter unicode61` in the base DDL, **schema v5** — migration is the standing disposable-index
posture (version bump → drop → the next `reindex` rebuilds). What guards it:

- `tests/fts_tokenizer.rs` pins the new contract model-free: the shipped default matches
  inflections out of the box, a `unicode61` rebuild restores literal-only matching (and back —
  the lever leaves no residue), and ingest stays in sync across a rebuild (the FTS triggers
  survive the table swap). The engine suite passed untouched — 34 binaries, zero edits.
- `just stability`: 9/10 probes drifted, all of it lexical-list reordering (the fake-vector dense
  half cannot see a tokenizer). Blessed per the recipe's intended-change rule; re-run clean.
- The retired arm stays standing: `--stemmer` now scores the **unstemmed ablation** beside every
  default run (the A/B's direction simply inverted), so the verdict is re-triable as the corpus
  grows — the confirmatory run (rows 20–21) prints the exact mirror, 0 improved / 7 worsened
  BM25-only, 0 / 3 hybrid. Every results row now records a top-level `tokenizer` key (absent =
  the pre-switch `unicode61`).
- `index-engine.md`'s tokenizer bullet rewrote from *measured-default* to *measured verdict*, with
  the trade's residual risk (Porter is English-only; code and proper nouns) named as the reason
  the ablation instrument stays.

### Where the numbers stand

| metric | session start (row 17) | session end (row 20) |
|---|---|---|
| bm25 note hit@1 / MRR | 0.80 / 0.870 | **0.95 / 0.976** |
| hybrid note hit@1 / MRR | 0.90 / 0.933 | **0.98 / 0.988** |
| vec-only note hit@1 / MRR | 0.98 / 0.988 | 0.98 / 0.988 |
| fusion demotions / rescues | 3 / 0 | **0 / 0** |
| chunk hit@1 / MRR (hybrid) | 0.65 / 0.774 | 0.65 / 0.780 |
| negatives clean | 0/4 | 0/4 (floor still unbuilt — #150) |
| `related-min − junk-max` | −0.113 | −0.113 (discovery never touches FTS) |
| standing note-level misses | 3 (all fusion demotions) | **0** |

**Status:** #157 resolved — the A/B ran under its pre-registered criteria and the switch shipped
with it. #158 downgraded from "standing demotions" to pure characterization: the vec-only column
keeps accumulating, and any fusion re-weighting still waits for evidence porter left behind. The
standing-miss list is empty at note level for the first time; the next actions are the ones that
were already queued — #44's wider sweep, and #150's per-anchor rule design.
