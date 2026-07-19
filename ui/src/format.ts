// Inline formatting, the pure half — the ⌘B/⌘I toggle engine, and the table future
// chords extend. main.ts derives the CodeMirror keymap from `FORMATS` and dispatches
// what `toggleInline` computes; this module never touches the editor, so it runs
// under plain node (the wikicomplete.ts / move.ts pattern).
//
// Adding a format later (strikethrough, inline code, highlight) is one new row in
// `FORMATS` — the engine is generic over the marker. The one wrinkle it encodes:
// bold (`**`) and italic (`*`) share a character, so "is this format already here?"
// is a *parity* question on the star run (`*a*` italic, `**a**` bold, `***a***`
// both), not a substring match.

/** One inline mark: its Markdown marker and the key chord that toggles it. */
export interface InlineFormat {
  id: string;
  /** The delimiter written on each side of the content (`**`, `*`, `~~`, …). */
  marker: string;
  /** The CodeMirror key name main.ts binds (`Mod-` = ⌘ on macOS, Ctrl elsewhere). */
  key: string;
}

export const BOLD: InlineFormat = { id: "bold", marker: "**", key: "Mod-b" };
export const ITALIC: InlineFormat = { id: "italic", marker: "*", key: "Mod-i" };

/** The keymap's source of truth — extend here and the binding exists. */
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
 * Toggle `fmt` over `[from, to]` of `doc`. Returns the edits (original-doc
 * coordinates) and the selection to land on (post-edit coordinates).
 *
 * The gesture, matched to what an editor hand expects:
 * - a selection wraps, or unwraps if the format is already around it (marker
 *   characters at the selection's own edges count as the wrapper, so selecting
 *   `**word**` whole behaves like selecting `word`);
 * - a bare cursor toggles the word under it, left selected for a follow-up chord;
 * - a bare cursor with no word inserts an empty pair with the caret centered, and
 *   toggling again inside removes it.
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
    // Shrink the selection past edge marker characters: the wrapper, if the user
    // grabbed it, is re-detected as the *surrounding* run below.
    while (from < to && doc[from] === ch) from++;
    while (to > from && doc[to - 1] === ch) to--;
  }

  // The format is "present" by the run of marker characters hugging the content.
  // Star/underscore emphasis stacks (`***a***` = bold + italic), so a 1-char
  // emphasis marker is present on an odd run; everything else on run >= marker.
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
