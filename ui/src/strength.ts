// The discovery card's strength band (GH #150). The card used to print the raw
// engine score — negated L2 like `-0.734`, a unit nobody should have to know —
// and the honest replacement is a band derived from the candidate's z: how far
// it stands above the anchor's own candidate population, the same number the
// discovery floor judged it by. Everything shown has already cleared the floor,
// so the bands grade *within* the shown, from "cleared the bar" to "towers over
// the field". A candidate with no z (floor off or inert) gets no band — no
// statistic was computed, so none is claimed.
//
// The z these read is the **stage-2 best-passage** z since GH #192 — the
// candidate's best matching passage against the anchor's own candidate
// population, which is also the excerpt the card shows and the number the floor
// gates on. The thresholds below were re-read in that unit (GH #182); before
// that they were the *centroid* z's, and left in place across the reorder they
// had stopped meaning anything: `●●●` needed z ≥ 3.0, which nothing in the
// labelled corpus reaches in the new unit, so the strongest genuine relation in
// the vault (`bicycle.md ↔ bike-maintenance.md`, +2.87) painted the same two
// dots as a middling one. A band in the wrong unit is worse than no band: it is
// a claim about strength that quietly grades everything down.
//
// The landmarks are the floor's own bars and the labelled-mate population, so
// each dot count says something a reader could check:
//
//   ●○○  above the member bar but under the leader bar — it is on the list
//        because a stronger sibling carried it, not on its own account
//   ●●○  at or above the leader bar — strong enough to have carried the list by
//        itself, which is what the engine's leader gate means
//   ●●●  at or above the labelled-mate population's upper quartile — the top of
//        what a human-confirmed relation reads on the eval corpus. Note the
//        rounding direction: the quartile measured +2.529, so the bar rounds
//        *down* to 2.52. Rounding up would put the bar past the very mate that
//        set the landmark, which is an accident of decimals rather than a
//        decision about strength.
//
// Which numbers those are today is a measurement, not a constant of nature:
// `just eval`'s **floor calibration** block re-derives the bars and dumps the
// mate population on every run, and `results.jsonl`'s `discovery_z` records it.
// Read them there before moving the two numbers below — the lesson of GH #187
// is that a measured window frozen into a comment goes stale silently, so this
// one cites its instrument instead of quoting a window as timeless. (The bars
// themselves live in `discover.rs`'s `DiscoveryFloor`; a dense real vault can
// read higher throughout, which costs the top band resolution but never
// correctness — the dots grade within one list.)

/** How many candidates a note needs before any of them can be graded — the UI-side
 *  mirror of `discover.rs`'s `FLOOR_MIN_POPULATION`. A z over a handful of distances is
 *  noise, so under this the floor stays inert and every candidate arrives with no z.
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
