// The keyboard reference: the single source of truth for Settings' Keyboard section.
// Pure data, no DOM, so node tests it off the source.
//
// K1 (GH #78) promises a keyboard path for every mouse action, and this table is where a
// user finds it. Rows name commands and chords are projected from the registry
// (bindings.ts), so the sheet can't drift from the wiring, and bindings.test.ts asserts
// every binding lands in some row. The prose is hand-written on purpose. Literal rows
// (`keys:`) are platform behaviour, not B2 chords. Each chord is a chip carrying the
// command it would rebind (#121).
//
// macOS only: modifiers are glyphs (⌘ ⇧ ⌫ ⏎), other keys spelled out (Esc, Tab), as
// `displayChord` renders them. Menu-bar chords (⌘Q, ⌘C, …) are not listed (#119): the menu
// bar already prints them, and this sheet is the keyboard B2 owns.
import {
  type BindingId,
  activeBindings,
  displayChord,
  displayKeys,
  findBinding,
} from "./bindings.ts";

/** One chord as the sheet paints it. */
export interface ShortcutKey {
  /** Display text — "⌘F", "↑", "Esc". */
  text: string;
  /** The command this chip would rebind, when exactly one rebindable command produced it.
   *  Absent for platform keys and for a chord two commands share. */
  id?: BindingId;
  /** Why this chord can't be changed (`Binding.fixed`), when that's why `id` is absent. */
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

/** A row of the sheet: the commands it documents, or literal text for platform keys. */
export type SheetRow =
  | { readonly ids: readonly BindingId[]; readonly action: string }
  | { readonly keys: string; readonly action: string };

export interface SheetGroup {
  readonly title: string;
  /** A chord the heading names after its title ("Settings (⌘,)"), resolved live. */
  readonly titleIds?: readonly BindingId[];
  readonly rows: readonly SheetRow[];
}

const SHEET: readonly SheetGroup[] = [
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
      // The platform's activation, not a B2 binding (the graph's ⏎ has its own row).
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
      {
        ids: ["menu.open"],
        action: "Open the row's menu — Rename, Move…, Copy vault / system path, Delete, Import files…",
      },
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
  // Own group: ↑↓ here means something different from ↑↓ in an open menu.
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
  // Own group: in chat, ⏎ sends rather than opening the focused card.
  {
    title: "Chat (flow ④)",
    rows: [
      { ids: ["chat.toggle"], action: "Ask your notes — open or close the chat pane" },
      {
        ids: ["chat.send"],
        action:
          "Ask the question (⇧⏎ for a new line); on a citation, open the note it came from",
      },
      {
        ids: ["side.row.prev", "side.row.next"],
        action: "Walk the conversation — each turn, and the citations under it",
      },
      { ids: ["dismiss"], action: "Stop the answer that's streaming, then close the pane" },
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
      { ids: ["help.keyboard"], action: "This table (Settings → Keyboard)" },
      { ids: ["overlay.focus.step"], action: "Step through the controls on screen" },
      // Here, not beside the graph's ⏎: the per-group duplicate check forbids two ⏎ rows.
      {
        ids: ["link.commit", "delete.confirm"],
        action: "Commit the open dialog — Link…, or a delete confirm",
      },
    ],
  },
  {
    title: "Settings",
    titleIds: ["settings.toggle"],
    rows: [
      {
        ids: ["settings.tab.prev", "settings.tab.next"],
        action: "Move between the sections, with the rail focused",
      },
      { ids: ["settings.tab.first", "settings.tab.last"], action: "First / last section" },
      {
        ids: ["settings.section.next", "settings.section.prev"],
        action: "Next / previous section, from anywhere in Settings",
      },
      { ids: ["dismiss"], action: "Close Settings" },
    ],
  },
];

/** The whole sheet, in order — every chord B2 dispatches, and nothing it doesn't. */
export function sheet(): readonly SheetGroup[] {
  return SHEET;
}

/**
 * The chips for a row: one per distinct rendered chord, so two commands sharing ⏎ make
 * one chip, which names no command. Aliases stay out; the row's prose covers them.
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
export function shortcuts(): ShortcutGroup[] {
  return sheet().map((group) => ({
    title: group.titleIds ? `${group.title} (${displayKeys(group.titleIds)})` : group.title,
    items: group.rows.map((row) => ({
      keys: "ids" in row ? keyChips(row.ids) : [{ text: row.keys }],
      action: row.action,
    })),
  }));
}

/** A row's chords as one string, for checks that read it as text. */
export function keyText(keys: readonly ShortcutKey[]): string {
  return keys.map((k) => k.text).join(" / ");
}
