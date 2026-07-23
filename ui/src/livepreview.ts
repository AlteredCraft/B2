// Live-preview decorations — a document feel over the byte-honest buffer
// (crates/b2-desktop/CLAUDE.md). Decorations conceal Markdown markup away from the
// cursor and style content in place; they change what the DOM shows, NEVER what
// `state.doc` holds (spec §0, insight §2.2). The save chain literally cannot observe
// this feature — every construct decorates *within* lines (marks, inline replaces,
// line classes), so the whole engine is one ViewPlugin (spec insight §2.4).
//
// Two exports: `wikilink` (the Lezer inline node giving the tree B2's most important
// construct) and `livePreview(onFollow)` (the ViewPlugin + the proportional-font body
// class). main.ts keeps `livePreview` in a Compartment so `</>` can swap it for raw
// source mode with no remount.

import { syntaxTree } from "@codemirror/language";
import {
  type EditorSelection,
  type EditorState,
  type Extension,
  type Range,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type { SyntaxNodeRef } from "@lezer/common";
import type { InlineContext, MarkdownConfig } from "@lezer/markdown";
import { renderMarkdown } from "./render";

// --- the wikilink tree extension (spec §4, insight §2.3) --------------------------
//
// A `[[target]]` / `[[target|label]]` inline node — the same grammar as the reading
// view's `marked` tokenizer (render.ts): target = one-or-more chars that aren't `]` or
// `|`, optional `|label` where label is one-or-more non-`]` chars. Giving the tree a
// `Wikilink` node lets the *one* decoration engine style wikilinks uniformly with every
// other construct, instead of a bolt-on. Positions are document-relative throughout.

const OPEN = 91; // [
const CLOSE = 93; // ]
const PIPE = 124; // |
const NEWLINE = 10; // \n

export const wikilink: MarkdownConfig = {
  defineNodes: [{ name: "Wikilink" }],
  parseInline: [
    {
      name: "Wikilink",
      // Before the standard Link parser so `[[` isn't first eaten as `[` + a link.
      before: "Link",
      parse(cx: InlineContext, next: number, pos: number): number {
        if (next !== OPEN || cx.char(pos + 1) !== OPEN) return -1;
        const contentStart = pos + 2;
        const end = cx.end;
        // Scan for the closing `]]`; a wikilink spans no `]` or line break internally.
        let i = contentStart;
        while (i < end) {
          const c = cx.char(i);
          if (c === CLOSE || c === NEWLINE) break;
          i++;
        }
        // Require `]]` here, non-empty content, and a non-empty target (no leading `|`).
        if (i + 1 >= end || cx.char(i) !== CLOSE || cx.char(i + 1) !== CLOSE) return -1;
        if (i === contentStart || cx.char(contentStart) === PIPE) return -1;
        return cx.addElement(cx.elt("Wikilink", pos, i + 2));
      },
    },
  ],
};

// The engine re-derives the `[[..]]` structure from the node text — its span is exactly
// the wikilink, so an anchored match yields the label/pipe offsets and the target.
const WIKILINK_RE = /^\[\[([^\]|]+)(?:\|([^\]]+))?\]\]$/;

// --- the decoration engine (spec §4) ----------------------------------------------

/** True when any selection range touches [from, to] (inclusive — a boundary counts). */
function touches(sel: EditorSelection, from: number, to: number): boolean {
  for (const r of sel.ranges) if (r.from <= to && from <= r.to) return true;
  return false;
}

/** A conceal is a zero-width replace of the markup bytes — emitted only when unrevealed. */
function conceal(
  decos: Range<Decoration>[],
  revealed: boolean,
  from: number,
  to: number,
): void {
  if (!revealed && to > from) decos.push(HIDE.range(from, to));
}
const HIDE = Decoration.replace({});

// `•`/HR are the two conceals that show *something* in the markup's place. Stateless
// singletons — `eq` returns true so CM never rebuilds their DOM on a recompute.
class BulletWidget extends WidgetType {
  eq(): boolean {
    return true;
  }
  toDOM(): HTMLElement {
    const s = document.createElement("span");
    s.className = "lp-bullet";
    s.textContent = "•";
    return s;
  }
}
class RuleWidget extends WidgetType {
  eq(): boolean {
    return true;
  }
  toDOM(): HTMLElement {
    const s = document.createElement("span");
    s.className = "lp-hr";
    return s;
  }
}
const bulletDeco = Decoration.replace({ widget: new BulletWidget() });
const ruleDeco = Decoration.replace({ widget: new RuleWidget() });

// An interactive task checkbox in place of `[ ]`/`[x]`. Unlike every other decoration
// this one *writes*: a click dispatches the single-byte toggle of the marker's state
// char, which flows through the normal editor transaction → autosave path (spec §8 —
// "a widget that writes"). The write stays byte-honest: only `[ ]` ↔ `[x]` changes,
// `state.doc` remains the source of truth. `from` is the marker's `[`; the state char
// sits at `from + 1`.
class TaskWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly from: number,
  ) {
    super();
  }
  eq(o: TaskWidget): boolean {
    return o.checked === this.checked && o.from === this.from;
  }
  toDOM(view: EditorView): HTMLElement {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "lp-task";
    box.checked = this.checked;
    // mousedown: keep the editor selection where it is (no focus steal, no cursor jump).
    box.addEventListener("mousedown", (e) => e.preventDefault());
    // click: preventDefault suppresses the native toggle — the doc change is the source
    // of truth, and the rebuilt widget reflects it.
    box.addEventListener("click", (e) => {
      e.preventDefault();
      view.dispatch({
        changes: { from: this.from + 1, to: this.from + 2, insert: this.checked ? " " : "x" },
      });
    });
    return box;
  }
  ignoreEvent(): boolean {
    return true;
  }
}

// A GFM table rendered in place — block-widget territory, so it is fed by a StateField
// (block/line-break-spanning decorations can't come from a ViewPlugin — spec §8). The
// body reuses the reading view's `renderMarkdown` so read ↔ edit stay pixel-identical,
// wikilinks inside cells carry their `data-target` (the app's click handler follows
// them), and inline markup renders. A block widget hides its source range, so a plain
// click can't land a cursor inside; clicking the table (but not a wikilink) drops the
// cursor at its start, revealing the raw source for editing. `from` is the table's
// first-line start.
class TableWidget extends WidgetType {
  constructor(
    readonly md: string,
    readonly from: number,
  ) {
    super();
  }
  eq(o: TableWidget): boolean {
    return o.md === this.md && o.from === this.from;
  }
  toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "lp-table";
    wrap.innerHTML = renderMarkdown(this.md);
    wrap.addEventListener("mousedown", (e) => {
      // Let a wikilink click fall through to the app's follow handler.
      if ((e.target as HTMLElement | null)?.closest?.("[data-target]")) return;
      e.preventDefault();
      view.dispatch({ selection: { anchor: this.from } });
      view.focus();
    });
    return wrap;
  }
  ignoreEvent(): boolean {
    return true;
  }
}

// The three spellings of a checked GFM task marker's state char.
function taskChecked(marker: string): boolean {
  return marker === "[x]" || marker === "[X]";
}

/** Run `cb` once per line the range [from, to] covers (line-local block decorations). */
function eachLine(
  view: EditorView,
  from: number,
  to: number,
  cb: (lineFrom: number, lineTo: number) => void,
): void {
  if (to < from) return;
  const doc = view.state.doc;
  const first = doc.lineAt(from).number;
  const last = doc.lineAt(to).number;
  for (let n = first; n <= last; n++) {
    const line = doc.line(n);
    // A range that only touches the last line at its very start doesn't cover it.
    if (n > first && line.from === to) continue;
    cb(line.from, line.to);
  }
}

/** Extend `to` past any spaces that follow a concealed block marker (so `## `, `> `
 *  conceal cleanly, matching the reading view which drops them). */
function skipSpaces(view: EditorView, to: number, lineTo: number): number {
  const doc = view.state.doc;
  while (to < lineTo && doc.sliceString(to, to + 1) === " ") to++;
  return to;
}

// Style decorations are emitted unconditionally; conceals only when the reveal range —
// the *line* for block markers, the *element span* for inline markup (spec §3 hybrid
// policy) — doesn't touch the selection. Every branch is line-local, so the plugin is
// legal and this is a pure function of (tree, selection, viewport).
function handleNode(
  node: SyntaxNodeRef,
  view: EditorView,
  sel: EditorSelection,
  decos: Range<Decoration>[],
): boolean | void {
  const doc = view.state.doc;
  const name = node.name;

  // Headings: line-scale style + conceal the leading `#`s (and their trailing space).
  if (name.length === 11 && name.startsWith("ATXHeading")) {
    const level = name.charCodeAt(10) - 48; // '1'..'6'
    const line = doc.lineAt(node.from);
    decos.push(Decoration.line({ class: `lp-h${level}` }).range(line.from));
    const revealed = touches(sel, line.from, line.to);
    for (const mark of node.node.getChildren("HeaderMark")) {
      conceal(decos, revealed, mark.from, skipSpaces(view, mark.to, line.to));
    }
    return;
  }

  switch (name) {
    case "StrongEmphasis":
    case "Emphasis":
    case "Strikethrough": {
      const cls =
        name === "StrongEmphasis" ? "lp-strong" : name === "Emphasis" ? "lp-em" : "lp-strike";
      const markName = name === "Strikethrough" ? "StrikethroughMark" : "EmphasisMark";
      const revealed = touches(sel, node.from, node.to);
      decos.push(Decoration.mark({ class: cls }).range(node.from, node.to));
      for (const m of node.node.getChildren(markName)) conceal(decos, revealed, m.from, m.to);
      return;
    }

    case "InlineCode": {
      const revealed = touches(sel, node.from, node.to);
      decos.push(Decoration.mark({ class: "lp-code" }).range(node.from, node.to));
      for (const m of node.node.getChildren("CodeMark")) conceal(decos, revealed, m.from, m.to);
      return;
    }

    // Inline links `[text](url)`: show the text, conceal `[` and `](url)`. Descent still
    // decorates any markup *inside* the text. Reference-style `[text]` (no URL) is left raw.
    case "Link": {
      const marks = node.node.getChildren("LinkMark");
      const url = node.node.getChild("URL");
      if (marks.length >= 2 && url) {
        const revealed = touches(sel, node.from, node.to);
        decos.push(Decoration.mark({ class: "lp-link" }).range(node.from, node.to));
        conceal(decos, revealed, marks[0].from, marks[0].to); // [
        conceal(decos, revealed, marks[1].from, node.to); // ](url…)
      }
      return;
    }

    // Wikilinks: show the label (accent, carrying `data-target` for mod-click follow),
    // conceal `[[`/`[[target|` and `]]`. A node whose text the anchored grammar rejects
    // (an odd `[[a|]]`) degrades to raw — never an error, never a changed byte (spec §4).
    case "Wikilink": {
      const raw = doc.sliceString(node.from, node.to);
      const m = WIKILINK_RE.exec(raw);
      if (!m) return;
      const target = m[1].trim();
      const labelStart = m[2] === undefined ? node.from + 2 : node.from + 2 + m[1].length + 1;
      const labelEnd = node.to - 2;
      const revealed = touches(sel, node.from, node.to);
      decos.push(
        Decoration.mark({ class: "lp-wikilink", attributes: { "data-target": target } }).range(
          labelStart,
          labelEnd,
        ),
      );
      conceal(decos, revealed, node.from, labelStart);
      conceal(decos, revealed, labelEnd, node.to);
      return;
    }

    // Blockquote: border + muted per line, conceal each `>` (reveal per its own line).
    case "Blockquote": {
      eachLine(view, node.from, node.to, (lineFrom) => {
        decos.push(Decoration.line({ class: "lp-quote" }).range(lineFrom));
      });
      for (const m of node.node.getChildren("QuoteMark")) {
        const line = doc.lineAt(m.from);
        const revealed = touches(sel, line.from, line.to);
        conceal(decos, revealed, m.from, skipSpaces(view, m.to, line.to));
      }
      return;
    }

    // Bullet list markers `-`/`*`/`+` → `•` (ordered lists keep their number).
    case "ListItem": {
      const m = node.node.getChild("ListMark");
      if (!m) return;
      const ch = doc.sliceString(m.from, m.to);
      if (ch !== "-" && ch !== "*" && ch !== "+") return;
      const line = doc.lineAt(m.from);
      if (!touches(sel, line.from, line.to)) decos.push(bulletDeco.range(m.from, m.to));
      return;
    }

    // Horizontal rule → a rule widget (reveal per line shows the raw `---`).
    case "HorizontalRule": {
      const line = doc.lineAt(node.from);
      const to = Math.min(node.to, line.to);
      if (!touches(sel, line.from, line.to) && to > node.from) {
        decos.push(ruleDeco.range(node.from, to));
      }
      return;
    }

    // Fenced code: block background per line; the fences stay visible (spec §3 — hiding
    // them would hide the language tag for little gain), so no conceal.
    case "FencedCode": {
      eachLine(view, node.from, node.to, (lineFrom) => {
        decos.push(Decoration.line({ class: "lp-fence" }).range(lineFrom));
      });
      return;
    }

    // Interactive task checkbox: replace `[ ]`/`[x]` with a real checkbox away from the
    // cursor; reveal the raw marker on the active line (the block-marker reveal policy,
    // matching the bullet/quote handlers). The list bullet stays — parity with the
    // reading view, which renders `• ☐ …` for a GFM task item.
    case "TaskMarker": {
      const line = doc.lineAt(node.from);
      if (touches(sel, line.from, line.to)) return;
      const checked = taskChecked(doc.sliceString(node.from, node.to));
      decos.push(
        Decoration.replace({ widget: new TaskWidget(checked, node.from) }).range(
          node.from,
          node.to,
        ),
      );
      return;
    }

    // Tables are block widgets fed by the StateField below; this plugin never decorates
    // inside one (returning false skips the subtree, so no inline decos land in a
    // block-replaced range, and an edited table reads as clean raw source).
    case "Table":
      return false;
  }
}

/** Fold the visible syntax tree + selection into a sorted DecorationSet. Cost scales
 *  with the *viewport*, not the note (insight §2.1). */
function buildDecorations(view: EditorView): DecorationSet {
  const decos: Range<Decoration>[] = [];
  const sel = view.state.selection;
  const tree = syntaxTree(view.state);
  for (const { from, to } of view.visibleRanges) {
    tree.iterate({ from, to, enter: (node) => handleNode(node, view, sel, decos) });
  }
  // `sort: true` orders line/mark/replace decorations for us — the one place ordering
  // across the mixed decoration kinds is fiddly to get right by hand.
  return Decoration.set(decos, true);
}

// --- block widgets (spec §8) ------------------------------------------------------
//
// Block widgets (and any replace spanning a line break) can't come from a ViewPlugin —
// CM6 forbids it — so tables live in a StateField instead. It has no viewport, so cost
// scales with the note rather than the screen; tables are rare and cheap to find, so a
// whole-tree pass on each doc/selection change is fine (only tables are visited deeply).

/** Replace each un-touched GFM table with a rendered block widget; reveal (leave raw)
 *  the one the selection is inside, so it can be edited as source. */
function buildBlockDecorations(state: EditorState): DecorationSet {
  const decos: Range<Decoration>[] = [];
  const sel = state.selection;
  const doc = state.doc;
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name !== "Table") return; // keep descending to reach any nested table
      // Snap to whole lines: a block replace must sit on line boundaries, and the widget
      // renders the exact lines it hides.
      const from = doc.lineAt(node.from).from;
      const to = doc.lineAt(node.to).to;
      if (!touches(sel, from, to)) {
        decos.push(
          Decoration.replace({
            widget: new TableWidget(doc.sliceString(from, to), from),
            block: true,
          }).range(from, to),
        );
      }
      return false; // a table's internals are never block-widget territory
    },
  });
  return Decoration.set(decos, true);
}

const blockField = StateField.define<DecorationSet>({
  create: (state) => buildBlockDecorations(state),
  update(deco, tr) {
    // Reveal keys on the selection, so a bare cursor move recomputes too.
    return tr.docChanged || tr.selection ? buildBlockDecorations(tr.state) : deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/**
 * The live-preview extension: the ViewPlugin folding tree+selection into inline/line
 * decorations, the `blockField` feeding block widgets (tables — spec §8), plus the
 * `lp-body` class that swaps the editor to the reading view's proportional voice
 * (spec §3, §5). Cmd/Ctrl+click a wikilink follows it via `onFollow`; a plain click
 * falls through to place the cursor, as an editor must (spec §3).
 */
export function livePreview(onFollow: (target: string) => void): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = buildDecorations(view);
      }
      update(u: ViewUpdate): void {
        if (u.docChanged || u.selectionSet || u.viewportChanged) {
          this.decorations = buildDecorations(u.view);
        }
      }
    },
    {
      decorations: (v) => v.decorations,
      eventHandlers: {
        mousedown(e: MouseEvent): boolean {
          if (!(e.metaKey || e.ctrlKey)) return false;
          const span = (e.target as HTMLElement | null)?.closest?.("[data-target]");
          const target = (span as HTMLElement | null)?.dataset.target;
          if (!target) return false;
          e.preventDefault();
          onFollow(target);
          return true;
        },
      },
    },
  );
  return [plugin, blockField, EditorView.contentAttributes.of({ class: "lp-body" })];
}
