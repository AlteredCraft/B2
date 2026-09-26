// Small pieces of markup more than one surface paints — kept in one place so the surfaces
// that share them (render.ts's panes, chatview.ts, explainview.ts, settingsview.ts, and
// main.ts's shell) can't drift apart on how they look or what they announce.

import { escapeHtml } from "./escape.ts";
import { strengthBand } from "./strength.ts";

/** A row's slot in the roving tabstop: exactly one row is tabbable, the rest are -1. */
export function sideTab(key: string, roving: string | null): string {
  return ` tabindex="${key === roving ? "0" : "-1"}"`;
}

// The discovery card's strength cell: a banded read of the candidate's z
// (`strength.ts`), replacing the raw negated-L2 the card used to print (GH #150).
// No z → no cell: a statistic that wasn't computed isn't claimed (raw mode, tiny
// pools). The glyph is decorative; the band name is the accessible content.
export function strengthHtml(z: number | undefined): string {
  const band = strengthBand(z);
  if (!band) return "";
  // The figure rides in the markup and CSS reveals it on the selected/hovered card, so
  // the number is one keystroke away rather than pointer-only (`title=` alone was a hole
  // in K1). The accessible name carries *both* halves the eye gets — the band and the
  // figure — because naming only the band would leave a screen reader with "clear match"
  // and no way to reach the 2.5σ behind it. The figure's own span stays `aria-hidden`, so
  // it is announced once (as part of this name) rather than twice.
  return `<span class="card-score" role="img" aria-label="${escapeHtml(
    `${band.label}, ${band.value}`,
  )}" title="${escapeHtml(band.title)}">${band.glyph}<span class="card-sigma" aria-hidden="true">${escapeHtml(
    band.value,
  )}</span></span>`;
}

/** A segmented control: mutually exclusive choices that read at a glance (the theme,
 *  Local / Cloud models). Each segment is a real button carrying `idPrefix + id` — the
 *  stable id the settings surface re-focuses by after a repaint — and `attr="id"` for the
 *  click delegation. */
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
 * The index-run meter: track, label and Cancel. Two are painted — the top bar's, and
 * Settings → Index's while a run is live (Settings covers the bar) — and `paintReindex`
 * (main.ts) writes the same values into every `.reindex-progress` on screen, so the two
 * can't disagree about a run. Classes, not ids, for that reason; the one id is the
 * Settings Cancel's, which that surface re-focuses by after a repaint.
 */
export function reindexMeterHtml(o: { hidden: boolean; indeterminate: boolean; cancelId?: string }): string {
  const id = o.cancelId ? `id="${o.cancelId}" ` : "";
  return `<div class="reindex-progress"${o.hidden ? " hidden" : ""} aria-live="polite">
         <div class="reindex-track"><div class="reindex-fill${o.indeterminate ? " is-indeterminate" : ""}"></div></div>
         <span class="reindex-label"></span>
         <button ${id}class="btn ghost small" data-cancel-reindex>Cancel</button>
       </div>`;
}
