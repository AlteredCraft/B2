// Dropping a discovery card into the note (droplink.ts), off the DOM: where the link lands
// and where it must not. The drag preview uses the same `planDrop`, over real
// `EditorState`s parsed by the app's grammar.
//
// Hand-rolled asserts. Run directly:
//   node --experimental-strip-types src/droplink.test.ts

import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { inCodeAt, lineDrop, planDrop, withoutCard } from "./droplink.ts";
import { wikilink } from "./livepreview.ts";
import { noteTarget } from "./wikicomplete.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// The editor's own language config (main.ts), so fences parse as fences.
const LANG = markdown({ base: markdownLanguage, extensions: [wikilink] });

function stateOf(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [LANG] });
}

/** The document a drop at `pos` would leave behind, or `null` when refused. */
function dropped(doc: string, pos: number, target = "notes/other"): string | null {
  const state = stateOf(doc);
  const plan = planDrop(state, pos, target);
  if (plan === null) return null;
  return state.update({ changes: { from: plan.from, insert: plan.insert } }).state.doc.toString();
}

// --- where the link lands ------------------------------------------------------------

check("a line with prose takes the link at its end, one space away", () => {
  assertEq(lineDrop("chunking is the unit", "notes/chunks"), {
    offset: 20,
    insert: " [[notes/chunks]]",
  }, "appended, spaced");
});

check("an empty line takes the link alone", () => {
  assertEq(lineDrop("", "notes/chunks"), { offset: 0, insert: "[[notes/chunks]]" }, "no space");
});

check("a line already ending in whitespace reuses it rather than adding a second", () => {
  assertEq(lineDrop("- ", "notes/chunks"), { offset: 2, insert: "[[notes/chunks]]" }, "bullet");
  assertEq(lineDrop("  ", "notes/chunks"), { offset: 2, insert: "[[notes/chunks]]" }, "indent kept");
});

check("the insertion is the true end of the line, so nothing dangles after the link", () => {
  // Inserting before trailing whitespace would leave a hard break the user never typed.
  const { offset } = lineDrop("done   ", "t");
  assertEq(offset, 7, "past the trailing run");
});

check("a drop anywhere on a line lands at that line's end, never mid-word", () => {
  const doc = "alpha beta gamma\nsecond line\n";
  assertEq(dropped(doc, 3), "alpha beta gamma [[notes/other]]\nsecond line\n", "from inside word 1");
  assertEq(dropped(doc, 13), "alpha beta gamma [[notes/other]]\nsecond line\n", "same line, later");
  assertEq(dropped(doc, 20), "alpha beta gamma\nsecond line [[notes/other]]\n", "the second line");
});

check("the blank line between paragraphs takes the link on its own", () => {
  const doc = "para one\n\npara two\n";
  assertEq(dropped(doc, 9), "para one\n[[notes/other]]\npara two\n", "the empty line only");
});

check("the trailing empty line — dropping under the last paragraph — is a target too", () => {
  // `posAtCoords(…, false)` clamps a release in the tail padding to the document's end.
  const doc = "only paragraph\n";
  assertEq(dropped(doc, doc.length), "only paragraph\n[[notes/other]]", "at the end");
});

// --- where it must not ---------------------------------------------------------------

check("a fenced code line refuses the drop rather than writing a link that can't resolve", () => {
  const doc = "prose\n\n```rust\nlet x = 1;\n```\n\nmore\n";
  assertEq(dropped(doc, doc.indexOf("let x")), null, "inside the fence");
  assertEq(dropped(doc, doc.indexOf("```rust")), null, "on the opening fence line");
  assert(dropped(doc, 0) !== null, "the paragraph above still accepts");
  assert(dropped(doc, doc.indexOf("more")) !== null, "and the one below");
});

check("an indented code block refuses too — it is code without a fence", () => {
  // The CodeBlock node begins after the indent, hence asking at the first non-blank char.
  const doc = "intro\n\n    cargo test\n\nafter\n";
  assertEq(dropped(doc, doc.indexOf("cargo")), null, "the indented block");
});

check("a blank line inside a fence refuses — it is still code", () => {
  const doc = "```\nlet x = 1;\n\nlet y = 2;\n```\n";
  assertEq(dropped(doc, doc.indexOf("\n\n") + 1), null, "the empty line between statements");
});

check("a line that merely ends in an inline span still accepts", () => {
  const doc = "see `foo`\n";
  assertEq(dropped(doc, 2), "see `foo` [[notes/other]]\n", "prose with code in it");
});

check("the code check reads a position, not the selection", () => {
  // Unlike the drop's check, this counts an inline span (paste.ts keeps HTML literal there).
  const state = stateOf("text `inline` text\n");
  assert(inCodeAt(state, 8), "inside the inline span");
  assert(!inCodeAt(state, 1), "and not outside it");
});

// --- the target's spelling -----------------------------------------------------------

check("the linked card leaves the list by its path, not by the link it wrote", () => {
  // PR #185: filtering by target instead of path left the linked card in the list.
  const card = { path: "notes/x.md", target: "notes/x" };
  const cards = [{ path: "notes/w.md" }, { path: "notes/x.md" }, { path: "notes/y.md" }];
  assertEq(withoutCard(cards, card), [{ path: "notes/w.md" }, { path: "notes/y.md" }], "dropped");
  assertEq(withoutCard(cards, { path: "notes/z.md", target: "notes/z" }), cards, "no false hit");
});

check("a note is linked by its path minus .md — the completion's spelling, once", () => {
  // Shared with wikicomplete.ts, so there is one spelling of a target.
  assertEq(noteTarget("notes/deep/idea.md"), "notes/deep/idea", "stripped");
  assertEq(noteTarget("notes/idea.markdown"), "notes/idea.markdown", "only the real suffix");
  const doc = "x\n";
  assertEq(dropped(doc, 0, noteTarget("a/b.md")), "x [[a/b]]\n", "and that is what lands");
});

console.log(`droplink: ${passed} checks passed`);
