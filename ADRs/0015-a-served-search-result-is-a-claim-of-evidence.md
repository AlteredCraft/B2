# ADR-0015 — A served search result is a claim of evidence

- **Status:** Accepted · 2026-08-22
- **Refs:** invariants D2 · GH #201, #202, #206

## Context

Flow ②'s vector half always has *k* nearest — nearest is a fact about the vault, never evidence
about the query — and RRF fuses ranks, discarding the absolute signals that could tell the
difference. As first shipped, the nonsense query `shjfasd` served ten confident-looking results. The
defect was structural, not a loose threshold.

## Decision

- `hybrid_search` returns a `Retrieval`: the **same fused order**, plus per hit its rank in each list
  and its own distance, plus a query-level reading. `Vault::search_evidence` serves exactly the rows
  `search` does, in the same order.
- The rule is **lexical OR semantic** — two independent signals, precisely so it can tell "nothing
  matches" from "everything matches", which one signal cannot.
- The lexical half is **IDF-weighted term coverage** (how much of the query's own weight the vault
  carries), *not* a df ceiling: the ceiling classed `drone` and `comb` as stopwords in a beekeeping
  vault and cut 3 of 15 queries naming notes the vault holds. A fraction of chunks is scale-free in a
  vault's size but not in its topical concentration. **The rule changed; the number was not re-tuned.**
- The semantic half is a cosine bar **keyed to `embed_model_id`** (device suffix included), calibrated
  in the harness and re-derived every run.
- The verdict is **three-state, each state a different behaviour**:
  - **evidence found** → serve as always;
  - **no evidence** → the honest "no matches" empty state and **none** of the rows — *strict*: no
    reveal, no `--all`, no expander, since a fold is still a surface putting the rows forward, and no
    disclosure boundary exists to put them behind (ADR-0014);
  - **no calibrated bar for the active model** → serve as always, never "no matches" — the fake
    embedder and every unmeasured model land here, and folding this into "no evidence" would blank a
    dev vault.

## Consequences

- `b2 search --json` is now an **object** (rows + verdict) — a documented break of the array
  contract, since a query-level verdict has nowhere to live in a list of rows. It keeps serving the
  rows at `vouched: false` where human surfaces show none: an agent handed rows *plus* a verdict can
  be honest about them; a reader given rows alone cannot.
- The frontend applies the rule at **one** boundary (`doSearch` drops the rows, so
  `state.searchResults` *is* what the pane serves), never by branching in each painter.
- The nearest list is unreachable from the human surface for that query. That cost is accepted
  because it is bounded to **one query**, never a whole vault — the distinction from ADR-0014.
- Exit-gate rows: zero labelled negatives served, zero labelled positives cut, zero dense-fixture
  title-queries cut — structural zeros, no headroom. Discovery rows are unchanged, because search's
  rule moves no discovery rank; movement there would be a bug.
- The per-hit **tail** fold is unshipped — since GH #206 (2026-08-25) by **measurement** rather
  than by missing labels: `tail_relevant` deepened the labels to per-hit exhaustiveness, a
  four-family prefix-cut bake-off ran on both corpora plus a real vault, and every admissible
  family is vacuous (2–23 of the 367 rows an oracle fold reaches), because the fused order is not
  an evidence order and D1 rightly forbids a fold from re-sorting it. The tail complaint is an
  ordering problem — standing evidence for the reranker seam, not for a disclosure rule. The
  bake-off re-derives this verdict every `make eval` run.
