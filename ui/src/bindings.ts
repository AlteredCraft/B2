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
//   - Here: every chord the global keydown handler matches by hand, and — since #121 —
//     the arrow-navigation families too. treenav.ts (`arrowMove`), sidenav.ts
//     (`sideArrowMove`) and settingstabs.ts (`tabMove`) used to own their own key → move
//     mapping, and that was right while the keyboard was fixed: one owner per mapping is
//     this module's whole rule. It stopped being right the moment a user could rebind,
//     because it made the arrows the one part of the keyboard nobody could change and the
//     checker couldn't see. So the split moved rather than collapsed: the registry owns
//     **key → command**, and each of those modules still owns **command → move** — its
//     function now switches on a binding id (`tree.row.next`) instead of a key name
//     ("ArrowDown"). Still one owner per mapping, and now both halves are visible.
//   - Not here: the app menu bar's chords — ⌘Q ⌘W ⌘M ⌘H ⌥⌘H ⌘Z ⇧⌘Z ⌘X ⌘C ⌘V ⌘A
//     ⌃⌘F. Those aren't key → action mappings this file could own at all: the host
//     declares the menu (crates/b2-desktop/src/menu.rs) and AppKit dispatches its
//     accelerators before the key window's responder chain, so the webview never gets a
//     keydown for them. What they are to *this* table is a list of keystrokes a new
//     binding may not be spelled with, and menukeys.ts is where that gate lives — a
//     separate check because `conflicts()` asks a same-scope question and these are taken
//     from every scope at once (#119).
//
// The table is the **default** keyboard, not necessarily the live one. `DEFAULT_BINDINGS`
// is what B2 ships; `activeBindings()` is what it currently answers to, which is the
// defaults with the user's rebindings laid over them (keymap.ts owns that algebra and its
// persistence). Everything downstream — `isBound`, the sheet, the conflict checkers —
// reads the *active* table, so a rebound chord moves in the dispatcher, in the reference
// and in the gate's opinion of the keyboard all at once. The suite passes tables
// explicitly, which is what keeps a global mutable registry testable.
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
  // The two navigable panes. Their listeners are bound to the pane rather than the
  // document, so these answer before anything in `global` — and they are siblings, which
  // is what lets ↑↓ mean "next row" in both without the checker calling it a clash.
  | "tree" // the keyboard is on a file-tree row
  | "side" // the keyboard is on a discovery row
  // The overlay layer. `currentOverlay()` returns exactly one of these or null — the
  // openers all run `dismissOverlays` first, so they can never stack — which is what
  // makes them siblings, and what makes two ⏎ bindings unambiguous rather than a
  // conflict. They nest under `overlay` because the Tab trap is bound to the *layer*:
  // it takes Tab from whichever one is up, so it shadows all of them.
  | "overlay"
  | "overlay:settings"
  | "overlay:menu"
  | "overlay:link"
  | "overlay:delete"
  | "textentry:create"
  | "textentry:rename"
  | "textentry:find"
  | "textentry:chat"; // the chat composer (GH #155)

/** One command and the chords that fire it. Deliberately *only* chord, scope and id (plus
 *  the two display strings below) — the conditions under which a chord applies stay
 *  ordinary code in main.ts's handler. A table rich enough to express "⌘G, but only while
 *  the find bar is open and not in a text field" is a `when`-clause expression language,
 *  which is a worse trade at this size than a guard you can read. That holds *after* #121
 *  too: a user-defined chord changes **which** keystroke fires a command, never **when**
 *  the command applies. */
export interface Binding {
  /** Stable command id. Referenced by main.ts's dispatcher and shortcuts.ts's rows. */
  readonly id: string;
  /** What this command is, in the user's words.
   *
   *  The sheet's prose is per *row* and a row often covers several commands ("Focus the
   *  files, the note, or discovery" is three), so it can't name the one being rebound.
   *  The recorder has to — "Change the chord for ⟨…⟩", "⌘F is already ⟨…⟩" — and a
   *  conflict message that named `pane.discovery` would be a stack trace, not a sentence. */
  readonly label: string;
  /** The chords that fire it, in the order the sheet shows them. */
  readonly keys: readonly string[];
  /** Chords that also fire it but the sheet doesn't list, because the row's prose
   *  already mentions them ("Back / forward (⌘← / ⌘→ too)").
   *
   *  A rebinding replaces `keys` and leaves these alone: an alias is a second way in that
   *  the sheet already documents in words, and the user choosing the chord the sheet
   *  *prints* is not the same act as deleting the app's conveniences. They still count as
   *  claimed keystrokes, so rebinding another command onto ⌘← is refused, not silent. */
  readonly aliases?: readonly string[];
  readonly scope: Scope;
  /** Present ⇒ not rebindable, and this is the reason, shown where the recorder would be.
   *
   *  These are the chords that are the **platform's reflex rather than B2's choice**: ⏎
   *  and Esc in a text field, ⏎ on a dialog's default button, the ⏎/Space a `<button>`
   *  would answer to if SVG had native activation. Offering to rebind them would be
   *  offering to break the thing every other app on the machine does. The two `Any-`
   *  chords are here for a second reason on top: a recorder observes one keystroke, and
   *  "this key with anything held" is not a keystroke anyone can press. */
  readonly fixed?: string;
}

// --- the table ---------------------------------------------------------------------

const TEXT_FIELD_REFLEX = "⏎ and Esc are what a text field does — B2 doesn't get to choose them.";
const DIALOG_DEFAULT = "⏎ is the open dialog's default button.";

/** The keyboard B2 ships with. The *live* one is `activeBindings()`. */
export const DEFAULT_BINDINGS = [
  // Global — the app's own chords, matched by the document-level keydown handler.
  { id: "find.open", label: "Find in this note", keys: ["Mod-f"], scope: "global" },
  { id: "search.focus", label: "Search the vault", keys: ["Mod-Shift-f"], scope: "global" },
  { id: "tree.new-note", label: "New note", keys: ["Mod-n"], scope: "global" },
  { id: "tree.new-folder", label: "New folder", keys: ["Mod-Shift-n"], scope: "global" },
  // The Menu key is the same gesture on a keyboard that has one; only ⇧F10 is listed.
  {
    id: "menu.open",
    label: "Open the focused row's menu",
    keys: ["Shift-F10"],
    aliases: ["ContextMenu"],
    scope: "global",
  },
  { id: "tree.rename", label: "Rename the focused row", keys: ["F2"], scope: "global" },
  { id: "help.keyboard", label: "The keyboard reference", keys: ["?"], scope: "global" },
  { id: "pane.tree", label: "Focus the file tree", keys: ["Mod-1"], scope: "global" },
  { id: "pane.note", label: "Focus the note", keys: ["Mod-2"], scope: "global" },
  { id: "pane.discovery", label: "Focus discovery", keys: ["Mod-3"], scope: "global" },
  { id: "delete.focused", label: "Delete the focused row", keys: ["Mod-Backspace"], scope: "global" },
  { id: "settings.toggle", label: "Settings", keys: ["Mod-,"], scope: "global" },
  { id: "edit.toggle", label: "Enter or leave edit mode", keys: ["Mod-e"], scope: "global" },
  // The `</>` chip's chord, and ⌘E's shift-sibling on purpose: ⌘E changes whether you're
  // editing, ⇧⌘E changes what you're looking at — and both work in either mode, because
  // the source flip is one sticky flag serving the reading view and the editor alike.
  // ⇧ as "the same idea, one step over" is the pattern ⌘N/⇧⌘N and ⌘F/⇧⌘F already set.
  {
    id: "source.toggle",
    label: "Show the Markdown source",
    keys: ["Mod-Shift-e"],
    scope: "global",
  },
  // ⌘G is Find Next everywhere on macOS, and `find.next` below keeps it — but only while
  // the find bar is open, which is the only time that meaning exists. The rest of the
  // time the chord is free, so the graph gets it: `shadows()` reports the pair and
  // bindings.test.ts pins it as deliberate rather than accumulated.
  {
    id: "graph.toggle",
    label: "Show or hide the connection graph",
    keys: ["Mod-g"],
    scope: "global",
  },
  // Chat takes the right column, so its chord sits with the other things that change what
  // a *pane* shows (⌘G, ⇧⌘E) rather than with ⌘1/⌘2/⌘3, which change where the keyboard
  // is. ⌘J is free in every one of the four keyboards that can claim a chord here — B2's
  // own, CodeMirror's, the menu bar's, and macOS's — which is what the checkers assert.
  { id: "chat.toggle", label: "Ask your notes", keys: ["Mod-j"], scope: "global" },
  // `Any-` because Escape is the way out and must not be conditional on what else is
  // held down: a user who just pressed ⌘F and hasn't let go of ⌘ yet still gets out.
  {
    id: "dismiss",
    label: "Close a menu, a dialog, the find bar, or the graph",
    keys: ["Any-Escape"],
    scope: "global",
    fixed: "Esc is the way out of every surface, and answers with any modifier still held.",
  },
  { id: "nav.back", label: "Back", keys: ["Mod-["], aliases: ["Mod-ArrowLeft"], scope: "global" },
  {
    id: "nav.forward",
    label: "Forward",
    keys: ["Mod-]"],
    aliases: ["Mod-ArrowRight"],
    scope: "global",
  },

  // The find bar, once it's open.
  { id: "find.next", label: "Next match", keys: ["Mod-g"], scope: "find" },
  { id: "find.prev", label: "Previous match", keys: ["Mod-Shift-g"], scope: "find" },

  // The editor. All but ⌘S are handed to CodeMirror's own keymap (ahead of its defaults,
  // so they win); ⌘S is the document handler's, and reaches it only because CodeMirror
  // leaves Mod-s unbound — editorkeys.test.ts is what keeps that true.
  { id: "format.bold", label: "Bold", keys: ["Mod-b"], scope: "editor" },
  { id: "format.italic", label: "Italic", keys: ["Mod-i"], scope: "editor" },
  { id: "editor.table", label: "Insert a table", keys: ["Mod-t"], scope: "editor" },
  { id: "editor.paste-plain", label: "Paste as plain text", keys: ["Mod-Shift-v"], scope: "editor" },
  { id: "editor.save", label: "Save now", keys: ["Mod-s"], scope: "editor" },
  // Tab, the one chord here that is also the *platform's*. It is claimed only with the
  // caret in a list item (list.ts declines otherwise), so Tab still walks the focus ring
  // everywhere else in the buffer. The overlay layer's own `Any-Tab` trap is a sibling
  // scope rather than a clash: a dialog holds the keyboard while it's up, so CodeMirror
  // never sees the key it would take — which is exactly what the trap is for.
  { id: "editor.list.indent", label: "Nest a list item", keys: ["Tab"], scope: "editor" },
  {
    id: "editor.list.outdent",
    label: "Lift a list item out",
    keys: ["Shift-Tab"],
    scope: "editor",
  },

  // The frontmatter drawer — a separate surface from the body editor, hence its own
  // scope: ⌘S means "save this drawer" here and "flush the note" there.
  {
    id: "fm.save",
    label: "Save the frontmatter drawer",
    keys: ["Mod-Enter"],
    aliases: ["Mod-s"],
    scope: "fm",
  },

  // The overlay layer. `Any-Tab` for the same reason as `Any-Escape`: the trap's contract
  // is that *no* Tab walks the page behind the backdrop, whatever rode along with it.
  {
    id: "overlay.focus.step",
    label: "Step through the controls on screen",
    keys: ["Any-Tab"],
    scope: "overlay",
    fixed: "The focus trap has to claim Tab itself, or Tab walks the page behind the dialog.",
  },
  {
    id: "link.commit",
    label: "Commit the Link… dialog",
    keys: ["Enter"],
    scope: "overlay:link",
    fixed: DIALOG_DEFAULT,
  },
  {
    id: "delete.confirm",
    label: "Confirm the delete",
    keys: ["Enter"],
    scope: "overlay:delete",
    fixed: DIALOG_DEFAULT,
  },
  { id: "menu.item.next", label: "Next item in an open menu", keys: ["ArrowDown"], scope: "overlay:menu" },
  { id: "menu.item.prev", label: "Previous item in an open menu", keys: ["ArrowUp"], scope: "overlay:menu" },
  {
    id: "settings.section.next",
    label: "Next Settings section, from anywhere in Settings",
    keys: ["Ctrl-Tab"],
    scope: "overlay:settings",
  },
  {
    id: "settings.section.prev",
    label: "Previous Settings section, from anywhere in Settings",
    keys: ["Ctrl-Shift-Tab"],
    scope: "overlay:settings",
  },
  // The Settings rail's own walk — the ARIA `tabs` pattern, live only with the keyboard
  // on a tab. settingstabs.ts turns these ids into moves.
  { id: "settings.tab.prev", label: "Previous section, on the rail", keys: ["ArrowUp"], scope: "overlay:settings" },
  { id: "settings.tab.next", label: "Next section, on the rail", keys: ["ArrowDown"], scope: "overlay:settings" },
  { id: "settings.tab.first", label: "First section", keys: ["Home"], scope: "overlay:settings" },
  { id: "settings.tab.last", label: "Last section", keys: ["End"], scope: "overlay:settings" },

  // SVG has no native button activation, so the graph binds what the platform would
  // otherwise give a <button> for free.
  {
    id: "graph.activate",
    label: "Open the focused graph node",
    keys: ["Enter", "Space"],
    scope: "graph",
    fixed: "A graph node stands in for a button, and these are what a button answers to.",
  },

  // The file tree's ARIA `tree` walk, and discovery's. Two scopes rather than one shared
  // set: the panes are siblings, so ↑ can mean "previous row" in both without the checker
  // seeing a clash, and a user who rebinds one is not silently rebinding the other.
  // treenav.ts / sidenav.ts turn these ids into moves (see the module header).
  { id: "tree.row.prev", label: "Previous row", keys: ["ArrowUp"], scope: "tree" },
  { id: "tree.row.next", label: "Next row", keys: ["ArrowDown"], scope: "tree" },
  { id: "tree.row.first", label: "First row", keys: ["Home"], scope: "tree" },
  { id: "tree.row.last", label: "Last row", keys: ["End"], scope: "tree" },
  { id: "tree.row.in", label: "Expand a folder, or step into it", keys: ["ArrowRight"], scope: "tree" },
  { id: "tree.row.out", label: "Collapse a folder, or step out to its parent", keys: ["ArrowLeft"], scope: "tree" },
  { id: "side.row.prev", label: "Previous row", keys: ["ArrowUp"], scope: "side" },
  { id: "side.row.next", label: "Next row", keys: ["ArrowDown"], scope: "side" },
  { id: "side.row.first", label: "First row", keys: ["Home"], scope: "side" },
  { id: "side.row.last", label: "Last row", keys: ["End"], scope: "side" },
  { id: "side.row.in", label: "Unfold a section or card, or step in", keys: ["ArrowRight"], scope: "side" },
  { id: "side.row.out", label: "Fold a section or card, or step out", keys: ["ArrowLeft"], scope: "side" },

  // Text entry. ⏎ commits and Esc backs out — the platform reflex, in the three inline
  // surfaces that need it. Exempt from the sheet (shortcuts.ts says why), and fixed for
  // the same reason they're exempt: nobody needs telling, and nobody wants them moved.
  {
    id: "create.commit",
    label: "Create the new note or folder",
    keys: ["Enter"],
    scope: "textentry:create",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "create.cancel",
    label: "Back out of the new note or folder",
    keys: ["Any-Escape"],
    scope: "textentry:create",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "rename.commit",
    label: "Commit the rename",
    keys: ["Enter"],
    scope: "textentry:rename",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "rename.cancel",
    label: "Back out of the rename",
    keys: ["Any-Escape"],
    scope: "textentry:rename",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "find.input.next",
    label: "Next match, from the find field",
    keys: ["Enter"],
    scope: "textentry:find",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "find.input.prev",
    label: "Previous match, from the find field",
    keys: ["Shift-Enter"],
    scope: "textentry:find",
    fixed: TEXT_FIELD_REFLEX,
  },
  {
    id: "find.input.close",
    label: "Close the find bar",
    keys: ["Any-Escape"],
    scope: "textentry:find",
    fixed: TEXT_FIELD_REFLEX,
  },
  // The chat composer. ⏎ asks and ⇧⏎ is a newline — the reflex of every multi-line
  // message field there has ever been, which is exactly what `fixed` is for: offering to
  // rebind it would be offering to break what the user's hands already know. (The
  // composer is a textarea, so ⇧⏎ needs no binding at all: it is the platform's default,
  // and B2's job is only to *not* claim it.)
  {
    id: "chat.send",
    label: "Ask the question",
    keys: ["Enter"],
    scope: "textentry:chat",
    fixed: TEXT_FIELD_REFLEX,
  },
] as const satisfies readonly Binding[];

/** Every command id in the table, as a type — so a typo in main.ts or shortcuts.ts is a
 *  compile error (`tsc --noEmit` runs in `npm run build`, which `just ci` runs) rather
 *  than a chord that silently stops working. */
export type BindingId = (typeof DEFAULT_BINDINGS)[number]["id"];

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
 *  `shift: false` would make the chord unfireable.
 *
 *  Exported because the recorder has to spell a chord the same way the table does: a
 *  ⇧-that-is-already-in-the-character must not be written down as a `Shift-` prefix, or
 *  the recorded chord and the pressed keystroke stop being the same thing. */
export function shiftDistinguishes(key: string): boolean {
  return /^[a-z0-9]$/.test(key) || isNamed(key);
}

/** Can a chord be built over this (already canonical) key?
 *
 *  The parser's own rule, asked ahead of time. A keydown reports plenty of keys no chord
 *  can hold — "Meta" and "Shift" for the modifiers themselves, "Dead" mid-composition,
 *  "AudioVolumeUp" from a media key — and the recorder needs to say so in words rather
 *  than hand `parseChord` something that throws. */
export function isBindableKey(key: string): boolean {
  return key.length === 1 || isNamed(key);
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
  // `-` is both the separator and a key you can press, so the split has to refuse to cut
  // at a *trailing* one: "Mod--" is ⌘ plus the hyphen key, and "-" is the bare hyphen.
  // This is character-for-character CodeMirror's own rule (`normalizeKeyName` splits on
  // `/-(?!$)/`), and that is the point rather than a coincidence — a spec CodeMirror
  // parses and B2 throws on would break the one agreement this module's header calls
  // load-bearing, and it would break it exactly where a user records an ordinary key.
  const parts = spec.split(/-(?!$)/);
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

// --- the live registry ---------------------------------------------------------------
//
// One keyboard, so one registry: the alternative is threading a table through fifty
// `isBound(e, …)` call sites in main.ts's handler to say the same thing fifty times.
// Everything that *reasons* about a table — the conflict checkers, keymap.ts's algebra —
// takes one as an argument instead, so the suite never has to install anything globally
// to test it, and the mutable cell below stays a detail of the running app.

let active: readonly Binding[] = DEFAULT_BINDINGS;
let byId = new Map<string, Binding>(DEFAULT_BINDINGS.map((b) => [b.id, b]));

/** The keyboard as it stands: the defaults with the user's rebindings laid over them. */
export function activeBindings(): readonly Binding[] {
  return active;
}

/** Install a table as the live one. keymap.ts's `applyOverrides` builds it; main.ts calls
 *  this once on boot and again on every change from the recorder. */
export function setActiveBindings(next: readonly Binding[]): void {
  active = next;
  byId = new Map(next.map((b) => [b.id, b]));
}

// --- lookup ------------------------------------------------------------------------

/** Every chord that fires a binding — what the sheet shows, plus the quiet aliases. */
export function allKeys(b: Binding): readonly string[] {
  return b.aliases ? [...b.keys, ...b.aliases] : b.keys;
}

function binding(id: string): Binding {
  const b = byId.get(id);
  if (!b) throw new Error(`no such binding: ${id}`);
  return b;
}

/** The binding for a command in a given table — the lookup keymap.ts and the recorder
 *  need, since they reason about *candidate* tables rather than the live one. */
export function findBinding(bindings: readonly Binding[], id: string): Binding | undefined {
  return bindings.find((b) => b.id === id);
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

/** The first of `ids` this event fires, or null.
 *
 *  What the arrow families ask, now that the registry owns their keys: main.ts hands over
 *  the ids one surface answers to and gets back the command, which treenav.ts /
 *  sidenav.ts / settingstabs.ts turn into a move. Order is the caller's tie-break, and
 *  never actually breaks a tie — `conflicts()` fails the suite before two of these could
 *  answer to one keystroke in one scope. */
export function boundOf<T extends BindingId>(e: KeyEventLike, ids: readonly T[]): T | null {
  for (const id of ids) {
    if (isBound(e, id)) return id;
  }
  return null;
}

// --- conflicts ---------------------------------------------------------------------
//
// "Conflict" is the keyboard word throughout — this section, menukeys.ts, editorkeys.ts
// and their tests. It was picked because "collision" was spoken for: the cross-note
// `b2id` collision of GH #81, which had nothing to do with keystrokes. GH #170 removed
// the stamp and that whole vocabulary with it, so nothing competes for either word now
// — but one word per meaning is the point (#120), so "conflict" stays the only one used.

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

/** Every same-scope keystroke clash in a table. Empty for the defaults, and kept that way
 *  by bindings.test.ts — and asked of a *candidate* table by keymap.ts, which is how the
 *  recorder refuses a chord before it is saved rather than after. */
export function conflicts(bindings: readonly Binding[] = activeBindings()): Conflict[] {
  const all = claims(bindings);
  const found: Conflict[] = [];
  const seen = new Set<string>();
  for (let i = 0; i < all.length; i++) {
    for (let j = i + 1; j < all.length; j++) {
      const [x, y] = [all[i], all[j]];
      if (x.id === y.id || x.form !== y.form || x.scope !== y.scope) continue;
      // One row per conflicting *pair*, not per overlapping keystroke: a Mod-vs-Mod clash
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
export function shadows(bindings: readonly Binding[] = activeBindings()): Shadow[] {
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
