// The keyboard's two surfaces: Settings → Keyboard (the reference, and the recorder that
// rebinds from it) and the ⌘-hold sheet. The table is shortcuts.ts; the algebra keymap.ts.

import { escapeHtml } from "./escape.ts";
import type { AppState } from "./state.ts";
import { type ShortcutGroup, type ShortcutKey, shortcuts } from "./shortcuts.ts";
import { cmdShortcuts } from "./cmdhold.ts";
import {
  DEFAULT_BINDINGS,
  activeBindings,
  displayChord,
  displayKeys,
  findBinding,
} from "./bindings.ts";
import { customized, refused } from "./keymap.ts";

// Keyboard — the discoverable half of invariant K1, and now its *editable* half too: one
// surface for the table, reached by `?` from anywhere or by walking the rail, where every
// chord B2 owns is a button that rebinds it (#121). The table itself is `shortcuts.ts`
// (GH #78); the algebra and the judgement are keymap.ts.
//
// The recorder is a strip at the top of the panel rather than a widget spliced into the
// row it edits. The grid is CSS multi-column, so an inline block would land wherever the
// column flow put it — and the panel is a page of table you scroll, so a control that
// appeared below the fold would be a control nobody saw. The chip being edited carries
// `.kbd-recording` instead, which is what ties the strip to its row.
export function keyboardPanelHtml(state: AppState): string {
  const changed = customized(DEFAULT_BINDINGS, state.keyOverrides).length;
  const resetAll = changed
    ? `<button class="btn small" id="keys-reset-all">Reset all (${changed})</button>`
    : "";
  // The ⌘-hold sheet is a *gesture*, not a chord, so it has no row in the table below —
  // and a keyboard affordance documented nowhere is the exact failure K1 names. The
  // reference is where someone looks for it, so the reference is where it is said.
  return `<div class="settings-subhead">Keyboard shortcuts</div>
      <p class="settings-detail muted">B2 is fully operable from the keyboard — the mouse is an accelerator, never a requirement. Click a chord to change it. Hold ⌘ on its own anywhere in the app for a quick reminder of the ⌘ chords below.</p>
      <div class="keys-toolbar">${resetAll}</div>
      ${recorderHtml(state)}
      ${shortcutsGridHtml(state)}`;
}

/** The recorder: what is being rebound, what has been pressed, and what that would mean.
 *
 *  Empty markup when nothing is recording, so the strip costs no vertical space until it
 *  is asked for. Every control carries a stable `id` — the settings builder's rule above
 *  — because a captured chord repaints the dialog under the keyboard that captured it. */
function recorderHtml(state: AppState): string {
  const rec = state.recorder;
  if (!rec) return "";
  const b = findBinding(activeBindings(), rec.id);
  if (!b) return "";
  const now = b.keys.map((k) => `<kbd>${escapeHtml(displayChord(k))}</kbd>`).join(" ");
  const captured = rec.candidate
    ? `<kbd class="keys-captured">${escapeHtml(displayChord(rec.candidate))}</kbd>`
    : `<span class="keys-waiting">Press a chord…</span>`;
  const lines = [
    ...(rec.hint ? [{ tier: "warn" as const, message: rec.hint }] : []),
    ...rec.problems,
  ]
    .map(
      (p) =>
        `<li class="keys-problem keys-${p.tier}">${escapeHtml(p.message)}</li>`,
    )
    .join("");
  const blocked = refused(rec.problems);
  const canSave = rec.candidate !== null && !blocked;
  const isChanged = state.keyOverrides[rec.id] !== undefined;
  // `tabindex="-1"`: focusable so main.ts can take the keyboard off whatever had it (a
  // tree row, or CodeMirror, which would otherwise type the chord into the buffer behind
  // the dialog), but not a Tab stop — the strip is a target to press keys at, not a
  // control to land on. The id is what `captureModalFocus` hands focus back by, which
  // matters here more than anywhere: every captured chord repaints this dialog.
  return `<div class="keys-recorder" id="keys-recorder" tabindex="-1"
        role="group" aria-label="Record a new chord for ${escapeHtml(b.label)}">
      <div class="keys-recorder-head">
        <span class="keys-recorder-what">${escapeHtml(b.label)}</span>
        <span class="keys-recorder-now muted">now ${now}</span>
      </div>
      <div class="keys-recorder-target">${captured}</div>
      ${lines ? `<ul class="keys-problems">${lines}</ul>` : ""}
      <p class="keys-recorder-note muted">Esc cancels, ⏎ accepts — every other key is recorded. If a chord you press never appears above, macOS or another app took it before B2 could see it; B2 can only tell you about the chord you actually pressed.</p>
      <div class="keys-recorder-actions">
        <button class="btn small primary" id="keys-save"${canSave ? "" : " disabled"}>Use this chord</button>
        <button class="btn small" id="keys-cancel">Cancel</button>
        ${isChanged ? `<button class="btn small" id="keys-reset-one">Reset to default</button>` : ""}
      </div>
    </div>`;
}

/** Every chord the app answers to, grouped, from the one table in shortcuts.ts. The app
 *  menu bar's chords are *not* here — macOS prints those beside their own menu items and
 *  nothing in this panel could move them (shortcuts.ts's header says why they left).
 *
 *  A chip that names a command is a `<button>`; everything else — the platform's own
 *  keys, a chord two commands print alike — stays a `<kbd>`. That split is the whole
 *  affordance: what looks pressable is what B2 can actually move.
 *
 *  Every chip is a Tab stop, and on this section that is forty of them. Deliberate: they
 *  are the controls the section exists to offer, and unlike the file tree's 1500 rows the
 *  list is bounded and every entry is genuinely actionable. Esc still closes the dialog
 *  from anywhere in it, which is the property K1 actually asks for. */
function shortcutsGridHtml(state: AppState): string {
  const recording = state.recorder?.id ?? null;
  // A chip's `id` is what `captureModalFocus` puts the keyboard back on after the repaint
  // a click here causes, so it has to be **unique in the document** — and a command can
  // legitimately appear in more than one row (⇧F10 opens a menu on a tree row *and* on a
  // discovery card). The sheet's position disambiguates, and is stable across repaints
  // because the sheet is a pure function of this same state.
  let seat = 0;
  const chip = (k: ShortcutKey): string => {
    const text = escapeHtml(k.text);
    seat++;
    if (!k.id) {
      const why = k.fixed ? ` title="${escapeHtml(k.fixed)}"` : "";
      return `<kbd${why}>${text}</kbd>`;
    }
    const b = findBinding(activeBindings(), k.id);
    const changed = state.keyOverrides[k.id] !== undefined;
    const cls = [
      "kbd-edit",
      changed ? "kbd-changed" : "",
      recording === k.id ? "kbd-recording" : "",
    ]
      .filter(Boolean)
      .join(" ");
    const hint = `Change the chord for ${b?.label ?? k.id}${changed ? " (changed from the default)" : ""}`;
    return `<button type="button" class="${cls}" id="keys-chip-${seat}-${escapeHtml(k.id)}"
        data-rebind="${escapeHtml(k.id)}" title="${escapeHtml(hint)}">${text}</button>`;
  };
  return `<div class="keys-grid">${keysGroupsHtml(shortcuts(), chip)}</div>`;
}

/** Groups of shortcut rows as the reference and the ⌘ sheet both lay them out; `chip`
 *  paints one chord (a rebind button in the reference, a plain `<kbd>` on the sheet). */
function keysGroupsHtml(groups: readonly ShortcutGroup[], chip: (k: ShortcutKey) => string): string {
  return groups
    .map(
      (g) => `<section class="keys-group">
        <h4>${escapeHtml(g.title)}</h4>
        <dl class="keys-list">${g.items
          .map((s) => `<dt>${s.keys.map(chip).join(" ")}</dt><dd>${escapeHtml(s.action)}</dd>`)
          .join("")}</dl>
      </section>`,
    )
    .join("");
}

/**
 * The ⌘-hold sheet: what a held ⌘ can do, painted over the app while it is held
 * (cmdhold.ts owns the machine and the projection; main.ts owns the timer).
 *
 * Empty markup when it isn't up, so the layer costs nothing the rest of the time — the
 * same shape `recorderHtml` and `contextMenuHtml` use. Empty markup too when the
 * projection has nothing in it, which is reachable: every ⌘ chord is rebindable, so a user
 * can move the lot off ⌘ and would otherwise get an empty card for their trouble.
 *
 * Three things it deliberately isn't. It is not a dialog: nothing here is focusable, focus
 * does not move to it and does not come back, because the whole gesture is one the user is
 * already mid-way through — taking the keyboard would cancel the chord they are about to
 * press. It is `pointer-events: none` in the stylesheet for the same reason on the other
 * device: ⌘-click and ⌘-drag have to keep landing on the app underneath. And it is
 * `aria-hidden`, which reads as a strange thing to say about a keyboard aid until you
 * consider who it would be talking to — a screen-reader user has the whole reference in
 * Settings → Keyboard, reachable by chord, and what an aria-live region would add here is
 * a page of chords announced every time a modifier is held a beat too long. The reference
 * is the accessible surface; this is a glance.
 */
export function cmdSheetHtml(state: AppState): string {
  if (!state.cmdSheet) return "";
  const groups = cmdShortcuts();
  if (groups.length === 0) return "";
  const body = keysGroupsHtml(groups, (k) => `<kbd>${escapeHtml(k.text)}</kbd>`);
  // The footer says both halves of the contract: how it goes away (the thing a user who
  // did not mean to summon it needs first), and where the rest of the keyboard lives —
  // this sheet is the ⌘ slice, and someone reading it is exactly the someone who would
  // want the whole table.
  return `<div class="cmdhold" aria-hidden="true">
      <div class="cmdhold-card">
        <div class="cmdhold-head">
          <span class="cmdhold-what"><kbd>⌘</kbd> does this</span>
          <span class="cmdhold-hint">Let go to dismiss · ${escapeHtml(
            displayKeys(["settings.toggle"]),
          )} → Keyboard to change any of them</span>
        </div>
        <div class="cmdhold-grid">${body}</div>
      </div>
    </div>`;
}
