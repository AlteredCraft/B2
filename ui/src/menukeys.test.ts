// Where B2's keyboard meets the app menu's (menukeys.ts), pinned, including the gate:
// `menuOverlaps()` is empty, since a chord the menu takes never reaches the webview. Uses
// the real @codemirror keymaps for the editor check.
import { type Binding, displayChord, keystrokes, parseChord } from "./bindings.ts";
import { editorChords } from "./editorkeys.ts";
import { MENU_CHORDS, menuDrift, menuOverlaps } from "./menukeys.ts";
import { sheet } from "./shortcuts.ts";

/** A synthetic row for tables meant to make the checker fail. */
function row(b: Omit<Binding, "label">): Binding {
  return { label: b.id, ...b };
}

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq(actual: unknown, expected: unknown, msg: string): void {
  const [a, b] = [JSON.stringify(actual, null, 1), JSON.stringify(expected, null, 1)];
  if (a !== b) throw new Error(`assertion failed: ${msg}\n  actual:   ${a}\n  expected: ${b}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- the mirror itself ----------------------------------------------------------------

check("every menu chord parses into the registry's model", () => {
  // A row the parser can't read is one the gate is blind to.
  for (const c of MENU_CHORDS) parseChord(c.keys);
});

check("menu items are unique, by id and by chord", () => {
  // One item per chord: `Menu::default` claims ⌘W twice, and menu.rs drops the duplicate.
  const ids = new Set<string>();
  const forms = new Set<string>();
  for (const c of MENU_CHORDS) {
    assert(!ids.has(c.id), `duplicate menu item id: ${c.id}`);
    assert(!forms.has(c.keys), `two menu items on ${c.keys}`);
    assert(c.label.trim() !== "", `menu item with no label: ${c.id}`);
    ids.add(c.id);
    forms.add(c.keys);
  }
});

// --- the gate -------------------------------------------------------------------------

check("no B2 chord lands on one the menu bar takes", () => {
  const found = menuOverlaps();
  assertEq(found, [], `${found.length} B2 chord(s) the menu would eat`);
});

check("the gate fails on a chord the menu already has", () => {
  // Proves the gate can fail: ⌘M is Minimize.
  const table: Binding[] = [row({ id: "pane.minimap", keys: ["Mod-m"], scope: "global" })];
  assertEq(
    menuOverlaps(table).map((o) => `${o.id} ${o.chord} → ${o.item} (${o.form})`),
    ["pane.minimap Mod-m → window.minimize (⌘m)"],
    "the clash",
  );
});

check("a menu chord is taken from every scope, not just the global one", () => {
  // Scope doesn't help against the menu: an editor-scoped ⌘Z is dead, not nearer the user.
  const table: Binding[] = [
    row({ id: "editor.history", keys: ["Mod-z"], scope: "editor" }),
    row({ id: "link.select-all", keys: ["Mod-a"], scope: "overlay:link" }),
  ];
  assertEq(
    menuOverlaps(table).map((o) => `${o.id} (${o.scope}) → ${o.item}`),
    ["editor.history (editor) → edit.undo", "link.select-all (overlay:link) → edit.select-all"],
    "scope buys nothing against the menu",
  );
});

check("an Any- chord meets every menu chord over its key", () => {
  // An `Any-` chord over a letter the menu uses would claim the menu's form too.
  const table: Binding[] = [row({ id: "panic", keys: ["Any-q"], scope: "global" })];
  assertEq(
    menuOverlaps(table).map((o) => o.form),
    ["⌘q"],
    "Any-q swallows ⌘Q",
  );
});

// --- what the menu takes from the editor ----------------------------------------------

check("the menu takes exactly these chords from CodeMirror", () => {
  // CodeMirror binds all three but never sees them: the editor's undo, redo and select-all
  // are the webview's native ones. A bug report about undo in the editor starts here.
  const stock = editorChords().map((c) => ({ ...c, forms: new Set(keystrokes(c.spec)) }));
  const taken: string[] = [];
  for (const c of MENU_CHORDS) {
    const forms = keystrokes(c.keys);
    for (const s of stock) {
      if (forms.some((f) => s.forms.has(f))) {
        taken.push(`${c.id} ${c.keys} — ${s.source}: ${s.command}`);
      }
    }
  }
  // The history pair reads "(anonymous)": CodeMirror builds them as closures.
  assertEq(
    taken,
    [
      "edit.undo Mod-z — historyKeymap: (anonymous)",
      "edit.redo Mod-Shift-z — historyKeymap: (anonymous)",
      "edit.select-all Mod-a — defaultKeymap: selectAll",
    ],
    "the chords the menu takes from the editor",
  );
});

// --- the sheet ------------------------------------------------------------------------

check("the keyboard reference leaves the menu bar's chords out", () => {
  // macOS prints these in the menu bar; the sheet lists only editable chords.
  const rows = sheet().flatMap((g) => g.rows);
  for (const c of MENU_CHORDS) {
    const shown = displayChord(c.keys);
    assert(
      !rows.some((r) => !("ids" in r) && r.keys === shown),
      `${c.id} (${shown} — ${c.label}) is the menu's, and the sheet reprints it`,
    );
    assert(
      !rows.some((r) => r.action === c.label),
      `${c.id} (${c.label}) is the menu's, and the sheet has a row for it`,
    );
  }
  assert(
    !sheet().some((g) => g.title.toLowerCase().includes("menu bar")),
    "and there is no menu-bar group left",
  );
});

// --- drift ----------------------------------------------------------------------------

check("a mirror that matches the host drifts by nothing", () => {
  assertEq(menuDrift([...MENU_CHORDS]), [], "no drift");
});

check("drift names what changed, in both directions", () => {
  // Added, removed, and changed items each report a line.
  const mirror = [
    { id: "app.quit", label: "Quit B2", keys: "Mod-q" },
    { id: "edit.copy", label: "Copy", keys: "Mod-c" },
  ];
  const host = [
    { id: "app.quit", label: "Quit B2", keys: "Mod-Shift-q" },
    { id: "view.fullscreen", label: "Toggle Full Screen", keys: "Mod-Ctrl-f" },
  ];
  assertEq(
    menuDrift(host, mirror),
    [
      "the host declares app.quit Mod-Shift-q (Quit B2); the mirror says app.quit Mod-q (Quit B2)",
      "the host declares view.fullscreen Mod-Ctrl-f (Toggle Full Screen); the mirror doesn't have it",
      "the mirror has edit.copy Mod-c (Copy); the host doesn't declare it",
    ],
    "the drift report",
  );
});

console.log(`menukeys: ${passed} checks passed`);
