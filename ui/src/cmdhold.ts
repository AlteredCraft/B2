// Hold ⌘ and the app tells you what ⌘ does — the keyboard reference as a *reflex* rather
// than a place you go.
//
// Settings → Keyboard (shortcuts.ts) is the complete, editable table, and it answers the
// question you sat down to ask. This answers the other one: the hand is already on ⌘, the
// chord is half-remembered, and walking to a settings dialog to look it up costs more than
// reaching for the mouse — which is exactly the moment K1 (docs/invariants.md) is lost.
// So the sheet comes to the hand, and leaves the instant the hand does.
//
// **Yes, a held ⌘ is observable.** menukeys.ts's header is about a chord: AppKit
// dispatches a menu *key equivalent* — ⌘ plus a key — inside `NSApplication.sendEvent`,
// before the key window's responder chain, so the webview never sees ⌘Q. A bare modifier
// press is not a key equivalent and has nothing to match, so it takes the ordinary path
// and arrives as `keydown` with `key === "Meta"`, and its release as `keyup`. That is the
// whole mechanism; there is no native call and nothing to poll.
//
// What the DOM can't be trusted for is the *release*. macOS stops delivering key events to
// a window that isn't key, so ⌘⇥ into another app, Spotlight, or Hide leaves the keyup
// somewhere B2 never receives it — the same silence recorder.ts reads as evidence. Hence
// `blur` and a hidden document are releases here too, and a stuck sheet is a bug this
// module is shaped to prevent rather than one it hopes not to hit.
//
// Everything here is pure, so node runs the machine's whole truth table off the source
// (`npm test`). The timer, the listeners and the paint stay in main.ts, which is the same
// split render.ts / state.ts already draw.
import type { ShortcutGroup } from "./shortcuts.ts";
import { shortcuts } from "./shortcuts.ts";

/** How long ⌘ must be held alone before the sheet appears.
 *
 *  A hold is only a *question* once it has outlasted every chord: ⌘S is ⌘ held for as long
 *  as it takes to hit S, and a sheet that flashed during it would be a strobe on the one
 *  surface that must stay calm. Any other keystroke cancels regardless (below), so this is
 *  belt and braces — but the braces are what make a deliberate pause read as deliberate.
 *  Long enough not to fire mid-chord, short enough that a hand waiting on it doesn't
 *  conclude nothing is coming. */
export const HOLD_MS = 700;

/** Idle, ⌘ down and the clock running, or the sheet up. */
export type HoldPhase = "idle" | "armed" | "open";

/** What the world did, in the machine's vocabulary.
 *
 *  Deliberately four, not one per DOM event: `blur`, a hidden document, a pointer press
 *  and a ⌘ release are one thing here — the hold is over — and collapsing them at the edge
 *  is what keeps the machine's truth table small enough to read. */
export type HoldEvent =
  /** ⌘ went down with nothing else held. `repeat` is the OS's auto-repeat, which is not a
   *  second press and must not restart the clock. */
  | { kind: "hold"; repeat: boolean }
  /** Any other key went down: the hold turned out to be the front half of a chord. */
  | { kind: "other" }
  /** ⌘ came up — or the window lost the keyboard, which is the same thing (see above). */
  | { kind: "release" }
  /** The clock finished. */
  | { kind: "elapsed" };

/** The next phase, and what the caller must do with its timer. */
export interface HoldStep {
  readonly phase: HoldPhase;
  readonly timer: "start" | "clear" | "keep";
}

/**
 * The machine. Total over (phase × event) on purpose — every pair below is reachable from
 * a real keyboard, and the ones that look impossible are the ones that actually happen:
 * a `release` with no hold in flight is every ⌘-chord's tail, since the chord's own key
 * cancelled the hold while ⌘ was still down.
 */
export function holdStep(phase: HoldPhase, e: HoldEvent): HoldStep {
  switch (e.kind) {
    case "hold":
      // Only from rest: an auto-repeat, or ⌘ re-reported while the sheet is up, changes
      // nothing. Restarting the clock on a repeat would mean the sheet never opened at all.
      return phase === "idle" && !e.repeat
        ? { phase: "armed", timer: "start" }
        : { phase, timer: "keep" };
    case "other":
      // A chord is being typed. From `open` this is the sheet getting out of the way of
      // the command it just finished explaining, which is the point of it.
      return phase === "idle" ? { phase: "idle", timer: "keep" } : { phase: "idle", timer: "clear" };
    case "release":
      return phase === "idle" ? { phase: "idle", timer: "keep" } : { phase: "idle", timer: "clear" };
    case "elapsed":
      // Only `armed` is waiting for it. A stale timer that fires after a cancel finds
      // `idle` here rather than opening a sheet nobody asked for.
      return phase === "armed" ? { phase: "open", timer: "keep" } : { phase, timer: "keep" };
  }
}

/**
 * What the sheet shows: the ⌘ half of the keyboard reference, in the reference's own
 * groups and order.
 *
 * A projection rather than a second table, for the reason shortcuts.ts exists at all — a
 * hand-written list of ⌘ chords would be a third place a chord is spelled and the first to
 * fall behind. It reads `activeBindings()` through `shortcuts()`, so a rebound chord shows
 * up here rebound, and a command moved *off* ⌘ leaves.
 *
 * The filter is on the chord's rendering, which is the honest test: "⌘" is in the text iff
 * the chord answers to a held ⌘, because `displayChord` prints the glyph from the parsed
 * modifier. A row keeps only its ⌘ chips — "Next / previous match" is ⌘G and ⇧⌘G, both
 * ⌘ — and a row left with none (the arrow families, ⏎, Tab) drops out with its group if
 * the group empties.
 */
export function cmdShortcuts(): ShortcutGroup[] {
  return shortcuts()
    .map((g) => ({
      title: g.title,
      items: g.items
        .map((s) => ({ action: s.action, keys: s.keys.filter((k) => k.text.includes("⌘")) }))
        .filter((s) => s.keys.length > 0),
    }))
    .filter((g) => g.items.length > 0);
}
