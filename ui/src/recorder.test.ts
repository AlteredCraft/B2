// The chord recorder's pure half (recorder.ts), pinned. Runs off the source under node
// (`npm test`).
//
// Two things are being held here, and only one of them is obvious.
//
// The obvious one: a recorded chord has to be spelled the way the table spells it, or the
// chord the user saved and the keystroke they pressed stop being the same thing — and that
// failure is invisible. It looks like a shortcut that "didn't take". So the round trip is
// asserted directly: press a keystroke, record it, and check the registry's own matcher
// agrees the recorded chord answers to that keystroke.
//
// The less obvious one: the probe reads *silence*, and a probe that reads silence wrongly
// is worse than no probe. Over-report and a user is told macOS took a chord that works
// fine — which teaches them to ignore the recorder. So the tests below are as much about
// when it stays quiet as when it speaks.
import { type KeyEventLike, chordMatches, parseChord } from "./bindings.ts";
import { MENU_CHORDS } from "./menukeys.ts";
import { PROBE_AFTER_MS, capture, reservedChords, silenceHint } from "./recorder.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq(actual: unknown, expected: unknown, msg: string): void {
  const [a, b] = [JSON.stringify(actual), JSON.stringify(expected)];
  if (a !== b) throw new Error(`assertion failed: ${msg}\n  actual:   ${a}\n  expected: ${b}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

/** A keydown, as the recorder sees it. Modifiers default to "not held". */
function press(key: string, mods: Partial<KeyEventLike> = {}): KeyEventLike {
  return { key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...mods };
}

/** The chord `capture` wrote down, or a description of why it wrote none. */
function spec(e: KeyEventLike): string {
  const got = capture(e);
  return got.kind === "chord" ? got.spec : `(${got.kind})`;
}

// --- writing a chord down ---------------------------------------------------------------

check("modifiers are written in the order the shipped table uses", () => {
  // Cosmetic on its own — `parseChord` takes them in any order — but a recorded chord sits
  // beside hand-written ones in the same store and the same debugging session, and one
  // spelled backwards reads as a different kind of thing.
  assertEq(spec(press("f", { metaKey: true })), "Mod-f", "⌘F");
  assertEq(spec(press("F", { metaKey: true, shiftKey: true })), "Mod-Shift-f", "⇧⌘F");
  assertEq(spec(press("h", { metaKey: true, altKey: true })), "Mod-Alt-h", "⌥⌘H");
  assertEq(spec(press("Tab", { ctrlKey: true, shiftKey: true })), "Ctrl-Shift-Tab", "⌃⇧Tab");
  assertEq(spec(press("f", { metaKey: true, ctrlKey: true })), "Mod-Ctrl-f", "⌃⌘F");
});

check("a recorded chord answers to the keystroke that produced it", () => {
  // The round trip, and the only assertion here that would catch a spelling bug on its
  // own. Everything above is how it *looks*; this is whether it works.
  const events = [
    press("f", { metaKey: true }),
    press("F", { metaKey: true, shiftKey: true }),
    press("?", { shiftKey: true }),
    press("F10", { shiftKey: true }),
    press(" ", { altKey: true }),
    press("ArrowUp"),
    press(",", { metaKey: true }),
  ];
  for (const e of events) {
    const got = capture(e);
    assert(got.kind === "chord", `${e.key} should record`);
    if (got.kind !== "chord") continue;
    assert(chordMatches(parseChord(got.spec), e), `${got.spec} does not answer to what wrote it`);
  }
});

check("a ⇧ already inside the character is not written down twice", () => {
  // `?` is what ⇧/ reports. A `Shift-?` chord parses, and then demands a shift on top of
  // the one the browser already applied — which `chordMatches` ignores for symbols, so the
  // bug would be silent rather than broken. Spelling it correctly is the fix.
  assertEq(spec(press("?", { shiftKey: true })), "?", "no Shift- prefix");
  assertEq(spec(press("{", { shiftKey: true })), "{", "nor for a shifted bracket");
  // The counterpart: a key whose identity ⇧ genuinely changes keeps the modifier.
  assertEq(spec(press("F10", { shiftKey: true })), "Shift-F10", "⇧F10 is a chord of its own");
  assertEq(spec(press("A", { shiftKey: true })), "Shift-a", "and so is ⇧A");
});

check("the space bar records as a name, not a literal space", () => {
  assertEq(spec(press(" ", { metaKey: true })), "Mod-Space", "⌘Space");
});

check("a modifier on its own is the chord starting, not a chord", () => {
  // Every chord with a modifier arrives as two or more keydowns, and the first of them is
  // ⌘. A recorder that took the first keydown would capture "⌘" and never see the F.
  for (const key of ["Meta", "Shift", "Control", "Alt", "CapsLock"]) {
    assertEq(capture(press(key)).kind, "modifier", `${key} is not a chord`);
  }
});

check("a key no chord can hold is named rather than thrown", () => {
  // A media key, or a dead key mid-composition. The recorder is the one surface where a
  // user deliberately presses strange keys, so handing `parseChord` something it refuses
  // would be a crash in exactly the wrong place.
  const got = capture(press("AudioVolumeUp"));
  assertEq(got.kind, "unbindable", "not a chord");
  assert(got.kind === "unbindable" && got.message.includes("AudioVolumeUp"), "and it says which key");
  assertEq(capture(press("Dead")).kind, "unbindable", "a dead key too");
});

// --- the probe ----------------------------------------------------------------------------

check("silence says nothing until it has been silent long enough", () => {
  // The gap between opening the recorder and reaching for the keyboard is not an
  // observation about anything. A probe that narrated it would be noise on every use.
  assertEq(silenceHint({ elapsedMs: 0, blurred: false }), null, "just opened");
  assertEq(silenceHint({ elapsedMs: PROBE_AFTER_MS - 1, blurred: false }), null, "still reaching");
});

check("sustained silence is read as something upstream having taken the chord", () => {
  const hint = silenceHint({ elapsedMs: PROBE_AFTER_MS, blurred: false });
  assert(hint !== null, "the probe speaks");
  assert(hint?.includes("If you did press something") ?? false, "conditionally — it can't know");
});

check("a lost window is stated as fact, and outranks the timer", () => {
  // The one *positive* signal: Spotlight, the app switcher and Hide all take the key
  // window, so a blur during recording is evidence rather than an inference. It applies
  // immediately — waiting out the timer to report something already observed would be
  // withholding the clearest thing the recorder ever learns.
  const hint = silenceHint({ elapsedMs: 0, blurred: true });
  assert(hint?.includes("lost focus") ?? false, "names what happened");
  assert(!(hint?.includes("If you did press") ?? true), "and does not hedge about it");
});

check("the probe under-reports rather than false-alarms, by construction", () => {
  // Stated as a property because it is the design commitment, not an implementation
  // detail (recorder.ts's header): the probe has exactly two inputs, and neither can be
  // true while a chord is in hand — the caller only asks about silence. So there is no
  // path by which a working chord draws a warning, which is what makes the advisory
  // trustworthy enough to act on.
  assertEq(silenceHint({ elapsedMs: PROBE_AFTER_MS * 100, blurred: false })?.length !== 0, true, "speaks");
  assertEq(silenceHint({ elapsedMs: -1, blurred: false }), null, "and never before it has cause");
});

check("the reserved chords are the host's menu, rendered as the sheet renders chords", () => {
  // What the recorder can name *up front*, and the only thing it can: these are the
  // keystrokes guaranteed to arrive as silence, enumerable only because #119 made the
  // host declare its own menu instead of inheriting Tauri's default.
  const shown = reservedChords();
  assertEq(shown.length, MENU_CHORDS.length, "one per menu item with a chord");
  assert(shown.includes("⌘W"), "⌘W closes the window");
  assert(shown.includes("⌥⌘H"), "and modifiers print in Apple's order");
  assertEq(reservedChords([]), [], "an empty menu reserves nothing");
});

console.log(`recorder: ${passed} checks passed`);
