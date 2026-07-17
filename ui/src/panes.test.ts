// The pane-sizing rules (panes.ts), pinned. Pure math only — no DOM — so node runs it
// straight off the source via its native type-stripping: `npm test`.
//
// Deliberately dependency-free (hence the hand-rolled `assert` below rather than
// node:assert, which would drag @types/node into a frontend that needs no Node types).
// The drag/persistence controller isn't covered here — it's DOM-bound; what's worth
// pinning is the arithmetic every gutter drag and window resize routes through.
import { BOUNDS, CENTER_MIN, GUTTER, ceilingFor, fit, type PaneWidths } from "./panes.ts";

const BOTH = { tree: true, side: true };
let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: number | string, expected: number | string, msg: string): void {
  assert(actual === expected, `${msg} — expected ${expected}, got ${actual}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

const centerOf = (w: PaneWidths, avail: number): number => avail - w.tree - w.side - 2 * GUTTER;

// --- each pane honors its own bounds ------------------------------------------------

check("a want below the min is lifted to the min", () => {
  const w = fit({ tree: 10, side: 10 }, 3000, BOTH);
  equal(w.tree, BOUNDS.tree.min, "tree");
  equal(w.side, BOUNDS.side.min, "side");
});

check("a want above the max is capped at the max (room to spare)", () => {
  const w = fit({ tree: 9999, side: 9999 }, 5000, BOTH);
  equal(w.tree, BOUNDS.tree.max, "tree");
  equal(w.side, BOUNDS.side.max, "side");
});

check("a want inside the bounds is returned untouched", () => {
  const w = fit({ tree: 300, side: 420 }, 3000, BOTH);
  equal(w.tree, 300, "tree");
  equal(w.side, 420, "side");
});

// --- the center's minimum is what we protect ----------------------------------------

check("the side pane yields first when the window can't hold both", () => {
  // 240 + 380 + 12 + 360 = 992 needed; give it 900 → 92px must come off the side.
  const w = fit({ tree: 240, side: 380 }, 900, BOTH);
  equal(w.tree, 240, "tree is untouched while the side still has slack");
  equal(w.side, 288, "side");
  equal(centerOf(w, 900), CENTER_MIN, "center holds its floor");
});

check("the tree yields only once the side is at its min", () => {
  const w = fit({ tree: 240, side: 380 }, 800, BOTH);
  equal(w.side, BOUNDS.side.min, "side bottoms out first");
  equal(w.tree, 800 - BOUNDS.side.min - 2 * GUTTER - CENTER_MIN, "tree gives up the rest");
  equal(centerOf(w, 800), CENTER_MIN, "center still holds its floor");
});

check("both mins hold even when the window is too small for the center", () => {
  // Nothing left to give: the mins win and the center takes the squeeze. The
  // stylesheet's breakpoints hide the panes long before this in practice.
  const w = fit({ tree: 240, side: 380 }, 400, BOTH);
  equal(w.tree, BOUNDS.tree.min, "tree");
  equal(w.side, BOUNDS.side.min, "side");
});

// --- hidden panes don't compete for room --------------------------------------------

check("a hidden side pane reserves nothing, so the tree keeps its width", () => {
  // At <=1040px the stylesheet drops the side pane. Its 380px must not squeeze the tree.
  const w = fit({ tree: 300, side: 380 }, 900, { tree: true, side: false });
  equal(w.tree, 300, "tree keeps its width — the side pane isn't on screen");
  equal(w.side, 380, "the hidden pane's stored width survives for when it returns");
});

check("a hidden tree reserves nothing either", () => {
  const w = fit({ tree: 300, side: 380 }, 800, { tree: false, side: true });
  equal(w.side, 380, "side");
  equal(w.tree, 300, "tree");
});

// --- ceilingFor: the live wall a drag stops at --------------------------------------

check("a drag stops at the pane's own max when the window is wide", () => {
  equal(ceilingFor("tree", 380, 5000, BOTH), BOUNDS.tree.max, "tree");
  equal(ceilingFor("side", 240, 5000, BOTH), BOUNDS.side.max, "side");
});

check("a drag stops at whatever the center can spare when it's tight", () => {
  // Tight enough that the spare room bites before the pane's own max does.
  // 1100 - 360 center - 12 gutters - 380 side = 348 for the tree (< its 420 max).
  equal(ceilingFor("tree", 380, 1100, BOTH), 348, "tree");
  // ...and symmetrically: 1100 - 360 - 12 - 240 tree = 488 (< the side's 560 max).
  equal(ceilingFor("side", 240, 1100, BOTH), 488, "side");
});

check("the pane's own max still wins when the window is merely roomy", () => {
  // 1200 could spare the tree 448, but no pane may outgrow its own max.
  equal(ceilingFor("tree", 380, 1200, BOTH), BOUNDS.tree.max, "tree");
});

check("dragging one pane never moves the other: the ceiling counts the other as fixed", () => {
  equal(ceilingFor("tree", 380, 992, BOTH), 240, "at exactly 992 the tree can't grow past its default");
});

check("the ceiling ignores a hidden neighbor", () => {
  // Side hidden: the tree may take everything but the center + its one gutter.
  equal(ceilingFor("tree", 380, 1000, { tree: true, side: false }), BOUNDS.tree.max, "wide");
  equal(ceilingFor("tree", 380, 700, { tree: true, side: false }), 700 - GUTTER - CENTER_MIN, "tight");
});

console.log(`\n${passed} passed`);
