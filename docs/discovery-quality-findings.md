# Findings: relationship-discovery quality — why `b2 similar` always shows ten cards, and what to do about it

*2026-08-04. A review of the discovery ("similar notes") feature prompted by the pool-width work in
PR #144 and the question: are we off track, optimizing for "always return 10 results" when the user
of B2 only ever wanted quality results — including zero results when nothing good exists?*

*This is an assessment document, not a design ruling. The canonical docs in `docs/design/` still
say what they say; §Recommendations below includes changing them, deliberately, as the first step.*

---

## TL;DR

The instinct behind the question is right, but it's aimed at the wrong target.

The oversampling work in the recent PRs (#137 → #144) is **search** plumbing — it exists so a result
that fails to load mid-reindex gets backfilled instead of silently shrinking the list, and #144
actually *narrowed* a pool that had grown too wide. That work is correctness, not padding, and it
should stay.

The "always ten results, quality be damned" behavior lives somewhere else entirely, and it was put
there **on purpose**: `b2 similar`'s candidate generation ranks every unlinked note by semantic
distance and truncates to the requested limit. There is no quality floor anywhere in the stack.
The design docs bless this explicitly ("generation is deliberately permissive: it over-produces,
and the human decides"). On any real vault, every note — including one about a grocery list — gets
its ten "nearest" notes, where nearest may mean *nearest of nothing*.

Three more things make this a real gap rather than a quick fix:

1. The open reranker issue (#28) **explicitly excludes** `b2 similar` from its scope. No open
   issue tracks discovery precision.
2. The eval harness — the project's report card for quality changes — measures discovery by
   *ranking only*. It is structurally incapable of seeing this problem: a perfect score is
   compatible with nine junk cards under one good one.
3. A distance threshold is technically well-founded here (the vectors are normalized, so the score
   maps cleanly to cosine similarity) — but the threshold must be model-relative and picked from
   measured data, not intuition, or it will be a magic number that quietly breaks on the next
   model swap.

The recommended path, in order: rule the posture change in the design docs → make the eval able to
*see* the problem (negative anchors + score distributions) → ship a model-relative quality floor
calibrated from that data → escalate to a pair-scoring reranker only if the measured data shows raw
embedding scores can't separate good from bad. Details below.

---

## Part 1 — The landscape: what was reviewed and what it showed

### 1.1 The #137 → #144 arc is search plumbing. Leave it alone.

The recent run of PRs that talked about oversampling and pools was about `b2 search`, not
discovery, and each piece has a correctness reason:

- **#137** — search could return *fewer* results than asked for when a concurrent reindex replaced
  a note's chunks mid-query (the C1 promise: readers are never refused while a writer rebuilds).
  The fix walks past a dead entry to the next live one instead of charging it against the budget.
  This is backfilling a *torn read*, not padding a list.
- **#140 / #142 / #144** — the passage view briefly inherited the note view's 3× headroom, which
  widened retrieval from 60 to 150 candidates per signal. The rank-stability probe (#141) showed
  that width *changes answers*, not just work — so #144 put the width back and split the constants
  (`note_hit_pool` vs. `chunk_hit_pool` with a small fixed `TORN_READ_HEADROOM`), precisely so a
  future measurement can't confuse the two views.

In all of this, `limit` is a **cap**. The extra candidates exist to survive concurrency races and
note-dedup; they are still ranked by the same scoring, and nothing promises the list will be full.
Unwinding any of it would reintroduce real bugs without touching quality. The confusion is
understandable — the PRs talk about "backfilling" and "pools" — but this arc is not where the
quality problem lives.

### 1.2 Where "always ten" actually lives: `discover::candidates`, by design

The discovery pipeline (`crates/b2-core/src/discover.rs`) works like this:

1. **Shortlist.** Rank every note in the vault by how close its centroid (average vector) is to
   the anchor's, skip the anchor and anything already linked to it, and keep a generous shortlist —
   at least 200 notes, or 20× the requested limit, whichever is larger.
2. **Exact scoring.** For each shortlisted note, find the single best chunk-pair match against the
   anchor's chunks.
3. **Sort, truncate to `limit`, return.**

Nowhere in those three steps is there a notion of *good enough*. The only ways to get an empty
answer are: the vault has no embedding space yet, or the anchor itself has no vectors. Otherwise
the list is always full. The CLI defaults to `--limit 10` (`b2-cli/src/main.rs`), and the desktop
discovery pane requests 10 on every single note-open (`ui/src/api.ts`) — which is why the cost of
this design is so *visible* in daily use: every note you open presents ten confident-looking cards,
whether or not anything in the vault genuinely relates.

And this is not an accident or drift. The module doc says it in so many words:

> Generation is deliberately **permissive**: it over-produces, and the human decides which are
> worth a link.

The design docs (`docs/design/index-engine.md` §3) carry the same stance. That matters for how a
fix lands: in this repo the docs are the source of truth and the code is a projection of them, so
the first artifact of any change here is a deliberate revision of that stance in the docs — not a
patch that quietly contradicts them.

It's worth being fair to the original reasoning: "the human is the precision gate" is a real
principle here (nothing in B2 auto-links; invariant W4 forbids silent auto-fixes), and on a small
young vault, permissive generation is harmless — everything really is a plausible neighbor.
The stance stops working as the vault grows: the top-10-of-everything list stays full forever,
and the human gate gets ten judgments to make per note instead of two worth making. Filtering
what's *surfaced* does not touch who *commits* links; the gate stays human. The principle and the
fix are compatible.

### 1.3 The reranker issue does not cover this

Issue #28 ("cross-encoder reranker over the fused top-N") is the one that comes to mind when
"reranker" is mentioned, and it is explicitly scoped away from discovery, in its own words:

> **Scope — reranks `b2 search`, not `b2 similar`.** The seam is `(query, candidate) → scores`,
> so it needs query text and reorders **query search** results. `b2 similar` has *no query* … so
> this reranker does **not** apply to it. The discovery-side ranking levers are the chunker
> upgrade (#19) and distance-weighting (#20), not this.

The two discovery-side levers it points at don't answer this question either:

- **#20** (graph-distance weighting) is a *reordering* experiment — boost triadic-closure
  candidates, or boost serendipitous far-graph ones. It changes which candidates float to the
  top; it cannot make a bad list shorter.
- **#44** (chunker quality gate) is about whether retrieval finds the right *passages*; it also
  never asks whether anything should be shown at all.

So the current state is: **no open issue, no code path, and no measurement addresses "return fewer
— or zero — when nothing is good."** That's the gap this document is about.

### 1.4 The eval harness cannot see the problem

The full plain-English tour of the harness is Part 2; the finding itself is short. The discovery
half of the eval scores *rank only* — "did the expected note land at #1? in the top 3?" (hit@1,
hit@3, MRR). Rank metrics are structurally blind to precision. If `b2 similar espresso.md` returns
`coffee-roasting.md` first and then nine notes about volcanoes and passwords, the eval scores that
run as **perfect**. We could ship a change that halves discovery quality — or one that fixes it —
and these numbers would not move.

There are also no *negative* labels: every anchor in `evals/similar.json` has expected matches.
The corpus contains no note for which the right answer is "nothing here relates," so even a
better metric would have nothing to measure suppression against.

The repo's own standing rule — stated in half the open quality issues — is "eval-gated, never on
intuition." Right now the eval cannot even express the failure being reported. Making it able to
is therefore not busywork before the fix; it *is* the first half of the fix.

### 1.5 A distance threshold is technically sound — with two caveats that shape the design

Could `similar` simply drop candidates whose score is too weak? Mathematically, yes, and cleanly:

The real embedder L2-normalizes every vector it produces (`b2-embed/src/model.rs`,
`l2_normalize` on both the single and batch paths). For normalized vectors, the L2 distance the
engine computes and cosine similarity are locked together: `cos = 1 − d²/2`. The score `similar`
already carries (negated L2 distance) is therefore monotonic with cosine similarity, and a floor
expressed in either unit is well-defined. No schema change, no new computation — the number is
already in hand at the moment of truncation.

Two caveats, both of which shape what "ship a threshold" has to mean:

1. **The threshold must be model-relative.** bge-family models compress their cosine scores into
   a narrow high band — roughly 0.6–0.9 for *everything*, related or not — and the usable
   good/bad boundary sits somewhere inside that band, at a spot that differs between bge-base and
   bge-small, and between CPU and Metal builds (which the engine already treats as distinct
   models, tagging `@metal` onto the recorded model id). The fake embedder used by the test suite
   is a different regime entirely — its vectors are hash-derived and not normalized at all. So a
   bare constant is wrong by construction. The natural home for the number is alongside the model
   identity the index already records (`meta`'s `embed_model_id`), with the fake embedder getting
   its own value or the gate simply not asserted under fake vectors.
2. **Raw bi-encoder scores are poorly calibrated.** This is the known weakness of embedding
   distance as a judge of "is this actually related?" — two texts can sit near each other for
   reasons a human wouldn't endorse, and the score bands for "genuinely related" and "merely
   nearby" can overlap. This is exactly the weakness cross-encoders exist to fix. It's why the
   measurement work in Part 2 comes first: if the measured score piles separate cleanly, a simple
   floor works and the heavy machinery is unnecessary; if they overlap badly, a floor was never
   going to hold and the escalation path (§3.4) is justified by data rather than taste. It's
   also why a *relative* cutoff — a drop-off gap from the top candidate, rather than an absolute
   line — deserves evaluation alongside the absolute one; relative cutoffs travel better across
   models.

### 1.6 Minor, but related: the score shown in the UI is meaningless

The desktop discovery card displays the raw score — a number like `-0.734` (`ui/src/render.ts`).
Negated L2 distance means nothing to a human. Whatever calibration work happens here, the display
should ride along: either translate to something honest (a strength band, e.g. strong/moderate)
or show nothing. A number that looks precise but can't be interpreted is worse than no number.

---

## Part 2 — The eval harness, in plain English, and what to build onto it

### What the harness is today

B2 has a small "report card" system that lives outside the normal test suite. It runs by hand
(`cargo run -p b2-embed --example eval`) because it needs the real AI model, which the fast tests
never touch — CI is model-free by construction. It works off a tiny practice vault of hand-written
notes — espresso, insomnia, volcanoes, bicycles — where a human has already written down the right
answers (`crates/b2-embed/evals/`).

For discovery, the answer key (`evals/similar.json`) looks like this:

```json
{ "anchor": "espresso.md", "expected": ["french-press.md", "coffee-roasting.md"] }
```

— "if you ask for notes similar to `espresso.md`, a person would say `french-press.md` and
`coffee-roasting.md` belong next to it." There are six such anchors. When the eval runs, it asks
`Vault::similar` the same questions and checks whether the expected notes appeared and how high:
first place, top three, and an average-rank summary. Each run appends its numbers to a log file
(`results.jsonl`, gitignored) so before/after comparisons are cheap.

The harness has a second, model-free half — the rank-stability probe (`just stability`, GH #141) —
that exists because the labelled corpus is tiny: 26 chunks, smaller than the candidate pools, so
one whole class of change (candidate width) is invisible to it. The probe can say *different*;
only the labelled corpus can say *better*. That division of labor is worth keeping in mind here,
because the work below extends the labelled half — the only half that can ever say "better."

### Why the current measurement can't see this problem

Everything the discovery eval checks is about *ordering*. It never asks "**should anything have
been shown at all?**" A run that returns the expected note at #1 followed by nine strangers scores
identically to a run that returns the expected note alone. The nine junk cards — the exact thing
prompting this review — are invisible to the scoring.

### The proposed work, in two pieces

**Piece 1 — add questions where the right answer is "nothing."**

The practice corpus needs a few deliberate loners: notes with no genuine relatives in the corpus —
say, a note about tax receipts in a corpus that is otherwise coffee, sleep, and geology — labelled
in `similar.json` as *expected: nothing*. Then the eval can finally ask the question that matters:
**when there's nothing good, does B2 say so, or does it confidently serve up ten strangers?**

Today, B2 will always serve the strangers, so the new metric starts out failing. That is the
point: it gives the fix a measurable target, and it permanently guards the behavior — a future
change that regresses precision will show up as this metric going red, the same way #141's probe
now guards candidate width.

**Piece 2 — write down the scores, not just the ranks.**

Every surfaced candidate already carries its similarity score; the eval currently throws the
scores away and keeps only positions. The change: record them, sorted into two piles —

- scores of candidates a human labelled *genuinely related*, and
- scores of everything else that got surfaced.

Then look at the two piles. If real matches cluster in one range and junk in another, the gap
between the piles **is** the threshold — read off actual data instead of guessed. If the piles
overlap heavily, that is equally valuable to learn early: it means raw embedding distance cannot
tell good from bad on its own, the cheap fix (a simple cutoff) won't hold, and the escalation to
a pair-scoring model (§3.4) is justified before a round is wasted on a threshold that was never
going to work.

Because the threshold is model-relative (§1.5), this measurement also future-proofs the number:
when a model swap happens later, re-running the same eval re-derives the right cutoff for the new
model, instead of leaving a stale constant behind.

### Scope of the work

Modest. Edits to one example program (`crates/b2-embed/examples/eval.rs`: a precision/suppression
metric over the new negative anchors, plus score-distribution recording into the same
`results.jsonl`), a handful of new practice notes in `evals/corpus/`, and new entries in
`evals/similar.json`. No new dependencies, nothing added to CI, and the six existing anchors and
all current metrics keep working unchanged. Dogfooding on a real vault complements it — `B2_LOG`
already captures discovery flow, so real-vault score distributions can be collected the same way.

---

## Part 3 — Recommendations, in order

### 3.1 Leave the search plumbing alone

The #137 → #144 work (torn-read backfill, pool-width discipline, the split pool constants) is
correctness under concurrency, already measured by the stability probe, and orthogonal to
discovery quality. Nothing here asks for it to change.

### 3.2 Rule the posture change in the design docs first

One sentence of stance change, made deliberately where the stance lives
(`docs/design/index-engine.md` §3, and the module doc of `discover.rs` as its projection):

> Discovery surfacing is quality-gated: `limit` is a cap, not a promise. Zero candidates is a
> legitimate — and honest — answer.

This does not touch the human-is-the-precision-gate principle (W4): the human still commits every
link; the machine merely stops presenting candidates it has no evidence for. Filtering what's
*surfaced* is not authoring.

### 3.3 Make the eval able to see the problem, then ship the floor

In that order — the Part 2 work first (negative anchors + score distributions), because it is
small, it produces the calibration data, and the repo rule ("eval-gated, never intuition")
exists precisely for changes like this one.

Then the floor itself, in `discover::candidates` / the façade:

- **Model-relative**, keyed alongside `embed_model_id` — never a bare constant; the fake embedder
  regime handled explicitly.
- **Both variants evaluated**: an absolute cosine floor and a relative drop-off cutoff (gap from
  the top candidate), against the piece-2 data; ship the one the data supports.
- **Config-surfaced** (a CLI flag and a desktop setting) with a calibrated default, so dogfooding
  can tune without rebuilding.
- **An honest empty state** in the two adapters: the desktop pane says "no strong candidates"
  calmly instead of rendering ten weak cards; the CLI prints the analogous line. The raw-score
  display (§1.6) gets replaced or removed in the same pass.

### 3.4 The escalation path, if the data demands it: a discovery-side reranker — a new issue, not #28

If piece 2 shows the score piles overlap too much for any floor to hold, the next lever is a
**pair-scorer**: a cross-encoder scoring (anchor evidence chunk, candidate chunk) pairs, with the
threshold applied to *that* score — better calibrated than raw embedding distance, which is the
actual strength of cross-encoders. Three things about its shape:

- It is a **filter/re-scorer of what's surfaced, never an auto-linker** — the human still commits
  every link, so it is compatible with the Bitter-Lesson posture that cut the LLM relator. It
  would be the project's second model seam, sibling to `Embedder`.
- It is a **new issue**, cross-referenced to #28 but not an extension of it — #28's seam needs
  query text and correctly excludes `similar`. A pair-scorer is a different seam with a different
  signature.
- It is **heavier than it sounds**: a second model dependency, and inference on every note-open
  in the desktop pane (mitigable by caching in the already-reserved `llm_cache` table, and by the
  pane's existing async-side-pane structure). Which is exactly why it is the escalation and not
  the first move — the floor is the cheap experiment that the data may prove sufficient.

### 3.5 Do not reach for #20 as the fix

Graph-distance weighting (triadic closure vs. serendipity) reorders candidates. It may well be
worth its dogfooding experiment on its own merits — *after* a quality floor exists, so it reorders
a list already worth looking at. It cannot make a bad list shorter, and treating it as the answer
to this problem would spend the effort in the wrong place.

---

## Appendix — pointers

| What | Where |
|---|---|
| Discovery candidate generation (the truncate-to-limit, no-floor pipeline) | `crates/b2-core/src/discover.rs` |
| The "deliberately permissive" stance | `discover.rs` module doc; `docs/design/index-engine.md` §3 |
| CLI default `--limit 10` | `crates/b2-cli/src/main.rs` (`Similar` command) |
| Desktop pane requesting 10 per note-open | `ui/src/api.ts` (`similar`), `ui/src/main.ts` (`refreshDiscovery`) |
| Raw score display | `ui/src/render.ts` (discovery card score) |
| Vector normalization (what makes a threshold well-defined) | `crates/b2-embed/src/model.rs` (`l2_normalize`) |
| Model identity the threshold should key on | `meta` (`embed_model_id`, `embed_dim`); device tagging per GH #40 |
| Eval harness, discovery half | `crates/b2-embed/examples/eval.rs` (`score_similar`), `crates/b2-embed/evals/similar.json` |
| Rank-stability probe (the model-free half) | `just stability`, GH #141 |
| Search plumbing arc (keep) | GH #137, #140, #141, #142, PR #144 |
| Search-side reranker (excludes `similar`) | GH #28 |
| Graph-distance reordering experiment | GH #20 |
| Chunker quality gate | GH #44 |
