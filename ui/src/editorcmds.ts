// The editor's commands: CodeMirror adapters over the pure engines (format.ts, list.ts,
// paste.ts, wikicomplete.ts). Each is a pure *plan* over an EditorState (tested in node)
// and a `run…` that dispatches it; main.ts builds the keymap from the `run…` halves.

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

/** Toggle an inline format over every selection range (multi-cursor via `changeByRange`). */
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
 * Tab / ⇧Tab over list.ts: a transaction, `true` to claim the key inertly, or `false` to
 * decline so Tab keeps walking the focus ring (why this isn't `indentWithTab`). Declines
 * in code, where `- item` is text. Main range only: an indent shifts every later offset.
 */
export function listShiftPlan(state: EditorState, shift: ListShift): TransactionSpec | boolean {
  if (inCodeContext(state)) return false;
  const { from, to } = state.selection.main;
  const r = shift(state.doc.toString(), from, to);
  if (!r) return inListItem(state);
  // Claimed but inert (list.ts's header).
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

/** Is the cursor in a list item per the syntax tree? Catches lists list.ts can't scan
 *  (`> - a`), where Tab is claimed but inert. */
export function inListItem(state: EditorState): boolean {
  const at = syntaxTree(state).resolveInner(state.selection.main.from, -1);
  for (let n: typeof at | null = at; n; n = n.parent) {
    if (n.name === "ListItem") return true;
  }
  return false;
}

// --- ⌘T: a table ----------------------------------------------------------------------

/** A fresh 3-column table at the cursor, caret in the first cell (format.ts). */
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
// Converts a paste's `text/html` flavor to Markdown. Declines (leaving CodeMirror's plain
// paste) in code, or when the HTML adds no formatting. ⌘⇧V always pastes plain (main.ts).

/** Is the cursor inside code (fence, indented block, or inline span)? droplink.ts's read. */
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

/** Web-page formatting survives the clipboard. */
export const richPaste = EditorView.domEventHandlers({ paste: handlePaste });

// --- `[[` completion --------------------------------------------------------------------

/** The vault's lists the picker ranks over, from app state. */
export interface WikiInventory {
  readonly notes: NoteSummary[];
  readonly resources: ResourceSummary[];
}

/**
 * `[[` completion over the vault's notes and files; the logic is wikicomplete.ts.
 * `filter: false` because the ranking is ours, and no `validFor` so each keystroke
 * re-queries against the live `inventory`.
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
