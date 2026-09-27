// The customization layer (keymap.ts): the algebra, `chordProblems`' tiers (and the cases
// that report nothing), and `adoptOverrides` re-judging a hand-editable store.
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

check("recording a command's own chord back onto it is a reset, not an override", () => {
  const o = withOverride({}, "find.open", ["Mod-f"]);
  assertEq(o, {}, "no entry is written");
  const back = withOverride({ "find.open": ["Mod-Alt-f"] }, "find.open", ["Mod-f"]);
  assertEq(back, {}, "and recording the default over a rebinding puts it back");
  // Order counts: `keys[0]` leads the sheet and is what CodeMirror gets.
  const swapped = withOverride({}, "graph.activate", ["Space", "Enter"]);
  assertEq(swapped, { "graph.activate": ["Space", "Enter"] }, "a reordering is a change");
});

check("withOverride does not mutate what it is given", () => {
  // The recorder builds candidate tables from the live overrides on every keystroke.
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
  // AppKit runs a menu key equivalent before B2 sees it (#119).
  assertEq(
    messages("find.open", "Mod-w"),
    ["refuse: ⌘W belongs to the menu bar (Close Window). macOS runs it before B2 sees the key."],
    "⌘W is the menu's",
  );
});

check("a chord another command already answers to in the same scope is refused", () => {
  assertEq(
    messages("find.open", "Mod-n"),
    ["refuse: ⌘N already runs New note here."],
    "and it names the command, not the id",
  );
});

check("an alias is as claimed as a listed chord", () => {
  // ⌘← is nav.back's hidden alias and a CodeMirror chord: both reports are shown, and only
  // the refusal blocks the save.
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
  const found = messages("edit.toggle", "Alt-ArrowUp");
  assert(
    found.some((m) => m.startsWith("warn:") && m.includes("moveLineUp")),
    `expected an editor advisory, got ${JSON.stringify(found)}`,
  );
  assert(!refused(chordProblems("edit.toggle", "Alt-ArrowUp")), "and it is not a refusal");
});

check("a chord an inner surface would take first is said out loud and allowed", () => {
  // The Settings rail answers ⌃Tab first while Settings is open.
  const found = messages("find.open", "Ctrl-Tab");
  assert(!refused(chordProblems("find.open", "Ctrl-Tab")), "shadowing is legal");
  assert(
    found.some((m) => m.includes("Next Settings section")),
    `expected a shadow advisory, got ${JSON.stringify(found)}`,
  );
});

check("a chord with no modifier is said out loud and allowed", () => {
  assertEq(
    messages("edit.toggle", "k"),
    ["warn: K has no modifier, so it can fire while you're typing."],
    "the warning, and nothing else",
  );
});

check("a chord nobody claims reports nothing at all", () => {
  assertEq(messages("find.open", "Mod-Alt-Shift-f"), [], "⌥⇧⌘F is free");
  assertEq(messages("tree.new-note", "F6"), [], "so is F6");
});

check("a command may be rebound onto a chord it already answers to", () => {
  // Not a self-conflict: `conflicts()` skips a row against itself.
  assertEq(messages("find.open", "Mod-f"), [], "⌘F is still find's own");
});

check("the judgement is made against the candidate table, not the live one", () => {
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
  const { overrides, dropped } = adoptOverrides({ "find.open": ["Mod-f"] });
  assertEq(overrides, {}, "dropped as a no-op");
  assertEq(dropped, [], "and not reported as a loss — nothing was lost");
});

check("a hand-edited store that would break the keyboard is defused entry by entry", () => {
  // The second entry goes, since the first was legal when it was read.
  const { overrides, dropped } = adoptOverrides({
    "find.open": ["Mod-k"],
    "edit.toggle": ["Mod-k"],
  });
  assertEq(overrides, { "find.open": ["Mod-k"] }, "the first one stands");
  assertEq(dropped, ["edit.toggle"], "the second is refused");
  assertEq(conflicts(applyOverrides(DEFAULT_BINDINGS, overrides)), [], "and the keyboard is clean");
});

check("an advisory in the store is honoured — only refusals are dropped", () => {
  const { overrides, dropped } = adoptOverrides({ "edit.toggle": ["Alt-ArrowUp"] });
  assertEq(overrides, { "edit.toggle": ["Alt-ArrowUp"] }, "kept");
  assertEq(dropped, [], "nothing dropped");
});

check("no store at all, or no storage at all, is the shipped keyboard", () => {
  // node has no `localStorage`, like a browser refusing it in private mode.
  assertEq(adoptOverrides(null), { overrides: {}, dropped: [] }, "nothing stored");
  assertEq(adoptOverrides("nonsense"), { overrides: {}, dropped: [] }, "not even an object");
  assertEq(adoptOverrides([1, 2]), { overrides: {}, dropped: [] }, "nor an array");
  assertEq(loadOverrides(), { overrides: {}, dropped: [] }, "and no storage is not an error");
});

check("the live registry starts as the shipped one", () => {
  assertEq(activeBindings().length, DEFAULT_BINDINGS.length, "same table");
  assertEq(conflicts(), [], "and the gate reads it");
});

console.log(`keymap: ${passed} checks passed`);
