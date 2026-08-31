// Live-preview decorations — a document feel over the byte-honest buffer
// (crates/b2-desktop/CLAUDE.md). Decorations conceal Markdown markup away from the
// cursor and style content in place; they change what the DOM shows, NEVER what
// `state.doc` holds (spec §0, insight §2.2). The save chain literally cannot observe
// this feature — every construct decorates *within* lines (marks, inline replaces,
// line classes), so the whole engine is one ViewPlugin (spec insight §2.4).
//
// Two exports the app uses: `wikilink` (the Lezer inline node giving the tree B2's most
// important construct) and `livePreview(onFollow)` (the ViewPlugin + the proportional-font
// body class). main.ts keeps `livePreview` in a Compartment so `</>` can swap it for raw
// source mode with no remount.
//
// The rest of the exports are for the suite. Everything that decides *what* to decorate is
// a pure function of (tree, selection, viewport) — so `inlineDecorations` and
// `blockDecorations` take an `EditorState` plus the ranges rather than an `EditorView`,
// which is the only thing the view ever contributed. That makes the whole engine reachable
// from node: the real Lezer grammar parses with no DOM (highlight.test.ts leans on the same
// fact), so livepreview.test.ts asserts against real trees rather than a mock (#120).

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
// Extension-qualified, unlike the app-only modules: node's test runner resolves
// specifiers literally, and this file is in the suite now (livepreview.test.ts).
import { externalUrl } from "./links.ts";
import {
  embedWidth,
  NO_EMBED_IMAGES,
  WIKILINK_EXACT,
  type EmbedImages,
} from "./embeds.ts";
import { renderMarkdown } from "./render.ts";

// --- the wikilink tree extension (spec §4, insight §2.3) --------------------------
//
// A `[[target]]` / `[[target|label]]` inline node — the same grammar as the reading
// view's `marked` tokenizer (render.ts): target = one-or-more chars that aren't `]` or
// `|`, optional `|label` where label is one-or-more non-`]` chars. Giving the tree a
// `Wikilink` node lets the *one* decoration engine style wikilinks uniformly with every
// other construct, instead of a bolt-on. Positions are document-relative throughout.

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
      // Before the standard Link parser — and so before `Image`, which sits after it in
      // the default inline order — so neither `[[` nor `![[` is first eaten as a link or
      // a reference-style image. It *was* only ahead of `Link`, and the embed form paid
      // for it: `![[shot.png]]` parsed as an Image wrapping a Link, no `Wikilink` node
      // formed at all, and the editor showed an embed as raw text while the reading view
      // rendered it.
      before: "Link",
      parse(cx: InlineContext, next: number, pos: number): number {
        // The embed marker is part of the construct, so the node covers it: one node per
        // wikilink however it was written, and no handler has to read a byte outside its
        // own node to find out which form it is looking at.
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

/** The engine re-derives the `[[..]]` / `![[..]]` structure from the node text — its span
 *  is exactly the wikilink, so a whole-string match yields the marker, the target, and the
 *  label/pipe offsets. It is the reading view's grammar (embeds.ts) with both ends pinned:
 *  one spelling of what a wikilink *is*, so read and edit cannot drift.
 *
 *  Deliberately *stricter* than the parse rule above, which accepts a `[[a|]]` the empty
 *  `([^\]]+)` label group rejects. The two are allowed to disagree: the node exists, the
 *  match fails, and the Wikilink handler leaves the text raw (spec §4). */
export const WIKILINK_RE = WIKILINK_EXACT;

// --- the note's pictures (the `![[image.png]]` embed) --------------------------------
//
// An embed's bytes arrive over IPC long after the editor mounted, and they arrive for
// the *document*, not for the editor — the reading view draws the same map (render.ts).
// So they enter the editor as ordinary editor state: a field main.ts writes with an
// effect, which every decoration pass then reads. That is what keeps `inlineDecorations`
// a pure function of `EditorState` (the property the whole suite leans on) instead of a
// function of whatever main.ts's module scope happened to hold at paint time.
//
// The field lives *outside* the live-preview compartment on purpose: which pictures the
// note has loaded is a fact about the document, not a viewing mode, so toggling `</>` to
// raw source and back must not drop them.

/** Hand the editor the note's loaded pictures (path → `data:` URL). */
export const setEmbedImages = StateEffect.define<EmbedImages>();

/** Where that map lives between paints. Add it to the editor's extensions once; the
 *  decorations read it, and nothing else in the editor knows about it. */
export const embedImagesField = StateField.define<EmbedImages>({
  create: () => NO_EMBED_IMAGES,
  update(images, tr) {
    for (const e of tr.effects) if (e.is(setEmbedImages)) return e.value;
    return images;
  },
});

/** The pictures this state carries, or none — `false` so a state assembled without the
 *  field (a test, or a future editor that doesn't want embeds) reads as empty rather
 *  than throwing. */
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
  // Fields declared and assigned rather than written as constructor parameter
  // properties: those are the one TypeScript construct node's `--experimental-strip-types`
  // can't erase, and using them here made the *whole module* unimportable by the test
  // runner — including the pure rules at the bottom of the file. Same below.
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
//
// One deliberate gap: it renders with **no pictures**, so an `![[image.png]]` inside a
// cell reads as its link here while the reading view draws it. The widget is keyed on
// its Markdown alone, so it wouldn't rebuild when the bytes landed in any case — and an
// image in a table cell is a corner worth less than a second cache key.
class TableWidget extends WidgetType {
  readonly md: string;
  readonly from: number;
  constructor(md: string, from: number) {
    super();
    this.md = md;
    this.from = from;
  }
  eq(o: TableWidget): boolean {
    return o.md === this.md && o.from === this.from;
  }
  toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("div");
    wrap.className = "lp-table";
    wrap.innerHTML = renderMarkdown(this.md);
    wrap.addEventListener("mousedown", (e) => {
      // Let a link click fall through to the app's own handlers rather than yanking the
      // caret out from under it: a wikilink to the follow path, a web link to the OS
      // handoff (links.ts owns which hrefs those are — both are one click delegation in
      // main.ts, and both are a *navigation*, not an edit of the table).
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

// The picture of an `![[image.png]]` embed, drawn in the buffer's place — the editor's
// half of the reading view's inline image (render.ts). An inline replace, not a block
// widget: it spans no line break, so it belongs to the ViewPlugin with every other
// inline conceal, and an embed written mid-sentence stays mid-sentence.
//
// Clicking it **reveals the source**, and it does so without a line of code here: the
// widget declines to ignore the event (`ignoreEvent` → false, against `WidgetType`'s
// default), so CodeMirror handles the click itself and puts the cursor at the replaced
// range — which is precisely the reveal condition the decoration is computed from. The
// same is true of arrowing onto it. A widget that swallowed its clicks would be a hole
// in the buffer: a picture you cannot get a caret next to is a picture you cannot edit
// the markup of.
//
// It carries `data-target` like every other wikilink, so ⌘-click follows it to the
// resource card through the plugin's own mousedown handler (`isFollowClick`).
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
    // The filename, for the reading view's reason: it is all B2 knows about the picture.
    img.alt = this.target.split("/").pop() ?? this.target;
    img.setAttribute("data-target", this.target);
    if (this.width !== null) img.width = this.width;
    return img;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

/** Is this GFM task marker checked? `[x]` and `[X]` are; the third spelling the grammar
 *  emits a `TaskMarker` for, `[ ]`, is not — and nothing else is a marker at all. */
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

// Style decorations are emitted unconditionally; conceals only when the reveal range —
// the *line* for block markers, the *element span* for inline markup (spec §3 hybrid
// policy) — doesn't touch the selection. Every branch is line-local, so the plugin is
// legal and this is a pure function of (tree, selection, viewport).
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
    //
    // The **embed** form `![[…]]` is the same node one byte to the right: the `!` is
    // plain text to the grammar (the tree has no node for it), so the construct's real
    // span starts at `node.from - 1` and every offset below is taken from that. Three
    // things follow from the marker, and all three are the reading view's rules —
    // read and edit must not disagree about what a note says (render.ts):
    //
    //   • the `|`-part is a display **width**, not a label, so an embed shows its
    //     *target* where a plain wikilink shows its label (never a bare "500");
    //   • with the picture in hand, the whole thing is replaced by the picture;
    //   • without one, it reads as its link — with the `!` concealed, because the
    //     marker is grammar and a grammar character in the prose is a rendering bug.
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
      // Offsets into the raw text, which the match is anchored to: the target always
      // starts just past the (optional) marker and `[[`, and it is what an embed shows.
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

    // Blockquote: border + muted per line. The `>` markers conceal themselves, one case
    // down — the tree does not hang them all off the Blockquote (see there).
    case "Blockquote": {
      eachLine(doc, node.from, node.to, (lineFrom) => {
        decos.push(Decoration.line({ class: "lp-quote" }).range(lineFrom));
      });
      return;
    }

    // A `>` marker, concealed wherever the tree keeps it (reveal per its own line, like
    // every other block marker). Its own case rather than the Blockquote's children,
    // because only the *first* line's mark is a child of the Blockquote: the continuation
    // lines of a wrapped quote hang theirs off the inner Paragraph, so collecting by
    // direct child left every line but the first showing a raw `>` under the quote bar.
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

    // Fenced code: block background per line; the fences stay visible (spec §3 — hiding
    // them would hide the language tag for little gain), so no conceal.
    case "FencedCode": {
      eachLine(doc, node.from, node.to, (lineFrom) => {
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

/** Fold the syntax tree + selection over `ranges` into a sorted DecorationSet. `ranges` is
 *  the view's *viewport* in the app, which is what makes cost scale with the screen rather
 *  than the note (insight §2.1) — and the only thing the plugin's `EditorView` was for. */
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
export function blockDecorations(state: EditorState): DecorationSet {
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
  create: (state) => blockDecorations(state),
  update(deco, tr) {
    // Reveal keys on the selection, so a bare cursor move recomputes too.
    return tr.docChanged || tr.selection ? blockDecorations(tr.state) : deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/**
 * Does this click mean "follow the wikilink" rather than "put the cursor here"?
 *
 * ⌘ only. It used to take ⌃ as well — the same `metaKey || ctrlKey` reflex the keyboard
 * handlers had (bindings.ts `Chord.mod`), and wrong here for a sharper reason than there:
 * on macOS **⌃-click *is* the secondary click**. The OS synthesizes a context-menu gesture
 * from it, so ⌃-clicking a wikilink navigated away *and* right-clicked, which is not a
 * thing any one gesture should do.
 *
 * Its own function, exported, because a mouse handler buried in a `ViewPlugin`'s
 * `eventHandlers` is unreachable from the suite — and this rule is exactly the sort that
 * gets casually re-broken by someone restoring "cross-platform" symmetry.
 */
export function isFollowClick(e: Pick<MouseEvent, "metaKey">): boolean {
  return e.metaKey;
}

/**
 * The live-preview extension: the ViewPlugin folding tree+selection into inline/line
 * decorations, the `blockField` feeding block widgets (tables — spec §8), plus the
 * `lp-body` class that swaps the editor to the reading view's proportional voice
 * (spec §3, §5). ⌘-click a wikilink follows it via `onFollow`; a plain click falls
 * through to place the cursor, as an editor must (spec §3).
 *
 * Not included: `embedImagesField`. It holds a fact about the *document*, so the editor
 * adds it once (main.ts) and it survives the `</>` swap that reconfigures this.
 */
export function livePreview(onFollow: (target: string) => void): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = inlineDecorations(view.state, view.visibleRanges);
      }
      update(u: ViewUpdate): void {
        // …and when a picture lands: the effect changes no text and moves no cursor, so
        // without this the note keeps reading as its links until the next keystroke.
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
