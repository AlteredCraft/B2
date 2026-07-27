// The Settings dialog's tab model — pure data + pure navigation, no DOM, so node runs its
// test straight off the source (`npm test`), like shortcuts.ts / treenav.ts / sidenav.ts.
//
// Settings outgrew a single scrolling column: it now carries Appearance, the embedding
// model (picker, compute device, in-app download, the per-model time ledger, where the
// files live) and the whole keyboard reference. Those are three different questions a
// human comes here with, and stacking them in one column means the answer to any of them
// is somewhere in a scroll. Tabs make each one a place you can *go*.
//
// Why the split is 3 and not 2: Appearance and the model share nothing — not a concept,
// not a failure mode, not a moment you'd open the dialog. And the keyboard reference is a
// *reference*, read start to finish, which is the one thing a settings column must never
// make you scroll past. The rail is where expansion lands (vault prefs, editor prefs,
// diagnostics): a new tab is a row here plus a panel in render.ts, and nothing else moves.
//
// Invariant K1 (docs/design/invariants.md, GH #78) governs the rail like every other
// surface, so it follows the ARIA `tabs` pattern: a **roving `tabindex`** (the rail is one
// Tab stop, not one per section), ↑↓ between tabs with wrap, Home/End to the ends. The
// moves live here rather than in main.ts's keydown for treenav.ts's reason — the paint and
// the arrows must agree on order, so the order is defined once and both sides read it.

/** The id of a Settings section — the tab, its panel, and `state.settingsTab`. */
export type SettingsTabId = "general" | "embedding" | "keyboard";

export interface SettingsTab {
  id: SettingsTabId;
  label: string;
  /** The tab button's `title`: what the section holds, for the hover that asks. */
  hint: string;
}

/** The rail, in paint order. Adding a section means adding a row here and a panel in
 *  render.ts's `settingsPanelHtml` — the navigation, the roving tabstop, and the wrap
 *  all follow from this list. */
export const SETTINGS_TABS: SettingsTab[] = [
  { id: "general", label: "General", hint: "Appearance and app-wide preferences" },
  {
    id: "embedding",
    label: "Embedding",
    hint: "The embedding model, its compute device, and how long it takes",
  },
  { id: "keyboard", label: "Keyboard", hint: "Every chord B2 answers to — ?" },
];

/** The default section — what ⌘, opens on the first time in a session. */
export const DEFAULT_SETTINGS_TAB: SettingsTabId = "general";

/** Whether an untrusted string (a `data-settings-tab` attribute, a stored preference)
 *  names a real section. */
export function isSettingsTab(value: unknown): value is SettingsTabId {
  return typeof value === "string" && SETTINGS_TABS.some((t) => t.id === value);
}

/**
 * Step `delta` tabs from `current`, **wrapping** at both ends — the ARIA tabs pattern's
 * own behavior, and what makes ⌃Tab a cycle rather than a walk that dead-ends on the
 * last section. An unknown `current` starts the walk from the first tab.
 */
export function tabStep(current: SettingsTabId, delta: 1 | -1): SettingsTabId {
  const i = SETTINGS_TABS.findIndex((t) => t.id === current);
  const from = i === -1 ? 0 : i;
  const n = SETTINGS_TABS.length;
  return SETTINGS_TABS[(from + delta + n) % n].id;
}

/**
 * The rail's arrow keys: ↑↓ step (wrapping), Home/End jump to the ends. Null for any
 * other key, so the caller knows to leave the event alone — a rail that swallowed keys
 * it has no move for would eat Tab, ⏎, and the global chords along with them.
 */
export function tabMove(current: SettingsTabId, key: string): SettingsTabId | null {
  switch (key) {
    case "ArrowDown":
      return tabStep(current, 1);
    case "ArrowUp":
      return tabStep(current, -1);
    case "Home":
      return SETTINGS_TABS[0].id;
    case "End":
      return SETTINGS_TABS[SETTINGS_TABS.length - 1].id;
    default:
      return null;
  }
}
