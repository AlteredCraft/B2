// The Settings dialog's tab model — pure data + pure navigation, no DOM, so node runs its
// test straight off the source (`npm test`), like shortcuts.ts / treenav.ts / sidenav.ts.
//
// Settings outgrew a single scrolling column: it now carries Appearance, the vault index
// (and the manual Reindex), the embedding model (picker, compute device, in-app download,
// the per-model time ledger, where the files live) and the whole keyboard reference. Those
// are four different questions a human comes here with, and stacking them in one column
// means the answer to any of them is somewhere in a scroll. Tabs make each one a place you
// can *go*.
//
// Why the split is 4 and not fewer: Appearance and the model share nothing — not a concept,
// not a failure mode, not a moment you'd open the dialog. And the keyboard reference is a
// *reference*, read start to finish, which is the one thing a settings column must never
// make you scroll past. The rail is where expansion lands (vault prefs, editor prefs,
// diagnostics): a new tab is a row here plus a panel in render.ts, and nothing else moves.
// Index is the worked example — it arrived as one button that used to live in the top bar.
//
// Invariant K1 (docs/design/invariants.md, GH #78) governs the rail like every other
// surface, so it follows the ARIA `tabs` pattern: a **roving `tabindex`** (the rail is one
// Tab stop, not one per section), ↑↓ between tabs with wrap, Home/End to the ends. The
// moves live here rather than in main.ts's keydown for treenav.ts's reason — the paint and
// the arrows must agree on order, so the order is defined once and both sides read it.
//
// The *keys* those moves answer to are the registry's since #121 (`tabMove` switches on a
// binding id, not on `e.key`), for the reason treenav.ts's header gives: an arrow nobody
// can rebind and no checker can see is the one part of the keyboard that gets left behind.

import { type BindingId, type KeyEventLike, boundOf } from "./bindings.ts";

/** The id of a Settings section — the tab, its panel, and `state.settingsTab`. */
export type SettingsTabId = "general" | "index" | "embedding" | "chat" | "keyboard";

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
    id: "index",
    label: "Index",
    hint: "The vault index — its coverage, and rebuilding it by hand",
  },
  {
    id: "embedding",
    label: "Embedding",
    hint: "The embedding model, its compute device, and how long it takes",
  },
  // The spec (GH #151/#155) calls this the **Models** tab. It ships as *Chat* because the
  // section above it is a model too: "Embedding" and "Models" side by side would name the
  // subsystem in one row and the technology in the next, and the question a human arrives
  // with is "which model answers my questions", not "which of these two is a model". The
  // spec's own open question — whether Embedding eventually folds in here — stays open,
  // and this naming is what would make that fold read as a merge rather than a rename.
  {
    id: "chat",
    label: "Chat",
    hint: "The model that answers your questions — local, or a cloud provider",
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

/** The navigation commands the rail answers to — registry ids, in the order the
 *  dispatcher tries them. Distinct from `settings.section.next`/`prev` (⌃Tab), which
 *  cycle from *anywhere* in the dialog; these apply only with the keyboard on a tab. */
export const TAB_NAV = [
  "settings.tab.prev",
  "settings.tab.next",
  "settings.tab.first",
  "settings.tab.last",
] as const satisfies readonly BindingId[];

export type TabNav = (typeof TAB_NAV)[number];

/** Which rail move — if any — this keystroke is, per the live registry. */
export function tabNavFor(e: KeyEventLike): TabNav | null {
  return boundOf(e, TAB_NAV);
}

/**
 * The rail's walk: prev/next step (wrapping), first/last jump to the ends. The caller
 * asks `tabNavFor` first and leaves the event alone when that returns null — a rail that
 * swallowed keys it has no move for would eat Tab, ⏎, and the global chords along with them.
 */
export function tabMove(current: SettingsTabId, nav: TabNav): SettingsTabId {
  switch (nav) {
    case "settings.tab.next":
      return tabStep(current, 1);
    case "settings.tab.prev":
      return tabStep(current, -1);
    case "settings.tab.first":
      return SETTINGS_TABS[0].id;
    case "settings.tab.last":
      return SETTINGS_TABS[SETTINGS_TABS.length - 1].id;
  }
}
