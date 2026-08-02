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
// the platform's own behavior — Tab, ⏎ on a focused button, first-letter typeahead. The
// arrow families used to be literal rows too, because their key → move mapping belonged
// to a pure module of its own; #121 moved the *key* half into the registry (bindings.ts's
// header says why), so they are ordinary id rows now and the recorder can reach them.
//
// Since #121 a row's chords are **chips, not a string**: one per distinct chord, each
// carrying the command it would rebind, because Settings' Keyboard section is now the
// surface that edits this table as well as the one that prints it. A row covering three
// commands ("Focus the files, the note, or discovery") is three chips, and each one knows
// which of the three it belongs to — which is why `Binding.label` exists.
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
import { type BindingId, activeBindings, displayChord, findBinding } from "./bindings.ts";
import { MENU_CHORDS } from "./menukeys.ts";
import type { MenuChord } from "./types.ts";

/** One chord as the sheet paints it. */
export interface ShortcutKey {
  /** Display text — "⌘F", "↑", "Esc". */
  text: string;
  /** The command this chip would rebind, when exactly one B2 command produced it and it
   *  is the registry's to move. Absent for the platform's own keys, for the menu bar's,
   *  and for the rare chord two commands print identically — a chip that can't say *which*
   *  command it edits must not offer to edit one. */
  id?: BindingId;
  /** Why this chord can't be changed (`Binding.fixed`), when that's the reason `id` is
   *  absent. The recorder shows it in place of itself. */
  fixed?: string;
}

/** One row: the chords, and what they do. */
export interface Shortcut {
  keys: ShortcutKey[];
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
      { ids: ["tree.row.prev", "tree.row.next"], action: "Move between rows" },
      { ids: ["tree.row.in"], action: "Expand a folder, or step into it" },
      { ids: ["tree.row.out"], action: "Collapse a folder, or step out to its parent" },
      { ids: ["tree.row.first", "tree.row.last"], action: "First / last row" },
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
      {
        ids: ["source.toggle"],
        action: "Show the Markdown source, or go back to the rendered note",
      },
      { ids: ["editor.save"], action: "Save now (editing autosaves anyway)" },
      { ids: ["format.bold", "format.italic"], action: "Bold / italic" },
      {
        ids: ["editor.list.indent", "editor.list.outdent"],
        action: "Nest / lift out the list item under the cursor (elsewhere, Tab moves on)",
      },
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
      { ids: ["side.row.prev", "side.row.next"], action: "Move between section heads and cards" },
      {
        ids: ["side.row.in", "side.row.out"],
        action: "Unfold / fold a section or a card's details",
      },
      { ids: ["side.row.first", "side.row.last"], action: "First / last row" },
      { keys: "⏎ / Space", action: "Open the card's note; fold a section head" },
      { ids: ["menu.open"], action: "Open a card's menu — Open note, Link…" },
    ],
  },
  {
    title: "The graph and menus",
    rows: [
      {
        ids: ["graph.toggle"],
        action: "Show the open note's connection graph, or go back to reading",
      },
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
      {
        ids: ["settings.tab.prev", "settings.tab.next"],
        action: "Move between the sections, with the rail focused",
      },
      { ids: ["settings.tab.first", "settings.tab.last"], action: "First / last section" },
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

/**
 * The chips for a row of the sheet: one per distinct chord the row's commands answer to.
 *
 * Distinct *renderings*, so a row covering two commands that share a chord — ⏎ commits
 * the link dialog and the delete confirm — is one chip rather than "⏎ / ⏎". That chip
 * names no command, because it can't say which of the two you'd be rebinding; it still
 * carries a `fixed` reason when every command behind it has one, which is the case
 * wherever this actually comes up.
 *
 * Aliases stay out, as they always have: the row's prose covers them in words, and a chip
 * per way in would turn "Back / forward" into four keys to read past.
 */
export function keyChips(ids: readonly BindingId[]): ShortcutKey[] {
  const order: string[] = [];
  const behind = new Map<string, BindingId[]>();
  const table = activeBindings();
  for (const id of ids) {
    const b = findBinding(table, id);
    if (!b) throw new Error(`no such binding: ${id}`);
    for (const spec of b.keys) {
      const text = displayChord(spec);
      if (!behind.has(text)) {
        behind.set(text, []);
        order.push(text);
      }
      behind.get(text)?.push(id);
    }
  }
  return order.map((text) => {
    const owners = behind.get(text) ?? [];
    const unique = new Set(owners).size === 1 ? owners[0] : null;
    const fixed = owners.map((id) => findBinding(table, id)?.fixed);
    const allFixed = fixed.every((f) => f !== undefined);
    return {
      text,
      ...(unique !== null && !allFixed ? { id: unique } : {}),
      ...(allFixed && fixed[0] ? { fixed: fixed[0] } : {}),
    };
  });
}

/** The sheet as render.ts paints it — every row's chords resolved to chips. */
export function shortcuts(menu?: readonly MenuChord[]): ShortcutGroup[] {
  return sheet(menu).map((group) => ({
    title: group.title,
    items: group.rows.map((row) => ({
      keys: "ids" in row ? keyChips(row.ids) : [{ text: row.keys }],
      action: row.action,
    })),
  }));
}

/** A row's chords as one string — what the sheet used to be, kept for the checks that
 *  read it as text (and for any caller that wants the old one-cell rendering). */
export function keyText(keys: readonly ShortcutKey[]): string {
  return keys.map((k) => k.text).join(" / ");
}
