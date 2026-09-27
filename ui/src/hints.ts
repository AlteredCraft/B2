// The chord hints on the shell's static chrome (top bar, find bar, Done), derived from the
// live keyboard. Chords are rebindable (#121) and the shell is painted once, so main.ts
// rewrites these by element id at boot and on every `setOverrides`. Pane hints need none
// of this: render.ts derives them on every paint.

import { displayKeys } from "./bindings.ts";

/** What one element says about its chord: its tooltip, or the search box's placeholder. */
export interface ChordHint {
  readonly title?: string;
  readonly placeholder?: string;
}

/** The Done button's tooltip while editing. */
export function editDoneTitle(): string {
  return `Save and return to reading — ${displayKeys(["edit.toggle"])} (${displayKeys([
    "editor.save",
  ])} flushes anytime)`;
}

/** Every static shell element's chord hint, by element id; the painter skips absent ones. */
export function shellHints(): Record<string, ChordHint> {
  const search = displayKeys(["search.focus"]);
  return {
    "nav-back": { title: `Back (${displayKeys(["nav.back"])})` },
    "nav-forward": { title: `Forward (${displayKeys(["nav.forward"])})` },
    "search-input": {
      placeholder: `Search the vault…  ${search}`,
      title: `Search the vault — ${search} (${displayKeys(["find.open"])} finds inside the open note)`,
    },
    "open-chat": { title: `Ask your notes (${displayKeys(["chat.toggle"])})` },
    "open-settings": { title: `Settings (${displayKeys(["settings.toggle"])})` },
    // Both the rebindable find-scope chords and the field's ⏎ / ⇧⏎ work, so both are named.
    "find-prev": { title: `Previous match (${displayKeys(["find.prev", "find.input.prev"])})` },
    "find-next": { title: `Next match (${displayKeys(["find.next", "find.input.next"])})` },
    "find-close": { title: `Close (${displayKeys(["dismiss"])})` },
    "edit-done": { title: editDoneTitle() },
  };
}
