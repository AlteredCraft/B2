// Dropping a discovery card into the note: drag a candidate card onto a line of the note
// being edited and a `[[wikilink]]` lands at the end of that line.
//
// The drop inserts into the editor buffer, as `[[` completion does, and the bytes reach
// disk through the normal `write_note` path, so W1 holds. It writes an untyped body link
// (data-model.md §2), not a typed `b2_relations:` entry like the card's Link… item.
//
// Where the link goes is pure (node tests it); main.ts owns the `DragEvent` plumbing,
// the save and the refresh.

import { syntaxTree } from "@codemirror/language";
import { type EditorState, type Extension, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";

/** The drag's own MIME type. Not `text/plain`, which CodeMirror would also handle if an
 *  interception were missed; a private type is also inert outside B2. */
export const CARD_DRAG_MIME = "application/x-b2-card";

/** Where a dropped card's link would land (`lineFrom` the line start, `from` the
 *  insertion point), and what it would insert. */
export interface DropInsertion {
  lineFrom: number;
  from: number;
  insert: string;
}

/**
 * A link lands at the end of the line, after a single space unless the line is empty or
 * already ends in whitespace. You aim at a line, not a column, so a drop can't split a
 * word.
 */
export function lineDrop(lineText: string, target: string): { offset: number; insert: string } {
  const link = `[[${target}]]`;
  const spaced = lineText !== "" && !/\s$/.test(lineText);
  return { offset: lineText.length, insert: spaced ? ` ${link}` : link };
}

/**
 * Is `pos` inside code (a fence, an indented block, or an inline span)? Takes a position,
 * not the selection, because a drop names a place the cursor isn't. editorcmds.ts's
 * `inCodeContext` delegates here.
 */
export function inCodeAt(state: EditorState, pos: number, side: -1 | 1 = -1): boolean {
  const at = syntaxTree(state).resolveInner(pos, side);
  for (let n: typeof at | null = at; n; n = n.parent) {
    if (n.name.includes("Code")) return true; // FencedCode / CodeBlock / CodeText / InlineCode
  }
  return false;
}

/** Is this line's block a fence or an indented block? Inline spans don't count: the link
 *  lands after any span that closes at the line's end. */
function inBlockCodeAt(state: EditorState, pos: number): boolean {
  const at = syntaxTree(state).resolveInner(pos, 1);
  for (let n: typeof at | null = at; n; n = n.parent) {
    if (n.name === "FencedCode" || n.name === "CodeBlock" || n.name === "CodeText") return true;
  }
  return false;
}

/**
 * What a drop at `pos` would do, or null on a line of code, where a `[[link]]` is literal
 * text. The refusal shows during the drag (no ghost, no-drop cursor).
 */
export function planDrop(state: EditorState, pos: number, target: string): DropInsertion | null {
  const line = state.doc.lineAt(pos);
  // Asked at the first non-blank character: an indented code block's node starts after
  // the indent, and the line end is a block edge where the answer depends on side. A
  // blank line asks at its start.
  const content = line.from + Math.max(0, line.text.search(/\S/));
  if (inBlockCodeAt(state, content)) return null;
  const { offset, insert } = lineDrop(line.text, target);
  return { lineFrom: line.from, from: line.from + offset, insert };
}

/** Show (or clear, with null) the drop preview. main.ts clears it when the pointer leaves
 *  the buffer. */
export const setDropTarget = StateEffect.define<DropInsertion | null>();

/** The ghost `[[target]]` at the insertion point. `aria-hidden`: a preview, not content. */
class DropGhost extends WidgetType {
  // Not a constructor parameter property: node's strip-only mode can't compile those.
  label: string;
  constructor(label: string) {
    super();
    this.label = label;
  }
  eq(other: DropGhost): boolean {
    return other.label === this.label;
  }
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "cm-drop-ghost";
    span.setAttribute("aria-hidden", "true");
    span.textContent = this.label;
    return span;
  }
}

/** A wash over the target line and the ghost at the insertion point. Derived, so the
 *  field holds a plain `DropInsertion`. */
function previewDecorations(plan: DropInsertion | null): DecorationSet {
  if (plan === null) return Decoration.none;
  return Decoration.set(
    [
      Decoration.line({ class: "cm-drop-line" }).range(plan.lineFrom),
      Decoration.widget({ widget: new DropGhost(plan.insert), side: 1 }).range(plan.from),
    ],
    true,
  );
}

const dropTargetField = StateField.define<DropInsertion | null>({
  create: () => null,
  update(plan, tr) {
    for (const e of tr.effects) if (e.is(setDropTarget)) return e.value;
    // Any user edit or cursor move ends the preview. A safety net: a repaint that destroys
    // the dragged card loses its `dragend`, which would strand the ghost. A drag moves
    // neither doc nor selection, so nothing legitimate is cut short.
    return tr.docChanged || tr.selection ? null : plan;
  },
  provide: (f) => EditorView.decorations.from(f, previewDecorations),
});

/** The one insertion path, shared by the drop and the card menu's *Insert link at cursor*.
 *  The caret lands after the link. */
export function insertDrop(view: EditorView, plan: DropInsertion): void {
  view.dispatch({
    changes: { from: plan.from, insert: plan.insert },
    selection: { anchor: plan.from + plan.insert.length },
    scrollIntoView: true,
    userEvent: "input.drop",
  });
}

/**
 * The card being dragged. A note has two spellings: `path` is the app's key (`notes/x.md`,
 * L1) and `target` is what a link says (`notes/x`). Both travel together so the app never
 * reverses a target into a key (PR #185).
 */
export interface DraggedCard {
  /** The note's vault-relative path, extension included — the app's key. */
  path: string;
  /** What a wikilink to it says: the text this module inserts. */
  target: string;
}

/** The candidate list minus the one just linked, keyed by path (see [`DraggedCard`]). */
export function withoutCard<T extends { path: string }>(
  cards: readonly T[],
  card: DraggedCard,
): T[] {
  return cards.filter((c) => c.path !== card.path);
}

/** What the extension needs from the app. */
export interface CardDropOptions {
  /** The dragged candidate, or null when the drag isn't ours. Asked per event, so it is
   *  read off the payload ([`CARD_DRAG_MIME`]) rather than app state a repaint may strand. */
  dragged: (e: DragEvent) => DraggedCard | null;
  /** Called after the insertion, with that same card — main.ts saves and refreshes. */
  onDrop: (card: DraggedCard) => void;
}

/**
 * The editor half of the gesture: a preview while a card is over the buffer, and the
 * insertion on drop. Handlers return `true` only on our drag, so CodeMirror's own drop
 * never also runs. `preventDefault` only where a drop may happen, so the OS cursor shows
 * no-drop over code.
 */
export function cardDrop(opts: CardDropOptions): Extension {
  const show = (view: EditorView, plan: DropInsertion | null): void => {
    const cur = view.state.field(dropTargetField, false) ?? null;
    if (cur?.from === plan?.from && cur?.insert === plan?.insert) return; // no-op repaint
    view.dispatch({ effects: setDropTarget.of(plan) });
  };
  const planAt = (e: DragEvent, view: EditorView, target: string): DropInsertion | null =>
    planDrop(view.state, view.posAtCoords({ x: e.clientX, y: e.clientY }, false), target);

  const over = (e: DragEvent, view: EditorView): boolean => {
    const card = opts.dragged(e);
    if (card === null) return false;
    const plan = planAt(e, view, card.target);
    show(view, plan);
    if (plan === null) return true; // over code: consumed, but not droppable
    e.preventDefault(); // this is what makes the line a drop zone
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    return true;
  };

  return [
    dropTargetField,
    EditorView.domEventHandlers({
      // Some engines need the first event (dragenter) cancelled to treat it as a drop zone.
      dragenter: over,
      dragover: over,
      dragleave(e, view) {
        if (opts.dragged(e) === null) return false;
        // Fires between child elements too; clear only when the pointer left the buffer.
        const to = e.relatedTarget;
        if (to instanceof Node && view.dom.contains(to)) return false;
        show(view, null);
        return false;
      },
      drop(e, view) {
        const card = opts.dragged(e);
        if (card === null) return false;
        const plan = planAt(e, view, card.target);
        show(view, null);
        if (plan === null) return true;
        e.preventDefault();
        insertDrop(view, plan);
        view.focus();
        // The whole card, so the app keys by path (see `DraggedCard`).
        opts.onDrop(card);
        return true;
      },
    }),
  ];
}
