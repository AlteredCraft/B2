// The keyboard reference's shape (shortcuts.ts), off the DOM. Building the sheet already
// proves every id resolves; these add coverage (every binding has a row, K1, GH #78) and
// the editorial drifts: blank rows, a chord twice in one group, "Cmd+N" among ⌘N.
import { type Binding, DEFAULT_BINDINGS, allKeys } from "./bindings.ts";
import { keyText, sheet, shortcuts } from "./shortcuts.ts";

/** The shipped table widened to read `.fixed` (see bindings.test.ts). */
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
  // render.ts paints a chip with an `id` as a rebind button, everything else as <kbd>.
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
  // Platform rows carry no id.
  const platform = shortcuts()
    .flatMap((g) => g.items)
    .filter((s) => s.action.startsWith("Jump to the next row"));
  assert(platform.length === 1, "the typeahead row is still there");
  assert(platform[0].keys.every((k) => k.id === undefined), "and it is not editable");
});

check("no chord is listed twice within one group", () => {
  // Across groups is fine (⇧F10 opens a menu on a tree row and on a card).
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
  // Guards the hand-written literal rows. Esc / Tab / Space stay words (shortcuts.ts).
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
  // K1 by construction. Text entry (⏎ commits, Esc backs out of an inline input) is the
  // one exempt category.
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
  // Building the sheet already throws on this; asserted so the failure names the row.
  const ids = new Set(DEFAULT_BINDINGS.map((b) => b.id));
  for (const group of sheet()) {
    for (const row of group.rows) {
      if (!("ids" in row)) continue;
      for (const id of row.ids) assert(ids.has(id), `${id} is documented but not bound`);
    }
  }
});

console.log(`shortcuts: ${passed} checks passed`);
