// Dropping a discovery card into the note — the gesture that turns "Similar & unlinked"
// from a list you read into a list you *act on*: drag a candidate card out of the right
// column, drop it on a line of the note you are editing, and a `[[wikilink]]` lands at the
// end of that line. Flow ③'s "the human is the precision gate" (root CLAUDE.md), with the
// pointer doing the committing instead of a modal.
//
// **This authors nothing on B2's behalf.** The drop inserts text into the *editor buffer*,
// exactly as `[[` completion does (wikicomplete.ts) — the human picked the target, the
// human picked the line, and the bytes reach disk through the same guarded `write_note`
// splice every keystroke does. Invariant W1 is untouched: no unbidden write, and no body
// content B2 decided on its own.
//
// A **body** link, not a frontmatter relation. The card's Link… item writes a *typed*
// `b2_relations:` entry (`b2 link`); this writes the other kind the data model has — an
// untyped inline `references` edge, which is what a `[[link]]` in prose always is
// (data-model.md §2). Two gestures because they are two different claims: "this note
// supports that one" is a judgement worth a verb, and "…see also [[that]]" is a sentence.
//
// The split here is the repo's usual one. Everything that decides *where the link goes* is
// a pure function of a line (or of an `EditorState` + a position), so node runs it straight
// off the source; main.ts owns the `DragEvent` plumbing, the save, and the pane refresh,
// because those are the running app's. `toDOM` is the one thing that wants a document, and
// nothing in the test reaches it (livepreview.test.ts's rule).

import { syntaxTree } from "@codemirror/language";
import { type EditorState, type Extension, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";

/** The drag's own MIME type. Deliberately **not** `text/plain`: CodeMirror has its own
 *  text-drop handling, so a plain flavor would give a missed interception a second,
 *  silent behavior (the bare path dropped mid-word). A private type also keeps the drag
 *  inert outside B2 — nothing else on the machine claims it. */
export const CARD_DRAG_MIME = "application/x-b2-card";

/** Where a dropped card's link would land, and what it would insert. Positions are
 *  document offsets; `lineFrom` is the target line's start (what the wash decorates),
 *  `from` the insertion point (what the ghost sits at). */
export interface DropInsertion {
  lineFrom: number;
  from: number;
  insert: string;
}

/**
 * The rule, on one line of text: **a link lands at the end of the line**, with a single
 * space in front of it unless the line already ends in whitespace (or is empty).
 *
 * End-of-line rather than at-the-cursor-column is a deliberate simplification, and it is
 * what makes the gesture teachable: you aim at a *line*, not at a character between two
 * words, so a drop can't split "seman|tics" — and an empty line takes the link on its own,
 * which is how a "see also" line gets written. The offset is the true end of the line, so
 * nothing is ever left dangling *after* the link (an indented blank line keeps its indent,
 * `- ` keeps its bullet and its space).
 */
export function lineDrop(lineText: string, target: string): { offset: number; insert: string } {
  const link = `[[${target}]]`;
  const spaced = lineText !== "" && !/\s$/.test(lineText);
  return { offset: lineText.length, insert: spaced ? ` ${link}` : link };
}

/**
 * Is `pos` inside code — a fence, an indented block, or an inline span? The read the rich
 * paste and the list keys make (main.ts's `inCodeContext` delegates here), taking a
 * position rather than reading the selection, because a *drop* names a place the cursor
 * isn't.
 */
export function inCodeAt(state: EditorState, pos: number, side: -1 | 1 = -1): boolean {
  const at = syntaxTree(state).resolveInner(pos, side);
  for (let n: typeof at | null = at; n; n = n.parent) {
    if (n.name.includes("Code")) return true; // FencedCode / CodeBlock / CodeText / InlineCode
  }
  return false;
}

/** The **block** half of that question — is this line's own block a fence or an indented
 *  block? An inline span is deliberately not code for this purpose: the link goes at the
 *  end of the line, *outside* a `\`span\`` that happens to close there, so refusing a line
 *  for containing one would be a refusal with no hazard behind it. */
function inBlockCodeAt(state: EditorState, pos: number): boolean {
  const at = syntaxTree(state).resolveInner(pos, 1);
  for (let n: typeof at | null = at; n; n = n.parent) {
    if (n.name === "FencedCode" || n.name === "CodeBlock" || n.name === "CodeText") return true;
  }
  return false;
}

/**
 * What a drop at `pos` would do, or **null** where it may not happen: on a line of code,
 * where a `[[link]]` is literal text that resolves to nothing. Refusing there (no ghost,
 * no-drop cursor) is the same posture the paste takes — B2 keeps its hands off code — and
 * it is visible *before* the mouse button comes up, which is the point of a drag preview.
 */
export function planDrop(state: EditorState, pos: number, target: string): DropInsertion | null {
  const line = state.doc.lineAt(pos);
  // Asked at the line's first non-blank character, looking forward. Not at `line.from`: an
  // indented code block's node starts *after* the indent, so a line-start read calls
  // `    cargo test` ordinary prose. Not at the insertion point either: that offset sits at
  // the block's edge, where the answer flips with the side you read from. A blank line has
  // no content to ask about, so it asks at its start — which is what a blank line inside a
  // fence resolves into.
  const content = line.from + Math.max(0, line.text.search(/\S/));
  if (inBlockCodeAt(state, content)) return null;
  const { offset, insert } = lineDrop(line.text, target);
  return { lineFrom: line.from, from: line.from + offset, insert };
}

/** Show (or clear, with null) the drop preview. Exported for main.ts, which clears it when
 *  the pointer leaves the buffer for the discovery column — the drag's cancel gesture. */
export const setDropTarget = StateEffect.define<DropInsertion | null>();

/** The ghost `[[target]]` at the insertion point: what the drop will write, where it will
 *  write it, in the note's own type. `aria-hidden` and non-editable — it is a preview of a
 *  document that doesn't exist yet, so nothing may read it as content. */
class DropGhost extends WidgetType {
  // An explicit field, not a constructor parameter property: node runs the suite off this
  // source in strip-only mode, which cannot compile that shorthand (livepreview.ts's
  // widgets are written the same way).
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

/** The two decorations a live preview paints: a wash over the line being aimed at, and the
 *  ghost at the exact offset the text would enter. Split out so the field stays a plain
 *  value (the insertion) rather than a set of ranges — the value is what `setDropTarget`
 *  carries and what a test can assert on. */
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
    // Anything the *user* does ends the preview, and that is a safety net rather than
    // tidiness: a repaint that destroys the dragged card mid-gesture takes the `dragend`
    // with it (the event fires at a node no longer in the document, so it never reaches
    // main.ts), and a ghost with no drag behind it would otherwise sit in the buffer until
    // the next one. A drag itself moves neither the document nor the selection — the drop
    // clears the preview before it inserts — so nothing legitimate is cut short here.
    return tr.docChanged || tr.selection ? null : plan;
  },
  provide: (f) => EditorView.decorations.from(f, previewDecorations),
});

/** The one insertion path — the drop and its keyboard half (the card menu's *Insert link at
 *  cursor*) both come through here, so there is one behavior to keep correct rather than
 *  two. The caret lands after the link, ready to keep typing the sentence it belongs to. */
export function insertDrop(view: EditorView, plan: DropInsertion): void {
  view.dispatch({
    changes: { from: plan.from, insert: plan.insert },
    selection: { anchor: plan.from + plan.insert.length },
    scrollIntoView: true,
    userEvent: "input.drop",
  });
}

/** What the extension needs from the app: the card currently being dragged (null when the
 *  drag isn't ours — a text drag inside the buffer is CodeMirror's business), and what to
 *  do once the link is in. */
export interface CardDropOptions {
  /** The wikilink target of the dragged candidate, or null for "not our drag" — asked per
   *  event, so the answer can be read off the payload ([`CARD_DRAG_MIME`]) rather than off
   *  app state a mid-drag repaint may have stranded. */
  dragged: (e: DragEvent) => string | null;
  /** Called after the insertion, with that same target — main.ts saves and refreshes. */
  onDrop: (target: string) => void;
}

/**
 * The editor half of the gesture: a drop preview while a card is over the buffer, and the
 * insertion when it lands.
 *
 * Every handler returns `true` on our drag so CodeMirror's own drop handling never also
 * runs, and `false` otherwise so an ordinary text drag inside the note behaves exactly as
 * it always has. `preventDefault` is called **only** where the drop may happen, which is
 * what makes the OS cursor honest: copy over a droppable line, no-drop over code.
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
    const target = opts.dragged(e);
    if (target === null) return false;
    const plan = planAt(e, view, target);
    show(view, plan);
    if (plan === null) return true; // over code: consumed, but not droppable
    e.preventDefault(); // this is what makes the line a drop zone
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    return true;
  };

  return [
    dropTargetField,
    EditorView.domEventHandlers({
      // dragenter as well as dragover, for the reason main.ts's file-drop pair states:
      // some engines want the *first* event over an element cancelled before they will
      // treat it as a drop zone at all.
      dragenter: over,
      dragover: over,
      dragleave(e, view) {
        if (opts.dragged(e) === null) return false;
        // Fires between child elements too, so only a pointer that actually left the
        // buffer clears the preview (main.ts clears it for the panes outside).
        const to = e.relatedTarget;
        if (to instanceof Node && view.dom.contains(to)) return false;
        show(view, null);
        return false;
      },
      drop(e, view) {
        const target = opts.dragged(e);
        if (target === null) return false;
        const plan = planAt(e, view, target);
        show(view, null);
        if (plan === null) return true;
        e.preventDefault();
        insertDrop(view, plan);
        view.focus();
        opts.onDrop(target);
        return true;
      },
    }),
  ];
}
