// The keyboard registry (bindings.ts), and the conflict gate itself: `conflicts` on the
// shipped table must be empty. Synthetic tables built to clash prove the gate can fail.
import { FORMATS } from "./format.ts";
import {
  type Binding,
  DEFAULT_BINDINGS,
  type KeyEventLike,
  allKeys,
  boundOf,
  canonicalKey,
  chordFor,
  chordMatches,
  conflicts,
  displayChord,
  displayKeys,
  isBindableKey,
  isBound,
  keystrokes,
  parseChord,
  scopeContains,
  shadows,
  shiftDistinguishes,
} from "./bindings.ts";

/** A synthetic row, labelled after its id. */
function row(b: Omit<Binding, "label">): Binding {
  return { label: b.id, ...b };
}

/** The shipped table widened from `as const`, so optional fields like `.fixed` can be read. */
const SHIPPED: readonly Binding[] = DEFAULT_BINDINGS;

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

/** A keydown, as the matcher sees it. Modifiers default to "not held". */
function press(key: string, mods: Partial<KeyEventLike> = {}): KeyEventLike {
  return { key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...mods };
}

// --- the table itself ---------------------------------------------------------------

check("every chord in the table parses", () => {
  for (const b of DEFAULT_BINDINGS) {
    for (const spec of allKeys(b)) parseChord(spec);
  }
});

check("command ids are unique", () => {
  // The lookup is a Map, so a duplicate would silently win over the earlier row.
  const seen = new Set<string>();
  for (const b of DEFAULT_BINDINGS) {
    assert(!seen.has(b.id), `duplicate command id: ${b.id}`);
    seen.add(b.id);
  }
});

check("every command carries a label a human could read", () => {
  for (const b of SHIPPED) {
    assert(b.label.trim() !== "", `${b.id} has no label`);
    assert(b.label !== b.id, `${b.id}'s label is just its id`);
  }
});

check("the chords that can't be rebound are exactly the platform's own reflexes", () => {
  // Pinned so the set can't accumulate: each row is a platform reflex or an `Any-` chord.
  assertEq(
    SHIPPED.filter((b) => b.fixed !== undefined).map((b) => b.id),
    [
      "dismiss",
      "overlay.focus.step",
      "link.commit",
      "delete.confirm",
      "graph.activate",
      "create.commit",
      "create.cancel",
      "rename.commit",
      "rename.cancel",
      "find.input.next",
      "find.input.prev",
      "find.input.close",
      "chat.send", // GH #155
    ],
    "the fixed set",
  );
  // An `Any-` chord can't be recorded as one keystroke, so it must be fixed.
  for (const b of SHIPPED) {
    const any = allKeys(b).some((k) => k.startsWith("Any-"));
    assert(!any || b.fixed !== undefined, `${b.id} has an Any- chord but is rebindable`);
  }
});

check("every format in FORMATS has a chord in the registry", () => {
  // main.ts asks for `format.<id>` at editor construction, where a miss throws in the app.
  for (const f of FORMATS) chordFor(`format.${f.id}`);
});

// --- the gate -----------------------------------------------------------------------

check("B2's own chords do not conflict", () => {
  const found = conflicts(DEFAULT_BINDINGS);
  assertEq(found, [], `${found.length} conflicting chord(s)`);
});

check("two commands on one chord in one scope is a conflict", () => {
  const table: Binding[] = [
    row({ id: "a", keys: ["Mod-k"], scope: "global" }),
    row({ id: "b", keys: ["Mod-k"], scope: "global" }),
  ];
  assertEq(conflicts(table), [{ a: "a", b: "b", form: "⌘k", scope: "global" }], "the clash");
});

check("an alias conflicts as loudly as a listed chord", () => {
  // Aliases are invisible in the sheet, so a new binding would land on one unnoticed.
  const table: Binding[] = [
    row({ id: "a", keys: ["Mod-["], aliases: ["Mod-ArrowLeft"], scope: "global" }),
    row({ id: "b", keys: ["Mod-ArrowLeft"], scope: "global" }),
  ];
  assertEq(conflicts(table).map((c) => c.form), ["⌘ArrowLeft"], "the alias clashes");
});

check("an Any- chord conflicts with every strict chord over its key", () => {
  const table: Binding[] = [
    row({ id: "a", keys: ["Any-Escape"], scope: "global" }),
    row({ id: "b", keys: ["Mod-Escape"], scope: "global" }),
  ];
  assertEq(conflicts(table).map((c) => c.form), ["⌘Escape"], "⌘Esc is where they meet");
});

check("the same chord in sibling scopes is not a conflict", () => {
  // Only one overlay can be open, so they don't compete.
  const table: Binding[] = [
    row({ id: "a", keys: ["Enter"], scope: "overlay:link" }),
    row({ id: "b", keys: ["Enter"], scope: "overlay:delete" }),
  ];
  assertEq(conflicts(table), [], "siblings don't conflict");
});

check("an inner scope shadows the outer one, and that is reported, not failed", () => {
  const table: Binding[] = [
    row({ id: "outer", keys: ["Escape"], scope: "global" }),
    row({ id: "inner", keys: ["Escape"], scope: "textentry:rename" }),
  ];
  assertEq(conflicts(table), [], "shadowing is legal");
  assertEq(shadows(table), [{ outer: "outer", inner: "inner", form: "Escape" }], "and reported");
});

check("B2 shadows exactly these six, and each is an ordering the handler relies on", () => {
  // Each shadow is a claim about branch order in main.ts's handler:
  //  - ⌘G: the find branch sits above the graph's (⌘G is Find Next only while the bar is
  //    open; `openFind` declines while the graph is up).
  //  - The Escapes: each inline input's branch comes before `dismiss`.
  //  - ⌃Tab/⌃⇧Tab: the rail's branch sits above the `Any-Tab` trap, which would swallow
  //    them. Easy to undo by tidying.
  // Pinned, so a seventh has to be argued for.
  assertEq(
    shadows(DEFAULT_BINDINGS).map((s) => `${s.outer} > ${s.inner} (${s.form})`),
    [
      "graph.toggle > find.next (⌘g)",
      "dismiss > create.cancel (Escape)",
      "dismiss > rename.cancel (Escape)",
      "dismiss > find.input.close (Escape)",
      "overlay.focus.step > settings.section.next (⌃Tab)",
      "overlay.focus.step > settings.section.prev (⌃⇧Tab)",
    ],
    "the shadow set",
  );
});

check("scope containment is global-over-all, then the : namespace", () => {
  assert(scopeContains("global", "textentry:find"), "global contains everything");
  assert(scopeContains("overlay:link", "overlay:link"), "a scope contains itself");
  assert(scopeContains("overlay", "overlay:link"), "and the layer contains its members");
  assert(!scopeContains("overlay:link", "overlay:delete"), "siblings contain nothing");
  assert(!scopeContains("editor", "global"), "and containment doesn't run backwards");
});

// --- matching -----------------------------------------------------------------------

check("a Mod chord answers to ⌘, and to nothing else", () => {
  assert(isBound(press("f", { metaKey: true }), "find.open"), "⌘F opens find");
  assert(!isBound(press("f"), "find.open"), "bare F types an f");
  assert(!isBound(press("f", { metaKey: true, altKey: true }), "find.open"), "⌥⌘F is not ⌘F");
  assert(!isBound(press("f", { ctrlKey: true }), "find.open"), "and ⌃F is not ⌘F");
});

check("⌃ is not a synonym for ⌘, anywhere in the table", () => {
  // ⌃ keys are macOS emacs motions (see `Chord.mod`). Stated as a property so it holds
  // for chords not yet written: a binding claims a ⌃ keystroke only by asking for ⌃.
  for (const b of DEFAULT_BINDINGS) {
    for (const spec of allKeys(b)) {
      const declares = spec.includes("Ctrl-") || spec.includes("Any-");
      for (const form of keystrokes(spec)) {
        assert(
          !form.startsWith("⌃") || declares,
          `${b.id} answers to ${form} but its chord (${spec}) never asks for ⌃`,
        );
      }
    }
  }
});

check("⌃X and ⌘X are two different chords", () => {
  assert(isBound(press("Tab", { ctrlKey: true }), "settings.section.next"), "⌃Tab");
  const table: Binding[] = [
    row({ id: "meta", keys: ["Mod-k"], scope: "global" }),
    row({ id: "control", keys: ["Ctrl-k"], scope: "global" }),
  ];
  assertEq(conflicts(table), [], "they no longer meet");
});

check("⇧ separates two commands on one letter", () => {
  const shiftF = press("F", { metaKey: true, shiftKey: true });
  assert(isBound(shiftF, "search.focus"), "⇧⌘F searches the vault");
  assert(!isBound(shiftF, "find.open"), "and is not ⌘F");
  assert(!isBound(press("f", { metaKey: true }), "search.focus"), "nor the reverse");
});

check("⌘G is the graph in the window and the next match in the find bar", () => {
  // Both fire on the same press; branch order in main.ts's handler decides.
  const g = press("g", { metaKey: true });
  assert(isBound(g, "graph.toggle"), "⌘G flips the pane to the graph");
  assert(isBound(g, "find.next"), "and steps the find bar's matches while it is open");
  assert(!isBound(press("g", { metaKey: true, shiftKey: true }), "graph.toggle"), "⇧⌘G is not it");
  assert(!isBound(press("g"), "graph.toggle"), "and a bare g types a g");
});

check("a shifted letter arrives uppercase and still matches", () => {
  assertEq(canonicalKey("A"), "a", "the key folds");
  assert(isBound(press("N", { metaKey: true, shiftKey: true }), "tree.new-folder"), "⇧⌘N");
});

check("? asks for no ⇧ of its own, because the browser already applied it", () => {
  // ⇧/ reports "?": the shift is in the character, and ? is unshifted on some layouts.
  assert(isBound(press("?", { shiftKey: true }), "help.keyboard"), "⇧/ opens the reference");
  assert(isBound(press("?"), "help.keyboard"), "and so does a ? that needed no shift");
  assert(!isBound(press("?", { metaKey: true }), "help.keyboard"), "but ⌘? is a different chord");
});

check("⇧ does separate two commands on a named key", () => {
  assert(isBound(press("F10", { shiftKey: true }), "menu.open"), "⇧F10 is the keyboard's right-click");
  assert(!isBound(press("F10"), "menu.open"), "bare F10 is not");
});

check("the space bar is spelled, not written as a literal space", () => {
  assertEq(canonicalKey(" "), "Space", "canonical");
  assert(isBound(press(" "), "graph.activate"), "Space opens a focused graph node");
  assert(isBound(press("Enter"), "graph.activate"), "so does ⏎");
});

check("⌃Tab is literal Control, and ⌘Tab is not it", () => {
  // ⌘Tab is macOS's app switcher and never reaches the webview.
  assert(!isBound(press("Tab", { metaKey: true }), "settings.section.next"), "⌘Tab is not ⌃Tab");
  assert(
    isBound(press("Tab", { ctrlKey: true, shiftKey: true }), "settings.section.prev"),
    "⌃⇧Tab steps back",
  );
});

check("an alias fires the command the sheet doesn't show it under", () => {
  assert(isBound(press("[", { metaKey: true }), "nav.back"), "⌘[");
  assert(isBound(press("ArrowLeft", { metaKey: true }), "nav.back"), "⌘← too");
  assert(isBound(press("s", { metaKey: true }), "fm.save"), "⌘S also saves the drawer");
});

check("Esc gets you out with anything held down", () => {
  for (const mods of [{}, { metaKey: true }, { shiftKey: true }, { altKey: true, ctrlKey: true }]) {
    assert(isBound(press("Escape", mods), "dismiss"), `Esc with ${JSON.stringify(mods)}`);
  }
  assert(isBound(press("Tab", { metaKey: true }), "overlay.focus.step"), "and no Tab escapes");
  assert(!isBound(press("Enter"), "dismiss"), "but Any- is about modifiers, not keys");
});

check("an Any- chord claims every keystroke over its key", () => {
  const table: Binding[] = [
    row({ id: "trap", keys: ["Any-Tab"], scope: "overlay" }),
    row({ id: "rail", keys: ["Ctrl-Tab"], scope: "overlay:settings" }),
    row({ id: "elsewhere", keys: ["Ctrl-Tab"], scope: "editor" }),
  ];
  assertEq(
    shadows(table).map((s) => `${s.outer} > ${s.inner}`),
    ["trap > rail"],
    "the trap takes the rail's chord, and nothing outside the overlay layer",
  );
});

check("Any- and a named modifier is a contradiction", () => {
  let threw = false;
  try {
    parseChord("Any-Shift-Escape");
  } catch {
    threw = true;
  }
  assert(threw, "Any-Shift- asks for both 'any modifier' and 'this one'");
});

check("an Any- chord prints as the bare key", () => {
  assertEq(displayChord("Any-Escape"), "Esc", "not a list of twelve ways to hold it");
  assertEq(displayKeys(["overlay.focus.step"]), "Tab", "likewise");
});

check("chordMatches reads the modifiers it is given, not the ones it isn't", () => {
  const chord = parseChord("Mod-Backspace");
  assert(chordMatches(chord, press("Backspace", { metaKey: true })), "⌘⌫");
  assert(!chordMatches(chord, press("Backspace", { metaKey: true, shiftKey: true })), "⇧⌘⌫ is not");
  assert(!chordMatches(chord, press("Backspace")), "and a bare ⌫ deletes a character");
});

check("parseChord refuses what it can't honour", () => {
  // Not `Mod-Ctrl-x`: ⌘⌃X is a valid chord.
  const rejects = ["Cmd+f", "Mod-Meh", "Mod-Retrun", ""];
  for (const spec of rejects) {
    let threw = false;
    try {
      parseChord(spec);
    } catch {
      threw = true;
    }
    assert(threw, `parseChord should refuse ${JSON.stringify(spec)}`);
  }
});

// --- display ------------------------------------------------------------------------

check("modifiers print in Apple's order — ⌃⌥⇧⌘ — then the key", () => {
  assertEq(displayChord("Mod-Shift-f"), "⇧⌘F", "shift before command");
  assertEq(displayChord("Ctrl-Shift-Tab"), "⌃⇧Tab", "control before shift");
  assertEq(displayChord("Mod-Shift-v"), "⇧⌘V", "paste as plain text");
});

check("keys print as macOS writes them — glyphs, but words where macOS uses words", () => {
  assertEq(displayChord("Mod-Backspace"), "⌘⌫", "delete is a glyph");
  assertEq(displayChord("Mod-Enter"), "⌘⏎", "so is return");
  assertEq(displayChord("Escape"), "Esc", "escape is a word — ⎋ exists, nobody reads it");
  assertEq(displayChord("Space"), "Space", "and so is space");
  assertEq(displayChord("ArrowUp"), "↑", "arrows are arrows");
  assertEq(displayChord("F2"), "F2", "function keys are themselves");
});

check("a row prints its commands' chords, distinct ones only", () => {
  assertEq(displayKeys(["find.next", "find.prev"]), "⌘G / ⇧⌘G", "two chords, two cells");
  assertEq(displayKeys(["link.commit", "delete.confirm"]), "⏎", "one chord, said once");
  assertEq(displayKeys(["graph.activate"]), "⏎ / Space", "one command, two chords");
});

// --- resolving a keystroke to one of several commands ----------------------------------

check("boundOf picks the command a keystroke fires, and nothing when none does", () => {
  const nav = ["tree.row.prev", "tree.row.next", "tree.row.in", "tree.row.out"] as const;
  assertEq(boundOf(press("ArrowDown"), nav), "tree.row.next", "↓ is the next row");
  assertEq(boundOf(press("ArrowLeft"), nav), "tree.row.out", "← steps out");
  assertEq(boundOf(press("k"), nav), null, "a letter is not a move, so the caller leaves it alone");
  assertEq(boundOf(press("ArrowDown", { metaKey: true }), nav), null, "⌘↓ is not ↓");
});

check("↑ means a different thing in each pane, and that is not a conflict", () => {
  const up = press("ArrowUp");
  assert(isBound(up, "tree.row.prev"), "↑ walks the tree");
  assert(isBound(up, "side.row.prev"), "↑ walks discovery");
  assert(isBound(up, "menu.item.prev"), "↑ walks an open menu");
  assertEq(conflicts(DEFAULT_BINDINGS), [], "and the gate is unmoved by any of it");
});

// --- what a recorder may write down -----------------------------------------------------

check("isBindableKey admits what parseChord can hold, and refuses what it can't", () => {
  // Must agree with the parser exactly, or the recorder crashes. `-` is the syntax's own
  // separator, which a naive split throws on (GH #125).
  for (const key of ["a", "?", "1", "-", "Enter", "ArrowUp", "F12", "Space"]) {
    assert(isBindableKey(key), `${key} should be bindable`);
    parseChord(key); // the agreement, asserted rather than assumed
  }
  for (const key of ["Meta", "Shift", "Dead", "Unidentified", "AudioVolumeUp"]) {
    assert(!isBindableKey(key), `${key} should not be bindable`);
  }
});

check("the separator is also a key, and chords over it parse", () => {
  // CodeMirror's own rule (`normalizeKeyName` splits on `/-(?!$)/`).
  assertEq(parseChord("-").key, "-", "the bare hyphen");
  const modHyphen = parseChord("Mod--");
  assertEq([modHyphen.key, modHyphen.mod], ["-", true], "⌘ plus the hyphen");
  assertEq(parseChord("Mod-Shift--").key, "-", "and it survives a stack of modifiers");
  assertEq(displayChord("Mod--"), "⌘-", "printing it says the same thing");
  assert(chordMatches(parseChord("Mod--"), press("-", { metaKey: true })), "and it fires");
  let threw = false;
  try {
    parseChord("Mod-");
  } catch {
    threw = true;
  }
  assert(threw, "Mod- names no key");
});

check("shiftDistinguishes separates a real ⇧ from one already in the character", () => {
  assert(shiftDistinguishes("a"), "letters");
  assert(shiftDistinguishes("F10"), "named keys");
  assert(!shiftDistinguishes("?"), "? is what ⇧/ already reports");
  assert(!shiftDistinguishes("["), "and [ is not { ");
});

check("a row does not print the aliases the prose covers", () => {
  assertEq(displayKeys(["nav.back", "nav.forward"]), "⌘[ / ⌘]", "brackets only");
  assertEq(displayKeys(["menu.open"]), "⇧F10", "not the Menu key");
});

console.log(`bindings: ${passed} checks passed`);
