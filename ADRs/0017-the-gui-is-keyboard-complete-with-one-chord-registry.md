# ADR-0017 — The GUI is keyboard-complete, and every chord lives in one registry

- **Status:** Accepted · 2026-07-28 (menu bar declared 2026-07; rebinding 2026-07-28)
- **Refs:** invariants K1 · GH #78, #119, #121 · `ui/src/bindings.ts`

## Context

Keyboard completeness decays silently: one new pane that only responds to a click, and the promise is
gone. And a chord table that exists in three places (dispatcher, editor keymap, help sheet) drifts.

## Decision

- Every action the mouse can take has a keyboard path. Panes follow ARIA patterns over the **same row
  order the painter uses**, so the arrows and the eye cannot disagree; every pane restores focus
  across its own repaint, and every overlay traps and restores it.
- **Chords are declared once** in `ui/src/bindings.ts`; the dispatcher, the editor keymap, and the
  help sheet all derive from it. Arrow families are in the registry too — navigation modules own
  *command → move*, the registry owns *key → command*.
- Four checkers gate the table: same-scope conflicts, CodeMirror's ~100 stock bindings, and the
  **macOS menu bar's** accelerators — which is why the menu bar is *declared* in
  `b2-desktop/src/menu.rs` rather than inherited from Tauri's default, since AppKit dispatches a menu
  accelerator before the webview sees the key at all.
- **The chords are the user's**: every row is re-recordable, stored as a UI preference in
  `localStorage` (never vault state). A candidate is judged against the table it *would* produce —
  refused on a same-scope or menu-bar clash, merely warned for a shadow or CodeMirror overlap — and
  the recorder reads the one thing no table can know: a chord producing **no keydown** was taken
  upstream by macOS.
- The loader re-judges what it reads, so a hand-edited store cannot install a keyboard you cannot use
  to fix itself.

## Consequences

- A new surface owes the four obligations listed in `crates/b2-desktop/CLAUDE.md`.
- A chord live in the app is B2's to document, whoever authored it — an unenumerated chord cannot be
  found, and the app cannot warn about landing on it.
