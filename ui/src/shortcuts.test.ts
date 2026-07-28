// The keyboard reference's own shape (shortcuts.ts), pinned. Pure data — no DOM — so
// node runs it straight off the source: `npm test`. Dependency-free like the others.
//
// These check the *sheet as an artifact*. Since the sheet became a projection of the
// keyboard registry, one old worry is gone and a better one has replaced it. Gone: a row
// spelling a chord the wiring doesn't answer to, because rows no longer spell chords —
// they name commands, and `displayKeys` renders whatever bindings.ts says. Merely
// importing this module proves every id in it resolves, since SHORTCUTS is built at load
// and the lookup throws on a miss.
//
// The new worry is the other direction: a chord that exists and is documented *nowhere*.
// That's the K1 failure that matters (docs/design/invariants.md, GH #78) — an action
// reachable from the keyboard but discoverable only by reading main.ts. The coverage
// check below is what makes adding a binding without a row impossible.
//
// What's left is the dull editorial stuff a table like this really gets wrong: a
// half-filled row that renders as a blank line, one chord listed twice in the same group
// meaning two different things, an entry spelled "Cmd+N" among a page of ⌘N. Those are
// the drifts that make a reference stop reading as authoritative.
import { BINDINGS, allKeys } from "./bindings.ts";
import { SHEET, SHORTCUTS } from "./shortcuts.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

check("every group has a title and at least one row", () => {
  assert(SHORTCUTS.length > 0, "the sheet is not empty");
  for (const g of SHORTCUTS) {
    assert(g.title.trim() !== "", "a group with no title");
    assert(g.items.length > 0, `an empty group: ${g.title}`);
  }
});

check("every row is a full pair — a chord and what it does", () => {
  for (const g of SHORTCUTS) {
    for (const s of g.items) {
      assert(s.keys.trim() !== "", `a row with no chord under ${g.title}`);
      assert(s.action.trim() !== "", `a chord with no action: ${s.keys}`);
    }
  }
});

check("no chord is listed twice within one group", () => {
  // Across groups is fine and deliberate — ⇧F10 opens a menu on a tree row *and* on a
  // discovery card, Esc closes an overlay and closes Settings. Two meanings in one group
  // is the contradiction: the reader has no way to tell which one applies.
  for (const g of SHORTCUTS) {
    const seen = new Set<string>();
    for (const s of g.items) {
      assert(!seen.has(s.keys), `${s.keys} appears twice under ${g.title}`);
      seen.add(s.keys);
    }
  }
});

check("modifiers are written as macOS glyphs, never spelled out", () => {
  // The sheet sits beside button tooltips that render ⌘/⇧, so a stray "Cmd+N" reads as
  // a different app's documentation. Projected rows get this from `displayChord`; the
  // literal rows — the platform's own keys, the arrow families — are hand-written, and
  // are what this still guards. Only the *modifiers* are held to it: macOS spells
  // Esc / Tab / Space / Home / End out in its own menus, and so does B2's existing
  // chrome ("Close (Esc)"), so those stay words — shortcuts.ts says why.
  const spelled = /\b(cmd|command|ctrl|control|shift|alt|option|enter|return|backspace)\b/i;
  for (const g of SHORTCUTS) {
    for (const s of g.items) {
      assert(!spelled.test(s.keys), `${JSON.stringify(s.keys)} spells out a modifier (${g.title})`);
      assert(!s.keys.includes("+"), `${JSON.stringify(s.keys)} joins with "+" instead of adjacency`);
    }
  }
});

check("every chord B2 binds is documented somewhere in the sheet", () => {
  // The K1 guarantee, held by construction: you cannot add a binding without a row here.
  //
  // The one exemption is text entry, and it's a category rather than a list — ⏎ commits
  // and Esc backs out of an inline input (rename, new note, the find field). Nobody
  // needs telling, and spelling out six rows of it would bury the chords that genuinely
  // have to be found. Anything else — including a chord that only applies inside a
  // modal, or only while the find bar is open — has to earn a row.
  const documented = new Set<string>();
  for (const group of SHEET) {
    for (const row of group.rows) {
      if ("ids" in row) for (const id of row.ids) documented.add(id);
    }
  }
  for (const b of BINDINGS) {
    if (b.scope.startsWith("textentry")) continue;
    assert(documented.has(b.id), `${b.id} (${allKeys(b).join(", ")}) is bound but undocumented`);
  }
});

check("the sheet documents no command that isn't bound", () => {
  // The mirror image, and the cheap half: SHORTCUTS is built at import, and resolving a
  // row's ids throws on an unknown one, so a stale row can't survive to be rendered.
  // Asserted anyway so the failure names the sheet rather than a module-load stack.
  const ids = new Set(BINDINGS.map((b) => b.id));
  for (const group of SHEET) {
    for (const row of group.rows) {
      if (!("ids" in row)) continue;
      for (const id of row.ids) assert(ids.has(id), `${id} is documented but not bound`);
    }
  }
});

console.log(`shortcuts: ${passed} checks passed`);
