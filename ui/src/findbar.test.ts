// Tests for the pure find-in-note logic (findbar.ts) — the ⌘F engine.
// Run directly:  node --experimental-strip-types src/findbar.test.ts
// Hand-rolled asserts, the panes.test.ts / format.test.ts idiom.

import { activeAfter, countLabel, findMatches, locate, stepActive } from "./findbar.ts";

let checks = 0;

function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
  checks++;
}

// --- findMatches: literal, case-insensitive, non-overlapping -----------------------

assertEq(
  findMatches("the cat sat on the mat", "the"),
  [
    { from: 0, to: 3 },
    { from: 15, to: 18 },
  ],
  "finds every occurrence with offsets",
);

assertEq(
  findMatches("The THEory of the", "the"),
  [
    { from: 0, to: 3 },
    { from: 4, to: 7 },
    { from: 14, to: 17 },
  ],
  "case-insensitive, and matches inside words",
);

assertEq(findMatches("aaa", "aa"), [{ from: 0, to: 2 }], "matches never overlap");

assertEq(
  findMatches("costs $5. ($5!)", "$5"),
  [
    { from: 6, to: 8 },
    { from: 11, to: 13 },
  ],
  "the query is literal — regex metacharacters find themselves",
);

assertEq(findMatches("c++ and c++", "c++"), [{ from: 0, to: 3 }, { from: 8, to: 11 }], "c++ too");

assertEq(findMatches("anything", ""), [], "an empty query matches nothing");
assertEq(findMatches("", "x"), [], "an empty document matches nothing");

assertEq(
  findMatches("xxxxxxxxxx", "x", 4).length,
  4,
  "the cap bounds pathological single-char queries",
);

// --- stepActive: wrap-around navigation --------------------------------------------

assertEq(stepActive(5, 2, 1), 3, "next steps forward");
assertEq(stepActive(5, 4, 1), 0, "next wraps off the end");
assertEq(stepActive(5, 0, -1), 4, "previous wraps off the start");
assertEq(stepActive(1, 0, 1), 0, "a single match steps to itself");
assertEq(stepActive(0, -1, 1), -1, "no matches → no active index");

// --- activeAfter: re-anchoring after the match set changes -------------------------

const MS = [
  { from: 5, to: 8 },
  { from: 20, to: 23 },
  { from: 40, to: 43 },
];
assertEq(activeAfter(MS, 0), 0, "a position before every match anchors on the first");
assertEq(activeAfter(MS, 20), 1, "a position at a match's start anchors there");
assertEq(activeAfter(MS, 21), 2, "a position inside a match moves to the next one");
assertEq(activeAfter(MS, 99), 2, "a position past every match falls back to the last");
assertEq(activeAfter([], 10), -1, "no matches → -1");

// --- countLabel --------------------------------------------------------------------

assertEq(countLabel(17, 2), "3 / 17", "active is displayed 1-based");
assertEq(countLabel(0, -1), "0 / 0", "no matches reads 0 / 0");
assertEq(countLabel(1000, 0, true), "1 / 1000+", "a capped scan says the count is a floor");

// --- locate: flat offset → (segment, offset) for DOM Range endpoints ---------------

// Three text nodes: "hello " (6) + "bold" (4) + " world" (6) — flat text "hello bold world".
const SEGS = [6, 4, 6];

assertEq(locate(SEGS, 0, "start"), { seg: 0, off: 0 }, "offset 0 is the first node's start");
assertEq(locate(SEGS, 7, "start"), { seg: 1, off: 1 }, "an interior offset lands mid-node");
assertEq(locate(SEGS, 9, "end"), { seg: 1, off: 3 }, "an end offset inside a node");

// A match spanning nodes: "o bold w" = flat [4, 12).
assertEq(locate(SEGS, 4, "start"), { seg: 0, off: 4 }, "span start in the first node");
assertEq(locate(SEGS, 12, "end"), { seg: 2, off: 2 }, "span end in the last node");

// Boundary offsets: 6 is both node 0's end and node 1's start.
assertEq(locate(SEGS, 6, "start"), { seg: 1, off: 0 }, "a start at a boundary opens the next node");
assertEq(locate(SEGS, 6, "end"), { seg: 0, off: 6 }, "an end at a boundary closes the previous node");

assertEq(locate(SEGS, 99, "end"), { seg: 2, off: 6 }, "past-the-end clamps to the last node's end");
assertEq(locate([], 3, "start"), { seg: 0, off: 0 }, "no segments degrades to zero, not a throw");

console.log(`findbar.test: ${checks} checks passed`);
