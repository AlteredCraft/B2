// Live-preview decorations over the byte-honest buffer (crates/b2-desktop/CLAUDE.md).
// Decorations conceal Markdown markup away from the cursor and style content in place;
// they change what the DOM shows, never what `state.doc` holds (spec §0, insight §2.2).
//
// The app uses `wikilink` (the Lezer inline node) and `livePreview(onFollow)`; main.ts
// keeps the latter in a Compartment so `</>` swaps to raw source with no remount. What to
// decorate is a pure function of (tree, selection, viewport), so the rest of the exports
// take an `EditorState` and are testable from node against real trees (#120).

import { syntaxTree } from "@codemirror/language";
import {
  type EditorSelection,
  type EditorState,
  type Extension,
  type Range,
  StateEffect,
  StateField,
  type Text,
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
// Extension-qualified: node's test runner resolves specifiers literally.
import { externalUrl } from "./links.ts";
import {
  embedWidth,
  NO_EMBED_IMAGES,
  WIKILINK_EXACT,
  type EmbedImages,
} from "./embeds.ts";
import { renderMarkdown } from "./markdown.ts";

// --- the wikilink tree extension (spec §4, insight §2.3) --------------------------
//
// A `[[target]]` / `[[target|label]]` inline node, the same grammar as the reading view's
// tokenizer (markdown.ts), so one decoration engine styles wikilinks like every other
// construct.

const BANG = 33; // !
const OPEN = 91; // [
const CLOSE = 93; // ]
const PIPE = 124; // |
const NEWLINE = 10; // \n

export const wikilink: MarkdownConfig = {
  defineNodes: [{ name: "Wikilink" }],
  parseInline: [
    {
      name: "Wikilink",
      // Before `Link` (and so before `Image`), so neither `[[` nor `![[` is first eaten
      // as a link or a reference-style image.
      before: "Link",
      parse(cx: InlineContext, next: number, pos: number): number {
        // The node covers the embed `!`, so handlers never read outside their node.
        const open = next === BANG ? pos + 1 : pos;
        if (cx.char(open) !== OPEN || cx.char(open + 1) !== OPEN) return -1;
        const contentStart = open + 2;
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

/** Matches a whole Wikilink node's text: the reading view's grammar (embeds.ts) with both
 *  ends pinned, so read and edit cannot drift. Stricter than the parse rule above (it
 *  rejects `[[a|]]`); such a node is left raw (spec §4). */
export const WIKILINK_RE = WIKILINK_EXACT;

// --- the note's pictures (the `![[image.png]]` embed) --------------------------------
//
// Embed bytes arrive over IPC after mount, so they enter as editor state (a field main.ts
// writes with an effect), keeping decorations a pure function of `EditorState`. The field
// lives outside the live-preview compartment so toggling `</>` doesn't drop them.

/** Hand the editor the note's loaded pictures (path → `data:` URL). */
export const setEmbedImages = StateEffect.define<EmbedImages>();

/** Where that map lives between paints. Add it to the editor's extensions once. */
export const embedImagesField = StateField.define<EmbedImages>({
  create: () => NO_EMBED_IMAGES,
  update(images, tr) {
    for (const e of tr.effects) if (e.is(setEmbedImages)) return e.value;
    return images;
  },
});

/** The pictures this state carries; a state without the field reads as empty. */
function embedImagesOf(state: EditorState): EmbedImages {
  return state.field(embedImagesField, false) ?? NO_EMBED_IMAGES;
}

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

// Stateless singletons: `eq` is true so CM never rebuilds their DOM on a recompute.
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

// A task checkbox in place of `[ ]`/`[x]`, the one decoration that writes: a click
// toggles the single state byte at `from + 1` through the normal transaction → autosave
// path (spec §8).
class TaskWidget extends WidgetType {
  // No constructor parameter properties: node's `--experimental-strip-types` can't erase
  // them, which makes the module unimportable by the test runner. Same below.
  readonly checked: boolean;
  readonly from: number;
  constructor(checked: boolean, from: number) {
    super();
    this.checked = checked;
    this.from = from;
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
    // Suppress the native toggle: the doc change is the truth, the rebuilt widget shows it.
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

// A GFM table rendered in place via the reading view's `renderMarkdown`, so read and edit
// match (spec §8). A block widget hides its source, so clicking the table (not a link)
// puts the cursor at `from`, revealing the raw source. `images` is compared by identity
// in `eq`: each `setEmbedImages` replaces the map wholesale, so this rebuilds exactly
// when bytes land.
class TableWidget extends WidgetType {
  readonly md: string;
  readonly from: number;
  readonly images: EmbedImages;
  constructor(md: string, from: number, images: EmbedImages) {
    super();
    this.md = md;
    this.from = from;
    this.images = images;
  }
  eq(o: TableWidget): boolean {
    return o.md === this.md && o.from === this.from && o.images === this.images;
  }
  toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "lp-table";
    wrap.innerHTML = renderMarkdown(this.md, this.images);
    wrap.addEventListener("mousedown", (e) => {
      // Let a link click fall through to the app's handlers (main.ts): it navigates.
      const el = (e.target as HTMLElement | null)?.closest?.("[data-target], a[href]") ?? null;
      if (el?.matches("[data-target]") || externalUrl(el?.getAttribute("href"))) return;
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

// The picture of an `![[image.png]]` embed, as an inline replace (it spans no line break).
// `ignoreEvent` is false so CodeMirror places the cursor on click, which reveals the
// source; a widget that swallowed clicks would make the markup uneditable. It carries
// `data-target`, so ⌘-click follows it (`isFollowClick`).
class EmbedImageWidget extends WidgetType {
  readonly src: string;
  readonly target: string;
  readonly width: number | null;
  constructor(src: string, target: string, width: number | null) {
    super();
    this.src = src;
    this.target = target;
    this.width = width;
  }
  eq(o: EmbedImageWidget): boolean {
    return o.src === this.src && o.target === this.target && o.width === this.width;
  }
  toDOM(): HTMLElement {
    const img = document.createElement("img");
    img.className = "lp-embed-image";
    img.src = this.src;
    // The filename: all B2 knows about the picture.
    img.alt = this.target.split("/").pop() ?? this.target;
    img.setAttribute("data-target", this.target);
    if (this.width !== null) img.width = this.width;
    return img;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

/** Is this GFM task marker checked (`[x]` or `[X]`)? */
export function taskChecked(marker: string): boolean {
  return marker === "[x]" || marker === "[X]";
}

/** Run `cb` once per line the range [from, to] covers (line-local block decorations). */
function eachLine(
  doc: Text,
  from: number,
  to: number,
  cb: (lineFrom: number, lineTo: number) => void,
): void {
  if (to < from) return;
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
function skipSpaces(doc: Text, to: number, lineTo: number): number {
  while (to < lineTo && doc.sliceString(to, to + 1) === " ") to++;
  return to;
}

// Styles are emitted always; conceals only when the reveal range (the line for block
// markers, the element span for inline markup; spec §3) doesn't touch the selection.
// Every branch is line-local, so this is legal in a ViewPlugin.
function handleNode(
  node: SyntaxNodeRef,
  doc: Text,
  sel: EditorSelection,
  images: EmbedImages,
  decos: Range<Decoration>[],
): boolean | void {
  const name = node.name;

  // Headings: line-scale style + conceal the leading `#`s (and their trailing space).
  if (name.length === 11 && name.startsWith("ATXHeading")) {
    const level = name.charCodeAt(10) - 48; // '1'..'6'
    const line = doc.lineAt(node.from);
    decos.push(Decoration.line({ class: `lp-h${level}` }).range(line.from));
    const revealed = touches(sel, line.from, line.to);
    for (const mark of node.node.getChildren("HeaderMark")) {
      conceal(decos, revealed, mark.from, skipSpaces(doc, mark.to, line.to));
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

    // Inline links `[text](url)`: conceal `[` and `](url)`. Reference-style is left raw.
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

    // Wikilinks: show the label (carrying `data-target` for ⌘-click), conceal the rest.
    // A node the anchored grammar rejects stays raw (spec §4). For an embed `![[…]]`, as
    // in the reading view (markdown.ts), the `|`-part is a width, so it shows its target;
    // with the picture loaded the whole thing becomes the picture; without, it reads as
    // its link with the `!` concealed.
    case "Wikilink": {
      const raw = doc.sliceString(node.from, node.to);
      const m = WIKILINK_RE.exec(raw);
      if (!m) return;
      const embed = m[1] === "!";
      const target = m[2].trim();
      const revealed = touches(sel, node.from, node.to);
      const src = embed ? images.get(target) : undefined;
      if (src !== undefined && !revealed) {
        decos.push(
          Decoration.replace({
            widget: new EmbedImageWidget(src, target, embedWidth(m[3])),
          }).range(node.from, node.to),
        );
        return;
      }
      // The target starts just past the optional `!` and `[[`.
      const open = node.from + m[1].length + 2;
      const targetEnd = open + m[2].length;
      const labelStart = embed || m[3] === undefined ? open : targetEnd + 1;
      const labelEnd = embed ? targetEnd : node.to - 2;
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

    // Blockquote: style per line; `>` markers are concealed under QuoteMark.
    case "Blockquote": {
      eachLine(doc, node.from, node.to, (lineFrom) => {
        decos.push(Decoration.line({ class: "lp-quote" }).range(lineFrom));
      });
      return;
    }

    // Its own case: only the first line's `>` is a child of the Blockquote; continuation
    // lines hang theirs off the inner Paragraph.
    case "QuoteMark": {
      const line = doc.lineAt(node.from);
      const revealed = touches(sel, line.from, line.to);
      conceal(decos, revealed, node.from, skipSpaces(doc, node.to, line.to));
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

    // Fenced code: background per line; fences stay visible to show the language (spec §3).
    case "FencedCode": {
      eachLine(doc, node.from, node.to, (lineFrom) => {
        decos.push(Decoration.line({ class: "lp-fence" }).range(lineFrom));
      });
      return;
    }

    // Task marker → checkbox, revealed per line. The bullet stays, as in the reading view.
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

    // Tables are block widgets (the StateField below); skip the subtree so no inline decos
    // land in a block-replaced range.
    case "Table":
      return false;
  }
}

/** Fold the syntax tree + selection over `ranges` (the viewport, so cost scales with the
 *  screen; insight §2.1) into a sorted DecorationSet. */
export function inlineDecorations(
  state: EditorState,
  ranges: readonly { from: number; to: number }[],
): DecorationSet {
  const decos: Range<Decoration>[] = [];
  const sel = state.selection;
  const doc = state.doc;
  const images = embedImagesOf(state);
  const tree = syntaxTree(state);
  for (const { from, to } of ranges) {
    tree.iterate({ from, to, enter: (node) => handleNode(node, doc, sel, images, decos) });
  }
  return Decoration.set(decos, true);
}

// --- block widgets (spec §8) ------------------------------------------------------
//
// CM6 forbids block widgets from a ViewPlugin, so tables live in a StateField. It has no
// viewport, but tables are rare and cheap to find, so a whole-tree pass is fine.

/** Replace each GFM table with a rendered widget, leaving the one the selection touches
 *  raw for editing. */
export function blockDecorations(state: EditorState): DecorationSet {
  const decos: Range<Decoration>[] = [];
  const sel = state.selection;
  const doc = state.doc;
  const images = embedImagesOf(state);
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name !== "Table") return; // keep descending to reach any nested table
      // A block replace must sit on line boundaries.
      const from = doc.lineAt(node.from).from;
      const to = doc.lineAt(node.to).to;
      if (!touches(sel, from, to)) {
        decos.push(
          Decoration.replace({
            widget: new TableWidget(doc.sliceString(from, to), from, images),
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
  create: (state) => blockDecorations(state),
  update(deco, tr) {
    // Recompute on cursor moves (reveal) and on pictures landing, which change no text.
    const pictures = tr.effects.some((e) => e.is(setEmbedImages));
    return tr.docChanged || tr.selection || pictures ? blockDecorations(tr.state) : deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/**
 * Does this click mean "follow the wikilink"? ⌘ only: on macOS ⌃-click is the secondary
 * click, so accepting ⌃ would navigate and open a context menu at once. Exported so the
 * suite can pin it.
 */
export function isFollowClick(e: Pick<MouseEvent, "metaKey">): boolean {
  return e.metaKey;
}

/**
 * The live-preview extension: inline decorations, table block widgets (spec §8) and the
 * proportional-font `lp-body` class (spec §3, §5). ⌘-click on a wikilink calls
 * `onFollow`; a plain click places the cursor. Excludes `embedImagesField` (added once
 * by main.ts, so it survives the `</>` swap).
 */
export function livePreview(onFollow: (target: string) => void): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = inlineDecorations(view.state, view.visibleRanges);
      }
      update(u: ViewUpdate): void {
        // A picture landing changes no text, so watch its effect too.
        const pictures = u.transactions.some((tr) =>
          tr.effects.some((e) => e.is(setEmbedImages)),
        );
        if (u.docChanged || u.selectionSet || u.viewportChanged || pictures) {
          this.decorations = inlineDecorations(u.view.state, u.view.visibleRanges);
        }
      }
    },
    {
      decorations: (v) => v.decorations,
      eventHandlers: {
        mousedown(e: MouseEvent): boolean {
          if (!isFollowClick(e)) return false;
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
