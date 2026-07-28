// The keyboard reference — the single source of truth for what Settings' Keyboard
// section shows. Pure data, no DOM, so node runs its test straight off the source
// (`npm test`), like newentry.ts / move.ts / treenav.ts.
//
// Why a table rather than prose in a docs page: invariant K1 (docs/design/invariants.md,
// GH #78) promises every mouse action has a keyboard path, and a promise nobody can
// *find* is not kept. A shortcut that exists only in a button's `title` is discoverable
// exactly once — by hovering the button you already knew about. This list is the app's
// answer to "what can I do without the mouse".
//
// What changed when bindings.ts arrived: a row no longer *spells* its chord, it **names
// the command** and the chord is projected from the registry. So the sheet can't drift
// from the wiring by a typo, and — because bindings.test.ts asserts every binding lands
// in some row — a new chord can't be added without a row here either. That's the K1
// promise held by construction instead of by remembering.
//
// The prose stays hand-written, deliberately. Grouping ⌘1/⌘2/⌘3 into one row, or saying
// "Back / forward (⌘← / ⌘→ too)" rather than listing four chords, is editorial judgement
// a generator would flatten — and the sheet's job is to be *read*.
//
// Literal rows (`keys:` instead of `ids:`) are the things that aren't B2 chords at all:
// the platform's own behavior (Tab, ⏎ on a focused button, first-letter typeahead), and
// the arrow families whose key → move mapping belongs to a pure module of its own —
// treenav.ts, sidenav.ts, settingstabs.ts. Copying those keys into the registry would
// give them two owners, which is the drift the registry exists to end; bindings.ts says
// the same thing from the other side.
//
// B2 ships on macOS only (crates/b2-desktop), so modifiers are the platform's glyphs —
// ⌘ command, ⇧ shift, ⌫ delete, ⏎ return — while keys macOS itself spells out in menus
// stay spelled out (Esc, Tab, Space, Home/End). That's the split the app's existing
// tooltips already use ("Close (Esc)"); `displayChord` in bindings.ts now applies it.
//
// One group has no ids and no hand-written keys either: the menu bar's (#119). Those
// chords aren't B2's — they're the app menu's, and AppKit takes them before the webview
// sees a key — so the sheet lists them from the *host's* declaration, passed in by
// render.ts (`menukeys.ts` supplies the offline mirror for the first paint and the
// suite). K1's promise is that a keyboard path is findable, and it says nothing about
// who authored it.
import { type BindingId, displayChord, displayKeys } from "./bindings.ts";
import { MENU_CHORDS } from "./menukeys.ts";
import type { MenuChord } from "./types.ts";

/** One chord and what it does. `keys` is display text, projected from the registry. */
export interface Shortcut {
  keys: string;
  action: string;
}

export interface ShortcutGroup {
  title: string;
  items: Shortcut[];
}

/** A row of the sheet: either the commands it documents, or — for the platform's own
 *  keys and the arrow families — the literal text to print. */
export type SheetRow =
  | { readonly ids: readonly BindingId[]; readonly action: string }
  | { readonly keys: string; readonly action: string };

export interface SheetGroup {
  readonly title: string;
  readonly rows: readonly SheetRow[];
}

/** The groups B2 authors — everything except the menu bar's, which is the host's and is
 *  appended by `sheet()`. */
const OWN_SHEET: readonly SheetGroup[] = [
  {
    title: "Getting around",
    rows: [
      {
        ids: ["pane.tree", "pane.note", "pane.discovery"],
        action: "Focus the files, the note, or discovery",
      },
      { ids: ["search.focus"], action: "Search the vault" },
      { ids: ["find.open"], action: "Find in this note" },
      { ids: ["find.next", "find.prev"], action: "Next / previous match" },
      { ids: ["nav.back", "nav.forward"], action: "Back / forward (⌘← / ⌘→ too)" },
      // The platform's own activation of a focused button or link — not a B2 binding.
      // The graph's ⏎ *is* one (SVG has no native activation); it has its own row below.
      { keys: "⏎", action: "Follow the focused link, card, or graph node" },
    ],
  },
  {
    title: "The file tree",
    rows: [
      { keys: "↑ / ↓", action: "Move between rows" },
      { keys: "→", action: "Expand a folder, or step into it" },
      { keys: "←", action: "Collapse a folder, or step out to its parent" },
      { keys: "Home / End", action: "First / last row" },
      { keys: "A–Z", action: "Jump to the next row starting with that letter" },
      { keys: "⏎ / Space", action: "Open the note or file; fold the folder" },
      {
        ids: ["tree.new-note", "tree.new-folder"],
        action: "New note / new folder in the selected folder",
      },
      { ids: ["tree.rename"], action: "Rename the focused row" },
      { ids: ["delete.focused"], action: "Delete the focused row (a folder confirms first)" },
      { ids: ["menu.open"], action: "Open the row's menu — Rename, Move…, Delete" },
    ],
  },
  {
    title: "Reading and editing",
    rows: [
      { ids: ["edit.toggle"], action: "Enter or leave edit mode" },
      { ids: ["editor.save"], action: "Save now (editing autosaves anyway)" },
      { ids: ["format.bold", "format.italic"], action: "Bold / italic" },
      { ids: ["editor.table"], action: "Insert a table" },
      { ids: ["editor.paste-plain"], action: "Paste as plain text" },
      { keys: "[[", action: "Wikilink completion — ↑↓ then ⏎" },
      { ids: ["fm.save"], action: "Save the frontmatter drawer (Esc discards)" },
    ],
  },
  // Discovery gets its own group now that the right column navigates like the tree
  // (sidenav.ts): ↑↓ there means something different from ↑↓ in an open menu, and one
  // chord with two meanings in a single group is a group the reader can't trust.
  {
    title: "Discovery (the right column)",
    rows: [
      { keys: "↑ / ↓", action: "Move between section heads and cards" },
      { keys: "→ / ←", action: "Unfold / fold a section or a card's details" },
      { keys: "Home / End", action: "First / last row" },
      { keys: "⏎ / Space", action: "Open the card's note; fold a section head" },
      { ids: ["menu.open"], action: "Open a card's menu — Open note, Link…" },
    ],
  },
  {
    title: "The graph and menus",
    rows: [
      {
        ids: ["graph.activate"],
        action: "Open a graph node; a ghost opens the link palette",
      },
      { ids: ["menu.item.prev", "menu.item.next"], action: "Move through an open menu" },
      { ids: ["dismiss"], action: "Close a menu, a modal, the find bar, or the graph" },
    ],
  },
  {
    title: "The app",
    rows: [
      { ids: ["settings.toggle"], action: "Settings" },
      { ids: ["anomalies.toggle"], action: "Review the last index pass's anomalies" },
      { ids: ["help.keyboard"], action: "This table (Settings → Keyboard)" },
      { ids: ["overlay.focus.step"], action: "Step through the controls on screen" },
      // The dialogs' commit chord lives here rather than beside the graph's ⏎: two ⏎ rows
      // in one group is the ambiguity the per-group duplicate check exists to catch, and
      // this one is a sibling of Tab above it — both are how a dialog is driven.
      {
        ids: ["link.commit", "delete.confirm"],
        action: "Commit the open dialog — Link…, or a delete confirm",
      },
    ],
  },
  // The settings dialog is a tabbed surface (settingstabs.ts), and a tab rail is exactly
  // the kind of thing that ends up mouse-only if its moves aren't written down: the
  // sections are visibly *there*, so nobody thinks to look for a chord.
  {
    title: "Settings (⌘,)",
    rows: [
      { keys: "↑ / ↓", action: "Move between the sections, with the rail focused" },
      { keys: "Home / End", action: "First / last section" },
      {
        ids: ["settings.section.next", "settings.section.prev"],
        action: "Next / previous section, from anywhere in the dialog",
      },
      { ids: ["dismiss"], action: "Close Settings" },
    ],
  },
];

/** The whole sheet, in order.
 *
 *  `menu` is the app menu bar's chords, defaulting to the mirror in `menukeys.ts`;
 *  render.ts passes the **host's own** list once it has arrived, so what the reader sees
 *  is the menu the app installed rather than the UI's copy of it. Its rows are literal
 *  because there are no ids to name: an item's `label` is what the menu itself shows,
 *  which is exactly what the sheet wants to print. */
export function sheet(menu: readonly MenuChord[] = MENU_CHORDS): readonly SheetGroup[] {
  return [
    ...OWN_SHEET,
    {
      title: "The menu bar",
      rows: menu.map((c) => ({ keys: displayChord(c.keys), action: c.label })),
    },
  ];
}

/** The sheet as render.ts paints it — every row's chords resolved to display text. */
export function shortcuts(menu?: readonly MenuChord[]): ShortcutGroup[] {
  return sheet(menu).map((group) => ({
    title: group.title,
    items: group.rows.map((row) => ({
      keys: "ids" in row ? displayKeys(row.ids) : row.keys,
      action: row.action,
    })),
  }));
}
