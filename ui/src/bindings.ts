// The keyboard registry: the one table of the chords B2 answers to, read by both the
// dispatcher (main.ts) and the reference sheet (shortcuts.ts), so the sheet can't fall behind
// the wiring (K1, docs/invariants.md). Pure data and functions, no DOM, so node tests it.
//
// The registry owns key → command, including the arrow families (#121); treenav.ts,
// sidenav.ts and settingstabs.ts own command → move. The app menu bar's chords are not here:
// AppKit dispatches them before the webview sees a keydown, so they are only keystrokes a
// binding may not use, and menukeys.ts gates them (#119).
//
// `DEFAULT_BINDINGS` is what ships; `activeBindings()` is the defaults with the user's
// rebindings laid over (keymap.ts). Everything downstream reads the active table.
//
// The chord syntax is CodeMirror's (`Mod-Shift-v`) because editor bindings are handed to
// `keymap.of` verbatim, so `Mod` must mean ⌘ in both (see `Chord.mod`). `Any-` is B2's one
// addition, and must never reach CodeMirror.

/** Where a chord applies. `global` is the whole window; `:` nests (`overlay:link` is inside
 *  `overlay`), which lets two ⏎ bindings coexist without ambiguity. */
export type Scope =
  | "global"
  | "editor" // CodeMirror has the keyboard (state.editing)
  | "fm" // the frontmatter drawer's mini-editor (state.fmEditing)
  | "find" // the find bar is open (findOpen)
  | "graph" // a graph node holds focus
  // The navigable panes: listeners on the pane, so they answer before `global`; siblings, so
  // ↑↓ can mean "next row" in both.
  | "tree" // the keyboard is on a file-tree row
  | "side" // the keyboard is on a discovery row
  // Overlays never stack (openers run `dismissOverlays`), so these are siblings. They nest
  // under `overlay` because the Tab trap is bound to the layer and shadows all of them.
  | "overlay"
  | "overlay:settings"
  | "overlay:menu"
  | "overlay:link"
  | "overlay:delete"
  | "textentry:create"
  | "textentry:rename"
  | "textentry:find"
  | "textentry:chat"; // the chat composer (GH #155)

/** One command and the chords that fire it. Only chord, scope and id: *when* a command
 *  applies stays ordinary code in main.ts's handler, not a `when`-clause language. A
 *  rebinding changes which keystroke fires a command, never when it applies (#121). */
export interface Binding {
  /** Stable command id. Referenced by main.ts's dispatcher and shortcuts.ts's rows. */
  readonly id: string;
  /** What this command is, in the user's words. The recorder and conflict messages name one
   *  command, which a sheet row (often covering several) can't. */
  readonly label: string;
  /** The chords that fire it, in the order the sheet shows them. */
  readonly keys: readonly string[];
  /** Chords that also fire it but the sheet doesn't list, because the row's prose mentions
   *  them. A rebinding replaces `keys` and leaves these alone; they still count as claimed,
   *  so rebinding another command onto one is refused. */
  readonly aliases?: readonly string[];
  readonly scope: Scope;
  /** Present ⇒ not rebindable, and this is the reason shown where the recorder would be.
   *  These are platform reflexes (⏎/Esc in a text field, a dialog's default button), plus
   *  the `Any-` chords, which no recorder can capture as one keystroke. */
  readonly fixed?: string;
}

// --- the table ---------------------------------------------------------------------

const TEXT_FIELD_REFLEX = "⏎ and Esc are what a text field does — B2 doesn't get to choose them.";
const DIALOG_DEFAULT = "⏎ is the open dialog's default button.";

/** The keyboard B2 ships with. The *live* one is `activeBindings()`. */
export const DEFAULT_BINDINGS = [
  // Global: matched by the document-level keydown handler.
  { id: "find.open", label: "Find in this note", keys: ["Mod-f"], scope: "global" },
  { id: "search.focus", label: "Search the vault", keys: ["Mod-Shift-f"], scope: "global" },
  { id: "tree.new-note", label: "New note", keys: ["Mod-n"], scope: "global" },
  { id: "tree.new-folder", label: "New folder", keys: ["Mod-Shift-n"], scope: "global" },
  // The Menu key is the same gesture on a keyboard that has one; only ⇧F10 is listed.
  {
    id: "menu.open",
    label: "Open the focused row's menu",
    keys: ["Shift-F10"],
    aliases: ["ContextMenu"],
    scope: "global",
  },
  { id: "tree.rename", label: "Rename the focused row", keys: ["F2"], scope: "global" },
  { id: "help.keyboard", label: "The keyboard reference", keys: ["?"], scope: "global" },
  { id: "pane.tree", label: "Focus the file tree", keys: ["Mod-1"], scope: "global" },
  { id: "pane.note", label: "Focus the note", keys: ["Mod-2"], scope: "global" },
  { id: "pane.discovery", label: "Focus discovery", keys: ["Mod-3"], scope: "global" },
  { id: "delete.focused", label: "Delete the focused row", keys: ["Mod-Backspace"], scope: "global" },
  { id: "settings.toggle", label: "Settings", keys: ["Mod-,"], scope: "global" },
  { id: "edit.toggle", label: "Enter or leave edit mode", keys: ["Mod-e"], scope: "global" },
  // ⌘E's shift-sibling: ⌘E changes whether you're editing, ⇧⌘E what you're looking at.
  {
    id: "source.toggle",
    label: "Show the Markdown source",
    keys: ["Mod-Shift-e"],
    scope: "global",
  },
  // ⌘G is Find Next only while the find bar is open (`find.next`); otherwise the graph
  // takes it. `shadows()` reports the pair and bindings.test.ts pins it as deliberate.
  {
    id: "graph.toggle",
    label: "Show or hide the connection graph",
    keys: ["Mod-g"],
    scope: "global",
  },
  // ⌘J is free in B2's, CodeMirror's, the menu bar's and macOS's keyboards; the checkers
  // assert it.
  { id: "chat.toggle", label: "Ask your notes", keys: ["Mod-j"], scope: "global" },
  // `Any-`: a modifier still held (⌘ after ⌘F) must not block the way out.
  {
    id: "dismiss",
    label: "Close a menu, a dialog, the find bar, or the graph",
    keys: ["Any-Escape"],
    scope: "global",
    fixed: "Esc is the way out of every surface, and answers with any modifier still held.",
  },
  { id: "nav.back", label: "Back", keys: ["Mod-["], aliases: ["Mod-ArrowLeft"], scope: "global" },
  {
    id: "nav.forward",
    label: "Forward",
    keys: ["Mod-]"],
    aliases: ["Mod-ArrowRight"],
    scope: "global",
  },

  // The find bar, once it's open.
  { id: "find.next", label: "Next match", keys: ["Mod-g"], scope: "find" },
  { id: "find.prev", label: "Previous match", keys: ["Mod-Shift-g"], scope: "find" },

  // The editor. All but ⌘S go to CodeMirror's keymap ahead of its defaults; ⌘S is the
  // document handler's, reachable only because CodeMirror leaves Mod-s unbound
  // (editorkeys.test.ts).
  { id: "format.bold", label: "Bold", keys: ["Mod-b"], scope: "editor" },
  { id: "format.italic", label: "Italic", keys: ["Mod-i"], scope: "editor" },
  { id: "editor.table", label: "Insert a table", keys: ["Mod-t"], scope: "editor" },
  { id: "editor.paste-plain", label: "Paste as plain text", keys: ["Mod-Shift-v"], scope: "editor" },
  { id: "editor.save", label: "Save now", keys: ["Mod-s"], scope: "editor" },
  // Tab is claimed only with the caret in a list item (list.ts declines otherwise), so it
  // still walks the focus ring elsewhere.
  { id: "editor.list.indent", label: "Nest a list item", keys: ["Tab"], scope: "editor" },
  {
    id: "editor.list.outdent",
    label: "Lift a list item out",
    keys: ["Shift-Tab"],
    scope: "editor",
  },

  // The frontmatter drawer: its own scope, since ⌘S here saves the drawer, not the note.
  {
    id: "fm.save",
    label: "Save the frontmatter drawer",
    keys: ["Mod-Enter"],
    aliases: ["Mod-s"],
    scope: "fm",
  },

  // The overlay layer. `Any-Tab`: no Tab may walk the page behind the backdrop.
  {
    id: "overlay.focus.step",
    label: "Step through the controls on screen",
    keys: ["Any-Tab"],
    scope: "overlay",
    fixed: "The focus trap has to claim Tab itself, or Tab walks the page behind the dialog.",
  },
  {
    id: "link.commit",
    label: "Commit the Link… dialog",
    keys: ["Enter"],
    scope: "overlay:link",
    fixed: DIALOG_DEFAULT,
  },
  {
    id: "delete.confirm",
    label: "Confirm the delete",
    keys: ["Enter"],
    scope: "overlay:delete",
    fixed: DIALOG_DEFAULT,
  },
  { id: "menu.item.next", label: "Next item in an open menu", keys: ["ArrowDown"], scope: "overlay:menu" },
  { id: "menu.item.prev", label: "Previous item in an open menu", keys: ["ArrowUp"], scope: "overlay:menu" },
  {
    id: "settings.section.next",
    label: "Next Settings section, from anywhere in Settings",
    keys: ["Ctrl-Tab"],
    scope: "overlay:settings",
  },
  {
    id: "settings.section.prev",
    label: "Previous Settings section, from anywhere in Settings",
    keys: ["Ctrl-Shift-Tab"],
    scope: "overlay:settings",
  },
  // The Settings rail's ARIA `tabs` walk, live only with the keyboard on a tab.
  { id: "settings.tab.prev", label: "Previous section, on the rail", keys: ["ArrowUp"], scope: "overlay:settings" },
  { id: "settings.tab.next", label: "Next section, on the rail", keys: ["ArrowDown"], scope: "overlay:settings" },
  { id: "settings.tab.first", label: "First section", keys: ["Home"], scope: "overlay:settings" },
  { id: "settings.tab.last", label: "Last section", keys: ["End"], scope: "overlay:settings" },

  // SVG has no native button activation, so the graph binds what a <button> gets for free.
  {
    id: "graph.activate",
    label: "Open the focused graph node",
    keys: ["Enter", "Space"],
    scope: "graph",
    fixed: "A graph node stands in for a button, and these are what a button answers to.",
  },

  // The file tree's and discovery's ARIA `tree` walks. Separate scopes so rebinding one
  // doesn't rebind the other.
  { id: "tree.row.prev", label: "Previous row", keys: ["ArrowUp"], scope: "tree" },
  { id: "tree.row.next", label: "Next row", keys: ["ArrowDown"], scope: "tree" },
  { id: "tree.row.first", label: "First row", keys: ["Home"], scope: "tree" },
  { id: "tree.row.last", label: "Last row", keys: ["End"], scope: "tree" },
  { id: "tree.row.in", label: "Expand a folder, or step into it", keys: ["ArrowRight"], scope: "tree" },
  { id: "tree.row.out", label: "Collapse a folder, or step out to its parent", keys: ["ArrowLeft"], scope: "tree" },
  { id: "side.row.prev", label: "Previous row", keys: ["ArrowUp"], scope: "side" },
  { id: "side.row.next", label: "Next row", keys: ["ArrowDown"], scope: "side" },
  { id: "side.row.first", label: "First row", keys: ["Home"], scope: "side" },
  { id: "side.row.last", label: "Last row", keys: ["End"], scope: "side" },
  { id: "side.row.in", label: "Unfold a section or card, or step in", keys: ["ArrowRight"], scope: "side" },
  { id: "side.row.out", label: "Fold a section or card, or step out", keys: ["ArrowLeft"], scope: "side" },

  // Text entry: ⏎ commits, Esc backs out. Exempt from the sheet (shortcuts.ts) and fixed.
  {
    id: "create.commit",
    label: "Create the new note or folder",
    keys: ["Enter"],
    scope: "textentry:create",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "create.cancel",
    label: "Back out of the new note or folder",
    keys: ["Any-Escape"],
    scope: "textentry:create",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "rename.commit",
    label: "Commit the rename",
    keys: ["Enter"],
    scope: "textentry:rename",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "rename.cancel",
    label: "Back out of the rename",
    keys: ["Any-Escape"],
    scope: "textentry:rename",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "find.input.next",
    label: "Next match, from the find field",
    keys: ["Enter"],
    scope: "textentry:find",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "find.input.prev",
    label: "Previous match, from the find field",
    keys: ["Shift-Enter"],
    scope: "textentry:find",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "find.input.close",
    label: "Close the find bar",
    keys: ["Any-Escape"],
    scope: "textentry:find",
    fixed: TEXT_FIELD_REFLEX,
  },
  // The chat composer: ⏎ asks. ⇧⏎ is the textarea's own newline, so B2 must not claim it.
  {
    id: "chat.send",
    label: "Ask the question",
    keys: ["Enter"],
    scope: "textentry:chat",
    fixed: TEXT_FIELD_REFLEX,
  },
] as const satisfies readonly Binding[];

/** Every command id in the table, as a type, so a typo in main.ts or shortcuts.ts is a
 *  compile error rather than a chord that silently stops working. */
export type BindingId = (typeof DEFAULT_BINDINGS)[number]["id"];

// --- chords ------------------------------------------------------------------------

/** A chord in normalized form: one key plus the modifiers held with it. */
export interface Chord {
  /** Canonical key name — a lowercase character, or a named key ("Enter", "ArrowUp"). */
  key: string;
  /** ⌘, and only ⌘. ⌃ is not an alias: on macOS ⌃F/⌃B/⌃N/⌃P/⌃A/⌃E are emacs motions in
   *  every text field and in CodeMirror, and aliasing put B2's chords on top of them (see
   *  editorkeys.ts). It also keeps `Mod` meaning ⌘ as it does in CodeMirror. */
  mod: boolean;
  /** ⌃ — the Settings rail's ⌃Tab is the only chord that asks for it. */
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  /** This key, whatever is held with it (`Any-Escape`). Esc and the overlay's Tab trap are
   *  about the key, not the chord: a modifier still held must not defeat either. */
  any: boolean;
}

/** The subset of KeyboardEvent this module reads, so the matcher stays testable in node. */
export interface KeyEventLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

const NAMED_KEYS = new Set([
  "Enter",
  "Escape",
  "Tab",
  "Space",
  "Backspace",
  "Delete",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ContextMenu",
]);

const isNamed = (key: string): boolean => NAMED_KEYS.has(key) || /^F\d{1,2}$/.test(key);

/** `KeyboardEvent.key` in the table's spelling: single characters lowercased (⇧F arrives
 *  as "F"), the space bar named rather than written as a literal " ". */
export function canonicalKey(raw: string): string {
  if (raw === " ") return "Space";
  return raw.length === 1 ? raw.toLowerCase() : raw;
}

/** Does ⇧ distinguish this key from itself? Yes for letters, digits and named keys. Not for
 *  a symbol, where the browser has already applied the shift (`?` is ⇧/). Exported so the
 *  recorder spells chords the same way. */
export function shiftDistinguishes(key: string): boolean {
  return /^[a-z0-9]$/.test(key) || isNamed(key);
}

/** Can a chord be built over this (already canonical) key? The parser's rule, asked ahead
 *  so the recorder can refuse "Meta", "Dead" or a media key in words. */
export function isBindableKey(key: string): boolean {
  return key.length === 1 || isNamed(key);
}

/** Parse a chord in CodeMirror's syntax (`Mod-Shift-v`, `F2`, `Ctrl-Tab`), plus `Any-`.
 *  Throws on anything unrecognized, so a typo in the table fails the suite on import rather
 *  than shipping a dead key. */
export function parseChord(spec: string): Chord {
  // `-` is also a key: never split at a trailing one ("Mod--" is ⌘-hyphen). This is
  // CodeMirror's own rule (`normalizeKeyName` splits on `/-(?!$)/`) and must stay identical.
  const parts = spec.split(/-(?!$)/);
  const raw = parts.pop();
  if (raw === undefined || raw === "") throw new Error(`chord has no key: ${spec}`);
  const chord: Chord = {
    key: canonicalKey(raw),
    mod: false,
    ctrl: false,
    shift: false,
    alt: false,
    any: false,
  };
  for (const part of parts) {
    switch (part) {
      case "Any":
        chord.any = true;
        break;
      case "Mod":
      case "Cmd":
      case "Meta":
        chord.mod = true;
        break;
      case "Ctrl":
      case "Control":
        chord.ctrl = true;
        break;
      case "Shift":
        chord.shift = true;
        break;
      case "Alt":
      case "Option":
        chord.alt = true;
        break;
      default:
        throw new Error(`unknown modifier "${part}" in chord: ${spec}`);
    }
  }
  if (chord.key.length > 1 && !isNamed(chord.key)) {
    throw new Error(`unknown key "${chord.key}" in chord: ${spec}`);
  }
  if (chord.any && (chord.mod || chord.ctrl || chord.shift || chord.alt)) {
    throw new Error(`chord combines Any with a named modifier: ${spec}`);
  }
  return chord;
}

/** Does this event press this chord? */
export function chordMatches(chord: Chord, e: KeyEventLike): boolean {
  if (chord.key !== canonicalKey(e.key)) return false;
  if (chord.any) return true;
  if (chord.alt !== e.altKey) return false;
  if (shiftDistinguishes(chord.key) && chord.shift !== e.shiftKey) return false;
  // No aliasing: a chord that doesn't ask for ⌃ must not answer to it (see `Chord.mod`).
  return chord.mod === e.metaKey && chord.ctrl === e.ctrlKey;
}

// --- the live registry ---------------------------------------------------------------
//
// One keyboard, one mutable registry, so main.ts's many `isBound` calls needn't thread a
// table. Everything that reasons about a table takes one as an argument, so tests never
// install anything globally.

let active: readonly Binding[] = DEFAULT_BINDINGS;
let byId = new Map<string, Binding>(DEFAULT_BINDINGS.map((b) => [b.id, b]));

/** The keyboard as it stands: the defaults with the user's rebindings laid over them. */
export function activeBindings(): readonly Binding[] {
  return active;
}

/** Install a table as the live one (built by keymap.ts's `applyOverrides`). */
export function setActiveBindings(next: readonly Binding[]): void {
  active = next;
  byId = new Map(next.map((b) => [b.id, b]));
}

// --- lookup ------------------------------------------------------------------------

/** Every chord that fires a binding — what the sheet shows, plus the quiet aliases. */
export function allKeys(b: Binding): readonly string[] {
  return b.aliases ? [...b.keys, ...b.aliases] : b.keys;
}

function binding(id: string): Binding {
  const b = byId.get(id);
  if (!b) throw new Error(`no such binding: ${id}`);
  return b;
}

/** The binding for a command in a given (usually candidate) table. */
export function findBinding(bindings: readonly Binding[], id: string): Binding | undefined {
  return bindings.find((b) => b.id === id);
}

/** The chord a binding is fired by, in CodeMirror's syntax, so it feeds `keymap.of`. */
export function chordFor(id: string): string {
  return binding(id).keys[0];
}

/** Does this event press the chord bound to `id`? *Whether* the command should run stays a
 *  guard beside the call in main.ts. */
export function isBound(e: KeyEventLike, id: BindingId): boolean {
  return allKeys(binding(id)).some((k) => chordMatches(parseChord(k), e));
}

/** The first of `ids` this event fires, or null. Order never breaks a real tie:
 *  `conflicts()` fails the suite first. */
export function boundOf<T extends BindingId>(e: KeyEventLike, ids: readonly T[]): T | null {
  for (const id of ids) {
    if (isBound(e, id)) return id;
  }
  return null;
}

// --- conflicts ---------------------------------------------------------------------
//
// "Conflict" is the one keyboard word for a clash, here and in menukeys.ts, editorkeys.ts
// and their tests (#120).

/** The physical key presses a chord answers to: one, except for `Any-`, which claims them
 *  all. Comparing these makes "same keystroke?" a set intersection, even against a keymap
 *  spelled by someone else's conventions (editorkeys.ts). */
export function keystrokes(spec: string): string[] {
  return physicalForms(parseChord(spec));
}

/** One chord, as one keystroke string — modifiers in Apple's order, then the key. */
function formOf(chord: Chord): string {
  return (
    (chord.ctrl ? "⌃" : "") +
    (chord.alt ? "⌥" : "") +
    (shiftDistinguishes(chord.key) && chord.shift ? "⇧" : "") +
    (chord.mod ? "⌘" : "") +
    chord.key
  );
}

function physicalForms(chord: Chord): string[] {
  if (!chord.any) return [formOf(chord)];
  // Enumerated through the same `formOf`, so `Any-` and strict forms can't drift apart.
  const out: string[] = [];
  for (const ctrl of [false, true]) {
    for (const alt of [false, true]) {
      for (const shift of shiftDistinguishes(chord.key) ? [false, true] : [false]) {
        for (const mod of [false, true]) {
          out.push(formOf({ ...chord, any: false, ctrl, alt, shift, mod }));
        }
      }
    }
  }
  return out;
}

/** `global` contains every scope; otherwise containment is the `:` namespace prefix, so
 *  `modal` contains `modal:link` and siblings contain nothing. */
export function scopeContains(outer: Scope | string, inner: Scope | string): boolean {
  return outer === inner || outer === "global" || inner.startsWith(`${outer}:`);
}

/** Two commands in the same scope that answer to the same keystroke: which runs depends on
 *  branch order, so this is an error and bindings.test.ts fails on any. Keystrokes, not
 *  chords: `Any-Escape` and `Escape` differ but meet on one key press. */
export interface Conflict {
  a: string;
  b: string;
  /** The keystroke they both answer to, e.g. "⌘f". */
  form: string;
  scope: string;
}

/** A scoped command taking a keystroke from one it sits inside (Esc in the rename field).
 *  Legal and mostly deliberate, so reported, never failed. */
export interface Shadow {
  outer: string;
  inner: string;
  form: string;
}

interface Claim {
  id: string;
  scope: string;
  form: string;
}

function claims(bindings: readonly Binding[]): Claim[] {
  const out: Claim[] = [];
  for (const b of bindings) {
    for (const spec of allKeys(b)) {
      for (const form of physicalForms(parseChord(spec))) {
        out.push({ id: b.id, scope: b.scope, form });
      }
    }
  }
  return out;
}

/** Every same-scope keystroke clash in a table. Empty for the defaults; asked of a
 *  candidate table by keymap.ts so the recorder refuses a chord before it is saved. */
export function conflicts(bindings: readonly Binding[] = activeBindings()): Conflict[] {
  const all = claims(bindings);
  const found: Conflict[] = [];
  const seen = new Set<string>();
  for (let i = 0; i < all.length; i++) {
    for (let j = i + 1; j < all.length; j++) {
      const [x, y] = [all[i], all[j]];
      if (x.id === y.id || x.form !== y.form || x.scope !== y.scope) continue;
      // One row per pair, not per overlapping keystroke.
      const key = `${x.id} ${y.id} ${x.scope}`;
      if (seen.has(key)) continue;
      seen.add(key);
      found.push({ a: x.id, b: y.id, form: x.form, scope: x.scope });
    }
  }
  return found;
}

/** Every keystroke an inner scope takes from an outer one. Advisory — see `Shadow`. */
export function shadows(bindings: readonly Binding[] = activeBindings()): Shadow[] {
  const all = claims(bindings);
  const found: Shadow[] = [];
  const seen = new Set<string>();
  for (const x of all) {
    for (const y of all) {
      if (x.id === y.id || x.form !== y.form) continue;
      if (x.scope === y.scope || !scopeContains(x.scope, y.scope)) continue;
      const key = `${x.id} ${y.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      found.push({ outer: x.id, inner: y.id, form: x.form });
    }
  }
  return found;
}

// --- display -----------------------------------------------------------------------

// macOS menus' split: glyphs for most keys, but Esc and Tab stay words.
const KEY_GLYPHS: Record<string, string> = {
  Enter: "⏎",
  Backspace: "⌫",
  Delete: "⌦",
  Escape: "Esc",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  ContextMenu: "Menu",
};

/** One chord as the sheet prints it: ⌃⌥⇧⌘ then the key (Apple's HIG order). */
export function displayChord(spec: string): string {
  const c = parseChord(spec);
  // An `Any-` chord prints bare: "Esc", not a list of the twelve ways to hold it.
  const mods = c.any
    ? ""
    : (c.ctrl ? "⌃" : "") +
      (c.alt ? "⌥" : "") +
      (shiftDistinguishes(c.key) && c.shift ? "⇧" : "") +
      (c.mod ? "⌘" : "");
  const key = KEY_GLYPHS[c.key] ?? (c.key.length === 1 ? c.key.toUpperCase() : c.key);
  return `${mods}${key}`;
}

/** The chords for a row of the sheet, as one cell: "⌘G / ⇧⌘G". Distinct renderings only,
 *  so two commands sharing ⏎ read "⏎". `sep` is for tight spots like a tooltip's "↑/↓". */
export function displayKeys(ids: readonly string[], sep = " / "): string {
  const out: string[] = [];
  for (const id of ids) {
    for (const spec of binding(id).keys) {
      const text = displayChord(spec);
      if (!out.includes(text)) out.push(text);
    }
  }
  return out.join(sep);
}
