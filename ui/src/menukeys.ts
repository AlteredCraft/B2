// The third keyboard in the app: macOS's menu bar (#119). AppKit dispatches a menu key
// equivalent before the key window's responder chain, so these chords never reach the
// webview; the menu is declared in `crates/b2-desktop/src/menu.rs`.
//
// This is a mirror of that declaration, so the conflict gate can run in node and the sheet
// can paint before the first `invoke`. `menuDrift` checks it against the host at boot.
//
// Not `conflicts()`: a menu accelerator can't be shadowed by an inner scope, so
// `menuOverlaps` compares keystrokes across every scope.
import { type Binding, activeBindings, allKeys, keystrokes } from "./bindings.ts";
import type { MenuChord } from "./types.ts";

/** Every menu item with a chord, in menu order. Mirrors the host's `menu_chords`; change
 *  them together. */
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
  // The View menu's three are B2's own chords, not macOS's. They live in the menu (so not
  // in bindings.ts); `menu.rs` forwards the id and `zoom.ts` acts on it.
  { id: "view.zoom-in", label: "Zoom In", keys: "Mod-=" },
  { id: "view.zoom-out", label: "Zoom Out", keys: "Mod--" },
  { id: "view.zoom-reset", label: "Actual Size", keys: "Mod-0" },
  { id: "view.fullscreen", label: "Toggle Full Screen", keys: "Mod-Ctrl-f" },
  { id: "window.minimize", label: "Minimize", keys: "Mod-m" },
] as const satisfies readonly MenuChord[];

/** A B2 chord the menu bar takes first. */
export interface MenuOverlap {
  /** The B2 command that would never run. */
  id: string;
  /** Its chord, as the registry spells it. */
  chord: string;
  /** The scope it was declared in; the menu wins in every scope. */
  scope: string;
  /** The menu item that takes it. */
  item: string;
  /** The keystroke they meet on, e.g. "⌘z". */
  form: string;
}

/** Every B2 binding whose keystroke the menu bar claims, in any scope. Empty for the
 *  defaults (menukeys.test.ts); keymap.ts asks it of a candidate table to refuse ⌘W. */
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

/** How the host's live menu differs from the mirror, one line per difference. Empty is
 *  healthy. Reported, not thrown: a stale mirror must not take the window down. */
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
