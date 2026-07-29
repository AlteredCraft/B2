// The *third* keyboard in the app: macOS's menu bar.
//
// bindings.ts owns the chords B2 answers to in the webview, editorkeys.ts owns the ones
// CodeMirror answers to inside the editor, and a dozen more belong to neither — ⌘Q, ⌘W,
// ⌘M, ⌘H, ⌥⌘H, ⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V, ⌘A, ⌃⌘F are the app menu's. Until #119 they came
// from Tauri's `Menu::default()`, which is to say from nowhere anyone could point at:
// the registry couldn't see them, the reference sheet couldn't list them, and
// `conflicts()` couldn't warn about landing a new B2 chord on one.
//
// Why the registry can't just watch for them. AppKit dispatches a menu key equivalent
// inside `NSApplication.sendEvent`, *before* the key window's responder chain — so these
// keystrokes never reach the webview's keydown handler at all. No amount of listening
// finds them; the menu has to be declared and then read. It is, in
// `crates/b2-desktop/src/menu.rs`, and the host hands the list over through the
// `menu_chords` command.
//
// What's here is the **mirror** of that declaration. The host is the authority — it is
// the one that builds the menu — and the UI holds a copy for the two jobs a runtime
// fetch can't do: the conflict gate below runs in node, in the suite, with no host to
// ask; and the sheet has to paint before the first `invoke` resolves. The copy is
// checked against the host on every launch (`menuDrift`, called from main.ts's boot),
// which is the same "change them together, and the app says so if you didn't" posture as
// WRITE_CONFLICT_MESSAGE and VAULT_CHANGED_EVENT in api.ts.
//
// The check itself is deliberately *not* `conflicts()`. That function asks "do two
// commands claim one keystroke in one scope?", and scope is the wrong question here: a
// menu accelerator is taken before the webview is consulted, so it cannot be shadowed by
// an inner surface the way a global B2 chord can be by the editor or a modal. ⌘Z is the
// menu's in the note editor, in the find bar, in a modal, everywhere. So `menuOverlaps`
// compares keystrokes across *every* scope, and that difference is the whole point of it
// being its own function rather than more rows in BINDINGS.
import { type Binding, activeBindings, allKeys, keystrokes } from "./bindings.ts";
import type { MenuChord } from "./types.ts";

/** The menu bar as `crates/b2-desktop/src/menu.rs` declares it — every item that carries
 *  a chord, in menu order. Mirrors the host's `menu_chords`; change them together (the
 *  Rust side pins this same list, and `menuDrift` catches it at runtime if you don't). */
export const MENU_CHORDS = [
  { id: "app.hide", label: "Hide B2", keys: "Mod-h" },
  { id: "app.hide-others", label: "Hide Others", keys: "Mod-Alt-h" },
  { id: "app.quit", label: "Quit B2", keys: "Mod-q" },
  { id: "file.close-window", label: "Close Window", keys: "Mod-w" },
  { id: "edit.undo", label: "Undo", keys: "Mod-z" },
  { id: "edit.redo", label: "Redo", keys: "Mod-Shift-z" },
  { id: "edit.cut", label: "Cut", keys: "Mod-x" },
  { id: "edit.copy", label: "Copy", keys: "Mod-c" },
  { id: "edit.paste", label: "Paste", keys: "Mod-v" },
  { id: "edit.select-all", label: "Select All", keys: "Mod-a" },
  { id: "view.fullscreen", label: "Toggle Full Screen", keys: "Mod-Ctrl-f" },
  { id: "window.minimize", label: "Minimize", keys: "Mod-m" },
] as const satisfies readonly MenuChord[];

/** A B2 chord the menu bar takes first. */
export interface MenuOverlap {
  /** The B2 command that would never run. */
  id: string;
  /** Its chord, as the registry spells it. */
  chord: string;
  /** The scope it was declared in — reported because it explains nothing away: the
   *  menu wins in every one of them. */
  scope: string;
  /** The menu item that takes it. */
  item: string;
  /** The keystroke they meet on, e.g. "⌘z". */
  form: string;
}

/** Every B2 binding whose keystroke the menu bar claims — in any scope, for the reason
 *  in the module header. Empty for the shipped defaults and kept that way by
 *  menukeys.test.ts — and asked of a *candidate* table by keymap.ts, which is how the
 *  recorder refuses ⌘W outright rather than leaving a user to wonder why the window closed. */
export function menuOverlaps(
  bindings: readonly Binding[] = activeBindings(),
  menu: readonly MenuChord[] = MENU_CHORDS,
): MenuOverlap[] {
  const reserved = menu.map((c) => ({ item: c.id, forms: new Set(keystrokes(c.keys)) }));
  const out: MenuOverlap[] = [];
  for (const b of bindings) {
    for (const spec of allKeys(b)) {
      for (const form of keystrokes(spec)) {
        for (const r of reserved) {
          if (r.forms.has(form)) {
            out.push({ id: b.id, chord: spec, scope: b.scope, item: r.item, form });
          }
        }
      }
    }
  }
  return out;
}

/** How the host's live menu differs from the mirror above, one line per difference.
 *
 *  Empty is the only healthy answer, and the only one a shipped build should ever
 *  produce — a difference means someone changed `menu.rs` without changing this file, so
 *  the sheet the *suite* checks and the gate it runs are both describing a menu the app
 *  no longer has. Reported rather than thrown: a stale mirror is a developer's problem,
 *  and it must not take the window down over a keyboard reference. */
export function menuDrift(
  host: readonly MenuChord[],
  mirror: readonly MenuChord[] = MENU_CHORDS,
): string[] {
  const line = (c: MenuChord): string => `${c.id} ${c.keys} (${c.label})`;
  const here = new Map(mirror.map((c) => [c.id, c]));
  const out: string[] = [];
  for (const c of host) {
    const mine = here.get(c.id);
    if (!mine) out.push(`the host declares ${line(c)}; the mirror doesn't have it`);
    else if (mine.keys !== c.keys || mine.label !== c.label) {
      out.push(`the host declares ${line(c)}; the mirror says ${line(mine)}`);
    }
  }
  const theirs = new Set(host.map((c) => c.id));
  for (const c of mirror) {
    if (!theirs.has(c.id)) out.push(`the mirror has ${line(c)}; the host doesn't declare it`);
  }
  return out;
}
