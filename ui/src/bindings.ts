// The keyboard registry — the one machine-readable table of the chords B2 answers to,
// and the source both the dispatcher (main.ts) and the reference sheet (shortcuts.ts)
// read. Pure data + pure functions, no DOM, so node runs its test straight off the
// source (`npm test`), like format.ts / treenav.ts / settingstabs.ts.
//
// Why it exists. A chord used to be spelled twice — once as a modifier test in main.ts's
// keydown handler (`(e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "f"`)
// and once as a row of display text in shortcuts.ts — with nothing but discipline keeping
// them equal. K1 (docs/design/invariants.md) promises every mouse action has a keyboard
// path *and* that the path is findable, so a sheet free to fall behind the wiring is a
// promise waiting to break. The repo's habit everywhere else is to close that kind of gap
// by construction rather than by convention (sanitize.ts as marked's `postprocess` hook,
// sidenav.ts's row order reused by the paint, the ui suite's globbed test files). This is
// the keyboard's version: the chord is written once, here, and the other two derive.
//
// The second thing it buys is a suite. main.ts pulls in CodeMirror and the stylesheet, so
// node can't import it, and until now *nothing* about which chord is bound to what was
// testable at all — the app's whole keyboard contract sat in the one file the suite
// can't see. Everything in this module is data or a pure function over data.
//
// What lives here and what doesn't. The rule is **who owns the key → action mapping**:
//
//   - Here: every chord the global keydown handler matches by hand.
//   - Not here: the arrow-navigation families, whose key → move mapping is already owned
//     and tested by a pure module of its own — treenav.ts (`arrowMove`), sidenav.ts
//     (`sideArrowMove`), settingstabs.ts (`tabMove`). Copying their keys in would recreate
//     exactly the two-sources-of-truth problem this module exists to end, so the sheet
//     carries them as literal rows instead and shortcuts.ts says why.
//
// The chord syntax is CodeMirror's (`Mod-Shift-v`) on purpose: the editor's own bindings
// come out of this same table and are handed to `keymap.of` verbatim, so there is one
// spelling of a chord for both halves of the app. Sharing the syntax means sharing the
// *semantics* too — `Mod` is ⌘ here because `Mod` is ⌘ there (CodeMirror's
// `normalizeKeyName` resolves it to meta on mac). That agreement is load-bearing, and
// see `Chord.mod` for what it cost to notice. `Any-` is B2's one addition to the syntax,
// and the one thing that must not be handed to CodeMirror.

/** Where a chord applies. `global` is the whole window; the rest are surfaces inside it,
 *  each named after the state that turns it on. `:` nests (`overlay:link` is inside
 *  `overlay`), which is what lets two ⏎ bindings coexist without ambiguity. */
export type Scope =
  | "global"
  | "editor" // CodeMirror has the keyboard (state.editing)
  | "fm" // the frontmatter drawer's mini-editor (state.fmEditing)
  | "find" // the find bar is open (findOpen)
  | "graph" // a graph node holds focus
  // The overlay layer. `currentOverlay()` returns exactly one of these or null — the
  // openers all run `dismissOverlays` first, so they can never stack — which is what
  // makes them siblings, and what makes two ⏎ bindings unambiguous rather than a
  // collision. They nest under `overlay` because the Tab trap is bound to the *layer*:
  // it takes Tab from whichever one is up, so it shadows all of them.
  | "overlay"
  | "overlay:settings"
  | "overlay:menu"
  | "overlay:link"
  | "overlay:delete"
  | "textentry:create"
  | "textentry:rename"
  | "textentry:find";

/** One command and the chords that fire it. Deliberately *only* chord, scope and id —
 *  the conditions under which a chord applies stay ordinary code in main.ts's handler.
 *  A table rich enough to express "⌘G, but only while the find bar is open and not in a
 *  text field" is a `when`-clause expression language, which is a worse trade at this
 *  size than a guard you can read. */
export interface Binding {
  /** Stable command id. Referenced by main.ts's dispatcher and shortcuts.ts's rows. */
  readonly id: string;
  /** The chords that fire it, in the order the sheet shows them. */
  readonly keys: readonly string[];
  /** Chords that also fire it but the sheet doesn't list, because the row's prose
   *  already mentions them ("Back / forward (⌘← / ⌘→ too)"). */
  readonly aliases?: readonly string[];
  readonly scope: Scope;
}

// --- the table ---------------------------------------------------------------------

export const BINDINGS = [
  // Global — the app's own chords, matched by the document-level keydown handler.
  { id: "find.open", keys: ["Mod-f"], scope: "global" },
  { id: "search.focus", keys: ["Mod-Shift-f"], scope: "global" },
  { id: "tree.new-note", keys: ["Mod-n"], scope: "global" },
  { id: "tree.new-folder", keys: ["Mod-Shift-n"], scope: "global" },
  // The Menu key is the same gesture on a keyboard that has one; only ⇧F10 is listed.
  { id: "menu.open", keys: ["Shift-F10"], aliases: ["ContextMenu"], scope: "global" },
  { id: "tree.rename", keys: ["F2"], scope: "global" },
  { id: "help.keyboard", keys: ["?"], scope: "global" },
  { id: "pane.tree", keys: ["Mod-1"], scope: "global" },
  { id: "pane.note", keys: ["Mod-2"], scope: "global" },
  { id: "pane.discovery", keys: ["Mod-3"], scope: "global" },
  { id: "delete.focused", keys: ["Mod-Backspace"], scope: "global" },
  { id: "settings.toggle", keys: ["Mod-,"], scope: "global" },
  { id: "anomalies.toggle", keys: ["Mod-Shift-a"], scope: "global" },
  { id: "edit.toggle", keys: ["Mod-e"], scope: "global" },
  // `Any-` because Escape is the way out and must not be conditional on what else is
  // held down: a user who just pressed ⌘F and hasn't let go of ⌘ yet still gets out.
  { id: "dismiss", keys: ["Any-Escape"], scope: "global" },
  { id: "nav.back", keys: ["Mod-["], aliases: ["Mod-ArrowLeft"], scope: "global" },
  { id: "nav.forward", keys: ["Mod-]"], aliases: ["Mod-ArrowRight"], scope: "global" },

  // The find bar, once it's open.
  { id: "find.next", keys: ["Mod-g"], scope: "find" },
  { id: "find.prev", keys: ["Mod-Shift-g"], scope: "find" },

  // The editor. The first four are handed to CodeMirror's own keymap (ahead of its
  // defaults, so they win); ⌘S is the document handler's, and reaches it only because
  // CodeMirror leaves Mod-s unbound — editorkeys.test.ts is what keeps that true.
  { id: "format.bold", keys: ["Mod-b"], scope: "editor" },
  { id: "format.italic", keys: ["Mod-i"], scope: "editor" },
  { id: "editor.table", keys: ["Mod-t"], scope: "editor" },
  { id: "editor.paste-plain", keys: ["Mod-Shift-v"], scope: "editor" },
  { id: "editor.save", keys: ["Mod-s"], scope: "editor" },

  // The frontmatter drawer — a separate surface from the body editor, hence its own
  // scope: ⌘S means "save this drawer" here and "flush the note" there.
  { id: "fm.save", keys: ["Mod-Enter"], aliases: ["Mod-s"], scope: "fm" },

  // The overlay layer. `Any-Tab` for the same reason as `Any-Escape`: the trap's contract
  // is that *no* Tab walks the page behind the backdrop, whatever rode along with it.
  { id: "overlay.focus.step", keys: ["Any-Tab"], scope: "overlay" },
  { id: "link.commit", keys: ["Enter"], scope: "overlay:link" },
  { id: "delete.confirm", keys: ["Enter"], scope: "overlay:delete" },
  { id: "menu.item.next", keys: ["ArrowDown"], scope: "overlay:menu" },
  { id: "menu.item.prev", keys: ["ArrowUp"], scope: "overlay:menu" },
  { id: "settings.section.next", keys: ["Ctrl-Tab"], scope: "overlay:settings" },
  { id: "settings.section.prev", keys: ["Ctrl-Shift-Tab"], scope: "overlay:settings" },

  // SVG has no native button activation, so the graph binds what the platform would
  // otherwise give a <button> for free.
  { id: "graph.activate", keys: ["Enter", "Space"], scope: "graph" },

  // Text entry. ⏎ commits and Esc backs out — the platform reflex, in the three inline
  // surfaces that need it. Exempt from the sheet (shortcuts.ts says why).
  { id: "create.commit", keys: ["Enter"], scope: "textentry:create" },
  { id: "create.cancel", keys: ["Any-Escape"], scope: "textentry:create" },
  { id: "rename.commit", keys: ["Enter"], scope: "textentry:rename" },
  { id: "rename.cancel", keys: ["Any-Escape"], scope: "textentry:rename" },
  { id: "find.input.next", keys: ["Enter"], scope: "textentry:find" },
  { id: "find.input.prev", keys: ["Shift-Enter"], scope: "textentry:find" },
  { id: "find.input.close", keys: ["Any-Escape"], scope: "textentry:find" },
] as const satisfies readonly Binding[];

/** Every command id in the table, as a type — so a typo in main.ts or shortcuts.ts is a
 *  compile error (`tsc --noEmit` runs in `npm run build`, which `just ci` runs) rather
 *  than a chord that silently stops working. */
export type BindingId = (typeof BINDINGS)[number]["id"];

// --- chords ------------------------------------------------------------------------

/** A chord in normalized form: one key plus the modifiers held with it. */
export interface Chord {
  /** Canonical key name — a lowercase character, or a named key ("Enter", "ArrowUp"). */
  key: string;
  /** ⌘ — and only ⌘.
   *
   *  The handlers used to read `metaKey || ctrlKey`, taking ⌃ as a synonym. That's the
   *  right reflex on Windows and Linux, where ⌃ *is* the platform modifier, and the wrong
   *  one here: B2 ships on macOS (crates/b2-desktop), where ⌃ is not spare. Cocoa gives
   *  ⌃F/⌃B/⌃N/⌃P/⌃A/⌃E their emacs meanings in every text field on the machine, and
   *  CodeMirror implements the same set so its editor feels native — so the alias put six
   *  of B2's chords on top of standard system bindings. ⌃E ran `cursorLineEnd` *and* left
   *  edit mode, from two listeners, because CodeMirror's keymap handler calls
   *  `preventDefault` but never `stopPropagation`, so the event still reached the
   *  document. See editorkeys.ts.
   *
   *  Dropping it also makes `Mod` mean the same thing here as it does in CodeMirror
   *  (`normalizeKeyName` resolves it to meta on mac), which matters because the two share
   *  this syntax — `chordFor` hands specs straight to `keymap.of`. */
  mod: boolean;
  /** ⌃ — the Settings rail's ⌃Tab is the only chord that asks for it. */
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  /** This key, *whatever* is held with it (`Any-Escape`).
   *
   *  Two bindings want it, and both for the same reason: their contract is about the key
   *  rather than the chord. Escape is the way out of any surface, and the overlay's Tab
   *  trap exists so that no Tab reaches the page behind the backdrop — a modifier the
   *  user happens to still be holding must not defeat either. Without this they'd be
   *  special cases in the handler, matching on `e.key` behind the registry's back, and
   *  the registry would no longer describe what the app actually does. */
  any: boolean;
}

/** The subset of KeyboardEvent this module reads, so the matcher stays testable in node. */
export interface KeyEventLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

const NAMED_KEYS = new Set([
  "Enter",
  "Escape",
  "Tab",
  "Space",
  "Backspace",
  "Delete",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ContextMenu",
]);

const isNamed = (key: string): boolean => NAMED_KEYS.has(key) || /^F\d{1,2}$/.test(key);

/** `KeyboardEvent.key` in the table's spelling: single characters lowercased (⇧F arrives
 *  as "F"), the space bar named rather than written as a literal " ". */
export function canonicalKey(raw: string): string {
  if (raw === " ") return "Space";
  return raw.length === 1 ? raw.toLowerCase() : raw;
}

/** Does ⇧ distinguish this key from itself?
 *
 *  For letters, digits and named keys, yes: ⇧F10 and F10 are different chords. For a
 *  symbol it must not be asked, because the browser has *already* applied the shift —
 *  `?` is what ⇧/ reports, and `{` is what ⇧[ reports. Requiring `shift: true` on top of
 *  a key that only exists when shift is down would double-count it, and requiring
 *  `shift: false` would make the chord unfireable. */
function shiftDistinguishes(key: string): boolean {
  return /^[a-z0-9]$/.test(key) || isNamed(key);
}

/** Parse a chord in CodeMirror's syntax (`Mod-Shift-v`, `F2`, `Ctrl-Tab`), plus B2's one
 *  extension to it, `Any-`.
 *
 *  The extension is why `chordFor` is only safe to hand to `keymap.of` for chords
 *  CodeMirror could have parsed itself — editorkeys.test.ts holds that line for the four
 *  the editor actually installs.
 *
 *  Throws on anything it doesn't recognize. A typo in the table is a chord that never
 *  fires, which is invisible at runtime and obvious here — so this fails the suite the
 *  moment the table is imported rather than shipping a dead key. */
export function parseChord(spec: string): Chord {
  const parts = spec.split("-");
  const raw = parts.pop();
  if (raw === undefined || raw === "") throw new Error(`chord has no key: ${spec}`);
  const chord: Chord = {
    key: canonicalKey(raw),
    mod: false,
    ctrl: false,
    shift: false,
    alt: false,
    any: false,
  };
  for (const part of parts) {
    switch (part) {
      case "Any":
        chord.any = true;
        break;
      case "Mod":
      case "Cmd":
      case "Meta":
        chord.mod = true;
        break;
      case "Ctrl":
      case "Control":
        chord.ctrl = true;
        break;
      case "Shift":
        chord.shift = true;
        break;
      case "Alt":
      case "Option":
        chord.alt = true;
        break;
      default:
        throw new Error(`unknown modifier "${part}" in chord: ${spec}`);
    }
  }
  if (chord.key.length > 1 && !isNamed(chord.key)) {
    throw new Error(`unknown key "${chord.key}" in chord: ${spec}`);
  }
  if (chord.any && (chord.mod || chord.ctrl || chord.shift || chord.alt)) {
    // "Any modifier" and "this exact modifier" are the same contradiction the other way.
    throw new Error(`chord combines Any with a named modifier: ${spec}`);
  }
  return chord;
}

/** Does this event press this chord? */
export function chordMatches(chord: Chord, e: KeyEventLike): boolean {
  if (chord.key !== canonicalKey(e.key)) return false;
  if (chord.any) return true;
  if (chord.alt !== e.altKey) return false;
  if (shiftDistinguishes(chord.key) && chord.shift !== e.shiftKey) return false;
  // Every modifier compared, none aliased: ⌃X is ⌃X and ⌘X is ⌘X. A chord that doesn't
  // ask for ⌃ must not answer to it — that's what keeps B2 off the system's own ⌃
  // bindings, and it's the whole of the fix described on `Chord.mod`.
  return chord.mod === e.metaKey && chord.ctrl === e.ctrlKey;
}

// --- lookup ------------------------------------------------------------------------

const BY_ID = new Map<string, Binding>(BINDINGS.map((b) => [b.id, b]));

/** Every chord that fires a binding — what the sheet shows, plus the quiet aliases. */
export function allKeys(b: Binding): readonly string[] {
  return b.aliases ? [...b.keys, ...b.aliases] : b.keys;
}

function binding(id: string): Binding {
  const b = BY_ID.get(id);
  if (!b) throw new Error(`no such binding: ${id}`);
  return b;
}

/** The chord a binding is fired by, in the table's own syntax — which is CodeMirror's,
 *  so this feeds `keymap.of` directly. */
export function chordFor(id: string): string {
  return binding(id).keys[0];
}

/** Does this event press the chord bound to `id`?
 *
 *  This is the whole of what main.ts's handler asks the registry. *Whether* the command
 *  should run — an overlay owns the keyboard, the tree has no focused row, the buffer
 *  isn't dirty — stays a guard beside the call, in the reading order the handler already
 *  had. */
export function isBound(e: KeyEventLike, id: BindingId): boolean {
  return allKeys(binding(id)).some((k) => chordMatches(parseChord(k), e));
}

// --- collisions --------------------------------------------------------------------

/** The physical key presses a chord answers to.
 *
 *  One, for every chord except `Any-` — which claims the lot, and is the reason this
 *  returns a list at all. Comparing *these* rather than the chords themselves is what
 *  makes "do these two answer to the same keystroke?" a set intersection, and it's why
 *  editorkeys.ts can ask the same question of a keymap B2 didn't write, whose chords are
 *  spelled by someone else's conventions. */
export function keystrokes(spec: string): string[] {
  return physicalForms(parseChord(spec));
}

/** Do two chords answer to any of the same keystrokes? */
export function chordsOverlap(a: string, b: string): boolean {
  const forms = new Set(keystrokes(a));
  return keystrokes(b).some((f) => forms.has(f));
}

/** One chord, as one keystroke string — modifiers in Apple's order, then the key. */
function formOf(chord: Chord): string {
  return (
    (chord.ctrl ? "⌃" : "") +
    (chord.alt ? "⌥" : "") +
    (shiftDistinguishes(chord.key) && chord.shift ? "⇧" : "") +
    (chord.mod ? "⌘" : "") +
    chord.key
  );
}

function physicalForms(chord: Chord): string[] {
  if (!chord.any) return [formOf(chord)];
  // An `Any-` chord answers to every keystroke over its key, so it claims every form a
  // strict chord over that key could produce — enumerated through the same `formOf`, so
  // the two can't drift apart into a near-miss that never intersects.
  const out: string[] = [];
  for (const ctrl of [false, true]) {
    for (const alt of [false, true]) {
      for (const shift of shiftDistinguishes(chord.key) ? [false, true] : [false]) {
        for (const mod of [false, true]) {
          out.push(formOf({ ...chord, any: false, ctrl, alt, shift, mod }));
        }
      }
    }
  }
  return out;
}

/** `global` contains every scope; otherwise containment is the `:` namespace prefix, so
 *  `modal` contains `modal:link` and siblings contain nothing. */
export function scopeContains(outer: Scope | string, inner: Scope | string): boolean {
  return outer === inner || outer === "global" || inner.startsWith(`${outer}:`);
}

/** Two commands in the same scope that answer to the same keystroke. Which one runs
 *  depends on the order of the handler's branches, which is not a contract anyone can
 *  read — so this is the error tier, and bindings.test.ts fails the suite on a non-empty
 *  result. That check is the gate: `npm test` runs in both `just check` and `just ci`.
 *
 *  Keystrokes rather than chords, because the two aren't the same question: `Any-Escape`
 *  and `Escape` are different chords that the same key press satisfies. */
export interface Conflict {
  a: string;
  b: string;
  /** The keystroke they both answer to, e.g. "⌘f". */
  form: string;
  scope: string;
}

/** One command shadowing another it sits inside — a scoped chord taking a keystroke the
 *  global one would otherwise get (Esc in the rename field, not the overlay cascade).
 *
 *  Legal and mostly deliberate: the inner surface is nearer the user, and B2's pane
 *  handlers are bound to their pane precisely so they answer first. Reported, never
 *  failed — it's what a customization UI would warn about, not something CI can judge. */
export interface Shadow {
  outer: string;
  inner: string;
  form: string;
}

interface Claim {
  id: string;
  scope: string;
  form: string;
}

function claims(bindings: readonly Binding[]): Claim[] {
  const out: Claim[] = [];
  for (const b of bindings) {
    for (const spec of allKeys(b)) {
      for (const form of physicalForms(parseChord(spec))) {
        out.push({ id: b.id, scope: b.scope, form });
      }
    }
  }
  return out;
}

/** Every same-scope keystroke clash in a table. Empty for BINDINGS, and kept that way. */
export function conflicts(bindings: readonly Binding[] = BINDINGS): Conflict[] {
  const all = claims(bindings);
  const found: Conflict[] = [];
  const seen = new Set<string>();
  for (let i = 0; i < all.length; i++) {
    for (let j = i + 1; j < all.length; j++) {
      const [x, y] = [all[i], all[j]];
      if (x.id === y.id || x.form !== y.form || x.scope !== y.scope) continue;
      // One row per colliding *pair*, not per overlapping keystroke: a Mod-vs-Mod clash
      // meets on both ⌘X and ⌃X, and saying so twice is noise around one problem.
      const key = `${x.id} ${y.id} ${x.scope}`;
      if (seen.has(key)) continue;
      seen.add(key);
      found.push({ a: x.id, b: y.id, form: x.form, scope: x.scope });
    }
  }
  return found;
}

/** Every keystroke an inner scope takes from an outer one. Advisory — see `Shadow`. */
export function shadows(bindings: readonly Binding[] = BINDINGS): Shadow[] {
  const all = claims(bindings);
  const found: Shadow[] = [];
  const seen = new Set<string>();
  for (const x of all) {
    for (const y of all) {
      if (x.id === y.id || x.form !== y.form) continue;
      if (x.scope === y.scope || !scopeContains(x.scope, y.scope)) continue;
      const key = `${x.id} ${y.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      found.push({ outer: x.id, inner: y.id, form: x.form });
    }
  }
  return found;
}

// --- display -----------------------------------------------------------------------

// macOS spells out the keys its own menus spell out and draws glyphs for the rest —
// the split shortcuts.ts has always followed, now in one place. ⎋ and ⇥ exist; nobody
// reads them, so Esc and Tab stay words.
const KEY_GLYPHS: Record<string, string> = {
  Enter: "⏎",
  Backspace: "⌫",
  Delete: "⌦",
  Escape: "Esc",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  ContextMenu: "Menu",
};

/** One chord as the sheet prints it: ⌃⌥⇧⌘ then the key, which is the order Apple's HIG
 *  puts modifiers in and the order every macOS menu shows them. */
export function displayChord(spec: string): string {
  const c = parseChord(spec);
  // An `Any-` chord prints bare: "Esc", not a list of the twelve ways to hold it.
  const mods = c.any
    ? ""
    : (c.ctrl ? "⌃" : "") +
      (c.alt ? "⌥" : "") +
      (shiftDistinguishes(c.key) && c.shift ? "⇧" : "") +
      (c.mod ? "⌘" : "");
  const key = KEY_GLYPHS[c.key] ?? (c.key.length === 1 ? c.key.toUpperCase() : c.key);
  return `${mods}${key}`;
}

/** The chords for a row of the sheet, printed as one cell: "⌘G / ⇧⌘G".
 *
 *  Distinct renderings only, so a row covering two commands that share a chord — ⏎
 *  commits the link dialog *and* the delete confirm — reads "⏎" rather than "⏎ / ⏎". */
export function displayKeys(ids: readonly string[]): string {
  const out: string[] = [];
  for (const id of ids) {
    for (const spec of binding(id).keys) {
      const text = displayChord(spec);
      if (!out.includes(text)) out.push(text);
    }
  }
  return out.join(" / ");
}
