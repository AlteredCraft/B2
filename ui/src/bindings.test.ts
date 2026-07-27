// The keyboard registry (bindings.ts), pinned — and the collision gate itself. Pure —
// no DOM — so node runs it straight off the source: `npm test`. Dependency-free like the
// others.
//
// Two jobs. The first is that B2's own chords don't collide: `conflicts(BINDINGS)` must
// be empty, and because `npm test` runs inside both `just check` and `just ci`, that
// assertion *is* the gate. The second is proving the gate can fail — a checker that has
// only ever seen a clean table is indistinguishable from one that returns `[]`
// unconditionally, so the interesting cases below are synthetic tables built to collide.
//
// The matcher gets the same treatment. What a keyboard layer actually gets wrong is
// dull and invisible: a chord that never fires because the table spells a key the way
// the docs write it rather than the way `KeyboardEvent.key` reports it, a `?` that
// demands ⇧ on top of the ⇧ the browser already applied, a ⌘-chord that also answers to
// ⌥⌘ because nobody checked `altKey`. None of that shows up until a user reports that a
// shortcut "sometimes" doesn't work.
import { FORMATS } from "./format.ts";
import {
  type Binding,
  BINDINGS,
  type KeyEventLike,
  allKeys,
  canonicalKey,
  chordFor,
  chordMatches,
  conflicts,
  displayChord,
  displayKeys,
  isBound,
  parseChord,
  scopeContains,
  shadows,
} from "./bindings.ts";

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
  // parseChord throws on an unknown key or modifier, so a typo — "Mod-Delete" for
  // "Mod-Backspace", "Cmd+f" for "Mod-f" — fails here rather than shipping a dead chord
  // that nothing reports because nothing was ever bound to notice.
  for (const b of BINDINGS) {
    for (const spec of allKeys(b)) parseChord(spec);
  }
});

check("command ids are unique", () => {
  // The lookup is a Map, so a duplicated id wouldn't error — the later row would quietly
  // win and the earlier command would stop responding.
  const seen = new Set<string>();
  for (const b of BINDINGS) {
    assert(!seen.has(b.id), `duplicate command id: ${b.id}`);
    seen.add(b.id);
  }
});

check("every format in FORMATS has a chord in the registry", () => {
  // format.ts carries the marker; the chord lives here, and main.ts builds the editor's
  // keymap by asking for `format.<id>`. That lookup throws on a miss — at editor
  // construction, in the app — so assert it now instead.
  for (const f of FORMATS) chordFor(`format.${f.id}`);
});

// --- the gate -----------------------------------------------------------------------

check("B2's own chords do not collide", () => {
  const found = conflicts();
  assertEq(found, [], `${found.length} colliding chord(s)`);
});

check("two commands on one chord in one scope is a conflict", () => {
  // The gate above proves nothing on its own — this is what proves it can fail.
  const table: Binding[] = [
    { id: "a", keys: ["Mod-k"], scope: "global" },
    { id: "b", keys: ["Mod-k"], scope: "global" },
  ];
  assertEq(conflicts(table), [{ a: "a", b: "b", form: "⌘k", scope: "global" }], "the clash");
});

check("an alias collides as loudly as a listed chord", () => {
  // ⌘← fires nav.back without appearing in the sheet. A chord nobody can *see* in the
  // reference is exactly the one a new binding would land on unnoticed.
  const table: Binding[] = [
    { id: "a", keys: ["Mod-["], aliases: ["Mod-ArrowLeft"], scope: "global" },
    { id: "b", keys: ["Mod-ArrowLeft"], scope: "global" },
  ];
  assertEq(conflicts(table).map((c) => c.form), ["⌘ArrowLeft"], "the alias clashes");
});

check("⌃X collides with a Mod chord, because Mod answers to ⌃ too", () => {
  // The subtle one. `Mod-x` and `Ctrl-x` look like different rows and are one keystroke
  // apart in the hardware: the matcher takes ⌃ as ⌘'s alias, so both fire on ⌃X.
  const table: Binding[] = [
    { id: "a", keys: ["Mod-Tab"], scope: "global" },
    { id: "b", keys: ["Ctrl-Tab"], scope: "global" },
  ];
  assertEq(conflicts(table).map((c) => c.form), ["⌃Tab"], "⌃ is where they meet");
});

check("the same chord in sibling scopes is not a conflict", () => {
  // ⏎ commits the link dialog and the delete confirm. Only one can be open, so they are
  // not competing — modelling that as two scopes is what keeps the gate from crying wolf.
  const table: Binding[] = [
    { id: "a", keys: ["Enter"], scope: "overlay:link" },
    { id: "b", keys: ["Enter"], scope: "overlay:delete" },
  ];
  assertEq(conflicts(table), [], "siblings don't collide");
});

check("an inner scope shadows the outer one, and that is reported, not failed", () => {
  const table: Binding[] = [
    { id: "outer", keys: ["Escape"], scope: "global" },
    { id: "inner", keys: ["Escape"], scope: "textentry:rename" },
  ];
  assertEq(conflicts(table), [], "shadowing is legal");
  assertEq(shadows(table), [{ outer: "outer", inner: "inner", form: "Escape" }], "and reported");
});

check("B2 shadows exactly these five, and each is an ordering the handler relies on", () => {
  // A shadow is a scoped binding taking a keystroke the surface around it would
  // otherwise get, so each one is a claim about branch order in main.ts's handler:
  //
  //  - The three Escapes: an inline input's Esc backs *that* input out instead of running
  //    the overlay cascade, so each input's branch has to come before `dismiss`.
  //  - ⌃Tab and ⌃⇧Tab: the overlay's Tab trap is `Any-Tab` and swallows unconditionally,
  //    so the Settings rail's section chords only ever run because their branch is above
  //    it. That ordering is load-bearing and easy to undo by tidying; this is what
  //    notices.
  //
  // Pinned, so a sixth has to be argued for rather than accumulated.
  assertEq(
    shadows().map((s) => `${s.outer} > ${s.inner} (${s.form})`),
    [
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

check("a Mod chord answers to ⌘ and to ⌃, and to nothing else", () => {
  assert(isBound(press("f", { metaKey: true }), "find.open"), "⌘F opens find");
  assert(isBound(press("f", { ctrlKey: true }), "find.open"), "⌃F does too — the dev alias");
  assert(!isBound(press("f"), "find.open"), "bare F types an f");
  assert(!isBound(press("f", { metaKey: true, altKey: true }), "find.open"), "⌥⌘F is not ⌘F");
});

check("⇧ separates two commands on one letter", () => {
  const shiftF = press("F", { metaKey: true, shiftKey: true });
  assert(isBound(shiftF, "search.focus"), "⇧⌘F searches the vault");
  assert(!isBound(shiftF, "find.open"), "and is not ⌘F");
  assert(!isBound(press("f", { metaKey: true }), "search.focus"), "nor the reverse");
});

check("a shifted letter arrives uppercase and still matches", () => {
  // `KeyboardEvent.key` reports "A" when ⇧ is down. The table writes chords lowercase,
  // so the canonical form has to fold the case or ⇧⌘A would never fire.
  assertEq(canonicalKey("A"), "a", "the key folds");
  assert(isBound(press("A", { metaKey: true, shiftKey: true }), "anomalies.toggle"), "⇧⌘A");
});

check("? asks for no ⇧ of its own, because the browser already applied it", () => {
  // ⇧/ reports key "?" — the shift is *in* the character. A chord that also demanded
  // shiftKey would be fine here but unfireable on a layout where ? is unshifted, and one
  // that demanded !shiftKey would never fire at all.
  assert(isBound(press("?", { shiftKey: true }), "help.keyboard"), "⇧/ opens the reference");
  assert(isBound(press("?"), "help.keyboard"), "and so does a ? that needed no shift");
  assert(!isBound(press("?", { metaKey: true }), "help.keyboard"), "but ⌘? is a different chord");
});

check("⇧ does separate two commands on a named key", () => {
  // The counterpart to the rule above: F10's identity doesn't change under ⇧, so there
  // the modifier is real and has to be matched.
  assert(isBound(press("F10", { shiftKey: true }), "menu.open"), "⇧F10 is the keyboard's right-click");
  assert(!isBound(press("F10"), "menu.open"), "bare F10 is not");
});

check("the space bar is spelled, not written as a literal space", () => {
  assertEq(canonicalKey(" "), "Space", "canonical");
  assert(isBound(press(" "), "graph.activate"), "Space opens a focused graph node");
  assert(isBound(press("Enter"), "graph.activate"), "so does ⏎");
});

check("⌃Tab is literal Control, not Mod", () => {
  // The one chord in the app that wants ⌃ itself. If it went through Mod it would also
  // fire on ⌘Tab — which macOS owns — and would collide with anything bound to Mod-Tab.
  assert(isBound(press("Tab", { ctrlKey: true }), "settings.section.next"), "⌃Tab");
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
  // `Any-Escape`. The escape hatch must not be conditional on a modifier the user hasn't
  // let go of yet — you press ⌘F, change your mind, and hit Escape with ⌘ still down.
  for (const mods of [{}, { metaKey: true }, { shiftKey: true }, { altKey: true, ctrlKey: true }]) {
    assert(isBound(press("Escape", mods), "dismiss"), `Esc with ${JSON.stringify(mods)}`);
  }
  assert(isBound(press("Tab", { metaKey: true }), "overlay.focus.step"), "and no Tab escapes");
  assert(!isBound(press("Enter"), "dismiss"), "but Any- is about modifiers, not keys");
});

check("an Any- chord claims every keystroke over its key", () => {
  // Which is what makes it shadow — and collide with — the bindings it would really take
  // the key from. A strict ⌃Tab under a scope the trap covers is not a free chord.
  const table: Binding[] = [
    { id: "trap", keys: ["Any-Tab"], scope: "overlay" },
    { id: "rail", keys: ["Ctrl-Tab"], scope: "overlay:settings" },
    { id: "elsewhere", keys: ["Ctrl-Tab"], scope: "editor" },
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
  const rejects = ["Cmd+f", "Mod-Ctrl-x", "Mod-Meh", "Mod-Retrun", ""];
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

check("a row does not print the aliases the prose covers", () => {
  // nav.back also answers to ⌘←; the row says so in words rather than listing it, which
  // is the whole reason `aliases` is a separate field from `keys`.
  assertEq(displayKeys(["nav.back", "nav.forward"]), "⌘[ / ⌘]", "brackets only");
  assertEq(displayKeys(["menu.open"]), "⇧F10", "not the Menu key");
});

console.log(`bindings: ${passed} checks passed`);
