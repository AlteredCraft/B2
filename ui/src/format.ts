// Inline formatting, the pure half: the ⌘B/⌘I toggle engine and the ⌘T table. main.ts
// pairs each `FORMATS` row with its `format.<id>` chord in bindings.ts to build the keymap.
// Bold and italic share `*`, so "already formatted?" is a parity question on the star run.

/** One inline mark and its Markdown marker. Its chord is `format.<id>` in bindings.ts. */
export interface InlineFormat {
  id: string;
  /** The delimiter written on each side of the content (`**`, `*`, `~~`, …). */
  marker: string;
}

export const BOLD: InlineFormat = { id: "bold", marker: "**" };
export const ITALIC: InlineFormat = { id: "italic", marker: "*" };

/** The keymap's source: a new row needs its `format.<id>` chord in bindings.ts. */
export const FORMATS: InlineFormat[] = [BOLD, ITALIC];

/** One text edit in original-document coordinates (CodeMirror's change shape). */
export interface FormatChange {
  from: number;
  to: number;
  insert: string;
}

const WORD = /[\p{L}\p{N}_]/u;

/** Length of the run of `ch` immediately before (`dir` -1) or at/after (`+1`) `i`. */
function runLen(doc: string, i: number, ch: string, dir: -1 | 1): number {
  let n = 0;
  let p = dir === -1 ? i - 1 : i;
  while (p >= 0 && p < doc.length && doc[p] === ch) {
    n++;
    p += dir;
  }
  return n;
}

/**
 * Toggle `fmt` over `[from, to]` of `doc`. Returns the edits (original-doc coordinates)
 * and the selection to land on (post-edit coordinates).
 * - a selection wraps, or unwraps if already formatted (selecting `**word**` whole counts);
 * - a bare cursor toggles the word under it, left selected;
 * - a bare cursor with no word inserts an empty pair with the caret inside.
 */
export function toggleInline(
  doc: string,
  from: number,
  to: number,
  fmt: InlineFormat,
): { changes: FormatChange[]; selFrom: number; selTo: number } {
  const ch = fmt.marker[0];
  const len = fmt.marker.length;

  if (from === to) {
    // A bare cursor targets the word under it (nothing found leaves from === to).
    while (from > 0 && WORD.test(doc[from - 1])) from--;
    while (to < doc.length && WORD.test(doc[to])) to++;
  } else {
    // Shrink past edge markers; a grabbed wrapper is re-detected as the surrounding run.
    while (from < to && doc[from] === ch) from++;
    while (to > from && doc[to - 1] === ch) to--;
  }

  // Emphasis stacks (`***a***` = bold + italic), so a 1-char `*`/`_` marker is present on
  // an odd run; anything else on run >= marker.
  const k = Math.min(runLen(doc, from, ch, -1), runLen(doc, to, ch, 1));
  const stacking = len === 1 && (ch === "*" || ch === "_");
  const present = stacking ? k % 2 === 1 : k >= len;

  if (present) {
    return {
      changes: [
        { from: from - len, to: from, insert: "" },
        { from: to, to: to + len, insert: "" },
      ],
      selFrom: from - len,
      selTo: to - len,
    };
  }
  if (from === to) {
    return {
      changes: [{ from, to, insert: fmt.marker + fmt.marker }],
      selFrom: from + len,
      selTo: from + len,
    };
  }
  return {
    changes: [
      { from, to: from, insert: fmt.marker },
      { from: to, to, insert: fmt.marker },
    ],
    selFrom: from + len,
    selTo: to + len,
  };
}

/** The ⌘T scaffold: a 3-column GFM table, a header row + two empty body rows. */
const TABLE_TEMPLATE = [
  "| Column 1 | Column 2 | Column 3 |",
  "| --- | --- | --- |",
  "|  |  |  |",
  "|  |  |  |",
].join("\n");

/**
 * Insert a fresh table at `[from, to]` (⌘T), padded to a blank line on each side only as
 * much as the neighbours lack. The caret lands in the first body cell.
 */
export function insertTable(
  doc: string,
  from: number,
  to: number,
): { changes: FormatChange[]; selFrom: number; selTo: number } {
  const nlBefore = (/\n*$/.exec(doc.slice(0, from))?.[0].length) ?? 0;
  const nlAfter = (/^\n*/.exec(doc.slice(to))?.[0].length) ?? 0;
  const lead = from === 0 ? "" : "\n".repeat(Math.max(0, 2 - nlBefore));
  const trail = to === doc.length ? "\n" : "\n".repeat(Math.max(0, 2 - nlAfter));
  const insert = lead + TABLE_TEMPLATE + trail;
  // Caret into the first body cell: after the "| " of the first empty row.
  const pos = from + lead.length + TABLE_TEMPLATE.indexOf("|  |  |  |") + 2;
  return { changes: [{ from, to, insert }], selFrom: pos, selTo: pos };
}
