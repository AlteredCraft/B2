// The ⌘-hold sheet (cmdhold.ts). The whole truth table is written out, since the machine
// can fail two ways: stick (a release shape nobody handled) or flash (opening during ⌘S).
// Then the projection: only ⌘ chords, only real rows, nothing invented.
import { cmdShortcuts, holdStep, type HoldPhase, HOLD_MS } from "./cmdhold.ts";
import { shortcuts } from "./shortcuts.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq<T>(got: T, want: T, msg: string): void {
  const [a, b] = [JSON.stringify(got), JSON.stringify(want)];
  if (a !== b) throw new Error(`assertion failed: ${msg}\n  got  ${a}\n  want ${b}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

const PHASES: HoldPhase[] = ["idle", "armed", "open"];

check("a held ⌘ arms the clock, and the clock opens the sheet", () => {
  assertEq(holdStep("idle", { kind: "hold", repeat: false }), { phase: "armed", timer: "start" }, "⌘ down");
  assertEq(holdStep("armed", { kind: "elapsed" }), { phase: "open", timer: "keep" }, "the hold outlasts a chord");
});

check("letting ⌘ go closes the sheet, from either side of the clock", () => {
  assertEq(holdStep("armed", { kind: "release" }), { phase: "idle", timer: "clear" }, "a tap");
  assertEq(holdStep("open", { kind: "release" }), { phase: "idle", timer: "clear" }, "a hold");
});

check("a chord cancels the hold rather than showing it a sheet", () => {
  assertEq(holdStep("armed", { kind: "other" }), { phase: "idle", timer: "clear" }, "mid-hold");
  assertEq(holdStep("open", { kind: "other" }), { phase: "idle", timer: "clear" }, "with the sheet up");
});

check("auto-repeat is not a second press", () => {
  assertEq(holdStep("armed", { kind: "hold", repeat: true }), { phase: "armed", timer: "keep" }, "still armed");
  assertEq(holdStep("open", { kind: "hold", repeat: true }), { phase: "open", timer: "keep" }, "still open");
  assertEq(holdStep("idle", { kind: "hold", repeat: true }), { phase: "idle", timer: "keep" }, "and never starts one");
});

check("a stale clock finds no one waiting", () => {
  // A `setTimeout` already queued fires even after it is cleared.
  assertEq(holdStep("idle", { kind: "elapsed" }), { phase: "idle", timer: "keep" }, "from rest");
  assertEq(holdStep("open", { kind: "elapsed" }), { phase: "open", timer: "keep" }, "and never re-opens");
});

check("a release with nothing in flight is a no-op, not a repaint", () => {
  // Every ⌘-chord ends here: the chord's key already cancelled the hold.
  assertEq(holdStep("idle", { kind: "release" }), { phase: "idle", timer: "keep" }, "the tail of ⌘S");
  assertEq(holdStep("idle", { kind: "other" }), { phase: "idle", timer: "keep" }, "and so is plain typing");
});

check("nothing but the clock opens the sheet, and every phase has a way out", () => {
  const events = [
    { kind: "hold", repeat: false },
    { kind: "hold", repeat: true },
    { kind: "other" },
    { kind: "release" },
    { kind: "elapsed" },
  ] as const;
  for (const phase of PHASES) {
    for (const e of events) {
      const step = holdStep(phase, e);
      assert(
        step.phase !== "open" || phase === "open" || e.kind === "elapsed",
        `${phase} + ${e.kind} opened the sheet without a hold`,
      );
      assert(PHASES.includes(step.phase), `${phase} + ${e.kind} left the machine`);
    }
    // The DOM may synthesize `release` (blur, hidden window), so it must always land at rest.
    assertEq(holdStep(phase, { kind: "release" }).phase, "idle", `${phase} releases to idle`);
  }
});

check("the hold outlasts a chord but not a pause", () => {
  assert(HOLD_MS >= 400, "shorter than this and ⌘S flashes the sheet");
  assert(HOLD_MS <= 1200, "longer and a hand waiting on it concludes nothing is coming");
});

// --- the projection -------------------------------------------------------------------

check("the sheet is the ⌘ half of the keyboard reference, and only that", () => {
  const groups = cmdShortcuts();
  assert(groups.length > 0, "there is something to show");
  for (const g of groups) {
    assert(g.items.length > 0, `an empty group survived: ${g.title}`);
    for (const item of g.items) {
      assert(item.keys.length > 0, `a row with no chords survived: ${item.action}`);
      for (const k of item.keys) {
        assert(k.text.includes("⌘"), `${k.text} (${item.action}) is not a ⌘ chord`);
      }
    }
  }
});

check("it invents nothing — every row is a row of the reference", () => {
  const full = new Map(
    shortcuts().flatMap((g) => g.items.map((s) => [s.action, s.keys.map((k) => k.text)] as const)),
  );
  for (const g of cmdShortcuts()) {
    for (const item of g.items) {
      const there = full.get(item.action);
      assert(there !== undefined, `"${item.action}" is in the ⌘ sheet and not in the reference`);
      for (const k of item.keys) {
        assert(there?.includes(k.text) === true, `${k.text} is not what the reference prints for "${item.action}"`);
      }
    }
  }
});

check("it keeps the rows worth keeping and drops the rest", () => {
  const rows = cmdShortcuts().flatMap((g) => g.items);
  const find = rows.find((s) => s.action === "Find in this note");
  assertEq(find?.keys.map((k) => k.text), ["⌘F"], "⌘F is the case this exists for");
  const match = rows.find((s) => s.action === "Next / previous match");
  assertEq(match?.keys.map((k) => k.text), ["⌘G", "⇧⌘G"], "shift is still a held ⌘");
  assert(
    !rows.some((s) => s.keys.some((k) => k.text === "↑" || k.text === "⏎" || k.text === "Esc")),
    "a chord with no ⌘ in it got through",
  );
  assert(
    !rows.some((s) => s.action === "Move between rows"),
    "and a row left with no chords was dropped, not shown empty",
  );
});

check("the sheet stays inside the size it was measured to fit", () => {
  // The layout was measured by hand: at 720×480, the smallest window (tauri.conf.json), the
  // card is 397px in a 464px budget. The sheet can't scroll (`pointer-events: none`), so
  // this pins the content that drives the height. Crossing a budget means re-measuring.
  const groups = cmdShortcuts();
  const rows = groups.flatMap((g) => g.items);
  assert(rows.length <= 24, `the ⌘ sheet is ${rows.length} rows — measured to fit at 17, budgeted to 24`);
  assert(groups.length <= 9, `the ⌘ sheet is ${groups.length} groups — each heading costs a row's height`);
  // Prose wraps: ~60 characters is two lines at the floor's narrowest column.
  for (const r of rows) {
    assert(
      r.action.length <= 90,
      `"${r.action}" is ${r.action.length} characters — long enough to wrap the card past its budget`,
    );
  }
});

check("a chip still knows the command behind it", () => {
  // The filter rebuilds rows; the chips must pass through untouched.
  const find = cmdShortcuts()
    .flatMap((g) => g.items)
    .find((s) => s.action === "Find in this note");
  assertEq(find?.keys[0]?.id, "find.open", "⌘F is find.open");
});

console.log(`cmdhold: ${passed} checks passed`);
