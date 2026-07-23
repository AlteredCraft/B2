// Tests for the pure inline-formatting logic (format.ts) — the ⌘B/⌘I toggle engine.
// Run directly:  node --experimental-strip-types src/format.test.ts
// Hand-rolled asserts, the panes.test.ts / wikicomplete.test.ts idiom.

import { BOLD, FORMATS, ITALIC, insertTable, toggleInline, type InlineFormat } from "./format.ts";

let checks = 0;

function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
  checks++;
}

/** Apply a toggle's changes to a doc string — verifies the coordinate math end-to-end. */
function applied(doc: string, r: ReturnType<typeof toggleInline>): string {
  let out = doc;
  // Changes are in original-doc coordinates; apply back-to-front so they don't shift.
  for (const c of [...r.changes].sort((x, y) => y.from - x.from)) {
    out = out.slice(0, c.from) + c.insert + out.slice(c.to);
  }
  return out;
}

// --- the format table: what main.ts builds the keymap from -------------------------

assertEq(
  FORMATS.map((f) => [f.id, f.marker, f.key]),
  [
    ["bold", "**", "Mod-b"],
    ["italic", "*", "Mod-i"],
  ],
  "the table carries id + marker + key — a future format is one new row",
);

// --- wrapping a selection ----------------------------------------------------------

{
  const r = toggleInline("hello world", 0, 5, BOLD);
  assertEq(applied("hello world", r), "**hello** world", "bold wraps the selection");
  assertEq([r.selFrom, r.selTo], [2, 7], "the content stays selected (markers outside)");
}

// --- unwrapping --------------------------------------------------------------------

{
  const r = toggleInline("**hello** world", 2, 7, BOLD);
  assertEq(applied("**hello** world", r), "hello world", "bold unwraps around the content");
  assertEq([r.selFrom, r.selTo], [0, 5], "selection lands on the bare content");
}
{
  const r = toggleInline("**hello** world", 0, 9, BOLD);
  assertEq(
    applied("**hello** world", r),
    "hello world",
    "a selection that includes the markers unwraps too",
  );
}

// --- the star-nesting rules: bold and italic share a character ---------------------

{
  const r = toggleInline("**hello**", 2, 7, ITALIC);
  assertEq(
    applied("**hello**", r),
    "***hello***",
    "italic on a bold span nests — never mistakes ** for italic",
  );
}
{
  const r = toggleInline("***hello***", 3, 8, ITALIC);
  assertEq(applied("***hello***", r), "**hello**", "italic lifts off bold+italic, keeping bold");
}
{
  const r = toggleInline("***hello***", 3, 8, BOLD);
  assertEq(applied("***hello***", r), "*hello*", "bold lifts off bold+italic, keeping italic");
}
{
  const r = toggleInline("_hi_", 1, 3, ITALIC);
  assertEq(applied("_hi_", r), "_*hi*_", "underscore emphasis is content, not a star marker");
}

// --- a bare cursor: toggle the word under it ---------------------------------------

{
  const r = toggleInline("make it bold now", 10, 10, BOLD);
  assertEq(applied("make it bold now", r), "make it **bold** now", "cursor-in-word wraps the word");
  assertEq([r.selFrom, r.selTo], [10, 14], "the word is left selected for a follow-up toggle");
}
{
  const r = toggleInline("an **inner** word", 7, 7, BOLD);
  assertEq(applied("an **inner** word", r), "an inner word", "cursor inside a bold word unwraps it");
}

// --- a bare cursor with no word: an empty pair, caret centered ---------------------

{
  const r = toggleInline("a  b", 2, 2, BOLD);
  assertEq(applied("a  b", r), "a **** b", "no word under the cursor → an empty pair");
  assertEq([r.selFrom, r.selTo], [4, 4], "caret lands between the markers, ready to type");
}
{
  const r = toggleInline("a **** b", 4, 4, BOLD);
  assertEq(applied("a **** b", r), "a  b", "toggling again inside the empty pair removes it");
  assertEq([r.selFrom, r.selTo], [2, 2], "caret returns to where the pair was");
}

// --- extensibility: a distinct-marker format works through the same engine ---------

const STRIKE: InlineFormat = { id: "strike", marker: "~~", key: "Mod-Shift-x" };
{
  const r = toggleInline("gone", 0, 4, STRIKE);
  assertEq(applied("gone", r), "~~gone~~", "a future format wraps with no new logic");
}
{
  const r = toggleInline("~~gone~~", 2, 6, STRIKE);
  assertEq(applied("~~gone~~", r), "gone", "and unwraps");
}

// --- ⌘T: insert a table --------------------------------------------------------------

const TABLE =
  "| Column 1 | Column 2 | Column 3 |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |";

/** Apply an insertTable result to a doc string (single change, forward-safe). */
function insApplied(doc: string, r: ReturnType<typeof insertTable>): string {
  const c = r.changes[0];
  return doc.slice(0, c.from) + c.insert + doc.slice(c.to);
}

{
  const r = insertTable("", 0, 0);
  assertEq(insApplied("", r), TABLE + "\n", "into an empty doc: the table, newline-terminated");
  // Caret sits in the first body cell (between its padding spaces).
  assertEq(insApplied("", r).slice(r.selFrom, r.selFrom + 1), " ", "caret lands inside the first cell");
  const typed = insApplied("", r);
  assertEq(
    typed.slice(0, r.selFrom) + "a" + typed.slice(r.selFrom),
    "| Column 1 | Column 2 | Column 3 |\n| --- | --- | --- |\n| a |  |  |\n|  |  |  |\n",
    "typing at the caret fills the first cell, padded",
  );
}

{
  // Mid-paragraph: pad both sides to a blank line so the table is its own block.
  const doc = "item one";
  const r = insertTable(doc, doc.length, doc.length);
  assertEq(insApplied(doc, r), "item one\n\n" + TABLE + "\n", "end of a line → blank line before, newline after");
}

{
  // A blank line already above → don't double it.
  const doc = "a\n\n";
  const r = insertTable(doc, doc.length, doc.length);
  assertEq(insApplied(doc, r), "a\n\n" + TABLE + "\n", "an existing blank line above isn't doubled");
}

{
  // Content follows → a blank line after as well, and only one is added.
  const doc = "before\n\nafter";
  const at = "before\n\n".length;
  const r = insertTable(doc, at, at);
  assertEq(
    insApplied(doc, r),
    "before\n\n" + TABLE + "\n\nafter",
    "content on both sides → single blank line each side",
  );
}

console.log(`format.test: ${checks} checks passed`);
