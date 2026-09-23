// The Explain view's words and geometry (GH #236): pure functions over a
// `SimilarExplainView`, so node tests them straight off the source and render.ts only
// paints. The engine decides the facts (rank, z, pairs, standing); this module decides
// how they read. It never decides what a card *is*.
//
// Every grade here is on the strength dots' yardstick (strength.ts): a pair's z is its
// squared distance read against the same population the card's z is, so "a passage pair
// at ●●○" means "as near as a clear candidate's best pair". No raw distance is shown,
// for the reason search-and-similarity.md §2 gives: its meaning depends on the vault.
//
// The summary label is chosen by fixed rules, in order, and grades nothing:
//
//   1. *Identical text*: the best pair is the same passage in both notes (a template or a
//      copy). First, because it is the one case where a high rank says nothing about
//      what the notes are about, and a real vault has dozens of them (GH #235).
//   2. No label when the field is ungraded: no z, so no landmark to count against.
//   3. Count the candidate's passages whose pair reaches the ●●○ landmark (CLEAR_Z):
//      none → *No passage stands out*; its only passage → *A clear match*; exactly one
//      of several → *One section matches*; at least half → *Broadly similar*;
//      otherwise → *A few sections match*.

import type { SimilarExplainView } from "./types.ts";
import { CLEAR_Z, STRONG_Z } from "./strength.ts";

/** How many passage pairs the view shows before *Show all*. */
export const EXPLAIN_PAIRS_SHOWN = 5;

export interface ExplainSummary {
  label: string;
  detail: string;
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** The one-line reading of the pairs, or null when there is nothing to say honestly. */
export function explainSummary(v: SimilarExplainView): ExplainSummary | null {
  const pairs = v.pairs;
  if (pairs.length === 0) return null;
  if (pairs[0].identical) {
    return {
      label: "Identical text",
      detail:
        "The closest match is the same passage in both notes, often a template or a copy. It says little about what the two notes are about.",
    };
  }
  if (pairs.some((p) => p.z === null)) return null;
  const n = pairs.length;
  const clear = pairs.filter((p) => (p.z as number) >= CLEAR_Z).length;
  if (clear === 0) {
    return {
      label: "No passage stands out",
      detail: `None of its ${plural(n, "passage")} reaches the ●●○ mark. It is here because it is among the nearest notes, not because one passage is a clear match.`,
    };
  }
  if (n === 1) {
    return { label: "A clear match", detail: "Its only passage matches clearly." };
  }
  if (clear === 1) {
    return {
      label: "One section matches",
      detail: `1 of its ${n} passages matches clearly. The rest of the note is about other things.`,
    };
  }
  return {
    label: clear * 2 >= n ? "Broadly similar" : "A few sections match",
    detail: `${clear} of its ${n} passages match clearly.`,
  };
}

/** Where the note stands, in words: why it is a card, or why it is not. */
export function standingText(v: SimilarExplainView): string {
  const s = v.standing;
  switch (s.kind) {
    case "ranked":
      return s.served
        ? `Card #${s.rank} of the ${v.limit} shown · ${s.of} notes compared`
        : `Ranked #${s.rank} of ${s.of} compared, past the ${v.limit} shown`;
    case "linked":
      return "Already linked, so it isn’t suggested";
    case "unembedded":
      return "Not embedded yet, so it can’t be compared";
    case "anchor_unembedded":
      return "This note isn’t embedded yet, so there is nothing to compare from";
    case "not_shortlisted":
      return `Never compared passage by passage: as a whole note it ranks #${
        v.centroid_rank ?? "?"
      }, past the first ${s.shortlist}`;
    case "same_note":
      return "This is the note itself";
  }
}

/**
 * Whole-note rank beside best-passage rank, when they differ: the buried-gem signal.
 * Null when there is no ranked standing or the two agree (nothing to point out).
 */
export function wholeNoteText(v: SimilarExplainView): string | null {
  if (v.standing.kind !== "ranked" || v.centroid_rank === null) return null;
  if (v.centroid_rank === v.standing.rank) return null;
  return `Judged as a whole note it ranks #${v.centroid_rank}. Its best passage ranks it #${v.standing.rank}.`;
}

/** The where-it-sits strip, as fractions of its width in [0, 1]. */
export interface FieldStrip {
  /** Every compared note's z. */
  dots: number[];
  /** This note's z, when it has one. */
  marker: number | null;
  /** The ●●○ and ●●● landmarks. */
  clear: number;
  strong: number;
  /** Axis labels at whole σ steps (every other step on a very wide strip). 0σ, the
   *  average of the notes compared, is always among them. */
  ticks: { at: number; label: string }[];
}

/** Past this many σ of width, label every other step so the labels don't collide. */
const TICK_WIDE_SIGMA = 8;

/**
 * Lay the population out on one axis. The axis always spans both landmarks, so a
 * compressed field (every candidate middling, GH #196) reads as a cluster left of
 * the marks, not as a spread that fills the strip. Null when ungraded.
 */
export function fieldStrip(population: number[], z: number | null): FieldStrip | null {
  const finite = population.filter(Number.isFinite);
  if (finite.length === 0) return null;
  const lo = Math.min(0, ...finite, z ?? 0);
  const hi = Math.max(STRONG_Z + 0.5, ...finite, z ?? 0);
  const at = (x: number) => (x - lo) / (hi - lo);
  const step = hi - lo > TICK_WIDE_SIGMA ? 2 : 1;
  const ticks = [];
  for (let t = Math.ceil(lo / step) * step; t <= hi; t += step) {
    ticks.push({ at: at(t), label: `${t < 0 ? "−" : ""}${Math.abs(t)}σ` });
  }
  return {
    dots: finite.map(at),
    marker: z === null || !Number.isFinite(z) ? null : at(z),
    clear: at(CLEAR_Z),
    strong: at(STRONG_Z),
    ticks,
  };
}

/** The two notes' display names: titles, else paths, and paths for both when the titles
 *  are the same (a vault of `polish.md` files), because this view's whole job is to say
 *  which side is which. */
export function explainNames(v: SimilarExplainView): { anchor: string; candidate: string } {
  const anchor = v.anchor.title ?? v.anchor.path;
  const candidate = v.candidate.title ?? v.candidate.path;
  return anchor === candidate
    ? { anchor: v.anchor.path, candidate: v.candidate.path }
    : { anchor, candidate };
}

/** The strip's caption: whose strip it is, and how many compared notes reach each band. */
export function fieldCaption(population: number[], anchor: string): string {
  const n = population.length;
  const strong = population.filter((z) => z >= STRONG_Z).length;
  const clear = population.filter((z) => z >= CLEAR_Z).length - strong;
  return `The ${plural(n, "note")} closest to ${anchor} · ${strong} at ●●● · ${clear} at ●●○ · ${
    n - strong - clear
  } below`;
}

/** The strip's longer account, behind its "?". It names both notes, because the strip
 *  is drawn from the anchor's side and that is the thing most easily misread: what a dot
 *  is, what the axis measures, what the dashed lines are (and aren't), and what not to
 *  read into the spread. */
export function fieldHelp(v: SimilarExplainView): string[] {
  const n = v.population.length;
  const { anchor, candidate } = explainNames(v);
  return [
    `This strip is drawn from ${anchor}’s side. Each dot is one of the ${n} notes closest to ${anchor}, placed by how close its best passage gets to any passage in ${anchor}. The blue line is ${candidate}.`,
    `The axis is σ (standard deviations) from the average of those ${n}. 0σ is their average; further right is closer.`,
    `The dashed lines are fixed cut-offs, the same on every note’s strip: ●●○ starts at ${CLEAR_Z}σ and ●●● at ${STRONG_Z}σ. They were measured on B2’s test notes, where a person marked which pairs really belong together, not on your vault.`,
    "They set the dots on each card in the Similar list. They don’t decide what is listed: the closest notes are shown whatever their grade.",
    `The ${n} are the closest to ${anchor} judged as whole notes, out of every note not already linked. So 0σ is the average of the closest, not of the whole vault, and every note’s strip has its own scale. The same pair can grade differently from ${candidate}’s side.`,
    "Gaps between dots are ordinary spread, not groups of topics.",
  ];
}
