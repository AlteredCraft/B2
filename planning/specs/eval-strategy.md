---
title: "B2 — Eval Strategy: measuring model quality"
type: note
tags: [b2, evals, testing, embedder, relator, model-quality, spec]
created: 2026-07-03
status: draft
---

# B2 — Eval Strategy: measuring model quality

> **How B2 measures the quality of its two AI seams — the [`Embedder`](../../crates/b2-core/src/embed.rs)
> and the [`Relator`](../../crates/b2-core/src/relate.rs) — without letting a real model into the CI
> suite.** The [build spec](index-engine-build.md) covers the deterministic engine; this doc covers the
> *non-deterministic* half: two hand-labelled evals that score model output, live **out of CI**, and are
> run on demand. It owns the eval philosophy, the labelled-set formats, the metrics, and how to read and
> grow them. It does **not** own the seams themselves ([embed.rs](../../crates/b2-core/src/embed.rs) /
> [relate.rs](../../crates/b2-core/src/relate.rs)) or the real backends
> ([`LocalEmbedder`](../../crates/b2-embed/src/lib.rs) / [`ClaudeRelator`](../../crates/b2-relate/src/claude.rs)).

## 0. Why evals are examples, not tests

The core invariant of the test suite is that **`cargo test` is fast, deterministic, and model-free**
([vision-and-scope.md](../vision-and-scope.md) testability point 5; [CLAUDE.md](../../CLAUDE.md)). Model
output is neither fast nor deterministic — a real embedder needs a downloaded model, a real relator needs
a network call and a paid key, and both can drift run-to-run. Letting either into CI would make the suite
slow and flaky and would gate every commit on model behavior.

So model quality is measured by **Cargo examples, never `#[test]`s**. An example is compiled but only run
on demand (`cargo run --example …`), so it never runs in `cargo test` and can never flake the suite. Each
eval builds its own throwaway inputs, drives the **real** backend, prints a score table, and exits
non-zero below a soft reference floor so it can double as a manual quality gate. The deterministic fakes
([`FakeEmbedder`](../../crates/b2-core/src/embed.rs), [`FakeRelator`](../../crates/b2-core/src/relate.rs))
stay the CI default; the evals are the only place a real model is exercised, alongside `b2 init` and the
`#[ignore]` live smoke test.

## 1. The two evals at a glance

| | **Semantic-retrieval** | **Suggestion-quality** |
|---|---|---|
| Crate | `b2-embed` | `b2-relate` |
| Run | `cargo run -p b2-embed --example eval` | `ANTHROPIC_API_KEY=… cargo run -p b2-relate --example suggest-eval` |
| Seam under test | [`Embedder`](../../crates/b2-core/src/embed.rs) (`LocalEmbedder`) | [`Relator`](../../crates/b2-core/src/relate.rs) (`ClaudeRelator`) |
| Question | does hybrid search rank the right note first? | does the relator propose the right typed connections and decline the rest? |
| Data | [`evals/corpus/`](../../crates/b2-embed/evals) + `queries.json` | [`evals/corpus/`](../../crates/b2-relate/evals) + `pairs.json` |
| Metrics | precision@1, precision@3, MRR@10 | firing precision / recall, verb accuracy |
| Floor | precision@1 ≥ 0.75 | firing precision ≥ 0.75 |

Both share the same shape: a small hand-labelled corpus, a JSON label set whose queries/pairs deliberately
probe the hard cases, a real-model run, and a printed score with a soft floor.

## 2. Semantic-retrieval eval (`b2-embed`)

The first eval, shipped with the real embedder. It builds a throwaway vault from
[`evals/corpus/`](../../crates/b2-embed/evals), reindexes it through the **real** `LocalEmbedder` and the
full hybrid pipeline (BM25 ⊕ vector → RRF), then scores each labelled query by the rank of its relevant
note. Queries in `queries.json` are written to **avoid the target's keywords** (synonyms / paraphrase), so
a passing score is genuine semantic lift, not lexical overlap. Reported: precision@1, precision@3, MRR@10;
floor `p@1 ≥ 0.75`. See [`examples/eval.rs`](../../crates/b2-embed/examples/eval.rs).

## 3. Suggestion-quality eval (`b2-relate`)

The relator-side parallel: [`examples/suggest-eval.rs`](../../crates/b2-relate/examples/suggest-eval.rs).

### 3.1 What it measures — and what it deliberately doesn't

The thing under test is the relator's **judgment** — the precision gate of connection discovery. So the
eval scores that gate **in isolation**: it does **not** build a vault, run candidate generation, or touch
the embedder. It hands hand-labelled note pairs straight to `ClaudeRelator::relate()` and scores the
verdicts.

That isolation is the central decision (**resolved 2026-07-03**). The alternative — build a vault, run
real [`discover::candidates`](../../crates/b2-core/src/discover.rs), score the pipeline's end-to-end output
— was rejected for a *relator*-quality eval: it would entangle the relator's score with embedder /
candidate-gen quality (separate, separately-tuned concerns) and make labeling *reactive* (you could only
label pairs generation happened to surface). Candidate-generation quality (the deferred distance-weighting
experiment) is a distinct measurement for later. The eval also does not cover the pipeline's own guards —
the `is_core` re-validation and pre-call dedup in [`generate_for_anchor`](../../crates/b2-core/src/discover.rs)
are engine behavior, exercised by the deterministic `b2-core` suite, not model quality.

### 3.2 The labelled set

Two files under [`crates/b2-relate/evals/`](../../crates/b2-relate/evals):

- **`corpus/*.md`** — small, realistic PKM notes (frontmatter `title:` + a short body). Reusable: one note
  participates in many pairs. Notes are authored as single-line paragraphs so evidence substrings match
  cleanly. The seed corpus spans a few clusters (coffee, sleep, note-taking methods, diet claims, HTTP
  versions) chosen to produce genuine typed links *and* same-topic traps.
- **`pairs.json`** — the labels. Each pair references two corpus notes by filename stem and carries a gold
  verdict:

  ```json
  { "anchor": "grind-size", "candidate": "espresso",
    "evidence": "forcing hot water under high pressure …",
    "gold": { "connect": ["elaborates", "references"] } }

  { "anchor": "espresso", "candidate": "cold-brew", "gold": "decline",
    "note": "hard: sibling brewing methods, neither references the other" }
  ```

  - `gold` is either the string `"decline"` (no typed connection a careful author would record) or
    `{ "connect": [verbs] }` listing **every defensible** core verb for the `anchor → candidate` direction,
    **most-apt first**.
  - `evidence` is the candidate passage that "surfaced" the pair — it stands in for candidate-gen's
    `semantic:maxsim` chunk and is the only pair-specific text that reaches the prompt. Omit it to default
    to the candidate's first paragraph. When present it **must** be a real substring of the candidate note.
  - `note` is a labeller comment (e.g. `hard: …`), ignored by scoring but printed beside misses.

The seed set is **22 pairs over 18 notes**, covering all 10 core verbs on the connect side and including
deliberate declines: unrelated pairs (easy true negatives), same-topic-but-not-connected traps (sibling
methods; two notes that each concern a third but not each other), and a **direction** case (the reverse of
a real `supports` pair). Over-firing is the relator's primary failure mode, so declines are weighted toward
the traps that expose it.

### 3.3 Metrics

Gold labels each pair `connect` or `decline`; the model fires (a `Proposal`) or declines (`None`). The
standard binary-classifier trio, "positive" = fired:

- **firing precision** = TP / (TP + FP) — of the pairs it fired on, how many should connect. This is the
  **over-firing gate**, the relator's whole job; the floor is set here.
- **firing recall** = TP / (TP + FN) — of the pairs that should connect, how many it caught.
- **verb accuracy** = (true positives whose verb ∈ the gold set) / TP.

Verb accuracy credits **any** verb in the gold set because the vocabulary genuinely overlaps
(`relates` / `references` / `elaborates` are often all defensible). Demanding a single exact match would
report fake errors; listing every defensible verb, primary first, keeps the metric honest while still
flagging a real category error (e.g. `part-of` where only `supports` fits).

### 3.4 How a run works

1. **Validate before spend.** Every pair is checked against the corpus up front — unknown note, non-core
   gold verb, or an `evidence` string that isn't in the candidate note all **fail fast before a single
   (paid) call is made**. A data typo costs nothing.
2. **Judge.** For each validated pair, build the anchor `NoteCtx` + candidate `Candidate` (with the evidence
   chunk) and call the real relator once.
3. **Score + report.** A per-pair table (`✓` correct, `~` right fire but verb outside the gold set, `✗`
   wrong fire/decline), a **misses** block that reprints every non-`✓` with its labeller comment, the
   precision / recall / F1 / verb-accuracy summary, and per-run **token usage**. Below the precision floor
   it exits non-zero.

The model is sampled **once per pair** — a run is a single sample, not an average (a `--repeat N` agreement
pass is a noted follow-up).

## 4. Baseline snapshot — 2026-07-03 (`claude-opus-4-8`)

First real-model run of the seed set. **A single sample**, recorded for reference:

```
pairs=22   connect-gold=14   decline-gold=8
firing:  precision=0.82 (14/17 fired)   recall=1.00 (14/14)   F1=0.90
verb:    accuracy=0.93 (13/14 true positives took an acceptable verb)
tokens:  ~ 34418 input + 2968 output over 22 call(s)
```

Reading the four misses — the useful part, and exactly the tuning signal the eval exists to surface:

- **Two topical-overfires** (`espresso → cold-brew` and `crema → grind-size`, both fired `relates` at
  0.55–0.60). The decline-by-default prompt says "same topic is not enough"; the model still reached for
  `relates` on sibling/adjacent notes, at low confidence. This is the archetypal over-firing the precision
  metric is meant to catch — a lever for prompt-tightening and/or a confidence threshold.
- **One direction/type miss** (`diet-heart-hypothesis → mediterranean-trial`, fired `example-of` at 0.70).
  The genuine error: the hypothesis is not an example of the later trial, and the pair is the *reverse* of a
  real `supports` link. Directionality is the weak spot to probe next.
- **One verb-within-connected mismatch** (`channeling → espresso`, model `part-of`, gold
  `[references, elaborates]`, marked `~`). Arguably a *label* question — `part-of` is not indefensible for a
  named sub-phenomenon — i.e. a signal to tighten the label, not necessarily the model.

The takeaway: recall and verb accuracy are strong; precision is bounded by genuinely borderline `relates`
firings. Whether to tighten the prompt, add a confidence gate, or accept some boundary `relates` (the human
`accept`/`reject` is the ultimate precision gate) is the judgment this baseline informs — **do not tune
against a single run**; re-run and grow the set first.

## 5. Growing the set & the tuning loop

- **Tune from numbers, not vibes.** Run → read the misses → change **one** thing (the
  [prompt](../../crates/b2-relate/src/prompt.rs)'s verb glosses or decline stance; the default model;
  a label) → re-run. The floor guards against regressions.
- **Grow the labelled set.** 22 pairs is a seed. The durable **audit log** (backlog:
  [tasks.md](../tasks.md)) — one line per real `relate()` call with `(pair, verdict, confidence,
  decline-reason)` — is the natural capture mechanism: real dogfooding runs become labelled data (after a
  human confirms the gold), which is a richer, less hand-curated set than authoring pairs by hand.
- **Noted follow-ups.** `--repeat N` to report agreement across samples (the model is sampled once today);
  and, once this eval can score it, the deferred **distance-weighting** experiment for candidate ranking
  ([tasks.md](../tasks.md) backlog) — the candidate-gen measurement this relator eval deliberately leaves out.

## 6. Paused 2026-07-03 — how to resume

**Status.** The harness, the 22-pair seed set, and one baseline (§4) ship. The tuning effort is **parked here on
purpose**: tuning the prompt or model against a single, once-sampled run would be fitting to noise. This section is
the handoff — do these **in order**, each step gating the next, so a cold resume (fresh session or later self)
knows exactly where to start.

1. **Establish variance first — the blocking step.** Every pair is sampled once today, so the baseline
   (precision 0.82) is a point, not a distribution; a single pair flipping moves it ≈ ±0.06. The first coding task
   on resume is **`--repeat N`**: run each pair N times and report each metric's mean + spread (and per-pair
   agreement). Run 3–5×. If the misses recur, they're real; if they flicker, they're sampling noise. **Do not tune
   until you know which.**
2. **Grow the labelled set** (§5). 22 pairs is too few to trust a one-pair precision swing and leans on the coffee
   cluster. Add pairs — weighted toward **declines** (precision is the bound) and **direction** cases (the one
   clear miss type) — by hand, or by wiring the **audit log** (backlog) to harvest real `relate()` calls and
   confirming their gold. Re-baseline after growing.
3. **Triage each miss into one of three buckets** (the eval prints them with the labeller comment):
   - **Label question** → widen/fix the gold in `pairs.json`, not the model. *(Baseline: `channeling → espresso`
     fired `part-of`, arguably defensible for a named sub-phenomenon.)*
   - **Real model error** → a prompt/model lever. *(Baseline: `diet-heart-hypothesis → mediterranean-trial` fired
     `example-of` — wrong type and wrong direction.)*
   - **Acceptable boundary** → leave it; the human `accept`/`reject` is the final gate. *(Baseline: the two
     low-confidence `relates` over-fires on adjacent notes.)*
4. **Then tune one lever at a time**, re-running to confirm the floor doesn't regress.

**Lever inventory** (what to reach for, and where it lives):

| Lever | Where | Use for |
|---|---|---|
| Verb glosses / decline stance | [`prompt.rs`](../../crates/b2-relate/src/prompt.rs) `gloss()`, `system_prompt()` | the two `relates` over-fires; the direction miss |
| Default model | [`config.rs`](../../crates/b2-relate/src/config.rs) `DEFAULT_MODEL` | opus (default) vs. cheaper `claude-haiku-4-5` — the eval justifies a downgrade |
| Confidence floor on firing *(not built)* | [`generate_for_anchor`](../../crates/b2-core/src/discover.rs) or the relator | the over-fires are low-confidence (0.55–0.60); trade a little recall for precision — add only if the eval says it helps |
| Gold verb sets | `pairs.json` | the cheapest fix when a "miss" is really a label |

**Guardrail:** do not change the prompt or model off the single 2026-07-03 baseline — steps 1–2 (variance, then a
bigger set) come first.
