// The keyboard registry (bindings.ts), pinned — and the conflict gate itself. Pure —
// no DOM — so node runs it straight off the source: `npm test`. Dependency-free like the
// others.
//
// Two jobs. The first is that B2's own chords don't conflict: `conflicts(DEFAULT_BINDINGS)`
// must be empty, and because `npm test` runs inside both `just check` and `just ci`, that
// assertion *is* the gate. The second is proving the gate can fail — a checker that has
// only ever seen a clean table is indistinguishable from one that returns `[]`
// unconditionally, so the interesting cases below are synthetic tables built to clash.
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

/** A synthetic row. These tables exist to make a checker *fail*, so the label carries no
 *  meaning here — it is required by the type and named after the id to keep it honest. */
function row(b: Omit<Binding, "label">): Binding {
  return { label: b.id, ...b };
}

/** The shipped table under its declared type. `DEFAULT_BINDINGS` is `as const` so that
 *  `BindingId` can be derived from it, which also means reading `.fixed` off a row that
 *  hasn't got one is a type error rather than `undefined` — the widening is here so the
 *  checks below can ask the questions a reader would. */
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
  // parseChord throws on an unknown key or modifier, so a typo — "Mod-Delete" for
  // "Mod-Backspace", "Cmd+f" for "Mod-f" — fails here rather than shipping a dead chord
  // that nothing reports because nothing was ever bound to notice.
  for (const b of DEFAULT_BINDINGS) {
    for (const spec of allKeys(b)) parseChord(spec);
  }
});

check("command ids are unique", () => {
  // The lookup is a Map, so a duplicated id wouldn't error — the later row would quietly
  // win and the earlier command would stop responding.
  const seen = new Set<string>();
  for (const b of DEFAULT_BINDINGS) {
    assert(!seen.has(b.id), `duplicate command id: ${b.id}`);
    seen.add(b.id);
  }
});

check("every command carries a label a human could read", () => {
  // The recorder names what it is rebinding and the conflict messages name what they
  // clash with, so an empty or id-shaped label is a sentence that reads like a stack
  // trace — "⌘F already runs pane.discovery here."
  for (const b of SHIPPED) {
    assert(b.label.trim() !== "", `${b.id} has no label`);
    assert(b.label !== b.id, `${b.id}'s label is just its id`);
  }
});

check("the chords that can't be rebound are exactly the platform's own reflexes", () => {
  // `fixed` is a promise about *why* something can't be moved, not a convenience list, so
  // it is pinned rather than left to accumulate: every row here is ⏎/Esc in a text field,
  // ⏎ on a dialog's default button, the ⏎/Space a <button> would answer to, or one of the
  // two `Any-` chords no recorder could capture in the first place. A sixth category has
  // to be argued for.
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
    ],
    "the fixed set",
  );
  // The other half of the promise: an `Any-` chord is unrecordable by construction (a
  // recorder observes one keystroke), so one that was *not* fixed would be a chip
  // offering to record something nobody can press.
  for (const b of SHIPPED) {
    const any = allKeys(b).some((k) => k.startsWith("Any-"));
    assert(!any || b.fixed !== undefined, `${b.id} has an Any- chord but is rebindable`);
  }
});

check("every format in FORMATS has a chord in the registry", () => {
  // format.ts carries the marker; the chord lives here, and main.ts builds the editor's
  // keymap by asking for `format.<id>`. That lookup throws on a miss — at editor
  // construction, in the app — so assert it now instead.
  for (const f of FORMATS) chordFor(`format.${f.id}`);
});

// --- the gate -----------------------------------------------------------------------

check("B2's own chords do not conflict", () => {
  const found = conflicts(DEFAULT_BINDINGS);
  assertEq(found, [], `${found.length} conflicting chord(s)`);
});

check("two commands on one chord in one scope is a conflict", () => {
  // The gate above proves nothing on its own — this is what proves it can fail.
  const table: Binding[] = [
    row({ id: "a", keys: ["Mod-k"], scope: "global" }),
    row({ id: "b", keys: ["Mod-k"], scope: "global" }),
  ];
  assertEq(conflicts(table), [{ a: "a", b: "b", form: "⌘k", scope: "global" }], "the clash");
});

check("an alias conflicts as loudly as a listed chord", () => {
  // ⌘← fires nav.back without appearing in the sheet. A chord nobody can *see* in the
  // reference is exactly the one a new binding would land on unnoticed.
  const table: Binding[] = [
    row({ id: "a", keys: ["Mod-["], aliases: ["Mod-ArrowLeft"], scope: "global" }),
    row({ id: "b", keys: ["Mod-ArrowLeft"], scope: "global" }),
  ];
  assertEq(conflicts(table).map((c) => c.form), ["⌘ArrowLeft"], "the alias clashes");
});

check("an Any- chord conflicts with every strict chord over its key", () => {
  // The subtle one, now that modifiers are compared literally. `Any-Escape` and
  // `Mod-Escape` look like unrelated rows and are the same key press.
  const table: Binding[] = [
    row({ id: "a", keys: ["Any-Escape"], scope: "global" }),
    row({ id: "b", keys: ["Mod-Escape"], scope: "global" }),
  ];
  assertEq(conflicts(table).map((c) => c.form), ["⌘Escape"], "⌘Esc is where they meet");
});

check("the same chord in sibling scopes is not a conflict", () => {
  // ⏎ commits the link dialog and the delete confirm. Only one can be open, so they are
  // not competing — modelling that as two scopes is what keeps the gate from crying wolf.
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
    shadows(DEFAULT_BINDINGS).map((s) => `${s.outer} > ${s.inner} (${s.form})`),
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

check("a Mod chord answers to ⌘, and to nothing else", () => {
  assert(isBound(press("f", { metaKey: true }), "find.open"), "⌘F opens find");
  assert(!isBound(press("f"), "find.open"), "bare F types an f");
  assert(!isBound(press("f", { metaKey: true, altKey: true }), "find.open"), "⌥⌘F is not ⌘F");
  assert(!isBound(press("f", { ctrlKey: true }), "find.open"), "and ⌃F is not ⌘F");
});

check("⌃ is not a synonym for ⌘, anywhere in the table", () => {
  // The regression guard for the alias B2 used to have. `(e.metaKey || e.ctrlKey)` is the
  // right reflex on Windows and Linux and the wrong one on the only platform B2 ships on:
  // macOS gives ⌃F/⌃B/⌃N/⌃P/⌃A/⌃E emacs meanings in every text field, CodeMirror
  // implements them, and a CodeMirror binding calls `preventDefault` without
  // `stopPropagation` — so the keystroke ran the caret move *and* B2's command. ⌃E went to
  // end-of-line and left edit mode at once.
  //
  // Stated as a property rather than a list of six, so it holds for chords not yet
  // written: a binding may claim a ⌃ keystroke only by asking for ⌃.
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
  // Which is what makes the Settings rail's ⌃Tab a chord of its own rather than a second
  // spelling of something else.
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

check("⌃Tab is literal Control, and ⌘Tab is not it", () => {
  // The one chord in the app that asks for ⌃. ⌘Tab is macOS's app switcher and never
  // reaches the webview at all, which is exactly why the rail wants the other one.
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
  // Which is what makes it shadow — and conflict with — the bindings it would really take
  // the key from. A strict ⌃Tab under a scope the trap covers is not a free chord.
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
  // `Mod-Ctrl-x` is absent on purpose: ⌘⌃X is a real chord now that the two modifiers
  // are compared separately, so the parser has nothing to object to. Nothing binds it.
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
  // What the arrow families ask now that their keys are the registry's (#121). The tree's
  // six navigation commands are tried as a set, and exactly one — or none — answers.
  const nav = ["tree.row.prev", "tree.row.next", "tree.row.in", "tree.row.out"] as const;
  assertEq(boundOf(press("ArrowDown"), nav), "tree.row.next", "↓ is the next row");
  assertEq(boundOf(press("ArrowLeft"), nav), "tree.row.out", "← steps out");
  assertEq(boundOf(press("k"), nav), null, "a letter is not a move, so the caller leaves it alone");
  // And the modifier discipline holds here too: ⌘↓ is a different chord, so it falls
  // through to the global handler rather than being eaten as a tree move.
  assertEq(boundOf(press("ArrowDown", { metaKey: true }), nav), null, "⌘↓ is not ↓");
});

check("↑ means a different thing in each pane, and that is not a conflict", () => {
  // The reason the two panes get scopes of their own rather than sharing one set of
  // navigation commands. Both answer to ↑; neither is ambiguous, because only one pane
  // has the keyboard at a time — and rebinding one leaves the other alone.
  const up = press("ArrowUp");
  assert(isBound(up, "tree.row.prev"), "↑ walks the tree");
  assert(isBound(up, "side.row.prev"), "↑ walks discovery");
  assert(isBound(up, "menu.item.prev"), "↑ walks an open menu");
  assertEq(conflicts(DEFAULT_BINDINGS), [], "and the gate is unmoved by any of it");
});

// --- what a recorder may write down -----------------------------------------------------

check("isBindableKey admits what parseChord can hold, and refuses what it can't", () => {
  // The recorder asks this before spelling a chord, so the answer has to agree with the
  // parser exactly — a key that passes here and throws there is a crash in the one place
  // the user is deliberately pressing strange keys.
  for (const key of ["a", "?", "1", "Enter", "ArrowUp", "F12", "Space"]) {
    assert(isBindableKey(key), `${key} should be bindable`);
    parseChord(key); // the agreement, asserted rather than assumed
  }
  for (const key of ["Meta", "Shift", "Dead", "Unidentified", "AudioVolumeUp"]) {
    assert(!isBindableKey(key), `${key} should not be bindable`);
  }
});

check("shiftDistinguishes separates a real ⇧ from one already in the character", () => {
  // The rule the recorder has to reproduce when it writes a chord down: ⇧F10 is a chord,
  // `Shift-?` is the same keystroke as `?` said twice.
  assert(shiftDistinguishes("a"), "letters");
  assert(shiftDistinguishes("F10"), "named keys");
  assert(!shiftDistinguishes("?"), "? is what ⇧/ already reports");
  assert(!shiftDistinguishes("["), "and [ is not { ");
});

check("a row does not print the aliases the prose covers", () => {
  // nav.back also answers to ⌘←; the row says so in words rather than listing it, which
  // is the whole reason `aliases` is a separate field from `keys`.
  assertEq(displayKeys(["nav.back", "nav.forward"]), "⌘[ / ⌘]", "brackets only");
  assertEq(displayKeys(["menu.open"]), "⇧F10", "not the Menu key");
});

console.log(`bindings: ${passed} checks passed`);
