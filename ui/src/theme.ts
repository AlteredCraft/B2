// The appearance preference (Settings → General → Theme): what it may be, how <html>
// carries it, and where it persists — main.ts applies it. Its own module like zoom.ts,
// panes.ts and keymap.ts own theirs, so the storage rules sit next to the value they keep.
//
// "system" (the default) defers to the OS via the stylesheet's `prefers-color-scheme`
// rules; "light"/"dark" pin a theme by stamping a `data-theme` attribute on <html> that
// those rules' overrides key on. Persisted in localStorage — a viewing choice, never vault
// state, so it doesn't touch the host.

import type { ThemePref } from "./state.ts";

const KEY = "b2:theme";

/** Does an untrusted string (a stored value, a `data-theme-choice`) name a preference? */
export function isThemePref(v: string | null): v is ThemePref {
  return v === "system" || v === "light" || v === "dark";
}

/** The `data-theme` value <html> carries for a preference, or null to follow the OS. */
export function themeAttr(theme: ThemePref): string | null {
  return theme === "system" ? null : theme;
}

// --- persistence -----------------------------------------------------------------------

/** The saved preference. Unreadable, unrecognised or unavailable storage is "system". */
export function loadThemePref(): ThemePref {
  try {
    const saved = localStorage.getItem(KEY);
    return isThemePref(saved) ? saved : "system";
  } catch {
    // localStorage can be unavailable (e.g. private mode) — fall back to System.
    return "system";
  }
}

export function saveThemePref(theme: ThemePref): void {
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Non-fatal: the choice still applies for this session if it can't persist.
  }
}
