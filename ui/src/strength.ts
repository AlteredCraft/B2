// The discovery card's strength band (GH #150). The card used to print the raw
// engine score — negated L2 like `-0.734`, a unit nobody should have to know —
// and the honest replacement is a band derived from the candidate's z: how far
// it stands above the anchor's own candidate population, **relative to this
// note's other candidates and nothing more**. Since GH #197 the list itself is
// always the ranked nearest — no statistic gates what is served — so the bands
// are the whole quality signal: they grade *within* the shown, and "every card
// is middling" is an honest reading the retired empty pane could never give.
// A candidate with no z (small pool, no spread, fake space) gets no band — no
// statistic was computed, so none is claimed.
//
// The z these read is the **stage-2 best-passage** z since GH #192 — the
// candidate's best matching passage against the anchor's own candidate
// population, which is also the excerpt the card shows. The thresholds below
// were re-read in that unit (GH #182); before that they were the *centroid*
// z's, and left in place across the reorder they had stopped meaning anything —
// a band in the wrong unit is worse than no band: it is a claim about strength
// that quietly grades everything down.
//
// The landmarks are measured on the eval corpus's labelled populations (they
// began as the retired existence gate's bars, GH #150/#192 — kept as landmarks
// after GH #197 retired the gate, because they still describe where labelled
// mates read on the corpus):
//
//   ●○○  under the 1.96 landmark — near, in this list's own terms
//   ●●○  at or above 1.96 — where the corpus's labelled leaders read
//   ●●●  at or above the labelled-mate population's upper quartile — the top of
//        what a human-confirmed relation reads on the eval corpus. Note the
//        rounding direction: the quartile measured +2.529, so the bar rounds
//        *down* to 2.52. Rounding up would put the bar past the very mate that
//        set the landmark, which is an accident of decimals rather than a
//        decision about strength.
//
// Which numbers those are today is a measurement, not a constant of nature:
// `make eval`'s **discovery z calibration** block dumps the populations on
// every run (`results.jsonl`'s `discovery_z`), and `make calibrate` reads the
// same bands on any real vault. Read them there before moving the two numbers
// below — the lesson of GH #187 is that a measured window frozen into a
// comment goes stale silently, so this one cites its instrument instead of
// quoting a window as timeless. A known open question rides on them: on a
// dense single-domain vault every z compresses (GH #196), so the bands can
// read uniformly low there — measured by `make calibrate`'s band histogram
// (GH #197's A6), with the re-reference decision deferred to Phase 2.

/** How many candidates a note needs before any of them can be graded — the UI-side
 *  mirror of `discover.rs`'s `STATS_MIN_POPULATION`. A z over a handful of distances
 *  is noise, so under this no statistic is computed and every candidate arrives with
 *  no z — banding changes at this threshold, membership never does (GH #197).
 *  Duplicated across the language boundary on purpose: the number is *copy* here — the
 *  one thing the ungraded caveat can tell a reader to act on — and the alternative is
 *  widening the IPC contract to carry a constant that has moved once. If the Rust
 *  constant moves, this line moves with it. */
export const STRENGTH_MIN_CANDIDATES = 12;

export interface StrengthBand {
  /** Three-dot glyph for the card (`●●○`). */
  glyph: string;
  /** The accessible name — what a screen reader calls the band. */
  label: string;
  /** Tooltip prose: the z spelled out for whoever wants the number. */
  title: string;
  /** The bare figure (`2.5σ`), which the *selected* card reveals beside the dots.
   *  The tooltip says the same thing in prose, but a tooltip needs a pointer —
   *  this is how the number reaches a keyboard (K1). */
  value: string;
}

export function strengthBand(z: number | undefined | null): StrengthBand | null {
  if (z === undefined || z === null || !Number.isFinite(z)) return null;
  const [glyph, label] = z >= 2.52
    ? ["●●●", "strong match"]
    : z >= 1.96
      ? ["●●○", "clear match"]
      : ["●○○", "near match"];
  const value = `${z.toFixed(1)}σ`;
  return {
    glyph,
    label,
    value,
    title: `${label} — stands ${value} above this note's other candidates`,
  };
}
