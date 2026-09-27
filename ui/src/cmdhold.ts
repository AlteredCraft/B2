// Hold ⌘ and a sheet shows what ⌘ does: the keyboard reference as a reflex (K1,
// docs/invariants.md). Pure state machine; the timer, listeners and paint live in main.ts.
//
// A bare ⌘ is not a menu key equivalent, so AppKit lets it through as an ordinary `keydown`
// (`key === "Meta"`). The release is unreliable: macOS stops delivering keys to a window
// that isn't key (⌘⇥, Spotlight), so `blur` and a hidden document count as releases too.
import type { ShortcutGroup } from "./shortcuts.ts";
import { shortcuts } from "./shortcuts.ts";

/** How long ⌘ must be held alone before the sheet appears: long enough not to flash
 *  mid-chord, short enough that a waiting hand doesn't give up. */
export const HOLD_MS = 700;

/** Idle, ⌘ down and the clock running, or the sheet up. */
export type HoldPhase = "idle" | "armed" | "open";

/** What the world did. DOM events collapse at the edge (blur, hidden, pointer press and ⌘ up
 *  are all `release`) to keep the truth table small. */
export type HoldEvent =
  /** ⌘ went down with nothing else held. `repeat` is OS auto-repeat, not a second press. */
  | { kind: "hold"; repeat: boolean }
  /** Any other key went down: the hold was the front half of a chord. */
  | { kind: "other" }
  /** ⌘ came up, or the window lost the keyboard. */
  | { kind: "release" }
  /** The clock finished. */
  | { kind: "elapsed" };

/** The next phase, and what the caller must do with its timer. */
export interface HoldStep {
  readonly phase: HoldPhase;
  readonly timer: "start" | "clear" | "keep";
}

/**
 * The machine, total over (phase × event). Every pair is reachable: a `release` while idle
 * is every ⌘-chord's tail, since the chord's key already cancelled the hold.
 */
export function holdStep(phase: HoldPhase, e: HoldEvent): HoldStep {
  switch (e.kind) {
    case "hold":
      // Only from rest: restarting the clock on a repeat would mean the sheet never opens.
      return phase === "idle" && !e.repeat
        ? { phase: "armed", timer: "start" }
        : { phase, timer: "keep" };
    case "other":
      return phase === "idle" ? { phase: "idle", timer: "keep" } : { phase: "idle", timer: "clear" };
    case "release":
      return phase === "idle" ? { phase: "idle", timer: "keep" } : { phase: "idle", timer: "clear" };
    case "elapsed":
      // Only `armed` is waiting: a stale timer after a cancel finds `idle`.
      return phase === "armed" ? { phase: "open", timer: "keep" } : { phase, timer: "keep" };
  }
}

/**
 * What the sheet shows: the ⌘ chips of the keyboard reference, in its own groups and order.
 * A projection of `shortcuts()`, so rebinding shows here too. Filtering on the rendered "⌘"
 * is exact, since `displayChord` prints it from the parsed modifier. Empty rows and groups
 * drop out.
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
