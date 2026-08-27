// The keyboard reference's own shape (shortcuts.ts), pinned. Pure data — no DOM — so
// node runs it straight off the source: `npm test`. Dependency-free like the others.
//
// These check the *sheet as an artifact*. Since the sheet became a projection of the
// keyboard registry, one old worry is gone and a better one has replaced it. Gone: a row
// spelling a chord the wiring doesn't answer to, because rows no longer spell chords —
// they name commands, and `displayKeys` renders whatever bindings.ts says. Merely
// calling `shortcuts()` proves every id in it resolves, since building a row resolves its
// ids and the lookup throws on a miss.
//
// The new worry is the other direction: a chord that exists and is documented *nowhere*.
// That's the K1 failure that matters (docs/invariants.md, GH #78) — an action
// reachable from the keyboard but discoverable only by reading main.ts. The coverage
// check below is what makes adding a binding without a row impossible.
//
// What's left is the dull editorial stuff a table like this really gets wrong: a
// half-filled row that renders as a blank line, one chord listed twice in the same group
// meaning two different things, an entry spelled "Cmd+N" among a page of ⌘N. Those are
// the drifts that make a reference stop reading as authoritative.
import { type Binding, DEFAULT_BINDINGS, allKeys } from "./bindings.ts";
import { keyText, sheet, shortcuts } from "./shortcuts.ts";

/** The shipped table under its declared type — see bindings.test.ts for why the widening
 *  is needed to read `.fixed`. */
const SHIPPED: readonly Binding[] = DEFAULT_BINDINGS;

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
  assert(shortcuts().length > 0, "the sheet is not empty");
  for (const g of shortcuts()) {
    assert(g.title.trim() !== "", "a group with no title");
    assert(g.items.length > 0, `an empty group: ${g.title}`);
  }
});

check("every row is a full pair — a chord and what it does", () => {
  for (const g of shortcuts()) {
    for (const s of g.items) {
      assert(s.keys.length > 0, `a row with no chord under ${g.title}`);
      for (const k of s.keys) assert(k.text.trim() !== "", `an empty chip under ${g.title}`);
      assert(s.action.trim() !== "", `a chord with no action: ${keyText(s.keys)}`);
    }
  }
});

check("a chip is editable exactly when it names one movable B2 command", () => {
  // The affordance is the contract: render.ts paints a chip with an `id` as a button and
  // everything else as a <kbd>, so a chip that carried an id it shouldn't would offer to
  // rebind the platform's own ⏎, and one that dropped an id it should have would make a
  // chord unreachable from the only surface that edits chords.
  const fixed = new Set(SHIPPED.filter((b) => b.fixed !== undefined).map((b) => b.id));
  for (const g of shortcuts()) {
    for (const s of g.items) {
      for (const k of s.keys) {
        if (k.id === undefined) continue;
        assert(!fixed.has(k.id), `${k.text} offers to rebind ${k.id}, which is fixed`);
        assert(k.fixed === undefined, `${k.text} is both editable and fixed`);
      }
    }
  }
  // The platform's own rows — Tab, ⏎ on a focused button, first-letter typeahead — carry
  // no id at all, since there is no B2 command behind them to move.
  const platform = shortcuts()
    .flatMap((g) => g.items)
    .filter((s) => s.action.startsWith("Jump to the next row"));
  assert(platform.length === 1, "the typeahead row is still there");
  assert(platform[0].keys.every((k) => k.id === undefined), "and it is not editable");
});

check("no chord is listed twice within one group", () => {
  // Across groups is fine and deliberate — ⇧F10 opens a menu on a tree row *and* on a
  // discovery card, Esc closes an overlay and closes Settings. Two meanings in one group
  // is the contradiction: the reader has no way to tell which one applies.
  for (const g of shortcuts()) {
    const seen = new Set<string>();
    for (const s of g.items) {
      const text = keyText(s.keys);
      assert(!seen.has(text), `${text} appears twice under ${g.title}`);
      seen.add(text);
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
  for (const g of shortcuts()) {
    for (const s of g.items) {
      const text = keyText(s.keys);
      assert(!spelled.test(text), `${JSON.stringify(text)} spells out a modifier (${g.title})`);
      assert(!text.includes("+"), `${JSON.stringify(text)} joins with "+" instead of adjacency`);
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
  for (const group of sheet()) {
    for (const row of group.rows) {
      if ("ids" in row) for (const id of row.ids) documented.add(id);
    }
  }
  for (const b of DEFAULT_BINDINGS) {
    if (b.scope.startsWith("textentry")) continue;
    assert(documented.has(b.id), `${b.id} (${allKeys(b).join(", ")}) is bound but undocumented`);
  }
});

check("the sheet documents no command that isn't bound", () => {
  // The mirror image, and the cheap half: every check above builds the sheet, and
  // resolving a row's ids throws on an unknown one, so a stale row can't survive a paint.
  // Asserted anyway so the failure names the sheet rather than a module-load stack.
  const ids = new Set(DEFAULT_BINDINGS.map((b) => b.id));
  for (const group of sheet()) {
    for (const row of group.rows) {
      if (!("ids" in row)) continue;
      for (const id of row.ids) assert(ids.has(id), `${id} is documented but not bound`);
    }
  }
});

console.log(`shortcuts: ${passed} checks passed`);
