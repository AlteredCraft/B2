// The Settings dialog's tab model: pure data and navigation, no DOM.
//
// The rail follows the ARIA `tabs` pattern (K1, GH #78): a roving `tabindex`, ↑↓ with
// wrap, Home/End to the ends. The order is defined once here so the paint and the arrows
// agree; the keys are the registry's (#121), so `tabMove` switches on binding ids.

import { type BindingId, type KeyEventLike, boundOf } from "./bindings.ts";

/** The id of a Settings section — the tab, its panel, and `state.settingsTab`. */
export type SettingsTabId = "general" | "index" | "embedding" | "chat" | "keyboard";

export interface SettingsTab {
  id: SettingsTabId;
  label: string;
  /** The tab button's `title`: what the section holds, for the hover that asks. */
  hint: string;
}

/** The rail, in paint order. A new section is a row here plus a panel in settingsview.ts. */
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
  // The spec's "Models" tab (GH #151/#155), named Chat since Embedding is a model too.
  {
    id: "chat",
    label: "Chat",
    hint: "The model that answers your questions — local, or a cloud provider",
  },
  { id: "keyboard", label: "Keyboard", hint: "Every chord B2 answers to — ?" },
];

/** The default section — what ⌘, opens on the first time in a session. */
export const DEFAULT_SETTINGS_TAB: SettingsTabId = "general";

/** A tab's element id, also used by `aria-labelledby` and to re-focus after a repaint. */
export function tabDomId(id: SettingsTabId): string {
  return `settings-tab-${id}`;
}

/** Whether an untrusted string (a `data-settings-tab` attribute, a stored preference)
 *  names a real section. */
export function isSettingsTab(value: unknown): value is SettingsTabId {
  return typeof value === "string" && SETTINGS_TABS.some((t) => t.id === value);
}

/** Step `delta` tabs from `current`, wrapping at both ends. Unknown `current` starts at 0. */
export function tabStep(current: SettingsTabId, delta: 1 | -1): SettingsTabId {
  const i = SETTINGS_TABS.findIndex((t) => t.id === current);
  const from = i === -1 ? 0 : i;
  const n = SETTINGS_TABS.length;
  return SETTINGS_TABS[(from + delta + n) % n].id;
}

/** The rail's navigation commands, live only with the keyboard on a tab (unlike ⌃Tab's
 *  `settings.section.*`, which work anywhere in the dialog). */
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
 * The rail's walk. The caller asks `tabNavFor` first and leaves the event alone on null, so
 * Tab, ⏎ and global chords pass through.
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
