// The editor's other keyboard: CodeMirror's stock bindings. This module declares which
// stock keymaps the editor installs (main.ts spreads `STOCK_EDITOR_KEYMAP`, so this list is
// what runs; add any new stock keymap here) and normalizes their chords into bindings.ts's
// model so the two keyboards can be compared.
//
// Some B2 chords (⌘E, ⌘S, ⌘F) work while editing only because CodeMirror leaves them
// alone. `editorOverlaps` turns that assumption into a list that editorkeys.test.ts pins.
import { completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, historyKeymap } from "@codemirror/commands";
import { markdownKeymap } from "@codemirror/lang-markdown";
import type { KeyBinding } from "@codemirror/view";
import { type Binding, type Scope, activeBindings, allKeys, keystrokes } from "./bindings.ts";

/** A stock keymap that is live in B2's editor, and how it gets installed. */
interface StockKeymap {
  readonly source: string;
  readonly keymap: readonly KeyBinding[];
}

export const STOCK_KEYMAPS: readonly StockKeymap[] = [
  // Spread after B2's chords in main.ts's `keymap.of`, so B2 shadows them (⌘I stays italic).
  { source: "defaultKeymap", keymap: defaultKeymap },
  { source: "historyKeymap", keymap: historyKeymap },
  // Installed by `autocompletion()` at Prec.highest; declines unless the menu is open.
  { source: "completionKeymap", keymap: completionKeymap },
  // Installed by `markdown()` at Prec.high, above B2's chords, so it must be listed for
  // "CodeMirror leaves Tab alone" (`editor.list.indent`) to be checked.
  { source: "markdownKeymap", keymap: markdownKeymap },
];

/** What main.ts spreads after B2's chords. Excludes keymaps their extensions install. */
export const STOCK_EDITOR_KEYMAP: readonly KeyBinding[] = [...defaultKeymap, ...historyKeymap];

/** One stock binding, in the registry's chord syntax. */
export interface EditorChord {
  /** The chord, in bindings.ts's syntax. */
  spec: string;
  /** Which stock keymap it came from. */
  source: string;
  /** The CodeMirror command it runs, by function name, for failure messages. */
  command: string;
}

/** Every chord the stock keymaps bind, on macOS. `mac` replaces `key` rather than adding
 *  to it, and a `shift` handler is a second binding on the shifted chord. */
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
  /** The keystrokes the two share; an `Any-` chord can share several. */
  shared: string[];
  source: string;
  command: string;
}

/** The scopes that can be live while CodeMirror holds the keyboard, the only ones where an
 *  overlap can bite. */
const SCOPES_LIVE_IN_EDITOR: readonly Scope[] = ["global", "editor"];

/** Every chord B2 and CodeMirror both bind, in scopes that coexist with the editor.
 *  Reported, not judged: who wins differs row by row (editorkeys.test.ts says why). */
export function editorOverlaps(bindings: readonly Binding[] = activeBindings()): EditorOverlap[] {
  const stock = editorChords().map((c) => ({ ...c, forms: new Set(keystrokes(c.spec)) }));
  const out: EditorOverlap[] = [];
  for (const b of bindings) {
    if (!SCOPES_LIVE_IN_EDITOR.includes(b.scope)) continue;
    for (const spec of allKeys(b)) {
      const mine = keystrokes(spec);
      for (const c of stock) {
        // Keystrokes, not chord equality: `Any-Escape` and `Escape` share a key press.
        // Sound only because both parsers agree on every modifier (`Mod` is ⌘ on macOS).
        const shared = mine.filter((f) => c.forms.has(f));
        if (shared.length > 0) {
          out.push({ id: b.id, chord: spec, shared, source: c.source, command: c.command });
        }
      }
    }
  }
  return out;
}
