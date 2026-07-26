// The keyboard reference — the single source of truth for what the `?` cheat sheet
// shows. Pure data, no DOM, so node runs its test straight off the source
// (`npm test`), like newentry.ts / move.ts / treenav.ts.
//
// Why a table rather than prose in a docs page: invariant K1 (docs/design/invariants.md,
// GH #78) promises every mouse action has a keyboard path, and a promise nobody can
// *find* is not kept. A shortcut that exists only in a button's `title` is discoverable
// exactly once — by hovering the button you already knew about. This list is the app's
// answer to "what can I do without the mouse", and adding a chord means adding a row
// here in the same change, so the sheet can't quietly fall behind the wiring.
//
// B2 ships on macOS only (crates/b2-desktop), so modifiers are the platform's glyphs —
// ⌘ command, ⇧ shift, ⌫ delete, ⏎ return — while keys macOS itself spells out in menus
// stay spelled out (Esc, Tab, Space, Home/End). That's the split the app's existing
// tooltips already use ("Close (Esc)"), and the one the sheet must not drift from:
// ⎋ and ⇥ exist, but nobody reads them.

/** One chord and what it does. `keys` is display text — the wiring lives in main.ts. */
export interface Shortcut {
  keys: string;
  action: string;
}

export interface ShortcutGroup {
  title: string;
  items: Shortcut[];
}

export const SHORTCUTS: ShortcutGroup[] = [
  {
    title: "Getting around",
    items: [
      { keys: "⌘1 / ⌘2 / ⌘3", action: "Focus the files, the note, or discovery" },
      { keys: "⇧⌘F", action: "Search the vault" },
      { keys: "⌘F", action: "Find in this note" },
      { keys: "⌘G / ⇧⌘G", action: "Next / previous match" },
      { keys: "⌘[ / ⌘]", action: "Back / forward (⌘← / ⌘→ too)" },
      { keys: "⏎", action: "Follow the focused link, card, or graph node" },
    ],
  },
  {
    title: "The file tree",
    items: [
      { keys: "↑ / ↓", action: "Move between rows" },
      { keys: "→", action: "Expand a folder, or step into it" },
      { keys: "←", action: "Collapse a folder, or step out to its parent" },
      { keys: "Home / End", action: "First / last row" },
      { keys: "A–Z", action: "Jump to the next row starting with that letter" },
      { keys: "⏎ / Space", action: "Open the note or file; fold the folder" },
      { keys: "⌘N / ⇧⌘N", action: "New note / new folder in the selected folder" },
      { keys: "F2", action: "Rename the focused row" },
      { keys: "⌘⌫", action: "Delete the focused row (a folder confirms first)" },
      { keys: "⇧F10", action: "Open the row's menu — Rename, Move…, Delete" },
    ],
  },
  {
    title: "Reading and editing",
    items: [
      { keys: "⌘E", action: "Enter or leave edit mode" },
      { keys: "⌘S", action: "Save now (editing autosaves anyway)" },
      { keys: "⌘B / ⌘I", action: "Bold / italic" },
      { keys: "⌘T", action: "Insert a table" },
      { keys: "⇧⌘V", action: "Paste as plain text" },
      { keys: "[[", action: "Wikilink completion — ↑↓ then ⏎" },
      { keys: "⌘⏎", action: "Save the frontmatter drawer (Esc discards)" },
    ],
  },
  // Discovery gets its own group now that the right column navigates like the tree
  // (sidenav.ts): ↑↓ there means something different from ↑↓ in an open menu, and one
  // chord with two meanings in a single group is a group the reader can't trust.
  {
    title: "Discovery (the right column)",
    items: [
      { keys: "↑ / ↓", action: "Move between section heads and cards" },
      { keys: "→ / ←", action: "Unfold / fold a section or a card's details" },
      { keys: "Home / End", action: "First / last row" },
      { keys: "⏎ / Space", action: "Open the card's note; fold a section head" },
      { keys: "⇧F10", action: "Open a card's menu — Open note, Link…" },
    ],
  },
  {
    title: "The graph and menus",
    items: [
      { keys: "⏎ / Space", action: "Open a graph node; a ghost opens the link palette" },
      { keys: "↑ / ↓", action: "Move through an open menu" },
      { keys: "Esc", action: "Close a menu, a modal, the find bar, or the graph" },
    ],
  },
  {
    title: "The app",
    items: [
      { keys: "⌘,", action: "Settings" },
      { keys: "⇧⌘A", action: "Review the last index pass's anomalies" },
      { keys: "?", action: "This sheet" },
      { keys: "Tab", action: "Step through the controls on screen" },
    ],
  },
];
