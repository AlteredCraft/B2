// Explain's paint: the Compare view for one Similar card (GH #236). The words and the
// strip's geometry are explain.ts's.

import { escapeHtml } from "./escape.ts";
import type { AppState } from "./state.ts";
import { STRENGTH_MIN_CANDIDATES, strengthBand } from "./strength.ts";
import {
  EXPLAIN_PAIRS_SHOWN,
  explainSummary,
  explainNames,
  fieldCaption,
  fieldHelp,
  fieldStrip,
  standingText,
  wholeNoteText,
} from "./explain.ts";
import { icon } from "./icons.ts";
import type { PassageView } from "./types.ts";
import { strengthHtml } from "./widgets.ts";

// --- Explain: the Compare view for one Similar card (GH #236) ------------------------
//
// The centre pane's fourth mode: the open note against one of its Similar cards, read
// model-free from the same computation as the list (`Vault::explain_similar`), so its
// rank, grade and best passage are the card's. The words and the strip's geometry are
// explain.ts's; this is only paint. Passage text is note content, so it is escaped and
// shown as text (E5), never rendered as Markdown. Every control is a real button in tab
// order, and Escape backs out, as it does from the graph (K1).

export function explainPaneHtml(state: AppState): string {
  const ec = state.explainCard;
  const bar = `<div class="explain-bar">
      <button id="explain-close" class="source-toggle explain-close" data-explain-close
        title="Back to the note — Esc">← Back to note</button>
    </div>`;
  if (!ec) return bar;
  if (ec.error !== null) {
    return `<div class="explain-view">${bar}<div class="empty"><p>${escapeHtml(ec.error)}</p></div></div>`;
  }
  const v = ec.view;
  if (v === null) {
    return `<div class="explain-view">${bar}<div class="side-empty" role="status" aria-label="Reading the index"><span class="spinner"></span></div></div>`;
  }
  const name = (x: { path: string; title: string | null }) => escapeHtml(x.title ?? x.path);
  const names = explainNames(v);
  const band = strengthBand(v.z);
  const summary = explainSummary(v);
  const whole = wholeNoteText(v);

  const strip = fieldStrip(v.population, v.z);
  const W = 600;
  const H = 44;
  const help = strip
    ? `<button id="explain-help" class="explain-help-btn" data-explain-help aria-expanded="${
        ec.help
      }" aria-controls="explain-help-text" aria-label="What this strip shows" title="What this strip shows">${icon(
        "question-circle",
        { size: 14 },
      )}</button>`
    : "";
  const fieldHtml = strip
    ? `<section class="explain-section">
        <h2 class="explain-h2-help">Where it sits ${help}</h2>
        ${
          ec.help
            ? `<div id="explain-help-text" class="explain-help">${fieldHelp(v)
                .map((t) => `<p>${escapeHtml(t)}</p>`)
                .join("")}</div>`
            : ""
        }
        <div class="strip-legend strip-bands" aria-hidden="true">
          <span style="left:${(strip.clear * 100).toFixed(1)}%">●●○</span>
          <span style="left:${(strip.strong * 100).toFixed(1)}%">●●●</span>
        </div>
        <svg class="explain-strip" viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" role="img"
             aria-label="${escapeHtml(
               `${fieldCaption(v.population, names.anchor)}${
                 v.z !== null ? `; ${names.candidate} at ${v.z.toFixed(1)}σ` : ""
               }`,
             )}">
          <line class="strip-axis" x1="0" y1="${H / 2}" x2="${W}" y2="${H / 2}" />
          ${strip.ticks
            .map(
              (t) =>
                `<line class="strip-tick" x1="${(t.at * W).toFixed(1)}" y1="${H / 2}" x2="${(t.at * W).toFixed(1)}" y2="${H / 2 + 6}" />`,
            )
            .join("")}
          ${[strip.clear, strip.strong]
            .map((x) => `<line class="strip-mark" x1="${x * W}" y1="4" x2="${x * W}" y2="${H - 4}" />`)
            .join("")}
          ${strip.dots
            .map((x) => `<circle class="strip-dot" cx="${(x * W).toFixed(1)}" cy="${H / 2}" r="3" />`)
            .join("")}
          ${
            strip.marker !== null
              ? `<line class="strip-this" x1="${strip.marker * W}" y1="2" x2="${strip.marker * W}" y2="${H - 2}" />`
              : ""
          }
        </svg>
        <div class="strip-legend strip-ticks" aria-hidden="true">${strip.ticks
          .map((t) => `<span style="left:${(t.at * 100).toFixed(1)}%">${escapeHtml(t.label)}</span>`)
          .join("")}</div>
        <div class="strip-key" aria-hidden="true">
          <span><span class="key-this"></span>${escapeHtml(names.candidate)}</span>
          <span><span class="key-mark"></span>where ●●○ and ●●● start</span>
          <span><span class="key-dot"></span>another note</span>
        </div>
        <p class="explain-caption">${escapeHtml(
          fieldCaption(v.population, names.anchor),
        )} · σ from their average</p>
      </section>`
    : `<section class="explain-section"><p class="explain-caption">Ungraded: ranked by nearness only. Grading needs ${STRENGTH_MIN_CANDIDATES} or more notes to compare against.</p></section>`;

  const passage = (who: string, p: PassageView) =>
    `<div class="explain-passage">
        <div class="explain-passage-head"><span class="explain-who">${escapeHtml(who)}</span>${
          p.heading_path ? ` · ${escapeHtml(p.heading_path)}` : ""
        }</div>
        <div class="explain-passage-text">${escapeHtml(p.text)}</div>
      </div>`;
  const shown = ec.allPairs ? v.pairs : v.pairs.slice(0, EXPLAIN_PAIRS_SHOWN);
  const pairsHtml = v.pairs.length
    ? `<section class="explain-section">
        <h2>Passage pairs <span class="muted">· each of its passages, with this note’s nearest</span></h2>
        <ol class="explain-pairs">${shown
          .map(
            (p, i) => `<li class="explain-pair">
              <div class="explain-pair-head">#${i + 1} ${strengthHtml(p.z ?? undefined)}${
                p.identical ? `<span class="explain-pill">identical text</span>` : ""
              }</div>
              <div class="explain-pair-body">${passage("This note", p.anchor)}${passage(
                "Suggested",
                p.candidate,
              )}</div>
            </li>`,
          )
          .join("")}</ol>
        ${
          v.pairs.length > EXPLAIN_PAIRS_SHOWN
            ? `<button id="explain-all" class="btn small" data-explain-all>${
                ec.allPairs ? "Show fewer" : `Show all ${v.pairs.length} pairs`
              }</button>`
            : ""
        }
      </section>`
    : "";

  const links = v.shared_neighbors.length
    ? `<p>Both link with ${v.shared_neighbors
        .map(
          (s) =>
            `<button class="linklike explain-link" data-open="${escapeHtml(s.path)}">${name(s)}</button>`,
        )
        .join(", ")}.</p>`
    : `<p class="muted">They share no linked notes.</p>`;

  return `<div class="explain-view">${bar}
    <article class="note explain">
      <header class="note-head">
        <div class="explain-kicker">Explain · suggested for ${name(v.anchor)}</div>
        <h1>${name(v.candidate)}</h1>
        <div class="note-meta">${escapeHtml(v.candidate.path)}</div>
        <p class="explain-standing">${band ? `${strengthHtml(v.z ?? undefined)} ` : ""}${escapeHtml(
          standingText(v),
        )}</p>
        ${whole ? `<p class="explain-standing muted">${escapeHtml(whole)}</p>` : ""}
      </header>
      ${
        summary
          ? `<section class="explain-summary"><strong>${escapeHtml(summary.label)}.</strong> ${escapeHtml(
              summary.detail,
            )}</section>`
          : ""
      }
      ${fieldHtml}
      ${pairsHtml}
      <section class="explain-section"><h2>Links</h2>${links}</section>
      <div class="explain-actions">
        <button id="explain-open" class="btn small" data-open="${escapeHtml(v.candidate.path)}">Open ${name(v.candidate)}</button>
        <button id="explain-why" class="btn small" data-why="${escapeHtml(v.candidate.path)}" data-why-title="${escapeHtml(
          v.candidate.title ?? "",
        )}">Ask chat why</button>
      </div>
    </article>
  </div>`;
}
