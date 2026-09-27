// The appearance preference (Settings → General → Theme); main.ts applies it. "system"
// defers to `prefers-color-scheme`; "light"/"dark" set `data-theme` on <html>, which the
// stylesheet's overrides key on. Persisted in localStorage: a viewing choice, not vault
// state.

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
    return "system";
  }
}

export function saveThemePref(theme: ThemePref): void {
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Non-fatal: the choice still applies for this session.
  }
}
