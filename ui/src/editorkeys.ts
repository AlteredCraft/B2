// The other keyboard in the app: CodeMirror's own. The note editor ships with roughly a
// hundred stock bindings B2 never wrote and doesn't list anywhere, and several of them
// land on chords B2 also uses — so "what is ⌘E bound to?" has two answers depending on
// where the keyboard is, and only one of them is in the registry.
//
// This module declares **which** stock keymaps the editor installs — main.ts spreads
// `STOCK_EDITOR_KEYMAP` rather than importing the keymaps itself, so the list here is the
// list that actually runs — and normalizes their chords into bindings.ts's model so the
// two keyboards can be compared. Adding a stock keymap to the editor (search, lint,
// close-brackets) means adding it here, which is what keeps the comparison honest.
//
// The point of comparing. Three of B2's global chords work *while editing* only because
// CodeMirror happens to leave them alone — ⌘E to leave edit mode, ⌘S to flush, ⌘F to open
// find. Nothing enforces that; it's an assumption about a dependency, restated in a
// comment beside each handler. A CodeMirror release that binds Mod-e would break ⌘E
// silently, in the one code path a user hits constantly and a test suite never does.
// `editorOverlaps` turns that assumption into a list, and editorkeys.test.ts pins it.
//
// The first thing it found was a bug rather than a risk. B2's matcher used to take ⌃X
// wherever it took ⌘X, and macOS gives ⌃F/⌃B/⌃N/⌃P/⌃A/⌃E emacs meanings in every text
// field — which CodeMirror implements. So six of B2's chords sat on standard system
// bindings, and because a CodeMirror binding calls `preventDefault` but not
// `stopPropagation`, the keystroke ran *both* commands: ⌃E moved the caret to end-of-line
// and left edit mode. The alias is gone (see `Chord.mod`); this is the check that noticed.
import { completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, historyKeymap } from "@codemirror/commands";
import type { KeyBinding } from "@codemirror/view";
import { type Binding, BINDINGS, type Scope, allKeys, keystrokes } from "./bindings.ts";

/** A stock keymap that is live in B2's editor, and how it gets installed. */
interface StockKeymap {
  readonly source: string;
  readonly keymap: readonly KeyBinding[];
}

export const STOCK_KEYMAPS: readonly StockKeymap[] = [
  // Spread into main.ts's own `keymap.of`, *after* B2's chords — so a B2 chord in the
  // same call shadows a stock binding, which is how ⌘I stays italic.
  { source: "defaultKeymap", keymap: defaultKeymap },
  { source: "historyKeymap", keymap: historyKeymap },
  // Installed by `autocompletion()` itself, at Prec.highest — above everything above.
  // It declines unless the completion menu is actually open, which is what keeps ⏎ a
  // newline the rest of the time.
  { source: "completionKeymap", keymap: completionKeymap },
];

/** What main.ts spreads into the editor's keymap after B2's own chords. `completionKeymap`
 *  is absent on purpose: `autocompletion()` installs that one itself. */
export const STOCK_EDITOR_KEYMAP: readonly KeyBinding[] = [...defaultKeymap, ...historyKeymap];

/** One stock binding, in the registry's chord syntax. */
export interface EditorChord {
  /** The chord, spelled as bindings.ts spells it — the syntaxes are the same. */
  spec: string;
  /** Which stock keymap it came from. */
  source: string;
  /** The CodeMirror command it runs, by function name — the useful half of a failure
   *  message when an upgrade takes a chord B2 was relying on. */
  command: string;
}

/** Every chord the stock keymaps bind, on macOS.
 *
 *  Two wrinkles of CodeMirror's `KeyBinding` shape are honoured here. `mac` *replaces*
 *  `key` on macOS rather than adding to it (its own `buildKeymap` reads
 *  `binding[platform] || binding.key`), and a `shift` handler is a second binding on the
 *  shifted chord that the `key` field doesn't mention. */
export function editorChords(): EditorChord[] {
  const out: EditorChord[] = [];
  for (const { source, keymap } of STOCK_KEYMAPS) {
    for (const b of keymap) {
      const spec = b.mac ?? b.key;
      if (spec === undefined) continue;
      out.push({ spec, source, command: b.run?.name || "(anonymous)" });
      if (b.shift) out.push({ spec: `Shift-${spec}`, source, command: b.shift.name || "(anonymous)" });
    }
  }
  return out;
}

/** A chord B2 binds that CodeMirror binds too. */
export interface EditorOverlap {
  /** The B2 command. */
  id: string;
  /** B2's chord, as the registry spells it. */
  chord: string;
  /** The keystrokes the two actually share — normally one, but `Any-` chords claim a
   *  spread of them, and knowing *which* is what tells you whether a row matters. */
  shared: string[];
  source: string;
  command: string;
}

/** The scopes that can be live while CodeMirror holds the keyboard, and so the only ones
 *  an overlap can actually bite in. A modal's ⏎, the tree rename field's Esc and the
 *  graph's Space all collide with a stock binding on paper, but none of those surfaces
 *  exists while the editor has focus — reporting them would bury the four that matter. */
const SCOPES_LIVE_IN_EDITOR: readonly Scope[] = ["global", "editor"];

/** Every chord B2 and CodeMirror both bind, for the scopes that coexist with the editor.
 *
 *  Reported, not judged: which side wins is a matter of install order (B2's own chords go
 *  into `keymap.of` ahead of the stock ones; the document-level handler runs only after
 *  CodeMirror's command declines), and it differs row by row. editorkeys.test.ts pins the
 *  list with that reasoning spelled out per row. */
export function editorOverlaps(bindings: readonly Binding[] = BINDINGS): EditorOverlap[] {
  const stock = editorChords().map((c) => ({ ...c, forms: new Set(keystrokes(c.spec)) }));
  const out: EditorOverlap[] = [];
  for (const b of bindings) {
    if (!SCOPES_LIVE_IN_EDITOR.includes(b.scope)) continue;
    for (const spec of allKeys(b)) {
      const mine = keystrokes(spec);
      for (const c of stock) {
        // Keystrokes, not chord equality: `Any-Escape` and CodeMirror's plain `Escape`
        // are different chords that the same key press satisfies, and that pair is one
        // of the ones worth knowing about. Reading CodeMirror's specs through B2's
        // parser is sound because the two agree on what every modifier means — `Mod` in
        // both is ⌘ on macOS. That agreement is load-bearing here and is the reason the
        // ⌃ alias had to go rather than be worked around.
        const shared = mine.filter((f) => c.forms.has(f));
        if (shared.length > 0) {
          out.push({ id: b.id, chord: spec, shared, source: c.source, command: c.command });
        }
      }
    }
  }
  return out;
}
