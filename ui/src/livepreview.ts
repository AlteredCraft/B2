// Live-preview decorations — a document feel over the byte-honest buffer
// (specs/desktop-live-preview.md). Decorations conceal Markdown markup away from the
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
import { type EditorSelection, type Extension, type Range } from "@codemirror/state";
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
): void {
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

/**
 * The live-preview extension: the ViewPlugin folding tree+selection into decorations,
 * plus the `lp-body` class that swaps the editor to the reading view's proportional
 * voice (spec §3, §5). Cmd/Ctrl+click a wikilink follows it via `onFollow`; a plain
 * click falls through to place the cursor, as an editor must (spec §3).
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
  return [plugin, EditorView.contentAttributes.of({ class: "lp-body" })];
}
