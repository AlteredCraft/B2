// The discovery card's strength band (GH #150). The card used to print the raw
// engine score — negated L2 like `-0.734`, a unit nobody should have to know —
// and the honest replacement is a band derived from the candidate's z: how far
// it stands above the anchor's own candidate population, the same number the
// discovery floor judged it by. Everything shown has already cleared the floor
// (z ≥ 1.85), so the bands grade *within* the shown, from "cleared the bar" to
// "towers over the field"; the thresholds match the calibration's measured
// landmarks (GH #150's calibration: labelled mates 1.9–3.0, dense-vault
// leaders up to ~6). A candidate with no z (floor off or inert) gets no band —
// no statistic was computed, so none is claimed.

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
  const [glyph, label] = z >= 3.0
    ? ["●●●", "strong match"]
    : z >= 2.3
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
