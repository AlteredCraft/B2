// The ⌘-hold sheet (cmdhold.ts), pinned. Pure — no DOM, no timers — so node runs it
// straight off the source: `npm test`.
//
// Two things here can go wrong, and they fail in opposite directions. The machine can
// **stick**: a sheet that opens and never closes because the release arrived in a shape
// nobody handled is a pane of chrome over the app with no way out, and the shapes it
// arrives in are exactly the ones a keyboard test can enumerate and a human tester can't
// (an auto-repeat, a stale timer, ⌘⇥ eating the keyup). Or it can **flash**: a sheet that
// opens during ⌘S is worse than no sheet at all. So the whole truth table is written out,
// including the pairs that look unreachable — the header says which of those are in fact
// every ⌘-chord's tail.
//
// The other half is the projection. It is derived from the same table Settings prints, so
// what is pinned is the *derivation*: only ⌘ chords, only real rows, and nothing invented.
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

/** The phases, so a test can walk all three without spelling them out each time. */
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
  // ⌘S: ⌘ down, S down before the clock finishes. The flash this prevents is the whole
  // reason the sheet is on a timer and not on the modifier itself.
  assertEq(holdStep("armed", { kind: "other" }), { phase: "idle", timer: "clear" }, "mid-hold");
  // And with the sheet already up, the chord it just explained takes it down — which is
  // the sheet doing its job, not losing it.
  assertEq(holdStep("open", { kind: "other" }), { phase: "idle", timer: "clear" }, "with the sheet up");
});

check("auto-repeat is not a second press", () => {
  // macOS repeats a held key. If a repeat restarted the clock the sheet would never open:
  // every tick would push the deadline out past the hold that earned it.
  assertEq(holdStep("armed", { kind: "hold", repeat: true }), { phase: "armed", timer: "keep" }, "still armed");
  assertEq(holdStep("open", { kind: "hold", repeat: true }), { phase: "open", timer: "keep" }, "still open");
  assertEq(holdStep("idle", { kind: "hold", repeat: true }), { phase: "idle", timer: "keep" }, "and never starts one");
});

check("a stale clock finds no one waiting", () => {
  // The timer is cleared on every cancel, but a `setTimeout` already in the task queue
  // fires anyway. Landing on `idle` must not conjure a sheet nobody is holding ⌘ for.
  assertEq(holdStep("idle", { kind: "elapsed" }), { phase: "idle", timer: "keep" }, "from rest");
  assertEq(holdStep("open", { kind: "elapsed" }), { phase: "open", timer: "keep" }, "and never re-opens");
});

check("a release with nothing in flight is a no-op, not a repaint", () => {
  // Every ⌘-chord ends here: the chord's own key cancelled the hold while ⌘ was still
  // down, so the eventual ⌘ keyup arrives at `idle`. It must ask for no timer work and no
  // phase change — this is the most frequent event the machine sees.
  assertEq(holdStep("idle", { kind: "release" }), { phase: "idle", timer: "keep" }, "the tail of ⌘S");
  assertEq(holdStep("idle", { kind: "other" }), { phase: "idle", timer: "keep" }, "and so is plain typing");
});

check("nothing but the clock opens the sheet, and every phase has a way out", () => {
  // The two properties that keep it from sticking, asserted over the whole table rather
  // than event by event — a fifth event added without a rule would fail this.
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
    // `release` is the one the DOM may have to synthesize (blur, a hidden window), so it
    // is the one that must always land at rest.
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
  // The point of deriving rather than hand-writing: a chord shown here that Settings
  // doesn't show is a chord with two spellings, which is the drift shortcuts.ts exists to
  // prevent. Prose and chords both, since either half could be retyped by hand.
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
  // A row whose chords are all ⌘ keeps them all — ⌘G and ⇧⌘G are both a held ⌘.
  const match = rows.find((s) => s.action === "Next / previous match");
  assertEq(match?.keys.map((k) => k.text), ["⌘G", "⇧⌘G"], "shift is still a held ⌘");
  // And the arrow families, ⏎, Tab and Esc are not answers to "what does ⌘ do".
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
  // The one regression this suite *can* catch about the sheet's size, and the one that
  // actually happens.
  //
  // The layout is verified by measurement, not from here: the card was rendered against
  // the real stylesheet and every row confirmed inside the viewport from 1600×1000 down to
  // 720×480, the smallest window B2 will open (tauri.conf.json), where it comes out 397px
  // in a 464px budget. node has no browser, and putting one in `npm test` to re-measure
  // that would cost the suite its speed and its independence from the DOM for a number
  // that only moves when the *content* does.
  //
  // So this pins the content. The sheet cannot scroll — the layer is `pointer-events: none`
  // so the app underneath keeps the mouse — which means a row past the bottom edge is a row
  // nobody can reach, and the way that happens is not someone editing the CSS: it is a
  // dozen new ⌘ chords arriving one at a time, each obviously fine on its own. The budget
  // is the shipped keyboard's, with room for a few more; crossing it means re-measuring the
  // floor before shipping, and the numbers above say against what.
  const groups = cmdShortcuts();
  const rows = groups.flatMap((g) => g.items);
  assert(rows.length <= 24, `the ⌘ sheet is ${rows.length} rows — measured to fit at 17, budgeted to 24`);
  assert(groups.length <= 9, `the ⌘ sheet is ${groups.length} groups — each heading costs a row's height`);
  // Prose is the other half of the height: a row that wraps to four lines is four rows of
  // card. The longest shipped description is ~60 characters; at the floor's narrowest
  // column that is two lines.
  for (const r of rows) {
    assert(
      r.action.length <= 90,
      `"${r.action}" is ${r.action.length} characters — long enough to wrap the card past its budget`,
    );
  }
});

check("a chip still knows the command behind it", () => {
  // The filter rebuilds the rows, and a shallow-copied chip that lost its `id` would be a
  // sheet that can't say what ⌘F is — the projection carries the reference's chips
  // through untouched rather than re-deriving them.
  const find = cmdShortcuts()
    .flatMap((g) => g.items)
    .find((s) => s.action === "Find in this note");
  assertEq(find?.keys[0]?.id, "find.open", "⌘F is find.open");
});

console.log(`cmdhold: ${passed} checks passed`);
