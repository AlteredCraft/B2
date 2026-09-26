// Find-in-note's editor engine (⌘F while editing): the match state as a CodeMirror
// StateField, so the highlights re-derive on every doc change — typing with the bar open
// keeps them honest, with no listener → dispatch round-trip.
//
// findbar.ts owns the matching and stepping; this owns only how that state lives in an
// EditorState and paints as decorations. main.ts dispatches `setFindEffect` and reads the
// field back to mirror it into the bar. No DOM, so node tests it on real EditorStates
// (`npm test`), the droplink.test.ts rig.

import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import { activeAfter, findMatches, type Match } from "./findbar.ts";

/** The bar's query, its matches in the current doc, and which one is active (-1: none). */
export interface EditorFind {
  readonly query: string;
  readonly matches: Match[];
  readonly active: number;
}

/** Set the query and the wanted active match — or null to close the bar's engine. */
export const setFindEffect = StateEffect.define<{ query: string; active: number } | null>();

const findMark = Decoration.mark({ class: "find-match" });
const findMarkActive = Decoration.mark({ class: "find-match is-active" });

/** Every match as a mark, the active one distinguished. */
export function findDecorations(v: EditorFind | null): DecorationSet {
  if (!v) return Decoration.none;
  return Decoration.set(
    v.matches.map((m, i) => (i === v.active ? findMarkActive : findMark).range(m.from, m.to)),
  );
}

export const findField = StateField.define<EditorFind | null>({
  create: () => null,
  update(value, tr) {
    let next = value;
    for (const ef of tr.effects) {
      if (ef.is(setFindEffect))
        next = ef.value && { query: ef.value.query, matches: [], active: ef.value.active };
    }
    if (!next) return null;
    if (next === value && !tr.docChanged) return value;
    const matches = findMatches(tr.newDoc.toString(), next.query);
    const active =
      next !== value || value === null
        ? matches.length === 0
          ? -1
          : Math.max(0, Math.min(next.active, matches.length - 1))
        : // A doc edit: re-anchor on where the old active match ended up.
          activeAfter(
            matches,
            value.active >= 0 && value.matches[value.active]
              ? tr.changes.mapPos(value.matches[value.active].from)
              : 0,
          );
    return { query: next.query, matches, active };
  },
  provide: (f) => EditorView.decorations.from(f, findDecorations),
});
