# ADR-0014 — Discovery always serves the ranked top-N; no anchor-local existence gate

- **Status:** Accepted · 2026-08-18 (bake-off closed 2026-08-22) — supersedes the z-score
  `DiscoveryFloor`
- **Refs:** invariants D1 · GH #182, #192, #196, #197, #200

## Context

Discovery shipped with a **z-score existence gate**: a candidate had to stand out from its anchor's
own pool to be served. Dogfooding a real **single-domain** vault broke it in one reading — with no
unrelated tail, every leader's z compresses under the bar, and **16 of 17 panes went dark on a vault
whose ranking was correct throughout**. The defect was the gate, not its calibration: a
single-population outlier test cannot distinguish *nothing is related* from *everything is related*.

**The evidence trail**, kept as citations rather than narrative:

| Issue | What it measured | Verdict |
|---|---|---|
| [#150](https://github.com/AlteredCraft/B2/issues/150) | absolute cosine floors vs. drop-off-from-top-1, against labelled score piles | both fail; shipped a per-anchor **z-score floor** instead |
| [#183](https://github.com/AlteredCraft/B2/issues/183) | a **multi-topic note family** + a **per-mate** metric (the per-anchor one saturates at an anchor's easiest mate) | the centroid-z order both demoted and **suppressed** labelled mates — 3 of 14 never served |
| [#187](https://github.com/AlteredCraft/B2/issues/187) | every candidate's ungated z, with both admissible windows re-derived each run | the **member bar has no window**: mates from +0.80, strangers to +1.62 |
| [#189](https://github.com/AlteredCraft/B2/issues/189) | the journal/daily-note archetype (N≥6 unrelated sections) | averaging disagreeing chunk vectors collapses the centroid toward the corpus mean — the note tops *loner* anchors while its own gem is suppressed |
| [#192](https://github.com/AlteredCraft/B2/issues/192) | judging **after stage 2, on the best-passage z** — the number that orders the list | the unit separates what the centroid could not; 14/15 mates, 0 strangers |
| [#182](https://github.com/AlteredCraft/B2/issues/182) | the desktop band, still calibrated on the *centroid* z after the reorder | bands re-read in the judged unit; the standing rule below |
| [#196](https://github.com/AlteredCraft/B2/issues/196)/[#197](https://github.com/AlteredCraft/B2/issues/197) | the first real vault dogfooded: single-domain, 17 notes | **16 of 17 panes dark on correct rankings** — the gate retired |

The finding that generalizes: a z-score treats the anchor's population mean as a *noise floor*, valid
only when related notes are rare outliers in a dominant unrelated tail. A single-domain vault has no
such tail — the mean *is* "moderately related" — so the test reads *everything is related* as
*nothing is*. "One diffuse cloud" and "a coherent single-subject vault" are the same geometry from
opposite ends. Large vaults reproduce it inside their own shortlist: above `SHORTLIST_MIN` the judged
population is the anchor's centroid-nearest slice, pre-selected related.

## Decision

- **Ranking, reachability, and default disclosure are three separate questions.** Ranking is
  relative (best-passage distance); the full ranked list stays **reachable**; `limit` is a cap that
  under-fills only for want of scorable notes.
- **No anchor-local statistic may make a candidate unreachable.** The existence gate is retired, not
  re-tuned; empty states say only "nothing to compare".
- The z survives **ungated**, as the strength band's input. The band is a within-list grading and
  gates nothing.
- A quality signal **may** set the default **disclosure boundary** later, but only as a *prefix* of
  the ranked order (row order, band, and fold must never visibly disagree), with everything below one
  gesture away — so a misjudged fold costs a keystroke where the retired gate cost the feature. Such
  a signal must win a measured bake-off on the orthogonal corpus, the dense single-domain fixture
  (where a non-empty default view is absolute), and real vaults — with **"no fold at all" an
  admissible winner**.
- **The first bake-off found none** (mutual-kNN reciprocity, GH #200): its admissible window is
  empty in both directions, and the two corpora's safe depths are the same *fraction* of their
  candidate pools rather than the same constant — a reciprocity depth is a rank in a population and
  transfers no better than the cosine and z constants already retired. The block stays in `just eval`
  reporting-only, to price the next candidate.

## Consequences

- The judged statistic is the same number that orders the list and paints the band, so the three
  cannot disagree. **A change to the judged statistic is a change to every surface that paints it** —
  the standing rule (GH #182: reordering left the desktop's bands calibrated in the old unit, and its
  top band unreachable).
- With no fold, honesty rides on the strength band and the empty-state copy; the dogfood complaint
  that opened GH #200 stays **unpaid** on the discovery axis.
- The harness keeps a **structural-zero tripwire** that re-arms on its own if a fold ever ships.
