// The chord recorder's pure half (#121): a keydown in, a chord spec out, plus reading the
// absence of a keydown as evidence. render.ts owns the markup; main.ts the listener and timer.
//
// The probe: macOS dispatches menu key equivalents and system hotkeys before the webview,
// so a chord that never arrives is one B2 could never answer to. Nothing can enumerate other
// apps' hotkeys, so observing silence is the only check that stays true (GH #122). It
// under-reports (some hotkeys pass the keydown through), the safe direction for an advisory.
// A blur mid-recording is stronger evidence: something outside B2 took the key window.
import { type KeyEventLike, canonicalKey, isBindableKey, shiftDistinguishes } from "./bindings.ts";

/** Keys that only start a chord, so the recorder keeps waiting. */
const MODIFIER_KEYS = new Set([
  "Meta",
  "Control",
  "Shift",
  "Alt",
  "AltGraph",
  "CapsLock",
  "Fn",
  "FnLock",
  "NumLock",
  "ScrollLock",
  "Hyper",
  "Super",
  "OS",
]);

/** What one keydown means to a recorder that is waiting for a chord. */
export type Capture =
  | { kind: "modifier" }
  | { kind: "chord"; spec: string }
  | { kind: "unbindable"; message: string };

/**
 * Read a keydown as a chord in the registry's syntax, spelled exactly as the table does: ⇧
 * only when it distinguishes the key, modifiers in Mod-Ctrl-Alt-Shift order.
 */
export function capture(e: KeyEventLike): Capture {
  const key = canonicalKey(e.key);
  if (MODIFIER_KEYS.has(key)) return { kind: "modifier" };
  if (!isBindableKey(key)) {
    // A media key, a dead key, `Unidentified`: say so rather than let `parseChord` throw.
    return { kind: "unbindable", message: `B2 can't build a shortcut out of ${key}.` };
  }
  const parts: string[] = [];
  if (e.metaKey) parts.push("Mod");
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey && shiftDistinguishes(key)) parts.push("Shift");
  parts.push(key);
  return { kind: "chord", spec: parts.join("-") };
}

/** How long the recorder waits before reading silence as an observation. */
export const PROBE_AFTER_MS = 2500;

/** What the recorder has observed while nothing has arrived. */
export interface Silence {
  /** Since the recorder opened. Passed in rather than read, so this stays pure. */
  elapsedMs: number;
  /** The window lost focus while recording — the strong signal (see the module header). */
  blurred: boolean;
}

/**
 * What to tell the user when no chord has arrived yet, or null. The messages differ in
 * confidence: a blur happened; silence is inferred.
 */
export function silenceHint(s: Silence): string | null {
  if (s.blurred) {
    return "Something outside B2 answered that — the window lost focus. That chord can't be B2's.";
  }
  if (s.elapsedMs >= PROBE_AFTER_MS) {
    return "Nothing has reached B2 yet. If you did press something, macOS or another app claimed it first — B2 never sees those keys, so it can't be bound to them.";
  }
  return null;
}
