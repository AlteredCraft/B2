// The keyboard reference's own shape (shortcuts.ts), pinned. Pure data — no DOM — so
// node runs it straight off the source: `npm test`. Dependency-free like the others.
//
// These check the *sheet as an artifact*, not the wiring: whether a given chord is
// actually bound lives in main.ts's keydown handler, which pulls in CodeMirror and the
// stylesheet and so can't be imported here. What a table like this really gets wrong is
// smaller and duller — a half-filled row that renders as a blank line, one chord listed
// twice in the same group meaning two different things, or an entry spelled "Cmd+N"
// among a page of ⌘N. Those are exactly the drifts that make a reference stop reading
// as authoritative, and they're what's asserted below (K1, GH #78).
import { SHORTCUTS } from "./shortcuts.ts";

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
  // discovery card, ⏎ activates whatever is focused. Two meanings in one group is the
  // contradiction: the reader has no way to tell which one applies.
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
  // a different app's documentation. Only the *modifiers* are held to this: macOS spells
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

console.log(`shortcuts: ${passed} checks passed`);
