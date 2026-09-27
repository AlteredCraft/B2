// Where B2's keyboard meets CodeMirror's (editorkeys.ts), pinned against the real
// @codemirror keymaps: an upgrade that binds Mod-e would otherwise break ⌘E silently.
import { DEFAULT_BINDINGS, chordFor, keystrokes, parseChord } from "./bindings.ts";
import {
  STOCK_EDITOR_KEYMAP,
  STOCK_KEYMAPS,
  editorChords,
  editorOverlaps,
} from "./editorkeys.ts";

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

check("every stock chord parses into the registry's model", () => {
  // A chord this can't read is one the overlap check is blind to.
  const chords = editorChords();
  assert(chords.length > 50, `only ${chords.length} stock chords — did a keymap go missing?`);
  for (const c of chords) parseChord(c.spec);
});

check("the keymap main.ts installs is the keymap this module compares against", () => {
  // Everything main.ts spreads must be in what the overlap check reads.
  const declared = new Set(STOCK_KEYMAPS.flatMap((k) => k.keymap));
  for (const b of STOCK_EDITOR_KEYMAP) {
    assert(declared.has(b), `a binding in STOCK_EDITOR_KEYMAP that STOCK_KEYMAPS doesn't list`);
  }
});

check("the chords B2 needs to reach it while editing are unbound by CodeMirror", () => {
  // These handlers have no edit-mode guard: they rely on CodeMirror declining first.
  const mustBubble: [string, string][] = [
    ["edit.toggle", "⌘E is how you leave edit mode; bound here, you'd be stuck in it"],
    ["editor.save", "⌘S is the explicit flush, and only ever pressed while editing"],
    ["find.open", "⌘F opens find-in-note over the buffer you're editing"],
    ["search.focus", "⇧⌘F jumps to vault search from anywhere, editor included"],
    ["find.next", "⌘G steps matches while the bar is up over the editor"],
    ["find.prev", "⇧⌘G, the same"],
    ["settings.toggle", "⌘, is the macOS Preferences reflex, live everywhere"],
    ["tree.new-note", "⌘N creates a note without leaving the one you're writing"],
    ["pane.tree", "⌘1 is how the keyboard gets *out* of the editor"],
  ];
  const taken = editorOverlaps();
  for (const [id, why] of mustBubble) {
    const hit = taken.find((o) => o.id === id);
    assert(!hit, `CodeMirror now binds ${hit?.chord} to ${hit?.command} — ${why}`);
  }
});

check("B2 and CodeMirror overlap on exactly these chords", () => {
  // Each row resolves differently:
  //  - ⌘⌫  CodeMirror keeps it; B2's ⌘⌫ guards on `state.editing || inTextEntry()`.
  //  - Esc  CodeMirror's completion and multi-selection handlers decline when idle, so
  //        Escape falls through to B2's overlay cascade; decided per keystroke.
  //  - ⌘[ ⌘] ⌘← ⌘→  CodeMirror keeps them; B2's history chords guard on `!state.editing`.
  //  - ⌘I   B2 wins by install order (format chords go into `keymap.of` first).
  // A row appearing or vanishing should be looked at, not re-pinned reflexively.
  assertEq(
    editorOverlaps().map((o) => `${o.id} ${o.chord} — ${o.source}: ${o.command}`),
    [
      "delete.focused Mod-Backspace — defaultKeymap: deleteLineBoundaryBackward",
      "dismiss Any-Escape — defaultKeymap: simplifySelection",
      "dismiss Any-Escape — completionKeymap: closeCompletion",
      "nav.back Mod-[ — defaultKeymap: indentLess",
      "nav.back Mod-ArrowLeft — defaultKeymap: cursorLineBoundaryLeft",
      "nav.forward Mod-] — defaultKeymap: indentMore",
      "nav.forward Mod-ArrowRight — defaultKeymap: cursorLineBoundaryRight",
      "format.italic Mod-i — defaultKeymap: selectParentSyntax",
    ],
    "the overlap set",
  );
});

check("B2 and the editor never meet on a ⌃ keystroke — the emacs bindings are CodeMirror's", () => {
  // A CodeMirror binding calls `preventDefault` without `stopPropagation`, so a B2 chord on
  // the editor's ⌃ emacs bindings would run both commands (⌃E: end-of-line and leave edit mode).
  // bindings.test.ts holds the matcher's end: no chord claims ⌃ without asking.
  const onControl = editorOverlaps().filter((o) => o.shared.some((f) => f.startsWith("⌃")));
  assertEq(
    onControl.map((o) => `${o.id} ${o.chord} [${o.shared.join(" ")}] — ${o.command}`),
    [],
    "B2 chords meeting CodeMirror on ⌃",
  );
});

check("the editor's own B2 chords are the ones installed ahead of the stock keymap", () => {
  // Only scope `editor` is compared against CodeMirror's keyboard.
  const editorIds = DEFAULT_BINDINGS.filter((b) => b.scope === "editor").map((b) => b.id);
  assertEq(
    editorIds,
    [
      "format.bold",
      "format.italic",
      "editor.table",
      "editor.paste-plain",
      "editor.save",
      "editor.list.indent",
      "editor.list.outdent",
    ],
    "the editor's chords",
  );
});

check("Tab reaches the editor's list commands — nothing in the editor binds it first", () => {
  // `editor.list.indent` assumes no stock Tab binding: `markdownKeymap` sits above B2's
  // chords, and `indentWithTab` in `defaultKeymap` would take Tab silently.
  const onTab = editorChords().filter((c) => keystrokes(c.spec).some((f) => f.endsWith("Tab")));
  assertEq(
    onTab.map((c) => `${c.spec} — ${c.source} ${c.command}`),
    [],
    "stock chords over Tab",
  );
});

check("chords handed to CodeMirror are ones CodeMirror can parse", () => {
  // Registry chords go straight into `keymap.of`, but `Any-` is B2's own: CodeMirror would
  // bind a modifier named "Any" that nothing presses.
  const installed = [
    "format.bold",
    "format.italic",
    "editor.table",
    "editor.paste-plain",
    "editor.list.indent",
    "editor.list.outdent",
  ];
  for (const id of installed) {
    const spec = chordFor(id);
    parseChord(spec); // our side
    assert(!spec.startsWith("Any-"), `${id} is installed in CodeMirror but spelled ${spec}`);
  }
});

console.log(`editorkeys: ${passed} checks passed`);
