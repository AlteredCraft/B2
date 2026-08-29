// The zoom ladder (zoom.ts), pinned. Pure arithmetic over a fixed list — no DOM, no
// host — so node runs it straight off the source via its native type-stripping:
// `npm test`.
//
// Dependency-free by the same rule as panes.test.ts: a hand-rolled `assert` rather than
// node:assert, which would drag @types/node into a frontend that needs no Node types.
// What's worth pinning is the step algebra every ⌘= / ⌘- routes through, and the
// adoption rule that stands between a hand-edited localStorage value and a window
// nobody can read.
import { type Columns, DEFAULT_ZOOM, STEPS, adoptZoom, hiddenNotice, stepZoom } from "./zoom.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: number, expected: number, msg: string): void {
  assert(actual === expected, `${msg} — expected ${expected}, got ${actual}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- the ladder itself ---------------------------------------------------------------

check("the ladder is sorted, has no duplicates, and contains 100%", () => {
  for (let i = 1; i < STEPS.length; i++) {
    assert(STEPS[i] > STEPS[i - 1], `step ${i} (${STEPS[i]}) must exceed ${STEPS[i - 1]}`);
  }
  assert(STEPS.includes(DEFAULT_ZOOM), "the default must be a rung, or ⌘0 lands off-ladder");
});

// --- stepping ------------------------------------------------------------------------

check("a step up moves to the next rung", () => {
  equal(stepZoom(1, 1), STEPS[STEPS.indexOf(1) + 1], "up from 100%");
});

check("a step down moves to the previous rung", () => {
  equal(stepZoom(1, -1), STEPS[STEPS.indexOf(1) - 1], "down from 100%");
});

check("the top rung is a wall, not a wrap", () => {
  const top = STEPS[STEPS.length - 1];
  equal(stepZoom(top, 1), top, "up from the top");
});

check("the bottom rung is a wall, not a wrap", () => {
  const bottom = STEPS[0];
  equal(stepZoom(bottom, -1), bottom, "down from the bottom");
});

check("a value between rungs steps to the rung on that side, never past it", () => {
  // 1.05 sits between 1 and 1.1: up is 1.1, down is 1 — both one rung away, so a value
  // that drifted off the ladder is back on it after a single keypress in either
  // direction. Stepping past the nearer rung would make ⌘- feel like it skipped.
  const between = (STEPS[STEPS.indexOf(1)] + STEPS[STEPS.indexOf(1) + 1]) / 2;
  equal(stepZoom(between, 1), STEPS[STEPS.indexOf(1) + 1], "up from between");
  equal(stepZoom(between, -1), 1, "down from between");
});

check("a value below the ladder climbs onto its bottom rung", () => {
  equal(stepZoom(0.1, 1), STEPS[0], "up from far below");
  equal(stepZoom(0.1, -1), STEPS[0], "down from far below");
});

check("a value above the ladder settles onto its top rung", () => {
  const top = STEPS[STEPS.length - 1];
  equal(stepZoom(99, -1), top, "down from far above");
  equal(stepZoom(99, 1), top, "up from far above");
});

// --- reading a stored value ------------------------------------------------------------

check("a rung is adopted unchanged", () => {
  for (const s of STEPS) equal(adoptZoom(s), s, `rung ${s}`);
});

check("a value between rungs snaps to the nearest one", () => {
  equal(adoptZoom(1.04), 1, "just above 100%");
  equal(adoptZoom(1.09), STEPS[STEPS.indexOf(1) + 1], "just below the next rung");
});

check("a value outside the ladder is clamped to its ends", () => {
  equal(adoptZoom(0.01), STEPS[0], "far below");
  equal(adoptZoom(50), STEPS[STEPS.length - 1], "far above");
});

check("anything that isn't a finite number is the default", () => {
  for (const raw of [null, undefined, "1.5", {}, [], Number.NaN, Infinity, -Infinity, 0]) {
    equal(adoptZoom(raw), DEFAULT_ZOOM, `${String(raw)}`);
  }
});

check("a negative value is the default, not a mirrored size", () => {
  // A negative page zoom is not a smaller window, it is an unrenderable one — so this
  // is a refusal, not a clamp onto the bottom rung.
  equal(adoptZoom(-1), DEFAULT_ZOOM, "negative");
});

// --- announcing a column the step cost ---------------------------------------------------

function notice(before: Columns, after: Columns): string {
  return hiddenNotice(before, after) ?? "";
}
const BOTH: Columns = { tree: true, side: true };

check("a step that hides nothing says nothing", () => {
  equal(notice(BOTH, BOTH).length, 0, "nothing lost");
  equal(notice({ tree: true, side: false }, { tree: true, side: false }).length, 0, "already gone");
});

check("losing one column names that column", () => {
  assert(
    notice(BOTH, { tree: true, side: false }).includes("Discovery"),
    "the side column is discovery",
  );
  assert(
    notice(BOTH, { tree: false, side: true }).includes("file tree"),
    "the left column is the file tree",
  );
});

check("losing both is one sentence, not two", () => {
  const msg = notice(BOTH, { tree: false, side: false });
  assert(msg.includes("file tree") && msg.includes("discovery"), `names both: ${msg}`);
  equal(msg.split(".").filter((s) => s.trim().length > 0).length, 1, "sentences");
});

check("a column coming back is not announced", () => {
  // Zooming out reveals; a revealed column is its own notice, and saying so would make
  // every ⌘- talk back.
  equal(notice({ tree: false, side: false }, BOTH).length, 0, "both back");
  equal(notice({ tree: true, side: false }, BOTH).length, 0, "one back");
});

check("a loss and a gain in one step reports only the loss", () => {
  // Not reachable by zooming (the breakpoints nest), but the rule is "announce losses",
  // and a rule that quietly depends on the breakpoints nesting is one that breaks when
  // they stop.
  const msg = notice({ tree: true, side: false }, { tree: false, side: true });
  assert(msg.includes("file tree"), `names the loss: ${msg}`);
  assert(!msg.includes("iscovery"), `and not the gain: ${msg}`);
});

console.log(`\n${passed} passed`);
