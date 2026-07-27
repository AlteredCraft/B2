// Where B2's keyboard meets CodeMirror's (editorkeys.ts), pinned.
//
// Not dependency-free, deliberately — this imports the real @codemirror keymaps, the way
// paste.test.ts exercises the real turndown. A check that B2's chords survive contact
// with CodeMirror is worthless against a hand-written copy of what CodeMirror binds; the
// whole point is to read the dependency that actually ships.
//
// What it's guarding. Several of B2's chords work while the editor has the keyboard only
// because CodeMirror leaves them alone: ⌘E to leave edit mode, ⌘S to flush, ⌘F to open
// find. That's an assumption about a dependency, and until now it lived as a parenthesis
// in a comment ("CodeMirror leaves Mod-e unbound, so the event bubbles here"). An upgrade
// that binds Mod-e would break ⌘E in the one path a user hits constantly — no error, no
// failing test, just a chord that stopped working. So the assumption is a list now, and
// the list is asserted.
import { BINDINGS, chordFor, parseChord } from "./bindings.ts";
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
  // The comparison is only as good as its coverage: a chord this can't read is a chord
  // the overlap check is blind to. CodeMirror's key syntax is the one bindings.ts adopted,
  // so this should hold — and if a release introduces a spelling it doesn't, the whole
  // check quietly stops meaning anything, which is why it's asserted rather than assumed.
  const chords = editorChords();
  assert(chords.length > 50, `only ${chords.length} stock chords — did a keymap go missing?`);
  for (const c of chords) parseChord(c.spec);
});

check("the keymap main.ts installs is the keymap this module compares against", () => {
  // STOCK_EDITOR_KEYMAP is what main.ts spreads; STOCK_KEYMAPS is what the overlap check
  // reads. Adding a keymap to the first and forgetting the second would leave the editor
  // with bindings nothing checks — the failure mode this module exists to prevent,
  // reintroduced one level up.
  const declared = new Set(STOCK_KEYMAPS.flatMap((k) => k.keymap));
  for (const b of STOCK_EDITOR_KEYMAP) {
    assert(declared.has(b), `a binding in STOCK_EDITOR_KEYMAP that STOCK_KEYMAPS doesn't list`);
  }
});

check("the chords B2 needs to reach it while editing are unbound by CodeMirror", () => {
  // The assertion with teeth. Each of these is a chord whose handler has no edit-mode
  // guard — it is *expected* to fire with the keyboard in the editor, and it can only do
  // that if CodeMirror declines to handle it first.
  const mustBubble: [string, string][] = [
    ["edit.toggle", "⌘E is how you leave edit mode; bound here, you'd be stuck in it"],
    ["editor.save", "⌘S is the explicit flush, and only ever pressed while editing"],
    ["find.open", "⌘F opens find-in-note over the buffer you're editing"],
    ["search.focus", "⇧⌘F jumps to vault search from anywhere, editor included"],
    ["find.next", "⌘G steps matches while the bar is up over the editor"],
    ["find.prev", "⇧⌘G, the same"],
    ["anomalies.toggle", "⇧⌘A opens a modal over the pane; it never touches the buffer"],
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
  // Every row is deliberate, and every row is a different resolution. Read as: B2's
  // chord, the chord, the stock keymap that also claims it, the command it runs.
  //
  //  - ⌘⌫  CodeMirror keeps it. Deleting to the line start is the platform's meaning
  //        inside text, so B2's own ⌘⌫ (delete the focused row) guards itself off with
  //        `state.editing || inTextEntry()` rather than competing.
  //  - Esc  CodeMirror gets first refusal twice over — closing an open completion menu,
  //        then collapsing a multi-selection. Both *decline* when there's nothing to do,
  //        which is what lets Escape fall through to B2's overlay cascade the rest of
  //        the time. This is the one row where "who wins" is decided per keystroke.
  //  - ⌘[ ⌘] and ⌘← ⌘→  CodeMirror keeps all four: indent, and caret-to-line-edge. B2's
  //        history chords guard on `!state.editing` (and the arrows additionally on
  //        `inTextEntry()`) precisely because these are the editor's first.
  //  - ⌘I   B2 wins, by install order — the format chords go into `keymap.of` ahead of
  //        the stock ones, so italic shadows selectParentSyntax. That ordering is load-
  //        bearing, and this row is what notices if it's ever shuffled.
  //
  // A new row appearing here means an upgrade took a chord. A row vanishing means B2 can
  // stop guarding against something. Either way it should be looked at, not re-pinned
  // reflexively.
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
  // The check that found the bug this file exists for, kept as the guard that it stays
  // fixed. B2's matcher used to read `(e.metaKey || e.ctrlKey)`, so every ⌘ chord answered
  // to ⌃ as well. On macOS that lands on the system's emacs text bindings — which
  // CodeMirror implements — and since a CodeMirror binding calls `preventDefault` without
  // `stopPropagation`, the event still reached the document handler. One keystroke ran
  // both commands: ⌃E moved the caret to end-of-line *and* left edit mode; ⌃F moved it
  // forward a character *and* opened the find bar. Six chords did this (⌃E ⌃F ⌃⇧F ⌃N ⌃⇧N
  // ⌃⇧A), and two more would have but for an unrelated `!state.editing` guard.
  //
  // bindings.test.ts holds the matcher's end of this — no chord claims ⌃ without asking
  // for it. This is the other end: whatever B2 binds, it doesn't land on the editor's ⌃.
  const onControl = editorOverlaps().filter((o) => o.shared.some((f) => f.startsWith("⌃")));
  assertEq(
    onControl.map((o) => `${o.id} ${o.chord} [${o.shared.join(" ")}] — ${o.command}`),
    [],
    "B2 chords meeting CodeMirror on ⌃",
  );
});

check("the editor's own B2 chords are the ones installed ahead of the stock keymap", () => {
  // ⌘B / ⌘I / ⌘T / ⇧⌘V go into the editor's keymap; ⌘S is the document handler's. All
  // five are scope `editor`, which is what makes the overlap check consider them at all
  // — a chord filed under the wrong scope would be compared against the wrong keyboard.
  const editorIds = BINDINGS.filter((b) => b.scope === "editor").map((b) => b.id);
  assertEq(
    editorIds,
    ["format.bold", "format.italic", "editor.table", "editor.paste-plain", "editor.save"],
    "the editor's chords",
  );
});

check("chords handed to CodeMirror are ones CodeMirror can parse", () => {
  // `chordFor` gives main.ts the registry's spelling and it goes straight into
  // `keymap.of` — which works because the syntax is CodeMirror's, with one exception.
  // `Any-` is B2's own, and CodeMirror would read it as a modifier named "Any" and bind
  // a chord nothing presses. Nothing installed in the editor uses it today; this is what
  // notices if that changes, since the failure is otherwise a chord that silently
  // stops working.
  const installed = ["format.bold", "format.italic", "editor.table", "editor.paste-plain"];
  for (const id of installed) {
    const spec = chordFor(id);
    parseChord(spec); // our side
    assert(!spec.startsWith("Any-"), `${id} is installed in CodeMirror but spelled ${spec}`);
  }
});

console.log(`editorkeys: ${passed} checks passed`);
