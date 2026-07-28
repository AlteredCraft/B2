// The customization layer (keymap.ts), pinned. Runs off the source under node
// (`npm test`); the two `localStorage` functions are exercised for the one behaviour that
// matters without a browser — that storage being *absent* yields the shipped keyboard
// rather than a thrown boot.
//
// What a rebinding layer actually gets wrong is not the algebra. It is the judgement:
// letting a user pick ⌘W and then wonder why the window closed, letting two commands land
// on one keystroke so which fires depends on branch order in a file the user has never
// seen, or "helpfully" refusing something legal like ⌘I that the editor also binds. So the
// bulk of this file is `chordProblems` — the tier each of the four checkers reports at,
// and, as importantly, the cases that report nothing.
//
// The second half is `adoptOverrides`, and it exists because the store is a file a human
// can edit. The recorder refuses a bad chord at record time; nothing stops someone hand-
// editing `b2:keymap` into a keyboard where two commands answer to ⌘F — and a keyboard
// that broken is one you can't use to fix itself. So the loader re-judges what it reads.
import { type Binding, DEFAULT_BINDINGS, activeBindings, conflicts } from "./bindings.ts";
import {
  type Overrides,
  adoptOverrides,
  applyOverrides,
  chordProblems,
  customized,
  isRebindable,
  loadOverrides,
  refused,
  withOverride,
} from "./keymap.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq(actual: unknown, expected: unknown, msg: string): void {
  const [a, b] = [JSON.stringify(actual), JSON.stringify(expected)];
  if (a !== b) throw new Error(`assertion failed: ${msg}\n  actual:   ${a}\n  expected: ${b}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

const SHIPPED: readonly Binding[] = DEFAULT_BINDINGS;
const keysOf = (table: readonly Binding[], id: string): readonly string[] =>
  table.find((b) => b.id === id)?.keys ?? [];
const messages = (id: string, spec: string, o: Overrides = {}): string[] =>
  chordProblems(id, spec, DEFAULT_BINDINGS, o).map((p) => `${p.tier}: ${p.message}`);

// --- the algebra ----------------------------------------------------------------------

check("an override replaces a command's chord and leaves every other row alone", () => {
  const table = applyOverrides(DEFAULT_BINDINGS, { "find.open": ["Mod-Alt-f"] });
  assertEq(keysOf(table, "find.open"), ["Mod-Alt-f"], "the rebound one");
  assertEq(keysOf(table, "search.focus"), ["Mod-Shift-f"], "its neighbour is untouched");
  assertEq(table.length, DEFAULT_BINDINGS.length, "and nothing is added or lost");
});

check("a rebinding keeps the aliases the sheet documents in prose", () => {
  // `Binding.aliases` states the reasoning: ⌘← is a second way in that the row's words
  // already cover, and choosing the chord the sheet *prints* is not the same act as
  // deleting the app's conveniences.
  const table = applyOverrides(DEFAULT_BINDINGS, { "nav.back": ["Mod-Alt-["] });
  const b = table.find((x) => x.id === "nav.back");
  assertEq(b?.keys, ["Mod-Alt-["], "the chord moved");
  assertEq(b?.aliases, ["Mod-ArrowLeft"], "⌘← still goes back");
});

check("an empty override is a reset, not a command with no chord", () => {
  const o = withOverride({ "find.open": ["Mod-Alt-f"] }, "find.open", []);
  assertEq(o, {}, "the entry is gone");
  assertEq(keysOf(applyOverrides(DEFAULT_BINDINGS, o), "find.open"), ["Mod-f"], "and ⌘F is back");
});

check("withOverride does not mutate what it is given", () => {
  // The recorder builds a *candidate* table from the live overrides on every keystroke, so
  // an in-place update would rewrite the user's keyboard while they were still trying
  // chords out — including chords that get refused.
  const before: Overrides = { "find.open": ["Mod-Alt-f"] };
  withOverride(before, "settings.toggle", ["Mod-Alt-,"]);
  assertEq(before, { "find.open": ["Mod-Alt-f"] }, "untouched");
});

check("customized lists the moved commands in the table's own order", () => {
  const o = { "settings.toggle": ["Mod-Alt-,"], "find.open": ["Mod-Alt-f"] };
  assertEq(
    customized(DEFAULT_BINDINGS, o).map((b) => b.id),
    ["find.open", "settings.toggle"],
    "table order, not insertion order — it drives a paint",
  );
  assertEq(customized(DEFAULT_BINDINGS, {}).length, 0, "and nothing is changed by default");
});

check("isRebindable is exactly the absence of a stated reason", () => {
  assert(isRebindable(SHIPPED.find((b) => b.id === "find.open") as Binding), "⌘F can move");
  assert(!isRebindable(SHIPPED.find((b) => b.id === "dismiss") as Binding), "Esc cannot");
});

// --- refusals -------------------------------------------------------------------------

check("a chord the menu bar owns is refused, and says which item has it", () => {
  // The blunt one. AppKit dispatches a menu key equivalent before the key window's
  // responder chain, so this is not a chord that loses a race — it is one that never
  // happens. A user who picked it would see a command that silently does nothing (or, for
  // ⌘W, closes the window), which is the exact failure #119 made detectable.
  assertEq(
    messages("find.open", "Mod-w"),
    ["refuse: ⌘W belongs to the menu bar (Close Window). macOS runs it before B2 sees the key."],
    "⌘W is the menu's",
  );
});

check("a chord another command already answers to in the same scope is refused", () => {
  // Which one would run depends on the order of branches in main.ts's keydown handler,
  // and that is not a contract anyone can read — so this is the error tier, exactly as it
  // is for the shipped table in CI.
  assertEq(
    messages("find.open", "Mod-n"),
    ["refuse: ⌘N already runs New note here."],
    "and it names the command, not the id",
  );
});

check("an alias is as claimed as a listed chord", () => {
  // ⌘← fires nav.back without appearing in the sheet, so it is precisely the keystroke a
  // user would pick believing it free. It also happens to be CodeMirror's
  // cursor-to-line-edge, which is why this chord draws a report from two checkers at once
  // — worth showing, since a refusal and an advisory are not alternatives: the user is
  // told everything true about the chord, and only the refusal blocks the save.
  assertEq(
    messages("edit.toggle", "Mod-ArrowLeft"),
    [
      "refuse: ⌘← already runs Back here.",
      "warn: CodeMirror binds ⌘← while you're editing (cursorLineBoundaryLeft).",
    ],
    "the invisible chord defends itself",
  );
});

check("a key no chord can hold is refused rather than thrown", () => {
  assertEq(
    messages("find.open", "Mod-Nonsense"),
    ["refuse: B2 can't build a shortcut out of that key."],
    "the parser's throw becomes a sentence",
  );
});

// --- advisories -----------------------------------------------------------------------

check("a chord CodeMirror also binds is said out loud and allowed", () => {
  // Legal, and frequently the right answer — ⌘I is italic in B2 *because* B2's chords go
  // into the editor's keymap ahead of the stock ones. Which side wins is install order and
  // differs row by row (editorkeys.ts), so this names the overlap instead of predicting it.
  const found = messages("edit.toggle", "Alt-ArrowUp");
  assert(
    found.some((m) => m.startsWith("warn:") && m.includes("moveLineUp")),
    `expected an editor advisory, got ${JSON.stringify(found)}`,
  );
  assert(!refused(chordProblems("edit.toggle", "Alt-ArrowUp")), "and it is not a refusal");
});

check("a chord an inner surface would take first is said out loud and allowed", () => {
  // The Settings rail answers ⌃Tab, so a global command spelled that way simply doesn't
  // run while the dialog has the keyboard. Deliberate everywhere it happens today (the
  // rename field's Esc, the trap's Tab), so: reported, never failed.
  const found = messages("find.open", "Ctrl-Tab");
  assert(!refused(chordProblems("find.open", "Ctrl-Tab")), "shadowing is legal");
  assert(
    found.some((m) => m.includes("Next Settings section")),
    `expected a shadow advisory, got ${JSON.stringify(found)}`,
  );
});

check("a chord with no modifier is said out loud and allowed", () => {
  // `?` is a shipped default, so refusing this would be a lie. Whether a bare chord fires
  // mid-sentence depends on the guard beside its branch in main.ts's handler — the one
  // thing the table deliberately doesn't model — so the honest move is to say so.
  assertEq(
    messages("edit.toggle", "k"),
    ["warn: K has no modifier, so it can fire while you're typing."],
    "the warning, and nothing else",
  );
});

check("a chord nobody claims reports nothing at all", () => {
  // The check that keeps the four above from being a checker that just always complains.
  assertEq(messages("find.open", "Mod-Alt-Shift-f"), [], "⌥⇧⌘F is free");
  assertEq(messages("tree.new-note", "F6"), [], "so is F6");
});

check("a command may be rebound onto a chord it already answers to", () => {
  // Re-recording the same chord is a no-op, not a self-conflict: `conflicts()` skips a row
  // against itself, and a recorder that refused "the chord it already has" would be a
  // recorder you can't press Escape out of by pressing the same key twice.
  assertEq(messages("find.open", "Mod-f"), [], "⌘F is still find's own");
});

check("the judgement is made against the candidate table, not the live one", () => {
  // The whole point of laying the chord over the current overrides first. Once ⌘F has been
  // freed by moving find-in-note elsewhere, ⌘F is available — and until then it isn't.
  assertEq(messages("edit.toggle", "Mod-f"), ["refuse: ⌘F already runs Find in this note here."], "before");
  assertEq(messages("edit.toggle", "Mod-f", { "find.open": ["Mod-Alt-f"] }), [], "after");
});

// --- reading the store ------------------------------------------------------------------

check("a stored keyboard is adopted whole when every entry stands up", () => {
  const { overrides, dropped } = adoptOverrides({
    "find.open": ["Mod-Alt-f"],
    "settings.toggle": ["Mod-Alt-,"],
  });
  assertEq(overrides, { "find.open": ["Mod-Alt-f"], "settings.toggle": ["Mod-Alt-,"] }, "kept");
  assertEq(dropped, [], "nothing lost");
});

check("junk in the store is dropped rather than believed", () => {
  const { overrides, dropped } = adoptOverrides({
    "no.such.command": ["Mod-Alt-q"],
    dismiss: ["Mod-Alt-x"], // fixed: Esc is not the user's to move
    "find.open": "Mod-Alt-f", // not a list
    "tree.rename": [], // a command with no chord is not a preference
    "edit.toggle": ["Cmd+e"], // not a chord this parser can hold
  });
  assertEq(overrides, {}, "none of it survives");
  assertEq(
    dropped.sort(),
    ["dismiss", "edit.toggle", "find.open", "no.such.command", "tree.rename"],
    "and each is named",
  );
});

check("an entry that merely restates the default is not an override", () => {
  // Otherwise the panel would mark ⌘F "changed" for a user who recorded ⌘F, and "Reset
  // all (1)" would offer to undo nothing.
  const { overrides, dropped } = adoptOverrides({ "find.open": ["Mod-f"] });
  assertEq(overrides, {}, "dropped as a no-op");
  assertEq(dropped, [], "and not reported as a loss — nothing was lost");
});

check("a hand-edited store that would break the keyboard is defused entry by entry", () => {
  // The one that matters. Two commands on ⌘K is a keyboard where which command runs
  // depends on branch order — and the second entry is the one that has to go, since the
  // first was legal when it was read. What survives is always a table CI would pass.
  const { overrides, dropped } = adoptOverrides({
    "find.open": ["Mod-k"],
    "edit.toggle": ["Mod-k"],
  });
  assertEq(overrides, { "find.open": ["Mod-k"] }, "the first one stands");
  assertEq(dropped, ["edit.toggle"], "the second is refused");
  assertEq(conflicts(applyOverrides(DEFAULT_BINDINGS, overrides)), [], "and the keyboard is clean");
});

check("an advisory in the store is honoured — only refusals are dropped", () => {
  // The tiers mean the same thing here as in the recorder. A user who accepted "CodeMirror
  // binds this too" gets to keep it across a relaunch.
  const { overrides, dropped } = adoptOverrides({ "edit.toggle": ["Alt-ArrowUp"] });
  assertEq(overrides, { "edit.toggle": ["Alt-ArrowUp"] }, "kept");
  assertEq(dropped, [], "nothing dropped");
});

check("no store at all, or no storage at all, is the shipped keyboard", () => {
  // node has no `localStorage`, which is the same shape as a browser refusing it in
  // private mode: the read throws, and a keyboard reference must not take the window down.
  assertEq(adoptOverrides(null), { overrides: {}, dropped: [] }, "nothing stored");
  assertEq(adoptOverrides("nonsense"), { overrides: {}, dropped: [] }, "not even an object");
  assertEq(adoptOverrides([1, 2]), { overrides: {}, dropped: [] }, "nor an array");
  assertEq(loadOverrides(), { overrides: {}, dropped: [] }, "and no storage is not an error");
});

check("the live registry starts as the shipped one", () => {
  // Nothing here installs a table, so `activeBindings()` is the default — which is what
  // lets every other suite call `isBound` without arranging global state first.
  assertEq(activeBindings().length, DEFAULT_BINDINGS.length, "same table");
  assertEq(conflicts(), [], "and the gate reads it");
});

console.log(`keymap: ${passed} checks passed`);
