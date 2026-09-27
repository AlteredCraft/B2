// The discovery card's strength band (GH #150): a band from the candidate's z, graded
// relative to this note's other candidates and nothing more. A candidate with no z gets no
// band: no statistic was computed, so none is claimed.
//
// The z is the stage-2 best-passage z (GH #192), and the thresholds were re-read in that
// unit (GH #182). They are landmarks measured on the eval corpus's labelled populations;
// `make eval` (`discovery_z`) and `make calibrate` measure them, so read those before moving
// them (GH #187). On a dense single-domain vault every z compresses (GH #196).

/** How many candidates a note needs before any can be graded. Mirrors `discover.rs`'s
 *  `STATS_MIN_POPULATION` as copy for the ungraded caveat; change them together. */
export const STRENGTH_MIN_CANDIDATES = 12;

export interface StrengthBand {
  /** Three-dot glyph for the card (`●●○`). */
  glyph: string;
  /** The accessible name — what a screen reader calls the band. */
  label: string;
  /** Tooltip prose with the z spelled out. */
  title: string;
  /** The bare figure (`2.5σ`), revealed on the selected card so the keyboard gets the
   *  number too (K1). */
  value: string;
}

/** The ●●● landmark: the labelled-mate population's upper quartile (+2.529), rounded
 *  down so the bar doesn't pass the mate that set it. */
export const STRONG_Z = 2.52;
/** The ●●○ landmark: where the corpus's labelled leaders read. */
export const CLEAR_Z = 1.96;

export function strengthBand(z: number | undefined | null): StrengthBand | null {
  if (z === undefined || z === null || !Number.isFinite(z)) return null;
  const [glyph, label] = z >= STRONG_Z
    ? ["●●●", "strong match"]
    : z >= CLEAR_Z
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
