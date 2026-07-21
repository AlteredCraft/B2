// Find-in-note (⌘F) — the pure logic. Match scanning, active-match stepping, and the
// flat-offset → text-node mapping the reading view needs to build DOM Ranges. No DOM
// and no CodeMirror here: main.ts adapts these over whichever surface is showing
// (rendered Markdown via the CSS Custom Highlight API, or the editor via decorations),
// the same pure-module/adapter split as format.ts and wikicomplete.ts.

/** One match, as flat text offsets — the same shape for both surfaces. */
export type Match = { from: number; to: number };

/** Bound on a scan, so a one-letter query over a huge note can't build 10⁵ ranges. */
export const FIND_CAP = 1000;

/**
 * Every occurrence of `query` in `text`: literal (metacharacters find themselves),
 * case-insensitive, non-overlapping, capped at `cap`. An empty query matches nothing —
 * the bar treats "" as "no search", never "match everything".
 */
export function findMatches(text: string, query: string, cap = FIND_CAP): Match[] {
  const out: Match[] = [];
  if (!query) return out;
  // A regex with the `i` flag folds case without re-writing the haystack, so offsets
  // are honest even where `toLowerCase()` would change a string's length (İ → i̇).
  const literal = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
  let m: RegExpExecArray | null;
  while (out.length < cap && (m = literal.exec(text)) !== null) {
    out.push({ from: m.index, to: m.index + m[0].length });
  }
  return out;
}

/** Step the active index by ±1 with wrap-around; -1 (stays -1) when there are no matches. */
export function stepActive(count: number, active: number, delta: 1 | -1): number {
  if (count <= 0) return -1;
  return (active + delta + count) % count;
}

/**
 * Re-anchor the active match after the set changes (a query keystroke, a doc edit):
 * the first match at-or-after `pos`, else the last one, else -1. Keeps "next" moving
 * forward from where the user was instead of snapping back to the top of the note.
 */
export function activeAfter(matches: Match[], pos: number): number {
  if (matches.length === 0) return -1;
  const at = matches.findIndex((m) => m.from >= pos);
  return at === -1 ? matches.length - 1 : at;
}

/** The bar's "3 / 17" pill; `capped` marks the count as a floor ("… / 1000+"). */
export function countLabel(count: number, active: number, capped = false): string {
  if (count === 0) return "0 / 0";
  return `${active + 1} / ${count}${capped ? "+" : ""}`;
}

/**
 * Map a flat offset into (text-node index, offset within it), given the nodes' text
 * lengths in document order. `bias` settles boundary offsets — a Range *start* opens
 * the next node, a Range *end* closes the previous one — so a match's endpoints always
 * land inside the nodes that actually hold its characters. Out-of-range clamps.
 */
export function locate(
  segLengths: number[],
  offset: number,
  bias: "start" | "end",
): { seg: number; off: number } {
  let cum = 0;
  for (let i = 0; i < segLengths.length; i++) {
    const end = cum + segLengths[i];
    if (bias === "start" ? offset < end : offset <= end) return { seg: i, off: offset - cum };
    cum = end;
  }
  const last = segLengths.length - 1;
  return last < 0 ? { seg: 0, off: 0 } : { seg: last, off: segLengths[last] };
}
