// Tests for the nested-list engine (list.ts) — the Tab / ⇧Tab commands.
// Run directly:  node --experimental-strip-types src/list.test.ts
// Hand-rolled asserts, the format.test.ts / panes.test.ts idiom.
//
// The assertions are on the **Markdown that comes out**, not on the change list: what
// matters is the note on disk, and a change list is one of several ways to spell the same
// document. `applyChanges` is list.ts's own mirror of what CodeMirror does with them.
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

/** The document with `|` marking the caret, or `[`…`]` marking a selection — the shape
 *  a reader can check against the prose without counting offsets. */
function at(marked: string): { doc: string; from: number; to: number } {
  if (marked.includes("|")) {
    const from = marked.indexOf("|");
    return { doc: marked.replace("|", ""), from, to: from };
  }
  const from = marked.indexOf("[");
  const to = marked.indexOf("]") - 1;
  return { doc: marked.replace("[", "").replace("]", ""), from, to };
}

/** Run a command over a marked-up document; returns the Markdown, and where the
 *  selection landed in it. */
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
  // The null is what leaves Tab meaning "next control" everywhere else — the whole
  // reason this isn't `indentWithTab`.
  assertEq(indentList("A plain paragraph.", 3, 3), null, "a paragraph");
  assertEq(indentList("# A heading\n\ntext", 4, 4), null, "a heading");
  assertEq(outdentList("A plain paragraph.", 3, 3), null, "⇧Tab, the same");
});

check("a thematic break is not a one-item list", () => {
  // `* * *` and `---` parse as bullets under a naive reading, and nesting one would turn
  // a rule into a list.
  assertEq(indentList("- a\n\n* * *", 6, 6), null, "* * *");
  assertEq(indentList("- a\n\n---", 6, 6), null, "---");
});

check("Tab in a list is claimed even when nothing can move", () => {
  // Claimed, not declined: a gesture that sometimes throws you out of the buffer is
  // worse than one that sometimes does nothing (list.ts's header).
  const first = run(indentList, "- |a\n- b");
  assertEq(first, { doc: "- a\n- b", from: 2, to: 2 }, "the first item has nothing to nest under");
  const top = run(outdentList, "- a\n- |b");
  assertEq(top, { doc: "- a\n- b", from: 6, to: 6 }, "a top-level item has nothing to leave");
});

// --- nesting -------------------------------------------------------------------------

check("Tab nests an item under the one above it", () => {
  assertEq(run(indentList, "- a\n- |b")?.doc, "- a\n  - b", "two spaces, the bullet's content column");
});

check("the new indent is the previous sibling's content column, not a fixed step", () => {
  // `1. ` is three columns wide, so a child of it starts at three — indent by a fixed
  // two and the nesting simply doesn't parse.
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
  // Without a block boundary this would reach back over the paragraph, adopt the first
  // list's `- a` as a sibling and nest under a list it isn't part of.
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
  // `parentOf` has to read past a paragraph: an item's own wrapped text sits between it
  // and its children, and stopping there would leave ⇧Tab inert.
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
  // Without this the nested list opens at "2." — which renders as "2." — and the run it
  // left counts 1, 3.
  assertEq(run(indentList, "1. a\n2. |b\n3. c")?.doc, "1. a\n   1. b\n2. c", "both runs");
});

check("an item joining an existing nested run takes the next number", () => {
  assertEq(run(indentList, "1. a\n   1. x\n2. |b")?.doc, "1. a\n   1. x\n   2. b", "x then b");
});

check("a list that opens at 5 goes on opening at 5", () => {
  // The start number is the author's; only a run that is newly *headed* restarts at 1.
  assertEq(run(indentList, "5. a\n6. |b")?.doc, "5. a\n   1. b", "a keeps its 5");
});

check("lifting an ordered item out renumbers the run it lands in", () => {
  assertEq(run(outdentList, "1. a\n   1. x\n   2. |y\n2. b")?.doc, "1. a\n   1. x\n2. y\n3. b", "y joins the top run");
});

check("the run left behind restarts when its first item moved away", () => {
  // `y` did not move, but it is the first item of that nested list now, and a list whose
  // first item says "2." renders as "2.".
  assertEq(run(outdentList, "1. a\n   1. |x\n   2. y")?.doc, "1. a\n2. x\n   1. y", "y restarts at 1");
});

check("the lazy 1. 1. 1. style is left as the author wrote it", () => {
  // It renders identically to 1, 2, 3 and is a deliberate way to write Markdown. Nothing
  // moved into or out of that run, so nothing about it is wrong.
  assertEq(run(indentList, "1. a\n1. b\n1. |c")?.doc, "1. a\n1. b\n   1. c", "a and b untouched");
});

check("a change of ordered delimiter is a new list, and numbering stops at it", () => {
  // `1.` then `1)` is two lists in CommonMark, not one list of two. Reading them as one
  // run had the renumbering walk straight over the boundary and rewrite `1) x` to `2) x`
  // — an edit to a list the author never touched, three lines from the caret.
  const r = run(indentList, "1. a\n1) x\n2) y\n3) |z");
  assertEq(r?.doc, "1. a\n1) x\n2) y\n   1) z", "the `)` list keeps its own count");
});

check("a second list's deliberate start number survives the list above it", () => {
  // The other half of the same boundary, and the one a run-level fix alone misses: `5)`
  // *is* the head of its list, so "was this item the head of its run?" has to ask about
  // the list too, or the 5 the author chose restarts at 1.
  assertEq(run(indentList, "1. a\n5) x\n6) |y")?.doc, "1. a\n5) x\n   1) y", "x keeps its 5");
});

check("a bullet run is not renumbered into an ordered one", () => {
  assertEq(run(indentList, "- a\n- b\n- |c")?.doc, "- a\n- b\n  - c", "bullets stay bullets");
});

check("the bullet character is the author's", () => {
  // `-` and `*` start *different* lists in CommonMark, so rewriting one to match its new
  // neighbours would restructure the note behind the author's back.
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
  // The line-wise reading every editor uses: `- c` is not in the selection, so it does
  // not move, and it is not what the step is measured from.
  const r = run(indentList, "- a\n- [b\n]- c");
  assertEq(r?.doc, "- a\n  - b\n- c", "only b moved");
});

// --- whitespace ----------------------------------------------------------------------

check("a tab of indentation is measured at four columns, and rewritten as spaces", () => {
  // CommonMark §2.2 is the measuring rule; spaces are what B2 writes, so the note stays
  // one thing rather than a mix.
  const r = run(outdentList, "- a\n\t- |b");
  assertEq(r?.doc, "- a\n- b", "a four-column tab, lifted to the top level");
});

check("an item with no content still offers a column to nest into", () => {
  assertEq(run(indentList, "-\n- |b")?.doc, "-\n  - b", "one past the bare marker");
});

check("five spaces after a marker is code indentation, not a deeper content column", () => {
  // CommonMark: a gap of five or more puts the content one column past the marker, and
  // the rest is an indented code block inside the item.
  assertEq(run(indentList, "-     a\n- |b")?.doc, "-     a\n  - b", "two, not six");
});

console.log(`\n${passed} checks passed`);
