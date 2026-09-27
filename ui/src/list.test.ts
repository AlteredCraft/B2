// Tests for the nested-list engine (list.ts), asserting on the Markdown that comes out
// rather than on the change list. Hand-rolled asserts. Run directly:
//   node --experimental-strip-types src/list.test.ts
import { applyChanges, indentList, outdentList } from "./list.ts";

let passed = 0;

function assertEq(actual: unknown, expected: unknown, msg: string): void {
  const [a, b] = [JSON.stringify(actual), JSON.stringify(expected)];
  if (a !== b) throw new Error(`assertion failed: ${msg}\n  actual:   ${a}\n  expected: ${b}`);
}
function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

/** The document with `|` marking the caret, or `[`…`]` marking a selection. */
function at(marked: string): { doc: string; from: number; to: number } {
  if (marked.includes("|")) {
    const from = marked.indexOf("|");
    return { doc: marked.replace("|", ""), from, to: from };
  }
  const from = marked.indexOf("[");
  const to = marked.indexOf("]") - 1;
  return { doc: marked.replace("[", "").replace("]", ""), from, to };
}

/** Run a command over a marked-up document; returns the Markdown and the new selection. */
function run(
  cmd: typeof indentList,
  marked: string,
): { doc: string; from: number; to: number } | null {
  const { doc, from, to } = at(marked);
  const r = cmd(doc, from, to);
  if (!r) return null;
  return { doc: applyChanges(doc, r.changes), from: r.selFrom, to: r.selTo };
}

// --- when the gesture applies at all -------------------------------------------------

check("Tab outside a list is not the editor's to take", () => {
  // The null leaves Tab meaning "next control"; why this isn't `indentWithTab`.
  assertEq(indentList("A plain paragraph.", 3, 3), null, "a paragraph");
  assertEq(indentList("# A heading\n\ntext", 4, 4), null, "a heading");
  assertEq(outdentList("A plain paragraph.", 3, 3), null, "⇧Tab, the same");
});

check("a thematic break is not a one-item list", () => {
  assertEq(indentList("- a\n\n* * *", 6, 6), null, "* * *");
  assertEq(indentList("- a\n\n---", 6, 6), null, "---");
});

check("Tab in a list is claimed even when nothing can move", () => {
  const first = run(indentList, "- |a\n- b");
  assertEq(first, { doc: "- a\n- b", from: 2, to: 2 }, "the first item has nothing to nest under");
  const top = run(outdentList, "- a\n- |b");
  assertEq(top, { doc: "- a\n- b", from: 6, to: 6 }, "a top-level item has nothing to leave");
});

// --- the rest of the item: continuations and blanks ----------------------------------

check("a continuation line acts on the item it belongs to", () => {
  const r = run(indentList, "- a\n- b\n  more |about b");
  assertEq(r?.doc, "- a\n  - b\n    more about b", "b moved, its text with it");
  const back = run(outdentList, "- a\n  - b\n    more |about b");
  assertEq(back?.doc, "- a\n- b\n  more about b", "⇧Tab, the same owner");
});

check("a lazy continuation at column 0 still names its item", () => {
  assertEq(run(indentList, "- a\n- b\nlazy |line")?.doc, "- a\n  - b\n  lazy line", "b took its lazy line along");
});

check("a paragraph after a blank is not a continuation", () => {
  assertEq(run(indentList, "- a\n\npara|graph"), null, "a new paragraph");
});

check("a block starter after an item is a new block, not the item's text", () => {
  assertEq(run(indentList, "- a\n- b\n# h|"), null, "a heading");
  assertEq(run(indentList, "- a\n- b\n---|"), null, "a rule");
});

check("the blank line inside a loose list swallows the key", () => {
  const r = run(indentList, "- a\n|\n- b");
  assertEq(r, { doc: "- a\n\n- b", from: 4, to: 4 }, "claimed but inert");
});

check("blank space around a list is document space", () => {
  assertEq(run(indentList, "- a\n|"), null, "a trailing blank");
  assertEq(run(indentList, "- a\n|\n\n- b"), null, "the first blank of a two-blank gap");
  assertEq(run(indentList, "|\n- a"), null, "above the list");
});

check("a blockquoted list is beyond this engine's reach", () => {
  // main.ts's `inListItem` keeps Tab claimed here instead.
  assertEq(run(indentList, "> - a\n> - |b"), null, "the adapter's tree check owns this case");
});

// --- nesting -------------------------------------------------------------------------

check("Tab nests an item under the one above it", () => {
  assertEq(run(indentList, "- a\n- |b")?.doc, "- a\n  - b", "two spaces, the bullet's content column");
});

check("the new indent is the previous sibling's content column, not a fixed step", () => {
  // A fixed two-space step under `1. ` wouldn't parse as nesting.
  assertEq(run(indentList, "1. a\n2. |b")?.doc, "1. a\n   1. b", "an ordered parent");
  assertEq(run(indentList, "10. a\n11. |b")?.doc, "10. a\n    1. b", "a two-digit one");
});

check("an item already nested goes one level deeper, not back to the top", () => {
  assertEq(run(indentList, "- a\n  - b\n  - |c")?.doc, "- a\n  - b\n    - c", "c under b");
});

check("nesting carries the item's own children with it", () => {
  const r = run(indentList, "- a\n- |b\n  - c\n    continuation\n- d");
  assertEq(r?.doc, "- a\n  - b\n    - c\n      continuation\n- d", "the subtree moves as one");
});

check("a blank line inside the subtree does not end it", () => {
  const r = run(indentList, "- a\n- |b\n\n  - c\n- d");
  assertEq(r?.doc, "- a\n  - b\n\n    - c\n- d", "the loose item's child came along");
});

check("the sibling search stops at the list it is in", () => {
  assertEq(run(indentList, "- a\n\nA paragraph.\n\n- |b")?.doc, "- a\n\nA paragraph.\n\n- b", "no reach");
});

// --- lifting out ---------------------------------------------------------------------

check("⇧Tab lifts an item out to its parent's column", () => {
  assertEq(run(outdentList, "- a\n  - |b")?.doc, "- a\n- b", "back to the top level");
  assertEq(run(outdentList, "- a\n  - b\n    - |c")?.doc, "- a\n  - b\n  - c", "one level, not all of them");
});

check("lifting out carries the children too", () => {
  const r = run(outdentList, "- a\n  - |b\n    - c\n- d");
  assertEq(r?.doc, "- a\n- b\n  - c\n- d", "c is still b's child");
});

check("an item's continuation lines do not hide its parent", () => {
  const r = run(outdentList, "- a\n  more about a\n  - |b");
  assertEq(r?.doc, "- a\n  more about a\n- b", "a is still the parent");
});

check("indent then outdent is the document you started with", () => {
  const start = "- a\n- b\n  - c\n- d";
  const there = indentList(start, 6, 6);
  assert(there !== null, "indent applied");
  const mid = applyChanges(start, there?.changes ?? []);
  const back = outdentList(mid, there?.selFrom ?? 0, there?.selTo ?? 0);
  assert(back !== null, "outdent applied");
  assertEq(applyChanges(mid, back?.changes ?? []), start, "round trip");
});

// --- ordered lists -------------------------------------------------------------------

check("nesting an ordered item renumbers what it left and what it joined", () => {
  assertEq(run(indentList, "1. a\n2. |b\n3. c")?.doc, "1. a\n   1. b\n2. c", "both runs");
});

check("an item joining an existing nested run takes the next number", () => {
  assertEq(run(indentList, "1. a\n   1. x\n2. |b")?.doc, "1. a\n   1. x\n   2. b", "x then b");
});

check("a list that opens at 5 goes on opening at 5", () => {
  assertEq(run(indentList, "5. a\n6. |b")?.doc, "5. a\n   1. b", "a keeps its 5");
});

check("lifting an ordered item out renumbers the run it lands in", () => {
  assertEq(run(outdentList, "1. a\n   1. x\n   2. |y\n2. b")?.doc, "1. a\n   1. x\n2. y\n3. b", "y joins the top run");
});

check("the run left behind restarts when its first item moved away", () => {
  assertEq(run(outdentList, "1. a\n   1. |x\n   2. y")?.doc, "1. a\n2. x\n   1. y", "y restarts at 1");
});

check("the lazy 1. 1. 1. style is left as the author wrote it", () => {
  assertEq(run(indentList, "1. a\n1. b\n1. |c")?.doc, "1. a\n1. b\n   1. c", "a and b untouched");
});

check("a change of ordered delimiter is a new list, and numbering stops at it", () => {
  // `1.` then `1)` is two lists in CommonMark.
  const r = run(indentList, "1. a\n1) x\n2) y\n3) |z");
  assertEq(r?.doc, "1. a\n1) x\n2) y\n   1) z", "the `)` list keeps its own count");
});

check("a second list's deliberate start number survives the list above it", () => {
  // `5)` heads its own list, so it keeps the author's 5.
  assertEq(run(indentList, "1. a\n5) x\n6) |y")?.doc, "1. a\n5) x\n   1) y", "x keeps its 5");
});

check("a bullet run is not renumbered into an ordered one", () => {
  assertEq(run(indentList, "- a\n- b\n- |c")?.doc, "- a\n- b\n  - c", "bullets stay bullets");
});

check("the bullet character is the author's", () => {
  assertEq(run(indentList, "- a\n  - x\n* |b")?.doc, "- a\n  - x\n  * b", "b is still a `*`");
});

// --- selections ----------------------------------------------------------------------

check("a selection over several items moves them as a block", () => {
  const r = run(indentList, "- a\n- [b\n- c]\n- d");
  assertEq(r?.doc, "- a\n  - b\n  - c\n- d", "both shifted by the head's step");
});

check("the selection survives the edit, so Tab Tab nests twice", () => {
  const start = "- a\n  - x\n- b";
  const once = indentList(start, 12, 12);
  const mid = applyChanges(start, once?.changes ?? []);
  assertEq(mid, "- a\n  - x\n  - b", "one step");
  const twice = indentList(mid, once?.selFrom ?? 0, once?.selTo ?? 0);
  assertEq(applyChanges(mid, twice?.changes ?? []), "- a\n  - x\n    - b", "two steps");
});

check("a caret keeps its distance from the content it sits in", () => {
  const r = run(indentList, "- a\n- b|");
  assertEq(r, { doc: "- a\n  - b", from: 9, to: 9 }, "still after the b");
});

check("a caret inside the indentation rides to the front of the text", () => {
  const r = run(outdentList, "- a\n | - b");
  assertEq(r?.doc, "- a\n- b", "outdented");
  assertEq(r?.from, 4, "at the line's new start");
});

check("a selection ending at a line start stops short of that line", () => {
  const r = run(indentList, "- a\n- [b\n]- c");
  assertEq(r?.doc, "- a\n  - b\n- c", "only b moved");
});

// --- whitespace ----------------------------------------------------------------------

check("a tab of indentation is measured at four columns, and rewritten as spaces", () => {
  const r = run(outdentList, "- a\n\t- |b");
  assertEq(r?.doc, "- a\n- b", "a four-column tab, lifted to the top level");
});

check("an item with no content still offers a column to nest into", () => {
  assertEq(run(indentList, "-\n- |b")?.doc, "-\n  - b", "one past the bare marker");
});

check("five spaces after a marker is code indentation, not a deeper content column", () => {
  assertEq(run(indentList, "-     a\n- |b")?.doc, "-     a\n  - b", "two, not six");
});

console.log(`\n${passed} checks passed`);
