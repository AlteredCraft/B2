# Evals

How B2 measures what `cargo test` can't: whether search, discovery and chat are any good. A
test can prove `reindex` is idempotent. Only a labelled corpus can say that "how do leaves
turn light into food" should rank `photosynthesis.md` first.

Everything here runs on demand, out of CI, against the real model (E2). The harness lives in
`crates/b2-embed/examples/`, its data in `crates/b2-embed/evals/`, and the chat half in
`crates/b2-llm/`.

**A change to ranking or model behaviour is judged here, never by intuition.** The
[process rules](#process-rules) at the bottom bind anyone editing the corpora, the labels or
the metrics.

## The instruments

| Command | The question it answers | Model |
|---|---|---|
| `make eval` | Are retrieval and discovery good? Scores both labelled corpora and asserts the exit gate. | real bge |
| `make eval-sweep` | Would a different `ChunkConfig` be better? | real bge |
| `make eval-stemmer` | Is the stemmed FTS tokenizer still right? | real bge |
| `make eval-metal` | The same eval on the Metal GPU. A different vector space: compare with a CPU run, never average. | real bge |
| `make stability` | Did the ranking move? It says *different*, never *better*. | fake |
| `make calibrate VAULT=…` | Does a threshold derived on the corpus survive a real vault? `ARGS=--search` adds the evidence bar. | stored vectors |
| `make eval-chat` | Does chat cite the right notes and refuse what the vault can't answer? | real bge + a chat server |
| `make compare-device` | CPU against Metal embedding throughput. | real bge |

Run `make init` first. Exit codes: 0 means the run completed (and for `make eval`, every
gate passed), 2 means a quality gate failed, 1 means the run itself broke. Runs are
bit-reproducible on one machine and build, so a moved number is a real change. Each run
appends a row to a gitignored `results.jsonl`.

**Why both `eval` and `stability`.** The labelled corpus is too small for the candidate
pools to bind, so a pool-width change prints identical eval numbers while reordering a real
vault. `stability` runs the fake embedder over `fixtures/test-vault` (about 200 notes),
where the pools do bind, and diffs the ranking against a blessed baseline.
`make stability-bless` accepts an intended change; editing `fixtures/test-vault` means
re-blessing in the same commit. A `pool_blind` warning on an eval run means the same thing:
don't read "unmoved" as "no effect".

## What to run

| You are changing | Run |
|---|---|
| Chunking | `make eval-sweep`, then `make stability` |
| The FTS tokenizer or lexical matching | `make eval-stemmer` |
| Candidate pools | `make stability`, plus `make eval` |
| The RRF constant or fusion | `make eval` and `make stability` |
| A corpus note or a label | The token audit (rule 2), then `make eval` |
| Any threshold read off a score distribution | `make calibrate` on a real vault (rule 5) |
| Discovery ranking | `make eval`, reading both corpora |
| The chat prompt, condensing or citations | `make eval-chat` |
| The embed device | `make eval-metal` against a CPU `make eval` |

## The corpora

All under `crates/b2-embed/evals/` unless a path says otherwise.

| Path | Role |
|---|---|
| `corpus/`, `queries.json`, `similar.json` | The **orthogonal** corpus: 31 hand-written notes in distinct topic clusters, with loners, multi-topic notes and a journal-shaped note. `queries.json` holds search labels, including negatives whose right answer is "no matches". `similar.json` holds discovery anchors and their expected mates. Each file's `description` is its labelling rulebook: read it before editing. |
| `corpus-dense/`, `similar-dense.json` | The **dense** fixture: 15 beekeeping notes, all related. It is the geometry the orthogonal corpus can't express, and the bench that caught both retired rules below. |
| `stability.json`, `stability-baseline.json` | The unlabelled probe set and its blessed ranking. |
| `crates/b2-llm/evals/questions.json` | The chat questions. `expect` names the notes a right answer cites; an empty `expect` means the right answer is a refusal. |

## Reading `make eval`

Notation: `✓1` is ranked first, `·3` ranked third, `✗>10` not in the top 10. `[check]` is
the harness verifying itself. `[FAULT]` means the harness and the engine disagree, so
distrust the numbers around it. `[warn]` is a failed gate or a caveat.

The report runs in this order:

1. **Per-query ranks** under BM25, vector-only, hybrid and chunk level. This is the primary
   evidence.
2. **Aggregates:** hit@1, hit@3, MRR, and semantic lift (hybrid minus BM25-only).
3. **Discovery:** each labelled mate's rank, plus the unlabelled strangers served.
4. **Calibration blocks that gate nothing:** the z windows, the discovery fold bake-off and
   the search tail bake-off. Each re-derives on every run why no such rule ships.
5. **The search evidence bake-off.** Its shipped-bar lines are gated.
6. **The dense report:** the same readings on the single-subject fixture.

### The exit gate

`passes()` in `crates/b2-embed/examples/eval/gate.rs`:

| Assertion | Kind |
|---|---|
| Hybrid note hit@1 ≥ `FLOOR_HIT1` | floor |
| Per-mate discovery MRR@5 ≥ `FLOOR_MATE_MRR` | floor |
| Dense per-mate MRR@5 ≥ `FLOOR_DENSE_MATE_MRR` | floor |
| Dense fixture: no note has an empty related list | absolute |
| Labelled negative queries served = 0 | structural zero |
| Labelled relevant queries cut = 0 | structural zero |
| Dense titles cut = 0, dense nonsense served = 0 | structural zero |

Floors sit below today's reading on purpose. A gate pinned to today's number fails on the
first honest corpus edit, and the cheapest way to clear it is to edit a label. The zeros
have no headroom, because headroom there would be permission to serve a nonsense query or
cut a real one. The evidence rows skip on a model with no calibrated bar.

When the gate goes red, argue with the notes or the rule, never with the labels or the
constant.

## `make eval-chat`

It asks the labelled questions through the real `Vault::ask` and reports four separable
scores, because a bad answer has more than one possible author:

- **Retrieval reach:** did a labelled note reach the passages at all? A miss here is
  search's, not chat's.
- **Citation accuracy:** did the answer cite the labelled note?
- **Grounding rate:** did the answer cite anything?
- **Refusal accuracy:** on the questions the vault can't answer, did it say so?

The exit code is only a liveness check. Quality is read off the numbers, because a gate that
fails on every small local model is a gate nobody runs. Follow-up questions (the condense
step) are not measured yet (#220).

## Rulings so far

What has been measured and decided. Reopen one with new evidence, not a hunch. The issue
holds the numbers.

| Ruling | Issue |
|---|---|
| The default `ChunkConfig` held against a seven-variant sweep. | #44 |
| A 3× pool widening was reverted: the eval saw nothing, and the stability probe saw every list change. | #141, #142 |
| RRF ties break on the semantic rank. | #156 |
| Porter stemming stays: it improved ten ranks and worsened none. | #157 |
| Discovery ranks on the best passage, not the note average, so a strong section in a sprawling note is found. | #192 |
| The discovery existence gate was the defect, and was retired. The list is always served. | #196, #197 |
| No discovery fold ships: no depth transfers across vault sizes. | #200 |
| Search's evidence bar shipped as IDF-weighted coverage OR cosine, after a document-frequency ceiling failed on the dense fixture. | #201, #202 |
| No per-hit tail fold ships: the fused order isn't an evidence order, so that is reranker work. | #206 |

Two threads are open on purpose: a real relation the model ranks under strangers
(`encryption ↔ phishing`), which is ordering work for a pair scorer or reranker; and strength
dots that compress into one band in a single-subject vault.

## Process rules

Each comes from a measured mistake.

1. **Read the per-query win/loss list, not the aggregate.** At n≈40 an aggregate point is
   one or two queries, and only named flips can be argued against the labels.
2. **A corpus edit is a change to the instrument.** It ships as its own commit, after a
   two-direction token audit: no existing query's content words may newly land in the
   edited note, and no new query's words may split toward a rival. A red gate is never a
   reason to edit a label.
3. **One person writing the notes, the queries and the fixes drifts toward measuring what
   the engine already does.** Source queries from outside your own head, and dogfood on a
   real vault before trusting a threshold.
4. **An unmoved metric is a claim to verify, not proof of no effect.** Check a continuous
   quantity before believing a discrete one.
5. **A constant read off a corpus's score distribution is invalid until it transfers to a
   real vault.** `make calibrate` is the check. Ranks transfer; distributions describe the
   corpus they came from.
