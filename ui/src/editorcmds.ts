// The editor's commands — the CodeMirror adapters over the pure engines (format.ts,
// list.ts, paste.ts, wikicomplete.ts). Each is split in two: a *plan* that reads an
// EditorState and says what should happen (a transaction, or a decline), and a `run…`
// that dispatches it on a view. The plans are pure, so node tests them on real
// EditorStates parsed by the app's own grammar (editorcmds.test.ts, the droplink.test.ts
// rig); main.ts only builds the keymap and the extensions out of the `run…` halves.

import type { CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import { EditorSelection, type EditorState, type TransactionSpec } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { inCodeAt } from "./droplink.ts";
import { insertTable, toggleInline, type InlineFormat } from "./format.ts";
import type { ListEdit } from "./list.ts";
import { markdownForPaste } from "./paste.ts";
import type { NoteSummary, ResourceSummary } from "./types.ts";
import { wikiCandidates, wikiInsertion, wikiQueryAt } from "./wikicomplete.ts";

// --- formatting (⌘B / ⌘I, …) ----------------------------------------------------------

/**
 * Toggle an inline format over every selection range. `changeByRange` runs the toggle per
 * range and maps the coordinates, so multi-cursor edits come free. The keymap derives
 * from the `FORMATS` table paired with each row's chord from the keyboard registry
 * (`format.<id>`, bindings.ts), so adding a format is one new row in each.
 */
export function formatPlan(state: EditorState, fmt: InlineFormat): TransactionSpec {
  const doc = state.doc.toString();
  return state.changeByRange((range) => {
    const r = toggleInline(doc, range.from, range.to, fmt);
    return { changes: r.changes, range: EditorSelection.range(r.selFrom, r.selTo) };
  });
}

export function runFormat(view: EditorView, fmt: InlineFormat): boolean {
  view.dispatch(formatPlan(view.state, fmt));
  return true;
}

// --- list nesting (Tab / ⇧Tab) --------------------------------------------------------

/** list.ts's nest or lift, as the key hands it over. */
export type ListShift = (doc: string, from: number, to: number) => ListEdit | null;

/**
 * Tab / ⇧Tab — nest or lift out the list item(s) the selection covers: a transaction to
 * dispatch, `true` to claim the key and do nothing, or `false` to decline it. The engine
 * is list.ts; this is the CodeMirror half, `formatPlan`'s pattern one construct up.
 *
 * Declining matters twice over. A `null` from the engine usually means the caret is not
 * in a list, and declining there is what leaves Tab walking the focus ring through the
 * rest of the app — the reason this isn't `indentWithTab`, which claims the key outright
 * and takes the keyboard's way out of the buffer with it. The one exception is a list the
 * engine can't see — `inListItem` gives the syntax tree the last word, so the no-ejection
 * contract holds there too. And a caret inside code declines before the engine is asked
 * at all: a `- item` line in a fence is text, not structure, and `inCodeContext` is the
 * same read the rich paste makes to keep its hands off code.
 *
 * One range rather than `changeByRange`: an indent moves every offset after it, so a
 * second cursor's edit would be computed against a document the first has already
 * shifted. Multi-cursor nesting is a gesture nobody makes; ⌘B's is one they do.
 */
export function listShiftPlan(state: EditorState, shift: ListShift): TransactionSpec | boolean {
  if (inCodeContext(state)) return false;
  const { from, to } = state.selection.main;
  const r = shift(state.doc.toString(), from, to);
  if (!r) return inListItem(state);
  // Claimed but inert — the first item of a list has nothing to nest under. Swallowing
  // the key is the point (list.ts's header): a gesture that sometimes ejects you from
  // the buffer is worse than one that sometimes does nothing.
  if (r.changes.length === 0) return true;
  return {
    changes: r.changes,
    selection: EditorSelection.range(r.selFrom, r.selTo),
    scrollIntoView: true,
  };
}

export function runListShift(view: EditorView, shift: ListShift): boolean {
  const plan = listShiftPlan(view.state, shift);
  if (typeof plan === "boolean") return plan;
  view.dispatch(plan);
  return true;
}

/** Is the cursor inside a list item the *scanner* can't see? list.ts reads only lists at
 *  the top level of the note — `> - a` is a bullet behind a container prefix it doesn't
 *  parse — so on its null the tree gets the last word before the key is handed back to
 *  the focus ring. Claimed-but-inert there: the caret is visibly on a list item, and
 *  ejecting from it would break the gesture's contract even where the edit itself isn't
 *  built yet (list.ts's header). */
export function inListItem(state: EditorState): boolean {
  const at = syntaxTree(state).resolveInner(state.selection.main.from, -1);
  for (let n: typeof at | null = at; n; n = n.parent) {
    if (n.name === "ListItem") return true;
  }
  return false;
}

// --- ⌘T: a table ----------------------------------------------------------------------

/** A fresh 3-column table (header + two rows) at the cursor, caret in the first cell.
 *  A block insert, not an inline toggle, so it's its own binding over the pure
 *  `insertTable` (format.ts). */
export function insertTablePlan(state: EditorState): TransactionSpec {
  const { from, to } = state.selection.main;
  const r = insertTable(state.doc.toString(), from, to);
  return {
    changes: r.changes,
    selection: EditorSelection.range(r.selFrom, r.selTo),
    scrollIntoView: true,
  };
}

export function runInsertTable(view: EditorView): boolean {
  view.dispatch(insertTablePlan(view.state));
  return true;
}

// --- rich paste -------------------------------------------------------------------------
//
// A copy from a web page carries a `text/html` flavor next to `text/plain`; CodeMirror's
// own paste takes the plain one, which is why every heading, bold and list used to vanish
// on the way in. This converts the HTML to Markdown instead, and *declines* in two cases,
// leaving CodeMirror's paste to run untouched:
//   - the cursor sits in code, where pasted text must stay literal;
//   - the HTML carried no formatting the plain flavor didn't already have (paste.ts's
//     `markdownForPaste` returns null) — pasting escaped Markdown there would be a loss.
// The third way out is the ⌘⇧V chord (main.ts `pastePlain`), which does its own plain paste.

/** Is the cursor inside code — a fenced block, an indented block, or an inline span?
 *  The read itself is droplink.ts's (by position, since a *drop* names a place the cursor
 *  isn't); this is the cursor's spelling of the same question. */
export function inCodeContext(state: EditorState): boolean {
  return inCodeAt(state, state.selection.main.from);
}

/** The paste as Markdown, or null to decline and let CodeMirror paste the plain text. */
export function pastePlan(state: EditorState, html: string, plain: string): TransactionSpec | null {
  if (inCodeContext(state)) return null;
  const md = markdownForPaste(html, plain);
  if (md === null) return null;
  return { ...state.replaceSelection(md), scrollIntoView: true, userEvent: "input.paste" };
}

function handlePaste(event: ClipboardEvent, view: EditorView): boolean {
  const data = event.clipboardData;
  if (!data) return false;
  const plan = pastePlan(view.state, data.getData("text/html"), data.getData("text/plain"));
  if (plan === null) return false;
  event.preventDefault();
  view.dispatch(plan);
  return true;
}

/** Web-page formatting survives the clipboard; an unformatted paste still takes
 *  CodeMirror's own path. */
export const richPaste = EditorView.domEventHandlers({ paste: handlePaste });

// --- `[[` completion --------------------------------------------------------------------

/** The vault's lists the picker ranks over — already loaded in the app's state, so a
 *  query is an in-memory scan. */
export interface WikiInventory {
  readonly notes: NoteSummary[];
  readonly resources: ResourceSummary[];
}

/**
 * Wikilink completion (the Obsidian gesture): typing `[[` opens a picker over the vault's
 * notes + files. The logic — trigger detection, ranking, bracket-closing — is the pure
 * wikicomplete.ts; this is only the CodeMirror adapter. `filter: false` because the
 * ranking is ours (title-prefix > title > path), and no `validFor` so each keystroke
 * re-queries it. `inventory` is read per query, so the lists are always the live ones.
 */
export function wikiCompletionSource(
  inventory: () => WikiInventory,
): (ctx: CompletionContext) => CompletionResult | null {
  return (ctx) => {
    const line = ctx.state.doc.lineAt(ctx.pos);
    const found = wikiQueryAt(line.text.slice(0, ctx.pos - line.from));
    if (!found) return null;
    const { notes, resources } = inventory();
    const options = wikiCandidates(notes, resources, found.query).map((c) => ({
      label: c.label,
      detail: c.detail,
      apply: (view: EditorView, _completion: unknown, from: number, to: number) => {
        const { insert, cursor } = wikiInsertion(c.target, view.state.sliceDoc(to, to + 2));
        view.dispatch({
          changes: { from, to, insert },
          selection: { anchor: from + cursor },
        });
      },
    }));
    return { from: line.from + found.from, options, filter: false };
  };
}
