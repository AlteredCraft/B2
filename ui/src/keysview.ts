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

// Settings → Keyboard: K1's reference, where every chord B2 owns is a button that rebinds
// it (#121, GH #78). The recorder is a strip at the top, not inline in its row: the grid
// is multi-column and scrolls, so an inline control could land out of sight. The chip
// being edited carries `.kbd-recording`.
export function keyboardPanelHtml(state: AppState): string {
  const changed = customized(DEFAULT_BINDINGS, state.keyOverrides).length;
  const resetAll = changed
    ? `<button class="btn small" id="keys-reset-all">Reset all (${changed})</button>`
    : "";
  // The ⌘-hold sheet is a gesture with no row in the table, so it is documented here (K1).
  return `<div class="settings-subhead">Keyboard shortcuts</div>
      <p class="settings-detail muted">B2 is fully operable from the keyboard — the mouse is an accelerator, never a requirement. Click a chord to change it. Hold ⌘ on its own anywhere in the app for a quick reminder of the ⌘ chords below.</p>
      <div class="keys-toolbar">${resetAll}</div>
      ${recorderHtml(state)}
      ${shortcutsGridHtml(state)}`;
}

/** The recorder: what is being rebound, what was pressed, and what that would mean. Empty
 *  when not recording. Controls carry stable ids because each captured chord repaints. */
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
  // `tabindex="-1"`: focusable so main.ts can pull focus off CodeMirror (which would type
  // the chord into the buffer), but not a Tab stop. `captureModalFocus` restores by the id.
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

/** Every chord the app answers to, from shortcuts.ts. A chip naming a command is a
 *  `<button>`, anything else a `<kbd>`: what looks pressable is what B2 can move. Every
 *  chip is a Tab stop on purpose: the list is bounded and each entry is actionable. */
function shortcutsGridHtml(state: AppState): string {
  const recording = state.recorder?.id ?? null;
  // Chip ids must be unique for `captureModalFocus`, and a command can appear in two rows,
  // so the sheet position (stable across repaints) disambiguates.
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
 * The ⌘-hold sheet, painted while ⌘ is held (cmdhold.ts; main.ts owns the timer). Empty
 * when not up or when no chord uses ⌘. Never takes focus, which would cancel the chord
 * being pressed; `pointer-events: none` so ⌘-click reaches the app; `aria-hidden`
 * because Settings → Keyboard is the accessible surface.
 */
export function cmdSheetHtml(state: AppState): string {
  if (!state.cmdSheet) return "";
  const groups = cmdShortcuts();
  if (groups.length === 0) return "";
  const body = keysGroupsHtml(groups, (k) => `<kbd>${escapeHtml(k.text)}</kbd>`);
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
