// The chord hints on the shell's static chrome — the top bar, the find bar, the editor's
// Done button — as data, derived from the live keyboard.
//
// Why a module. Chords are rebindable (#121), so a tooltip that spells "⌘[" is wrong for
// exactly the user who moved Back somewhere else — and the shell is painted once at boot,
// so even a hint that *was* derived went stale on the first rebind. Everything the shell
// says about a chord is listed here, keyed by element id, and main.ts writes it on to the
// DOM twice: when the shell is built, and again whenever the keyboard changes
// (`setOverrides`). The pane surfaces need no such help — render.ts derives their hints
// from `displayKeys` on every paint.
//
// Pure: it reads the registry and returns strings, so node tests it off the source, and
// hints.test.ts also scans the UI's string literals for a chord spelled by hand.

import { displayKeys } from "./bindings.ts";

/** What one element says about its chord. `title` is its tooltip; `placeholder` is the
 *  search box's in-field hint, the one control that advertises its chord that way. */
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

/** Every static shell element's chord hint, by element id. An element that isn't on
 *  screen (Done, outside the editor) is simply skipped by the painter. */
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
    // The buttons answer to the find-scope chords (rebindable) as well as to ⏎ / ⇧⏎ in
    // the field (the text field's reflex), so both are named.
    "find-prev": { title: `Previous match (${displayKeys(["find.prev", "find.input.prev"])})` },
    "find-next": { title: `Next match (${displayKeys(["find.next", "find.input.next"])})` },
    "find-close": { title: `Close (${displayKeys(["dismiss"])})` },
    "edit-done": { title: editDoneTitle() },
  };
}
