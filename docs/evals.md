# The eval suite

How B2 measures the one thing `cargo test` cannot: whether retrieval, discovery, and grounded
chat are any *good*. Read this before you run an instrument, read its output, or touch the
corpora, the labels, or the metrics; the [process rules](#process-rules) at the bottom bind
every such edit.

A unit test can prove `reindex` is idempotent. Only a human-labelled corpus can say that "how
do leaves turn light into food" should rank `photosynthesis.md` first. Everything here runs
out of CI, on demand (ADR-0013): `cargo test` stays fast, deterministic, and model-free, so
model quality can never flake CI. Decision history lives in git and in
[GitHub Issues](https://github.com/AlteredCraft/B2/issues): every verdict in the
[record below](#the-verdict-record) names the issue that drove it, and the commit that
shipped it is the record of what changed and why.

Not the audience for this page?
[search-and-similarity.md](search-and-similarity.md) is the plain-language tour of everything
these metrics score, written for people *using* B2 rather than measuring it.

The harness code and its data live in
[`crates/b2-embed/evals/`](../crates/b2-embed/evals/); this page is the guide to all of it.

## The instruments at a glance

| Command | The question it answers | Model | Deterministic |
|---|---|---|---|
| `make eval` | Is retrieval/discovery *good*? Scores both labelled corpora through the real pipeline and asserts the exit gate | real bge | per machine+build¹ |
| `make eval-sweep` | Would a different `ChunkConfig` be better? (the [#44](https://github.com/AlteredCraft/B2/issues/44) chunker A/B, seven variants) | real bge | per machine+build¹ |
| `make eval-stemmer` | Is `porter unicode61` still the right FTS tokenizer? (the [#157](https://github.com/AlteredCraft/B2/issues/157) ablation) | real bge | per machine+build¹ |
| `make eval-metal` | The same eval on the Apple-Silicon GPU, a *different vector space* (`@metal`, ADR-0007). Compare against a CPU run, never average with one | real bge | per machine+build¹ |
| `make stability` | Did the *ranking* move, under widening candidate pools and since the blessed baseline? Says *different*, never *better* ([#141](https://github.com/AlteredCraft/B2/issues/141)) | fake | yes |
| `make calibrate VAULT=…` | Does a corpus-derived constant survive a real vault? (process rule 5; [#196](https://github.com/AlteredCraft/B2/issues/196)/[#197](https://github.com/AlteredCraft/B2/issues/197)) | stored vectors (pure read); `--search` loads the model | yes, per vault |
| `make eval-chat` | Does grounded chat cite the right notes and refuse what the vault can't answer? ([#154](https://github.com/AlteredCraft/B2/issues/154)) | real bge + a chat model server | no (LLM output varies) |
| `make compare-device` | CPU vs Metal embed *throughput* (a performance A/B, not a quality one) | real bge | no |

¹ Measured bit-reproducible run-to-run on an unchanged corpus/model/build
([#188](https://github.com/AlteredCraft/B2/issues/188): five runs, identical rows). So "noise
floor" means corpus and label drift, not run variance. Across devices the numbers differ: the
device is part of the embedding space's identity (ADR-0007), which is why rows record the
model id and are never averaged across `@metal` and CPU.

**Why two quality instruments.** `make eval` scores quality: it can say *better*.
`make stability` scores movement: it can only say *different*, but it sees what the labelled
corpus is structurally near-blind to, candidate width. The eval corpus barely exceeds the
passage view's candidate pool and never the note view's, so a pool-width change prints
(nearly) bit-identical eval numbers while genuinely reordering a real vault. The worked
example is
[#140](https://github.com/AlteredCraft/B2/issues/140)/[#142](https://github.com/AlteredCraft/B2/issues/142):
the eval saw nothing, the probe saw 10 of 10 probes change, and the change was reverted. A
probe can say *different*; only labels can say *better*. Run both.

## Quick start

```console
make init             # provision bge-base-en-v1.5 (one time)
make eval             # ~1min warm; appends rows to results.jsonl; non-zero exit on a gate regression
make stability        # model-free, seconds; drift vs the blessed baseline
make calibrate VAULT=$HOME/notes                 # the real-vault transfer check (any built vault)
make calibrate VAULT=$HOME/notes ARGS=--search   # …plus the search evidence bar's half (loads the model)
ollama serve & make eval-chat                    # grounded-chat scores (or any OpenAI-compatible server)
```

Exit codes, everywhere in the suite: **0** means the run completed (and, for `make eval`,
every gate cleared); **2** means a quality gate failed; **1** means the run itself broke (a
missing model, a label lint fault, a server that isn't there). `make stability` never gates:
any completed measurement exits 0, because drift is a signal, not a failure.

## When to run what

| You are changing… | Run | Because |
|---|---|---|
| chunking (`ChunkConfig`, boundaries, overlap) | `make eval-sweep`, then `make stability` | the A/B prices the change per query; passage-level ranks are where chunking shows |
| FTS tokenizer / lexical matching | `make eval-stemmer` | isolates the tokenizer over identical chunks + vectors |
| candidate pools, `pool_size`, RRF headroom | `make stability` (+ `make eval`) | the eval is near-blind to width ([#141](https://github.com/AlteredCraft/B2/issues/141)); the probe is its instrument |
| RRF constant / fusion weighting | `make eval` + `make stability` | `RRF_K` re-weights the *same* lists, so the eval does see it |
| a corpus note or a label | the token audit (process rule 2; [#218](https://github.com/AlteredCraft/B2/issues/218)), then `make eval` | a corpus edit is a change to the instrument: own commit, audited |
| any threshold read off a score distribution (a cosine bar, a z window, a band landmark) | `make calibrate` on ≥1 real vault | process rule 5: the constant is invalid until it transfers |
| discovery ranking / the `similar` surface | `make eval` (both corpora's per-mate blocks) | the dense fixture is the geometry the orthogonal corpus can't express |
| the grounded-chat prompt, condensation, citations | `make eval-chat` | retrieval reach is reported beside it, so a miss is attributed, not guessed |
| the embed device (CPU ↔ Metal) | `make eval-metal` vs a CPU `make eval`; `make compare-device` | a device switch is a model swap; quality and throughput are separate questions |

---

## `make eval`: the scored quality gate

One run builds two throwaway vaults, the orthogonal corpus
([`corpus/`](../crates/b2-embed/evals/corpus/), 31 notes) and the dense single-domain fixture
([`corpus-dense/`](../crates/b2-embed/evals/corpus-dense/), 15 beekeeping notes), and scores
everything through the real `Vault` pipeline. Nothing touches your vault, and nothing here
writes to the repo except the gitignored `results.jsonl`.

The run refuses to start on labels the corpus cannot honor: a **label lint** checks every
labelled path exists and every `passage` is verbatim in a relevant note (a typo'd label
otherwise scores as a permanent miss and reads as an engine regression). Two **instrument
checks** then print before any score and gate everything after them: the model id (never
compare CPU and `@metal` rows), and `batch ≡ single` embedding faithfulness.

### How to read the report, block by block

Rank notation, used everywhere: `✓1` means ranked first; `·3` means ranked third; `✗>10`
means not in the top K=10 (discovery reads at K=5). `[check]` lines are instrument
self-verification passing. `[FAULT]` means *distrust the surrounding numbers*: the harness
and engine disagree. `[warn]` is a gate failure or a measurement caveat. `[note]` is a
skipped reading with its reason.

1. **The per-query table.** Every positive query's note rank under `bm25` (projection only,
   the lexical floor), `vec` (dense-only ablation,
   [#158](https://github.com/AlteredCraft/B2/issues/158)), `hybrid` (the shipped fusion), and
   `chunk` (passage-labelled queries only). This paired list is the primary evidence (process
   rule 1); the aggregates below it are the smoke alarm. At n≈44, one aggregate point is
   about one query.
2. **Aggregates + semantic lift.** hit@1/hit@3/MRR per mode. `semantic lift` is hybrid hit@1
   minus BM25-only hit@1: the measured value of the AI seam. The **fusion** lines name every
   query hybrid ranks *worse* than the dense signal alone would (RRF's consensus bias),
   counted and named on every run instead of rediscovered by hand.
3. **Chunk rank.** The passage-labelled subset (n=20) scored at chunk level. This is where
   chunking levers show, and what the sweep is judged on.
4. **Discovery (`similar`).** Four readings: the **per-mate** ranks (every labelled mate
   scored on its own, so a hard mate can't hide behind an anchor's easy one,
   [#183](https://github.com/AlteredCraft/B2/issues/183)); the **strangers** list (unlabelled
   notes served on positive anchors: a smoke alarm with names attached, deliberately ungated,
   because the cheapest way to shrink it is to label the stranger, and labels aren't
   exhaustive); the **negatives** line (loner anchors serve their ranked nearest under
   always-serve, ADR-0014; the honesty rides on the strength bands); and the **cosine piles**
   (labelled-related vs everything-else served: if they separate, the gap is a floor; if they
   overlap, no simple floor exists and the data says so).
5. **Discovery z calibration** ([#187](https://github.com/AlteredCraft/B2/issues/187)).
   Gates nothing. Re-derives, every run, the windows a z existence rule *would* need (leader:
   negative-anchor leaders vs positive; member: strangers vs mates). `open` means a constant
   exists *on this corpus*; a real vault is the other half of any such claim (process rule
   5). `EMPTY` means the populations invert and no constant separates them. The negatives'
   leaders print with the band each would paint (`●○○`/`●●○`/`●●●`): what an always-served
   card *claims* to the human whose label says "nothing relates". A `[check]` line confirms
   the harness's independent z recomputation matches the engine's.
6. **Discovery fold bake-off** ([#200](https://github.com/AlteredCraft/B2/issues/200)).
   Gates nothing. Prices the candidate default-disclosure rules (mutual-k reciprocity, and
   "no fold", the incumbent) on the same served lists. Columns: `cards above` (default-view
   size), `mates folded` (labelled relations hidden by default: the fold's own cost, judged
   at zero), `strangers above`, `loners empty` (the claim a fold exists to make), `dark
   panes` (non-loner views emptied: disqualifying on the dense bench). The swept-`k` table
   re-derives the admissible window; the verdict line says which bound failed. The ruling of
   record: **no fold ships**. The window is empty, and no `k` transfers across vault scales.
7. **Search evidence calibration + bake-off**
   ([#201](https://github.com/AlteredCraft/B2/issues/201), ADR-0015). Per labelled query, the
   absolute signals RRF discards (OR-sanitized BM25 hit count and best score, dense top-1
   cosine) and what the surface serves, positives and negatives apart. The bake-off sweeps
   the shipped rule's shape (IDF-weighted term **coverage** OR a **cosine** bar) over the
   whole coverage grid; a cell is `✗` the moment it anchors a labelled negative (no cosine
   bar can rescue it). The **shipped bar** lines are gated: positives it would cut and
   negatives it still serves, both asserted at zero. A `[FAULT]` here means the engine's
   verdict and the harness's restatement of the same rule drifted apart: read the engine's
   `LexicalEvidence`, not the wording.
8. **Search tail bake-off** ([#206](https://github.com/AlteredCraft/B2/issues/206)). Gates
   nothing. Prices four per-hit prefix-cut families against the `tail_relevant` keep-set,
   each constraint re-derived per run, every payoff read against the **oracle ceiling** (what
   a fold placed by the labels themselves would cut). The ruling of record: **no tail fold
   ships**. The fused order and the evidence disagree mid-list, so the tail complaint is
   *ordering* work (the reranker seam), not disclosure work.
9. **The dense report.** Per-mate ranks on `similar-dense.json`; the **zero empty panes**
   sweep over *every* note (asserted: a vault where everything relates may never read as
   "nothing relates"); the shipped search bar replayed over every note's own **title** as a
   query plus built-in nonsense (`search_transfer`: titles need no labels, so nothing here
   can be relabelled to clear a number); and the tail families against whole title lists
   (every served row there is a real match by geometry).
10. **The cross-bench join.** Printed once both corpora are read: a tail family ships only if
    some constant is admissible on the labelled corpus *and* folds nothing on the dense
    fixture *and* still buys something. Admissibility is not a shipping order: a joint edge
    sits at a bench's own binding row with zero headroom, which the sizing method forbids.

A `pool_blind` warning means the run's chunk count fits inside the narrower candidate pool,
so candidate-width changes cannot move any number in that run
([#141](https://github.com/AlteredCraft/B2/issues/141)). Never read an unmoved number there
as "no effect"; `make stability` is the instrument.

### The exit gate

`make eval` exits `0` only when the default config clears **all** assertions (the gate at the
end of `run()` in [`eval.rs`](../crates/b2-embed/examples/eval.rs)):

| Assertion | Constant | Direction | When it goes red, the fix is… |
|---|---|---|---|
| hybrid note hit@1 ≥ 0.75 | `FLOOR_HIT1` | floor, below the 0.95 reading | read the per-query misses above the aggregate; argue with the notes, never the labels |
| per-mate discovery MRR@5 ≥ 0.52 | `FLOOR_MATE_MRR` | floor, below the 0.650 reading | read the per-mate lines: which mate slid, on which anchor |
| dense fixture: zero empty panes | — | absolute | an existence gate is refusing a vault whose every note relates ([#196](https://github.com/AlteredCraft/B2/issues/196)); remove it |
| dense per-mate MRR@5 ≥ 0.32 | `FLOOR_DENSE_MATE_MRR` | floor, below the 0.467 reading | as above; relabelling toward the model's order *always* looks plausible here, so don't |
| labelled negative queries served = 0 | `MAX_NEGATIVES_SERVED` | structural zero | the evidence bar is serving a query the vault holds nothing for: a rule regression or a mislabel; both want red |
| labelled relevant queries cut = 0 | `MAX_POSITIVES_CUT` | structural zero | change the **rule**, never the constant; the `df` ceiling died exactly here |
| dense titles cut = 0, dense nonsense served = 0 | `MAX_DENSE_TITLES_CUT` | structural zero | the lexical half has gone inert on a single-subject vault: the retired ceiling's failure shape |

**How the gates are placed is the point.** A rank floor never sits *at* its reading: a gate
pinned to today's number fails on the first legitimate corpus edit, and the cheapest way to
clear a red rank is to edit a label, the one habit this harness must never train (process
rule 2). Run-to-run noise is zero (see ¹ above), so each MRR floor sits roughly two
lost-from-rank-1 mates under its reading, and the headroom is for *corpus drift*. The
search-evidence rows are the deliberate exception: they sit *at* their structural zeros with
no headroom, because headroom there would read as permission to serve a nonsense query or cut
a real one ([#202](https://github.com/AlteredCraft/B2/issues/202)). They skip, with a printed
`[note]`, on a model with no calibrated bar (M2): asserting the absence of a verdict would
fail every run on a model nobody has measured yet.

Two rows have **retired** from the gate, deliberately. The negatives' suppression assertion
(`neg_clean == neg_n`) went with the existence gate it watched: under always-serve a loner
serves its ranked nearest, and the honesty moved to the band readout
([#197](https://github.com/AlteredCraft/B2/issues/197)). And the pass-vs-pass suppression
tripwire went when it was found to compare a `similar` call against itself, structurally
incapable of firing; a returning existence gate is caught instead by the dense
zero-empty-panes assertion and the per-mate floors
([#217](https://github.com/AlteredCraft/B2/issues/217)).

### The A/Bs: `--sweep` and `--stemmer`

The sweep re-chunks and re-embeds the same vault under seven `ChunkConfig` variants. The
stemmer flag rebuilds `chunks_fts` under the unstemmed `unicode61` over **identical** chunks
and vectors, so every rank move is the tokenizer's alone (its dense column doubles as an
instrument check: FTS cannot reach it, so movement there means the harness broke).

Read the `Δ vs default` lines, not the scoreboard (process rule 1): at this n, every
aggregate delta is 1 or 2 queries, and only the named flips can be argued against the labels.
"No per-query rank moved" is itself a claim to verify against a continuous quantity (the
piles) before believing "no effect" (process rule 4: the `prepend-heading-path` lesson). The
sweep's variant rows record no calibration blocks: a disclosure rule judged on a non-shipped
chunker would be a number about the chunker.

---

## `make stability`: the rank-movement probe

Runs on `fixtures/test-vault` (~200 notes / ~780 chunks: big enough for the candidate pools
to **bind**) with the deterministic fake embedder, so the committed baseline means the same
thing on every machine. It scores no relevance: the corpus is unlabelled, so it can only say
*different, and by how much*.

**Section 1: pool sensitivity.** Each probe is asked at depths 4/10/30; a cell compares the
top-4 across one widening step, position by position. `=4` means the shallow answer is an
exact prefix of the deep one (pool-invariant); `3/4` means one position changed; `n/a` means
the probe matched nothing (measured probes only enter the denominators). The summary counts
probes whose top-4 moved per step: that is what candidate width is worth on this vault.

**Section 2: baseline drift.** The shipped top-10 against the blessed snapshot
(`stability-baseline.json`): `=10` (exact), `kept, reordered`, or `n in, m out`. Drift is a
signal, not a failure: a knob change is *supposed* to move ranking. Once the change is the
intended one, `make stability-bless` accepts it (fake embedder + committed vault only; a
real-model or private-vault baseline would commit a number nobody else can reproduce).
Editing `fixtures/test-vault` or `stability.json` invalidates the baseline: re-bless in the
same commit.

Flags: `ARGS=--verbose` prints the diverging lists; `ARGS=--model` runs real bge (magnitude
only, no baseline); `ARGS="--vault <path>"` probes any vault.
`--vault crates/b2-embed/evals/corpus` is the control experiment where every prefix holds:
that *is* [#141](https://github.com/AlteredCraft/B2/issues/141)'s blindness, demonstrated.

---

## `make calibrate`: the real-vault transfer check

Process rule 5 made mechanical. Run it against any built vault: no labels needed, a pure read
over stored vectors, seconds even on a large vault. This is the instrument that retired the
existence gate ([#196](https://github.com/AlteredCraft/B2/issues/196) measured 16 of 17 panes
dark on a real single-domain vault) and killed the `df`-ceiling evidence rule. Any future
distributional constant answers to it **before** shipping.

Per-anchor columns: pool size `n`; the pool's cosine `min/med/max`; the leader's cosine and
z; `gate serves`, what the retired z gate would serve (`DARK` means it would empty this pane;
a simulation, since the shipped surface gates nothing); `fold`, the mutual-k reciprocity
fold's default view ([#200](https://github.com/AlteredCraft/B2/issues/200)'s candidate 1,
replayed); `e-bar`, the authored-edge reference bar's fold (candidate 2: this is the only
instrument that can price it, since its calibration population is your own committed edges
and both eval corpora are link-free); `bands`, the strength-band histogram the pane would
paint. The summary block repeats each as a vault-level reading, plus the engine-z drift
`[check]`.

Interpretation: a replayed gate darkening panes on a vault you know is inter-related is the
[#196](https://github.com/AlteredCraft/B2/issues/196) failure reproduced. An `e-bar` at 0 or
`UNPRICEABLE` is a reading about the vault (no scorable authored edges), not an instrument
failure. A band histogram compressed into `●○○` on a dense vault is the open A6 thread (band
compression), not a bug. A fake-embedded vault warns and prints noise: reindex with the real
model first.

`ARGS=--search` adds the search-side transfer bench
([#201](https://github.com/AlteredCraft/B2/issues/201)): first the vault's function-word
weights beside an absent word's (the lexical anchor's whole premise, checked per vault rather
than assumed of English), then the shipped evidence bar replayed over every note's own
**title** (positives by construction: nothing to relabel) and built-in nonsense (the
vault-independent negatives), the ten lowest-cosine positives (where a bar placement bites
first), and the per-hit tail families priced on the one row a title query certifies with no
label, its own note. `the bar would cut N/…` is the tripwire direction: D2 permits zero. This
is the one part of the instrument that is not a pure read (judging the cosine half embeds
each probe query, so it loads the model); it has no `--json` form yet
([#219](https://github.com/AlteredCraft/B2/issues/219)).

Other flags: `--limit N` (pane depth); `--leader-z`/`--member-z` (replay a different gate;
defaults are the retired constants, so
[#196](https://github.com/AlteredCraft/B2/issues/196)'s dark-vault reading reproduces);
`--mutual-k N`; `--json` (the discovery reading as one object, for scripting sweeps).

---

## `make eval-chat`: the grounded-chat scores

Needs a model server (`ollama serve` + `ollama pull llama3.2`, or `B2_LLM_URL` /
`B2_LLM_MODEL` at any OpenAI-compatible endpoint). Builds a throwaway vault from the
retrieval eval's corpus (chat is scored over notes whose retrieval behavior is already
characterized, so a surprise is attributable) and asks the labelled questions in
[`questions.json`](../crates/b2-llm/evals/questions.json) through the real `Vault::ask`.
Unlike `queries.json`, questions do **not** avoid the target's vocabulary: this scores what
the model does with retrieved passages, not what retrieval finds.

Four deliberately separable scores, because a bad answer has more than one possible author:

| Line | Meaning | A miss is… |
|---|---|---|
| **retrieval reach** | did a labelled note make it into the passages at all? | `make eval`'s result, not chat's: the ceiling every other score is judged under |
| **citation accuracy** | did the answer cite the labelled note? (scored only over reached questions) | the headline chat number |
| **grounding rate** | did the answer cite *anything*? | an uncited answer is a refusal or general knowledge, and the prompt forbids the second |
| **refusal accuracy** | on the two deliberate negatives, did it say "I don't find that in your notes"? | a confabulation. One negative is near the corpus's coverage on purpose, because models confabulate from loosely related passages |

Per-question verdicts: `cited the note` / `refused (correct)` are the good ends;
`retrieval missed (not a chat result)` blames the other half; `MISCITED`,
`REFUSED WITH EVIDENCE PRESENT`, and `CONFABULATED` are the model's failures, with
hallucinated `[n]` markers (markers naming no passage) counted beside them. TTFT and total
latency print per question.

**The exit code is a liveness check, not a quality bar**: `2` only when retrieval reached
labelled notes and *no* answer cited one, a broken pipeline. Model quality is read off the
numbers; a gate that fails on every small local model would be a gate nobody runs. Known
coverage gap: every question is single-turn, so the condense step is unmeasured
([#220](https://github.com/AlteredCraft/B2/issues/220)).

---

## `results.jsonl`: the run log

Both harnesses append one JSON line per scored run to their `evals/results.jsonl`
(gitignored, machine-local: scores depend on the machine's models). Every number ever cited
in an issue traces to a row here. Conventions:

- **Append-only**, so runs accumulate into one comparable dataset.
- **Keys are additive, never redefined.** A metric that changes meaning gets a *new* key; the
  old one goes absent, so a reader of a mixed file sees a missing field rather than one
  number silently meaning something narrower than it used to. (Retired keys so far:
  `discovery_z.shipped`/`replay_faults` with
  [#197](https://github.com/AlteredCraft/B2/issues/197);
  `similar_per_mate_raw`/`similar_mates_suppressed` with
  [#217](https://github.com/AlteredCraft/B2/issues/217).)
- **`"corpus"` tags every current row** (`"orthogonal"` / `"dense"`). Rows written before
  the key landed (2026-08-18) lack it, and an absent value reads as `"orthogonal"`, so older
  rows stay comparable. Rows are never averaged across corpora, and the dense row's transfer
  reading lives under its own `search_transfer` key rather than overloading
  `search_evidence`.
- **Re-derivability.** The calibration subtrees (`discovery_z`, `discovery_fold`,
  `search_evidence`, `search_tail`, `search_transfer`) record the raw per-candidate /
  per-row data, so any window, fold depth, or bar can be re-derived from a row without
  re-running the model.
- `pool_blind: true` marks a row no candidate-width comparison may be read from.

A few readers:

```console
# the latest orthogonal default-config row
jq -s '[.[] | select(.corpus == "orthogonal" and .config.label == "default")] | last' results.jsonl

# hybrid note MRR across runs
jq -r 'select(.corpus == "orthogonal") | [(.ts | todate), .git, .config.label, .note.hybrid.mrr] | @tsv' results.jsonl

# per-mate discovery MRR, both corpora
jq -r '[(.ts | todate), .corpus, .similar_per_mate.mrr] | @tsv' results.jsonl

# which queries missed hybrid rank 1 in a given row
jq -r 'select(.corpus == "orthogonal") | .queries[] | select(.hybrid != 1) | [.q, (.hybrid // "miss")] | @tsv' results.jsonl
```

## The corpora and labels (ground truth)

| Path | Role |
|---|---|
| [`corpus/`](../crates/b2-embed/evals/corpus/) | the hand-written **orthogonal** vault (31 notes): topic clusters, six long multi-chunk notes, five unambiguous loners, the stemmer-adversarial block, the [#183](https://github.com/AlteredCraft/B2/issues/183) multi-topic family, and `week-log.md`, the journal-shaped dilution extreme ([#189](https://github.com/AlteredCraft/B2/issues/189)/[#192](https://github.com/AlteredCraft/B2/issues/192)). Its token audit *minimizes* shared vocabulary, which is exactly why it cannot express topical concentration |
| [`queries.json`](../crates/b2-embed/evals/queries.json) | retrieval labels: 44 positives (20 with a chunk-level `passage`; three date-shaped, [#202](https://github.com/AlteredCraft/B2/issues/202)) + 5 **negatives** (empty `relevant` means the labelled answer is *no matches*). Seven positives carry `tail_relevant` ([#206](https://github.com/AlteredCraft/B2/issues/206)): the per-hit keep-set, exhaustive by label and encoded by note, never by rank. Its `description` field is the labelling rulebook; read it before editing |
| [`similar.json`](../crates/b2-embed/evals/similar.json) | discovery labels: positive anchors with expected mates; empty `expected` marks a negative anchor (a loner whose correct answer is *nothing*). Its `description` is the loner-orthogonality rulebook |
| [`corpus-dense/`](../crates/b2-embed/evals/corpus-dense/) + [`similar-dense.json`](../crates/b2-embed/evals/similar-dense.json) | the **dense single-domain fixture** (15 beekeeping notes, all inter-related, no loner): the vault-level geometry the orthogonal corpus is structurally incapable of expressing, and the bench that killed the existence gate and the `df` ceiling. Rankings-only labels; scored in its own vault and row |
| [`stability.json`](../crates/b2-embed/evals/stability.json) + [`stability-baseline.json`](../crates/b2-embed/evals/stability-baseline.json) | the unlabelled probe set and its blessed ranking snapshot (fake embedder over `fixtures/test-vault`) |
| [`questions.json`](../crates/b2-llm/evals/questions.json) | the chat set: questions phrased as a person would type them, `expect` names the note(s) a correct answer must cite; empty `expect` means the only correct answer is the refusal |
| `results.jsonl` (both harnesses) | append-only run logs, gitignored |

## The verdict record

Every ruling this suite has produced, one line each. The issue holds the full argument and
the numbers, and the commit that shipped it is the record of the change.

| Issue | Verdict |
|---|---|
| [#44](https://github.com/AlteredCraft/B2/issues/44) | `ChunkConfig::default()` held against a seven-variant sweep; the "winning" rows were impeached by a measured boundary-luck noise floor. Retrial is one `make eval-sweep` away |
| [#141](https://github.com/AlteredCraft/B2/issues/141)/[#142](https://github.com/AlteredCraft/B2/issues/142) | the labelled corpus is near-blind to candidate width; a 3× pool widening was reverted on the stability probe's evidence |
| [#150](https://github.com/AlteredCraft/B2/issues/150) | the z-score discovery quality floor shipped: per-anchor z over the centroid population, suppression entering the gate; **superseded by [#197](https://github.com/AlteredCraft/B2/issues/197)** |
| [#156](https://github.com/AlteredCraft/B2/issues/156) | RRF fused-score ties break on the dense signal's rank: a policy the eval decided, not walk order |
| [#157](https://github.com/AlteredCraft/B2/issues/157) | `porter unicode61` FTS stemming: 7–0 BM25 / 3–0 hybrid on the paired readout, precision probes unmoved; the unstemmed arm stays measurable via `make eval-stemmer` |
| [#183](https://github.com/AlteredCraft/B2/issues/183) | the multi-topic note family landed, and with it the **per-mate** metric: the per-anchor one saturates at 1.000 and hides every hard mate |
| [#187](https://github.com/AlteredCraft/B2/issues/187) | the member window is empty on the shipped corpus's own numbers: mates and strangers invert, so no constant separates them; windows are re-derived every run, never frozen in a docstring |
| [#189](https://github.com/AlteredCraft/B2/issues/189) | the journal-shaped note was built, measured, and **rejected**: the first corpus edit this harness refused (its diluted centroid tops loner anchors on content it doesn't contain) |
| [#192](https://github.com/AlteredCraft/B2/issues/192) | the floor moved to the stage-2 best-passage unit, and `week-log.md` landed: read correctly in both directions at once |
| [#182](https://github.com/AlteredCraft/B2/issues/182) | the buried gem is served, and the desktop's strength bands were re-read in the judged unit (the last surface still reading the retired one) |
| [#188](https://github.com/AlteredCraft/B2/issues/188) | discovery rank entered the exit gate; the harness measured bit-reproducible run-to-run, so floors are sized for corpus drift, not noise |
| [#196](https://github.com/AlteredCraft/B2/issues/196)/[#197](https://github.com/AlteredCraft/B2/issues/197) | the existence gate itself was the defect: a single-domain vault went 16/17 dark while the ranking was correct throughout. The gate retired; discovery **always serves the ranked list** (ADR-0014); `make calibrate` and the dense fixture landed *before* the fix, per the sequencing rule |
| [#200](https://github.com/AlteredCraft/B2/issues/200) | the discovery fold bake-off ran and **no fold ships**: mutual-k's admissible window is empty, and its safe depth is a different rule on every vault scale; the authored-edge bar is unpriceable on link-free corpora and stays open |
| [#201](https://github.com/AlteredCraft/B2/issues/201) | search's evidence bar was **earned**, and its first form (a hard `df` ceiling) read clean on the labelled bench and cut 3/15 answerable queries on the dense one, so the *rule* changed (IDF as a weight, not a bin), not the constant |
| [#202](https://github.com/AlteredCraft/B2/issues/202) | the verdict reached the surfaces ("no matches" is a real answer; `--json` became an object), the date-shaped block landed, and search's three structural-zero rows entered the gate |
| [#206](https://github.com/AlteredCraft/B2/issues/206) | the per-hit tail bake-off ran and **no tail fold ships**: 367 of 386 filler rows sit below the oracle fold, but the fused order misplaces the evidence, so every admissible prefix cut is near-vacuous; the tail complaint is ordering work (the reranker seam) |
| [#217](https://github.com/AlteredCraft/B2/issues/217) | the pass-vs-pass suppression tripwire compared a call against itself and could never fire; removed. The dense pane assertion and the per-mate floors are what catch a returning existence gate |

**Standing principles from the record**: when a label and a note disagree, fix whichever one
is lying (the watercolor → throat-singing precedent); an arguable negative is replaced, not
argued; instruments land *before* the rules they price; and a reading that disqualifies a
rule is re-derived every run, never frozen into a comment.

**The deliberately open threads**: the **phishing pair** (a real relation ranked under three
stranger pairs even in the best-passage unit; served, so ordering residue, the pair-scorer's
standing evidence); the **tail's 367 rows of headroom** (the second exhibit for a reranker,
measured on search's flow); and the **dense-vault band compression** (#197's A6: on a vault
where every candidate is close, the within-list z compresses and the dots lose resolution;
first read on a real-embedded build of the dense fixture: 0 ●●● / 6 ●●○ / 144 ●○○).

## Process rules

Adopted 2026-08-10. Each traces to a measured mistake or a named risk, and each is binding on
anyone editing the corpora, the labels, or the metrics.

1. **A paired per-query win/loss list is the primary readout of any A/B; the aggregate is a
   smoke alarm.** At n≈40, every aggregate point is 1 or 2 queries. "hit@1 +0.05" and "these
   two flipped" are the same fact, but only the second can be argued against the labels. The
   sweep prints the diff (`Δ vs default`) automatically.
2. **A corpus edit is a change to the instrument, so it ships as its own commit** whose
   message says what changed and why, and every edit runs the **two-direction token audit**
   before it lands: no existing query's content tokens may newly land in the edited or added
   note, and no new query's content tokens may split evenly toward a rival. Word-boundary and
   stem-prefix both (the `insomnia.md` steal and the `recover`+`mistake` near-miss are the
   precedents). Run the audit, don't eyeball it
   ([#218](https://github.com/AlteredCraft/B2/issues/218) tracks committing it as a script).
   And, since the gate reads discovery rank, **a red gate is never an argument for editing a
   label**: per-mate MRR and the strangers count both move when labels move, so the only
   honest response to red is to argue about the *notes*.
3. **The same person authoring notes, queries, and fixes is a ratchet toward measuring what
   the engine already does.** Mitigations in order of cheapness: rule 2's audit; sourcing
   future queries from outside the corpus author's head (from note titles alone, or another
   person); dogfooding on a real vault before trusting any threshold.
4. **A bit-identical or unmoved metric is a claim to verify, never proof of "no effect".**
   Compare a continuous quantity (the piles) before believing a discrete one. (The
   `prepend-heading-path` trace; the sweep diff prints its own reminder.)
5. **A constant derived from a corpus's score *distribution* is invalid until
   transfer-checked on a real vault.** `make calibrate` is the check (adopted with
   [#197](https://github.com/AlteredCraft/B2/issues/197), from a measured mistake made
   twice). Rank-derived readings transfer, because the corpus's *orderings* are engineered to
   be checkable; its score **distributions** are an artifact of engineered orthogonality, so
   any threshold read off them (a cosine bar, a z window, a band landmark) describes the
   corpus, not a vault. A distributional constant ships only with a `calibrate` reading from
   at least one real vault beside the corpus numbers.
