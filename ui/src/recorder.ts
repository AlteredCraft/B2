// The chord recorder's pure half (#121): a keydown in, a chord spec out, plus the one
// piece of reasoning the recorder can do that no lookup table can — reading the *absence*
// of a keydown as evidence.
//
// No DOM, so node runs its test straight off the source (`npm test`). render.ts owns the
// markup and main.ts owns the listener and the timer; what lives here is the decision each
// makes, which is the half worth pinning.
//
// **The empirical probe.** macOS dispatches menu key equivalents inside
// `NSApplication.sendEvent` and system hotkeys ahead of the key window's responder chain,
// so a chord either arrives at the webview or it does not — and a chord that does not
// arrive is a chord B2 could never answer to, whoever took it. That makes silence a real
// observation rather than a missing one, and it is the only observation that stays true
// when the user remaps Spotlight or installs Raycast tomorrow: nothing enumerates another
// process's `RegisterEventHotKey` claims, so any curated "commonly taken" list would be a
// guess with a shelf life. (GH #122 proposed the Carbon `CopySymbolicHotKeys` table for
// the pre-emptive half of this and was closed on that reasoning — it can't see third-party
// hotkeys either, and it buys unsafe FFI for an advisory string.)
//
// Two limits, both stated in the UI copy rather than only here, because a user who
// misreads this probe misreads it as B2 being broken:
//
//   - It only ever evaluates the chord actually pressed. There is no pre-validating a
//     list, because there is no way to press a chord on the user's behalf.
//   - It is sufficient, not necessary. Some system hotkeys pass the keydown through
//     anyway, so silence proves a chord was taken while noise proves nothing. It
//     **under-reports**, which is the right failure direction for an advisory: a recorder
//     that cried "macOS has this" over a chord that works would be worse than one that
//     occasionally stays quiet about one that doesn't.
//
// The one signal stronger than silence is `blurred`. Spotlight, the app switcher and Hide
// all take the key window away, and a window that lost focus mid-recording is positive
// evidence that something outside B2 answered — not an inference from nothing happening.
import { type KeyEventLike, canonicalKey, displayChord, isBindableKey, shiftDistinguishes } from "./bindings.ts";
import { MENU_CHORDS } from "./menukeys.ts";
import type { MenuChord } from "./types.ts";

/** Keys that are only ever *part* of a chord. A keydown for one of these is the user
 *  starting a chord, not finishing one, so the recorder keeps waiting. */
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
 * Read a keydown as a chord in the registry's syntax.
 *
 * The spelling has to match the table's exactly or the recorded chord and the pressed
 * keystroke stop being the same thing — so ⇧ is written down only when it *distinguishes*
 * the key (`?` is already what ⇧/ reports; a `Shift-?` chord would double-count it), and
 * modifiers come out in the order the shipped table uses, Mod-Ctrl-Alt-Shift.
 */
export function capture(e: KeyEventLike): Capture {
  const key = canonicalKey(e.key);
  if (MODIFIER_KEYS.has(key)) return { kind: "modifier" };
  if (!isBindableKey(key)) {
    // A media key, a dead key mid-composition, an `Unidentified` from a layout B2 has
    // nothing to say about. Naming it beats handing `parseChord` something that throws.
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

/** How long the recorder waits before reading silence as an observation.
 *
 *  Long enough that it isn't narrating the gap between opening the recorder and reaching
 *  for the keyboard, short enough that a user who pressed ⌘W and saw nothing is still
 *  wondering why. */
export const PROBE_AFTER_MS = 2500;

/** What the recorder has observed while nothing has arrived. */
export interface Silence {
  /** Since the recorder opened. Passed in rather than read, so this stays pure. */
  elapsedMs: number;
  /** The window lost focus while recording — the strong signal (see the module header). */
  blurred: boolean;
}

/**
 * What to tell the user when no chord has been captured yet, or null while there is
 * nothing honest to say.
 *
 * The two messages are deliberately different in confidence. A blur *happened*; silence
 * is read.
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

/** The menu bar's chords, as display text — the list the recorder names up front, since
 *  these are the ones guaranteed to arrive as silence (menukeys.ts says why AppKit takes
 *  them first). Enumerable only because #119 made the host declare its own menu. */
export function reservedChords(menu: readonly MenuChord[] = MENU_CHORDS): string[] {
  return menu.map((c) => displayChord(c.keys));
}
