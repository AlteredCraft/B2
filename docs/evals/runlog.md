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
| [`crates/b2-embed/evals/corpus/`](../../crates/b2-embed/evals/corpus/) | the throwaway vault the eval builds fresh each run — topic clusters (coffee, sleep, plants, security, geology, cycling) plus deliberate loners |
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

Then the per-query table, with three rank columns — `✓1` is a first-place hit, `·3` is rank 3,
`✗>10` is a miss beyond K=10:

```text
 bm25 hybrid  chunk  query                                     top hybrid hit
   ·2     ✓1         how do leaves turn light into food        photosynthesis.md
 ✗>10     ✓1     ·2  steeping coarse grounds then pushing …    french-press.md
```

A query BM25 misses but hybrid nails is the entire point of the AI seam. The aggregate of that is
the **semantic lift** line.

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
