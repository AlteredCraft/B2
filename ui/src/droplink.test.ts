// Dropping a discovery card into the note (droplink.ts) — the rules, off the DOM.
//
// Why it's worth pinning. This gesture is the one place a *pointer* writes into a note's
// body, and its whole contract is two claims about byte offsets: where the link lands, and
// where it must not land at all. Both are invisible until they're wrong — a link inserted
// mid-word splits a sentence in a file the user owns, and one inserted into a fence is a
// link that will never resolve, silently. The preview the drag paints is computed by the
// same `planDrop` these cases call, so what a test asserts here is exactly what the ghost
// promises on screen.
//
// The insertions are applied to real `EditorState`s parsed by the app's own grammar (the
// livepreview.test.ts rig, for its reason: a mocked tree would let a code fence go
// unrecognized here and undetected there). `toDOM` is the one thing that wants a document,
// and nothing below reaches it.
//
// Hand-rolled asserts, the livepreview.test.ts idiom. Run directly:
//   node --experimental-strip-types src/droplink.test.ts

import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { inCodeAt, lineDrop, planDrop } from "./droplink.ts";
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

// The editor's own language config (main.ts) — anything less and the fences below would be
// paragraphs, which is precisely the distinction under test.
const LANG = markdown({ base: markdownLanguage, extensions: [wikilink] });

function stateOf(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [LANG] });
}

/** The document a drop at `pos` would leave behind — the assertion that matters, since a
 *  plan is only ever right or wrong about the text it produces. `null` when refused. */
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
  // The "see also" line — the gesture's most common target, and the one where a leading
  // space would be a stray byte in an otherwise clean file.
  assertEq(lineDrop("", "notes/chunks"), { offset: 0, insert: "[[notes/chunks]]" }, "no space");
});

check("a line already ending in whitespace reuses it rather than adding a second", () => {
  // `- ` is the case this exists for: a fresh bullet the user has just typed. The link
  // completes the item (`- [[x]]`) instead of `- <space>[[x]]`.
  assertEq(lineDrop("- ", "notes/chunks"), { offset: 2, insert: "[[notes/chunks]]" }, "bullet");
  assertEq(lineDrop("  ", "notes/chunks"), { offset: 2, insert: "[[notes/chunks]]" }, "indent kept");
});

check("the insertion is the true end of the line, so nothing dangles after the link", () => {
  // The offset is `lineText.length`, never the trimmed length: inserting *before* trailing
  // whitespace would leave `[[x]] ` behind, and two spaces there are a Markdown hard break
  // the user never typed.
  const { offset } = lineDrop("done   ", "t");
  assertEq(offset, 7, "past the trailing run");
});

check("a drop anywhere on a line lands at that line's end, never mid-word", () => {
  // Aiming at a *line* rather than a character is what makes the gesture teachable — and
  // what stops a release over the middle of "semantics" from splitting it.
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
  // `posAtCoords(…, false)` clamps a release in the pane's tail padding to the document's
  // end, which is this line. It is where "add a link at the bottom" naturally aims.
  const doc = "only paragraph\n";
  assertEq(dropped(doc, doc.length), "only paragraph\n[[notes/other]]", "at the end");
});

// --- where it must not ---------------------------------------------------------------

check("a fenced code line refuses the drop rather than writing a link that can't resolve", () => {
  const doc = "prose\n\n```rust\nlet x = 1;\n```\n\nmore\n";
  assertEq(dropped(doc, doc.indexOf("let x")), null, "inside the fence");
  assertEq(dropped(doc, doc.indexOf("```rust")), null, "on the opening fence line");
  // …and the prose around it is unaffected: the refusal is about code, not about the note.
  assert(dropped(doc, 0) !== null, "the paragraph above still accepts");
  assert(dropped(doc, doc.indexOf("more")) !== null, "and the one below");
});

check("an indented code block refuses too — it is code without a fence", () => {
  // And it is why the question is asked at the line's first non-blank character: the
  // CodeBlock node begins *after* the four spaces, so a read at the line's start would
  // call this prose and drop a wikilink into a command.
  const doc = "intro\n\n    cargo test\n\nafter\n";
  assertEq(dropped(doc, doc.indexOf("cargo")), null, "the indented block");
});

check("a blank line inside a fence refuses — it is still code", () => {
  const doc = "```\nlet x = 1;\n\nlet y = 2;\n```\n";
  assertEq(dropped(doc, doc.indexOf("\n\n") + 1), null, "the empty line between statements");
});

check("a line that merely ends in an inline span still accepts", () => {
  // The link lands *after* the closing backtick, outside the span, so there is no hazard
  // to refuse — and "see `foo`" is an ordinary sentence to want a link on.
  const doc = "see `foo`\n";
  assertEq(dropped(doc, 2), "see `foo` [[notes/other]]\n", "prose with code in it");
});

check("the code check reads a position, not the selection", () => {
  // main.ts's cursor-shaped `inCodeContext` delegates to this: a drop names a place the
  // caret isn't, so the position has to be the argument rather than an implicit read.
  // Unlike the drop's own question, this one counts an inline span — pasted HTML inside
  // one must stay literal (paste.ts), which is a different rule for a different reason.
  const state = stateOf("text `inline` text\n");
  assert(inCodeAt(state, 8), "inside the inline span");
  assert(!inCodeAt(state, 1), "and not outside it");
});

// --- the target's spelling -----------------------------------------------------------

check("a note is linked by its path minus .md — the completion's spelling, once", () => {
  // Shared with wikicomplete.ts on purpose: two spellings of a target would be two ways to
  // author a dangling link (the engine resolves the extension-less form for notes).
  assertEq(noteTarget("notes/deep/idea.md"), "notes/deep/idea", "stripped");
  assertEq(noteTarget("notes/idea.markdown"), "notes/idea.markdown", "only the real suffix");
  const doc = "x\n";
  assertEq(dropped(doc, 0, noteTarget("a/b.md")), "x [[a/b]]\n", "and that is what lands");
});

console.log(`droplink: ${passed} checks passed`);
