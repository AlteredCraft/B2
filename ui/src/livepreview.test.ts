// The live-preview engine (livepreview.ts), off the DOM: the real Lezer parser (with the
// `Wikilink` node) and assertions over the decorations it yields. Every row is a byte-offset
// claim no type checks: conceal one byte too many and a character vanishes from view.
//
// Hand-rolled asserts. Run directly:
//   node --experimental-strip-types src/livepreview.test.ts

import { syntaxTree } from "@codemirror/language";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import type { DecorationSet, WidgetType } from "@codemirror/view";
import {
  WIKILINK_RE,
  blockDecorations,
  embedImagesField,
  inlineDecorations,
  isFollowClick,
  taskChecked,
  wikilink,
} from "./livepreview.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual, null, 1);
  const e = JSON.stringify(expected, null, 1);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- the fixture rig ----------------------------------------------------------------

// The editor's own language config (main.ts): GFM plus `Wikilink`.
const LANG = markdown({ base: markdownLanguage, extensions: [wikilink] });

/** A state over `doc` with the cursor at `cursor`, by default the end. Fixtures end in a
 *  newline so the default caret sits on an empty line and reveals nothing. */
function stateOf(doc: string, cursor = doc.length, images?: Map<string, string>): EditorState {
  const extensions = images ? [LANG, embedImagesField.init(() => images)] : [LANG];
  return EditorState.create({ doc, selection: { anchor: cursor }, extensions });
}

/** One loaded picture. By default states have no field: the app before bytes arrive. */
const PICTURES = new Map([["a/shot.png", "data:image/png;base64,AAAA"]]);

/** The part of livepreview.ts's `TableWidget` the checks read. */
interface TableWidget {
  md: string;
  images: Map<string, string>;
  eq(o: TableWidget): boolean;
}

interface DecoSpec {
  class?: string;
  attributes?: Record<string, string>;
  widget?: WidgetType;
}

/** A widget as `Name(detail)`. `constructor.name` is reliable: node runs off the source,
 *  not a bundle. */
function widgetLabel(w: WidgetType): string {
  const spec = w as unknown as { checked?: boolean; target?: string; width?: number | null };
  if (spec.checked !== undefined) return `${w.constructor.name}(${spec.checked ? "x" : " "})`;
  if (spec.target !== undefined) {
    return `${w.constructor.name}(${spec.target}${spec.width === null ? "" : ` @${spec.width}`})`;
  }
  return w.constructor.name;
}

/** One decoration per line, as a string, so a mismatch reads as a diff. */
function show(doc: string, set: DecorationSet): string[] {
  const out: string[] = [];
  for (const it = set.iter(); it.value; it.next()) {
    const spec = it.value.spec as DecoSpec;
    const text = JSON.stringify(doc.slice(it.from, it.to));
    if (!it.value.point) {
      const target = spec.attributes?.["data-target"];
      out.push(`mark ${spec.class}${target ? ` →${target}` : ""} ${it.from}-${it.to} ${text}`);
    } else if (spec.widget) {
      out.push(`widget ${widgetLabel(spec.widget)} ${it.from}-${it.to} ${text}`);
    } else if (spec.class) {
      out.push(`line ${spec.class} @${it.from}`);
    } else {
      out.push(`hide ${it.from}-${it.to} ${text}`);
    }
  }
  return out;
}

/** The inline/line decorations over the whole document. */
function inline(doc: string, cursor?: number, images?: Map<string, string>): string[] {
  return show(doc, inlineDecorations(stateOf(doc, cursor, images), [{ from: 0, to: doc.length }]));
}

/** The block decorations (tables; spec §8). */
function block(doc: string, cursor?: number, images?: Map<string, string>): string[] {
  return show(doc, blockDecorations(stateOf(doc, cursor, images)));
}

/** Every node of one name in the real parse tree, as `offset:source`. */
function nodes(doc: string, name: string): string[] {
  const found: string[] = [];
  syntaxTree(stateOf(doc)).iterate({
    enter: (n) => {
      if (n.name === name) found.push(`${n.from}:${doc.slice(n.from, n.to)}`);
    },
  });
  return found;
}

// --- following a wikilink with the mouse ---------------------------------------------

check("⌘-click follows a wikilink; a plain click does not", () => {
  assert(isFollowClick({ metaKey: true }), "⌘-click follows");
  assert(!isFollowClick({ metaKey: false }), "a bare click places the cursor");
});

check("⌃-click does not follow — on macOS it is the secondary click", () => {
  assert(!isFollowClick({ metaKey: false, ctrlKey: true } as MouseEvent), "⌃-click is a right-click");
  assert(isFollowClick({ metaKey: true, ctrlKey: true } as MouseEvent), "⌃⌘-click still follows");
});

// --- the task marker ------------------------------------------------------------------

check("a task marker is checked for `[x]` and `[X]`, and for nothing else", () => {
  assert(taskChecked("[x]"), "the lowercase spelling");
  assert(taskChecked("[X]"), "GFM accepts the uppercase one, so the widget must too");
  assert(!taskChecked("[ ]"), "the third spelling the grammar emits is unchecked");
  for (const s of ["", "[]", "[-]", "[ x ]", "x", "[x", "[x] "]) {
    assert(!taskChecked(s), `not a marker: ${JSON.stringify(s)}`);
  }
});

// --- the wikilink grammar --------------------------------------------------------------
//
// `parseInline` decides where a `Wikilink` node is; `WIKILINK_RE` re-derives its parts.

check("a wikilink parses with or without a label, and only where it closes", () => {
  assertEq(nodes("see [[a/b]] here\n", "Wikilink"), ["4:[[a/b]]"], "bare target");
  assertEq(nodes("[[a/b|Label]]\n", "Wikilink"), ["0:[[a/b|Label]]"], "target|label");
  assertEq(nodes("[[a]] [[b]]\n", "Wikilink"), ["0:[[a]]", "6:[[b]]"], "two on a line");
  assertEq(nodes("[[a\n", "Wikilink"), [], "unterminated");
  assertEq(nodes("[[a]\n", "Wikilink"), [], "one closer is not two");
  assertEq(nodes("[[a\nb]]\n", "Wikilink"), [], "a wikilink never spans a line break");
  assertEq(nodes("[[a]b]]\n", "Wikilink"), [], "an inner `]` ends the scan, and `]b` is not `]]`");
});

check("empty content and a leading `|` are rejected, so `[[` never eats a plain link", () => {
  assertEq(nodes("[[]]\n", "Wikilink"), [], "empty content");
  assertEq(nodes("[[|x]]\n", "Wikilink"), [], "a leading `|` means no target");
  assertEq(nodes("[[|x]]\n", "Link"), ["1:[|x]"], "and the standard parser gets its turn");
  assertEq(nodes("see [[a]] and [b](c)\n", "Wikilink"), ["4:[[a]]"], "the wikilink wins");
  assertEq(nodes("see [[a]] and [b](c)\n", "Link"), ["14:[b](c)"], "the markdown link is untouched");
});

check("WIKILINK_RE splits a node into target and label", () => {
  // Groups: the embed marker, the target, the `|`-part.
  assertEq(WIKILINK_RE.exec("[[a]]")?.slice(1), ["", "a", undefined], "no marker, no label");
  assertEq(WIKILINK_RE.exec("[[a/b|Label]]")?.slice(1), ["", "a/b", "Label"], "target|label");
  assertEq(WIKILINK_RE.exec("![[a/b.png|500]]")?.slice(1), ["!", "a/b.png", "500"], "the embed");
  assertEq(WIKILINK_RE.exec("[[a|b|c]]")?.slice(1), ["", "a", "b|c"], "only the first `|` splits");
  // Offsets are derived from the match, so a floating match would misplace the label.
  assertEq(WIKILINK_RE.exec("[[a]] x"), null, "anchored: trailing text is not a match");
});

check("the parse rule accepts `[[a|]]`, the regex refuses it, and the text stays raw", () => {
  // The designed disagreement (spec §4): an unmatched node degrades to raw.
  assertEq(nodes("[[a|]]\n", "Wikilink"), ["0:[[a|]]"], "the node exists");
  assertEq(nodes("![[a|]]\n", "Wikilink"), ["0:![[a|]]"], "…and so does the embed's");
  assertEq(WIKILINK_RE.exec("[[a|]]"), null, "the regex declines it");
  assertEq(inline("[[a|]]\n"), [], "so nothing is decorated and the source shows through");
});

// --- inline and line decorations --------------------------------------------------------

check("a heading is a line class plus a concealed `#` run", () => {
  // The trailing space goes with the marker, as in the reading view.
  assertEq(inline("## Deep work\n"), ["line lp-h2 @0", 'hide 0-3 "## "'], "level 2");
  assertEq(inline("###### six\n"), ["line lp-h6 @0", 'hide 0-7 "###### "'], "level 6");
});

check("a block marker reveals for the whole line the cursor is on", () => {
  // The style survives the reveal, so the line under the cursor doesn't resize.
  assertEq(inline("## Deep work\n", 3), ["line lp-h2 @0"], "cursor inside the heading");
  assertEq(inline("## Deep work\n", 0), ["line lp-h2 @0"], "and at the line's very start");
});

check("inline markup styles the element and conceals its marks", () => {
  assertEq(
    inline("a *b* **c** ~~d~~ `e`\n"),
    [
      'hide 2-3 "*"',
      'mark lp-em 2-5 "*b*"',
      'hide 4-5 "*"',
      'hide 6-8 "**"',
      'mark lp-strong 6-11 "**c**"',
      'hide 9-11 "**"',
      'hide 12-14 "~~"',
      'mark lp-strike 12-17 "~~d~~"',
      'hide 15-17 "~~"',
      'hide 18-19 "`"',
      'mark lp-code 18-21 "`e`"',
      'hide 20-21 "`"',
    ],
    "four constructs, each styled across the marks it then hides",
  );
});

check("inline markup reveals per element, not per line — the hybrid policy (spec §3)", () => {
  assertEq(
    inline("a *b* **c** ~~d~~ `e`\n", 3),
    [
      'mark lp-em 2-5 "*b*"',
      'hide 6-8 "**"',
      'mark lp-strong 6-11 "**c**"',
      'hide 9-11 "**"',
      'hide 12-14 "~~"',
      'mark lp-strike 12-17 "~~d~~"',
      'hide 15-17 "~~"',
      'hide 18-19 "`"',
      'mark lp-code 18-21 "`e`"',
      'hide 20-21 "`"',
    ],
    "the cursor's own element is raw; its neighbours are not",
  );
});

check("an inline link shows its text; a reference-style one is left alone", () => {
  assertEq(
    inline("see [text](http://x) and [ref]\n"),
    ['hide 4-5 "["', 'mark lp-link 4-20 "[text](http://x)"', 'hide 9-20 "](http://x)"'],
    "`[` and `](url)` go; the text stays",
  );
  assertEq(inline("[ref]\n"), [], "reference-style stays raw");
});

check("a wikilink shows its label, carrying the target for the click handler", () => {
  assertEq(
    inline("see [[a/b|Label]] and [[plain]]\n"),
    [
      'hide 4-10 "[[a/b|"',
      'mark lp-wikilink →a/b 10-15 "Label"',
      'hide 15-17 "]]"',
      'hide 22-24 "[["',
      'mark lp-wikilink →plain 24-29 "plain"',
      'hide 29-31 "]]"',
    ],
    "labelled and bare, each with `data-target` — what `isFollowClick`'s handler reads",
  );
});

// --- the embed form, `![[…]]` ---------------------------------------------------------
//
// The node claims the `!`, so the Image parser never takes `![[shot.png]]`.

check("the embed marker is part of the node, not an Image wrapping a link", () => {
  assertEq(nodes("![[a/shot.png]]\n", "Wikilink"), ["0:![[a/shot.png]]"], "one node, marker in");
  assertEq(nodes("![[a/shot.png]]\n", "Image"), [], "the Image parser never gets it");
  assertEq(nodes("![alt](u.png)\n", "Wikilink"), [], "a real Markdown image is not a wikilink");
  assertEq(nodes("![alt](u.png)\n", "Image"), ["0:![alt](u.png)"], "…and is still an Image");
});

check("an embed with no picture reads as its link, marker concealed", () => {
  // As in the reading view: the `!` is concealed, and the `|`-part is a width, not a label.
  assertEq(
    inline("![[a/shot.png]]\n"),
    ['hide 0-3 "![["', 'mark lp-wikilink →a/shot.png 3-13 "a/shot.png"', 'hide 13-15 "]]"'],
    "no bytes in hand — the embed is its link",
  );
  assertEq(
    inline("![[a/shot.png|500]]\n"),
    ['hide 0-3 "![["', 'mark lp-wikilink →a/shot.png 3-13 "a/shot.png"', 'hide 13-19 "|500]]"'],
    "the width hint is concealed with the closing brackets, never shown as a label",
  );
  assertEq(
    inline("![[a/paper.pdf]]\n", undefined, PICTURES),
    ['hide 0-3 "![["', 'mark lp-wikilink →a/paper.pdf 3-14 "a/paper.pdf"', 'hide 14-16 "]]"'],
    "a target that is no picture stays a link however many pictures are loaded",
  );
});

check("an embed whose picture is loaded is replaced by it, width and all", () => {
  assertEq(
    inline("![[a/shot.png]]\n", undefined, PICTURES),
    ['widget EmbedImageWidget(a/shot.png) 0-15 "![[a/shot.png]]"'],
    "the whole construct, marker included, becomes the picture",
  );
  assertEq(
    inline("![[a/shot.png|500]]\n", undefined, PICTURES),
    ['widget EmbedImageWidget(a/shot.png @500) 0-19 "![[a/shot.png|500]]"'],
    "`|500` is the width it is drawn at",
  );
  assertEq(
    inline("![[a/shot.png|500x300]]\n", undefined, PICTURES),
    ['widget EmbedImageWidget(a/shot.png) 0-23 "![[a/shot.png|500x300]]"'],
    "a hint that isn't a width draws the picture at its own size",
  );
  assertEq(
    inline("see ![[a/shot.png]] ok\n", undefined, PICTURES),
    ['widget EmbedImageWidget(a/shot.png) 4-19 "![[a/shot.png]]"'],
    "an embed written mid-sentence is replaced in place, not lifted out of the line",
  );
});

check("the cursor on an embed reveals its source, picture or no picture", () => {
  // Clicking the picture puts the caret in the replaced range, i.e. this state.
  assertEq(
    inline("![[a/shot.png]]\n", 5, PICTURES),
    ['mark lp-wikilink →a/shot.png 3-13 "a/shot.png"'],
    "the picture gives way to the bytes, and only the style survives",
  );
  assertEq(
    inline("![[a/shot.png]]\n", 0, PICTURES),
    ['mark lp-wikilink →a/shot.png 3-13 "a/shot.png"'],
    "the marker is inside the reveal range, so a caret before the `!` reveals too",
  );
  assertEq(
    inline("![[a/shot.png]]\n", 16, PICTURES),
    ['widget EmbedImageWidget(a/shot.png) 0-15 "![[a/shot.png]]"'],
    "…and a caret on the next line does not",
  );
});

check("a plain wikilink to a picture stays a link", () => {
  assertEq(
    inline("[[a/shot.png]]\n", undefined, PICTURES),
    ['hide 0-2 "[["', 'mark lp-wikilink →a/shot.png 2-12 "a/shot.png"', 'hide 12-14 "]]"'],
    "no marker, no picture",
  );
});

check("a wikilink's target is trimmed, and the label span is not", () => {
  assertEq(
    inline("[[ a/b | Label ]]\n"),
    ['hide 0-8 "[[ a/b |"', 'mark lp-wikilink →a/b 8-15 " Label "', 'hide 15-17 "]]"'],
    "target trimmed for the follow, label shown as written",
  );
});

check("a blockquote is muted per line, and every `>` is concealed", () => {
  // A continuation line's QuoteMark hangs off the inner Paragraph, not the Blockquote.
  assertEq(
    inline("> one\n> two\n"),
    ["line lp-quote @0", 'hide 0-2 "> "', "line lp-quote @6", 'hide 6-8 "> "'],
    "two quoted lines, two markers gone",
  );
  assertEq(
    inline("> one\n> two\n", 7),
    ["line lp-quote @0", 'hide 0-2 "> "', "line lp-quote @6"],
    "and the reveal is per line: the cursor's line shows its own `>` only",
  );
});

check("a bullet becomes `•`; an ordered list keeps its number", () => {
  assertEq(
    inline("- a\n* b\n+ c\n1. d\n"),
    [
      'widget BulletWidget 0-1 "-"',
      'widget BulletWidget 4-5 "*"',
      'widget BulletWidget 8-9 "+"',
    ],
    "the three bullet spellings normalize; `1.` is content, not markup",
  );
  assertEq(inline("- a\n", 2), [], "revealed on its own line, like the other block markers");
});

check("a horizontal rule becomes a rule widget", () => {
  assertEq(inline("a\n\n---\n\nb\n"), ['widget RuleWidget 3-6 "---"'], "the `---` is replaced");
  assertEq(inline("a\n\n---\n\nb\n", 4), [], "and revealed as raw with the cursor on it");
});

check("a fenced block is shaded per line, fences included", () => {
  assertEq(
    inline("```rust\nfn a() {}\n```\n"),
    ["line lp-fence @0", "line lp-fence @8", "line lp-fence @18"],
    "three lines shaded, nothing concealed",
  );
});

check("a task marker becomes a checkbox carrying its state and its own offset", () => {
  // The widget's `from` is the marker's `[`; it toggles `from + 1`, so a wrong offset
  // writes the wrong byte of the note.
  assertEq(
    inline("- [ ] todo\n- [x] done\n- [X] shout\n"),
    [
      'widget BulletWidget 0-1 "-"',
      'widget TaskWidget( ) 2-5 "[ ]"',
      'widget BulletWidget 11-12 "-"',
      'widget TaskWidget(x) 13-16 "[x]"',
      'widget BulletWidget 22-23 "-"',
      'widget TaskWidget(x) 24-27 "[X]"',
    ],
    "unchecked, checked, and the uppercase spelling",
  );
  assertEq(
    inline("- [ ] todo\n- [x] done\n", 2),
    ['widget BulletWidget 11-12 "-"', 'widget TaskWidget(x) 13-16 "[x]"'],
    "the cursor's line shows its raw marker; the other keeps its checkbox",
  );
});

check("the viewport bounds the work — a range is a range, not a suggestion", () => {
  // A builder that ignored the ranges would look right and stop scaling (insight §2.1).
  const doc = "# one\n\n# two\n";
  const state = stateOf(doc);
  assertEq(
    show(doc, inlineDecorations(state, [{ from: 0, to: 5 }])),
    ["line lp-h1 @0", 'hide 0-2 "# "'],
    "only the first heading is reached",
  );
  assertEq(show(doc, inlineDecorations(state, [])).length, 0, "no viewport, no decorations");
});

// --- block decorations (tables) ---------------------------------------------------------

check("a table is one block widget over whole lines, and no inline decoration at all", () => {
  const doc = "| a | b |\n| - | - |\n| 1 | 2 |\n";
  // An inline decoration inside a block-replaced range is a CM6 error (spec §8).
  assertEq(inline(doc), [], "the inline pass steps over the table entirely");
  assertEq(block(doc), ['widget TableWidget 0-29 "| a | b |\\n| - | - |\\n| 1 | 2 |"'], "one widget");
});

check("the table widget carries the exact source it hides", () => {
  // A widget whose text drifted from its range would show one table and conceal another.
  const doc = "x\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";
  const set = blockDecorations(stateOf(doc));
  const it = set.iter();
  assert(it.value !== null, "a table is decorated");
  const spec = it.value?.spec as DecoSpec;
  const md = (spec.widget as unknown as { md: string }).md;
  assertEq(md, doc.slice(it.from, it.to), "the widget's markdown is the replaced range verbatim");
  assertEq([it.from, it.to], [3, 32], "snapped to whole lines — a block replace must be");
});

check("the table widget carries the note's pictures, and is re-keyed when they change", () => {
  // The markdown doesn't change when bytes arrive, so the map must re-key the widget.
  const doc = "| a | b |\n| - | - |\n| ![[a/shot.png]] | 2 |\n";
  const widgetOf = (images?: Map<string, string>): TableWidget => {
    const it = blockDecorations(stateOf(doc, doc.length, images)).iter();
    const w = (it.value?.spec as DecoSpec).widget as unknown as TableWidget;
    assert(w !== undefined, "a table is decorated");
    return w;
  };
  assertEq(
    widgetOf(PICTURES).images.get("a/shot.png"),
    "data:image/png;base64,AAAA",
    "the picture reaches the widget, so `renderMarkdown` can draw it in the cell",
  );
  assert(!widgetOf().eq(widgetOf(PICTURES)), "bytes arriving re-key the widget");
  assert(widgetOf(PICTURES).eq(widgetOf(PICTURES)), "…and an unchanged map does not");
});

check("the table the cursor is inside stays raw source", () => {
  const doc = "| a | b |\n| - | - |\n| 1 | 2 |\n";
  assertEq(block(doc, 12), [], "no widget over the table being edited");
  assertEq(inline(doc, 12), [], "nor any inline decoration in its place");
});

console.log(`livepreview: ${passed} checks passed`);
