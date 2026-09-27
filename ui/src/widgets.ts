// Small pieces of markup several surfaces paint, kept in one place so they can't drift.

import { escapeHtml } from "./escape.ts";
import { strengthBand } from "./strength.ts";

/** A row's slot in the roving tabstop: exactly one row is tabbable, the rest are -1. */
export function sideTab(key: string, roving: string | null): string {
  return ` tabindex="${key === roving ? "0" : "-1"}"`;
}

// The discovery card's strength cell: a banded read of the candidate's z (`strength.ts`,
// GH #150). No z, no cell: an uncomputed statistic isn't claimed.
export function strengthHtml(z: number | undefined): string {
  const band = strengthBand(z);
  if (!band) return "";
  // CSS reveals the figure on the selected card, so it isn't pointer-only (K1). The
  // accessible name carries band and figure; the figure's span is `aria-hidden` so it is
  // announced once.
  return `<span class="card-score" role="img" aria-label="${escapeHtml(
    `${band.label}, ${band.value}`,
  )}" title="${escapeHtml(band.title)}">${band.glyph}<span class="card-sigma" aria-hidden="true">${escapeHtml(
    band.value,
  )}</span></span>`;
}

/** A segmented control of mutually exclusive choices. Each segment is a button with a
 *  stable `idPrefix + id` (for re-focus after a repaint) and `attr="id"` for delegation. */
export function segmentedHtml(
  label: string,
  idPrefix: string,
  attr: string,
  choices: readonly { id: string; label: string }[],
  selected: string,
): string {
  const buttons = choices
    .map((c) => {
      const on = selected === c.id;
      return `<button type="button" class="seg${on ? " seg-on" : ""}" id="${idPrefix}${
        c.id
      }" ${attr}="${c.id}" aria-pressed="${on}">${c.label}</button>`;
    })
    .join("");
  return `<div class="segmented" role="group" aria-label="${label}">${buttons}</div>`;
}

/**
 * The index-run meter: track, label and Cancel. Painted in the top bar and in Settings →
 * Index; `paintReindex` (main.ts) writes every `.reindex-progress`, hence classes, not ids.
 * The one id is Settings' Cancel, for re-focus after a repaint.
 */
export function reindexMeterHtml(o: { hidden: boolean; indeterminate: boolean; cancelId?: string }): string {
  const id = o.cancelId ? `id="${o.cancelId}" ` : "";
  return `<div class="reindex-progress"${o.hidden ? " hidden" : ""} aria-live="polite">
         <div class="reindex-track"><div class="reindex-fill${o.indeterminate ? " is-indeterminate" : ""}"></div></div>
         <span class="reindex-label"></span>
         <button ${id}class="btn ghost small" data-cancel-reindex>Cancel</button>
       </div>`;
}
