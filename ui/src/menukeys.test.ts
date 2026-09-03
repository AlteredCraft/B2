// Where B2's keyboard meets the app menu's (menukeys.ts), pinned — and the reserved-chord
// gate itself.
//
// The gate is `menuOverlaps()` being empty: no B2 binding may be spelled with a chord the
// menu bar already takes. It matters more than the ordinary conflict check, because it is
// the one clash a user reports as "that shortcut does nothing" — the keystroke never
// reaches the webview, so no handler runs, nothing logs, and there is no ordering trick
// that would let B2 have it back. Before #119 nothing could see these chords at all: they
// came from Tauri's `Menu::default()`, which the app inherited without enumerating.
//
// It imports the real @codemirror keymaps for the last check, the way editorkeys.test.ts
// does — a claim about what the menu takes *from the editor* is worthless against a
// hand-written guess at what CodeMirror binds.
import { type Binding, displayChord, keystrokes, parseChord } from "./bindings.ts";
import { editorChords } from "./editorkeys.ts";
import { MENU_CHORDS, menuDrift, menuOverlaps } from "./menukeys.ts";
import { sheet } from "./shortcuts.ts";

/** A synthetic row. These tables exist to make the checker *fail*, so the label carries
 *  no meaning here — it is required by the type and named after the id to stay honest. */
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
  // The host spells these in the registry's syntax precisely so this works; a row it
  // can't read is a row the gate below is blind to, and would pass by being ignored.
  for (const c of MENU_CHORDS) parseChord(c.keys);
});

check("menu items are unique, by id and by chord", () => {
  // The chord half is what keeps the sheet honest: it prints one action per chord, so two
  // items claiming ⌘W (which `Menu::default` does — Close Window in both File and Window)
  // would print two rows the reader can't choose between. menu.rs drops that duplicate.
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
  // The check above proves nothing on its own — this is what proves it can fail. ⌘M is
  // Minimize; a B2 command spelled that way would simply never run.
  const table: Binding[] = [row({ id: "pane.minimap", keys: ["Mod-m"], scope: "global" })];
  assertEq(
    menuOverlaps(table).map((o) => `${o.id} ${o.chord} → ${o.item} (${o.form})`),
    ["pane.minimap Mod-m → window.minimize (⌘m)"],
    "the clash",
  );
});

check("a menu chord is taken from every scope, not just the global one", () => {
  // The reason this is its own function rather than more rows in the registry run through
  // `conflicts()`. Scope is how B2 lets an inner surface answer first — the rename field's
  // Esc before the overlay cascade, the Settings rail's ⌃Tab before the Tab trap — and it
  // is exactly the move that does *not* work here: AppKit dispatches a menu key equivalent
  // inside `NSApplication.sendEvent`, before the key window's responder chain, so the
  // webview is never asked. An editor-scoped ⌘Z is not "nearer the user"; it is dead.
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
  // `Any-Escape` claims every way of holding Escape, so an `Any-` chord over a letter the
  // menu uses would claim the menu's form too. Nothing binds one today; this is what
  // notices if something does.
  const table: Binding[] = [row({ id: "panic", keys: ["Any-q"], scope: "global" })];
  assertEq(
    menuOverlaps(table).map((o) => o.form),
    ["⌘q"],
    "Any-q swallows ⌘Q",
  );
});

// --- what the menu takes from the editor ----------------------------------------------

check("the menu takes exactly these chords from CodeMirror", () => {
  // Not a B2-vs-menu clash — a menu-vs-dependency one, and the reason it's worth pinning
  // is that it is invisible from inside the editor. CodeMirror binds all three; none of
  // them ever reaches it, because the menu item is dispatched first. So the note editor's
  // undo, redo and select-all are the *webview's* native ones, not CodeMirror's history
  // and selection commands, and a bug report about undo behaving oddly in the editor
  // starts here rather than in @codemirror/commands.
  //
  // This is what "you can't document what you can't enumerate" cost: the fact was true
  // before #119 too, and there was nowhere to write it down.
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
  // The history pair reads "(anonymous)" because CodeMirror builds those two commands as
  // closures; the keymap they came from is what names them, which is why the row carries
  // it — the same shape editorkeys.test.ts pins its overlaps in.
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
  // The sheet listed them once (#119, on K1's "a chord live in the app is B2's to
  // document"). It doesn't any more: macOS prints ⌘Q beside Quit in the menu bar itself,
  // and a row in a table of *editable* chords that nothing here can edit is worse than no
  // row. This is the assertion that keeps the group from drifting back in — the mirror is
  // still read, by the conflict gate above and by the recorder, just never printed.
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
  // The three ways menu.rs and menukeys.ts come apart: an item added, an item removed, an
  // item whose chord or label moved. Each line is meant to be read in a console by someone
  // who has just edited one of the two files.
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
