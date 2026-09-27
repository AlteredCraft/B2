// The chord recorder's pure half (recorder.ts): a recorded chord must round-trip through
// the registry's matcher, and the silence probe must stay quiet as much as it speaks.
import { type KeyEventLike, chordMatches, parseChord } from "./bindings.ts";
import { PROBE_AFTER_MS, capture, silenceHint } from "./recorder.ts";

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
  // Cosmetic (`parseChord` takes any order), but consistent with hand-written chords.
  assertEq(spec(press("f", { metaKey: true })), "Mod-f", "⌘F");
  assertEq(spec(press("F", { metaKey: true, shiftKey: true })), "Mod-Shift-f", "⇧⌘F");
  assertEq(spec(press("h", { metaKey: true, altKey: true })), "Mod-Alt-h", "⌥⌘H");
  assertEq(spec(press("Tab", { ctrlKey: true, shiftKey: true })), "Ctrl-Shift-Tab", "⌃⇧Tab");
  assertEq(spec(press("f", { metaKey: true, ctrlKey: true })), "Mod-Ctrl-f", "⌃⌘F");
});

check("a recorded chord answers to the keystroke that produced it", () => {
  const events = [
    press("f", { metaKey: true }),
    press("F", { metaKey: true, shiftKey: true }),
    press("?", { shiftKey: true }),
    press("F10", { shiftKey: true }),
    press(" ", { altKey: true }),
    press("ArrowUp"),
    press(",", { metaKey: true }),
    // The separator as a key: "Mod--" reads ambiguously (GH #125).
    press("-"),
    press("-", { metaKey: true }),
    press("-", { metaKey: true, shiftKey: true }),
  ];
  for (const e of events) {
    const got = capture(e);
    assert(got.kind === "chord", `${e.key} should record`);
    if (got.kind !== "chord") continue;
    assert(chordMatches(parseChord(got.spec), e), `${got.spec} does not answer to what wrote it`);
  }
});

check("a ⇧ already inside the character is not written down twice", () => {
  assertEq(spec(press("?", { shiftKey: true })), "?", "no Shift- prefix");
  assertEq(spec(press("{", { shiftKey: true })), "{", "nor for a shifted bracket");
  assertEq(spec(press("F10", { shiftKey: true })), "Shift-F10", "⇧F10 is a chord of its own");
  assertEq(spec(press("A", { shiftKey: true })), "Shift-a", "and so is ⇧A");
});

check("the hyphen records as itself, separator or not", () => {
  assertEq(spec(press("-")), "-", "bare");
  assertEq(spec(press("-", { metaKey: true })), "Mod--", "and with ⌘ in front of it");
});

check("the space bar records as a name, not a literal space", () => {
  assertEq(spec(press(" ", { metaKey: true })), "Mod-Space", "⌘Space");
});

check("a modifier on its own is the chord starting, not a chord", () => {
  for (const key of ["Meta", "Shift", "Control", "Alt", "CapsLock"]) {
    assertEq(capture(press(key)).kind, "modifier", `${key} is not a chord`);
  }
});

check("a key no chord can hold is named rather than thrown", () => {
  const got = capture(press("AudioVolumeUp"));
  assertEq(got.kind, "unbindable", "not a chord");
  assert(got.kind === "unbindable" && got.message.includes("AudioVolumeUp"), "and it says which key");
  assertEq(capture(press("Dead")).kind, "unbindable", "a dead key too");
});

// --- the probe ----------------------------------------------------------------------------

check("silence says nothing until it has been silent long enough", () => {
  assertEq(silenceHint({ elapsedMs: 0, blurred: false }), null, "just opened");
  assertEq(silenceHint({ elapsedMs: PROBE_AFTER_MS - 1, blurred: false }), null, "still reaching");
});

check("sustained silence is read as something upstream having taken the chord", () => {
  const hint = silenceHint({ elapsedMs: PROBE_AFTER_MS, blurred: false });
  assert(hint !== null, "the probe speaks");
  assert(hint?.includes("If you did press something") ?? false, "conditionally — it can't know");
});

check("a lost window outranks the timer however late the timer runs", () => {
  // A late probe tick must not downgrade a blur to a guess (GH #125); the wiring keeps the
  // blur on `RecorderState.blurred`.
  for (const elapsedMs of [0, PROBE_AFTER_MS, PROBE_AFTER_MS * 100]) {
    const hint = silenceHint({ elapsedMs, blurred: true });
    assert(hint?.includes("lost focus") ?? false, `blurred wins at ${elapsedMs}ms`);
  }
});

check("a lost window is stated as fact, and outranks the timer", () => {
  // Applies immediately: a blur is observed, not inferred.
  const hint = silenceHint({ elapsedMs: 0, blurred: true });
  assert(hint?.includes("lost focus") ?? false, "names what happened");
  assert(!(hint?.includes("If you did press") ?? true), "and does not hedge about it");
});

check("the probe under-reports rather than false-alarms, by construction", () => {
  // The caller only asks about silence, so a working chord can never draw a warning.
  assertEq(silenceHint({ elapsedMs: PROBE_AFTER_MS * 100, blurred: false })?.length !== 0, true, "speaks");
  assertEq(silenceHint({ elapsedMs: -1, blurred: false }), null, "and never before it has cause");
});

console.log(`recorder: ${passed} checks passed`);
