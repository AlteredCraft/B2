// The view: Markdown → HTML (with clickable wikilinks) and the pane HTML builders.
// Pure functions of state — no IPC, no DOM mutation (main.ts writes the output in).
//
// Safety: output comes from the user's *own* local notes, and the webview CSP
// (script-src 'self', no 'unsafe-inline') neutralizes inline `<script>`/`onclick`
// from a note and blocks remote script/style loads (specs/desktop-ui-mvp.md §6). We
// still HTML-escape every value B2 itself interpolates (titles, paths, snippets) so
// UI chrome can't be broken by note content. A DOMPurify pass is a later hardening,
// not needed for a local-first, own-content MVP.

import { marked, type Tokens, type TokenizerAndRendererExtension } from "marked";
import { RELATION_VERBS, type AppState } from "./state";

export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// A `[[target]]` / `[[target|label]]` wikilink becomes an in-app anchor carrying the
// raw target; main.ts delegates a click on `.wikilink` to open that note. This is the
// MVP's in-app navigation (spec §4) — the buffer stays byte-honest Markdown.
const wikilink: TokenizerAndRendererExtension = {
  name: "wikilink",
  level: "inline",
  start(src: string) {
    const i = src.indexOf("[[");
    return i < 0 ? undefined : i;
  },
  tokenizer(src: string) {
    const m = /^\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/.exec(src);
    if (!m) return undefined;
    return {
      type: "wikilink",
      raw: m[0],
      target: m[1].trim(),
      label: (m[2] ?? m[1]).trim(),
    } as Tokens.Generic;
  },
  renderer(token: Tokens.Generic) {
    return `<a class="wikilink" data-target="${escapeHtml(
      String(token.target),
    )}" href="#">${escapeHtml(String(token.label))}</a>`;
  },
};

marked.use({ extensions: [wikilink], gfm: true, breaks: false });

export function renderMarkdown(md: string): string {
  return marked.parse(md, { async: false }) as string;
}

// --- pane builders --------------------------------------------------------------

export function notePaneHtml(state: AppState): string {
  const n = state.current;
  if (n) {
    const metaBits = [n.type, n.created].filter(Boolean).map((s) => escapeHtml(s as string));
    const meta = [escapeHtml(n.path), ...metaBits].join(" · ");
    const tags = n.tags.length
      ? `<div class="tags">${n.tags
          .map((t) => `<span class="tag">${escapeHtml(t)}</span>`)
          .join("")}</div>`
      : "";
    return `<article class="note">
        <header class="note-head">
          <h1>${escapeHtml(n.title ?? n.path)}</h1>
          <div class="note-meta">${meta}</div>
          ${tags}
        </header>
        <div class="note-body">${renderMarkdown(n.body)}</div>
      </article>`;
  }
  if (state.loading) return `<div class="empty"><p>Loading…</p></div>`;
  if (state.vaultRoot === null) {
    return `<div class="empty">
        <h2>No vault open</h2>
        <p>Launch B2 with a vault path, or set <code>B2_VAULT_PATH</code> to your vault folder, then relaunch.</p>
      </div>`;
  }
  return `<div class="empty">
      <h2>Read → discover → link</h2>
      <p>Search above to open a note. B2 will surface its similar-but-unlinked notes on the right, so you can connect them.</p>
    </div>`;
}

export function sidePaneHtml(state: AppState): string {
  return state.searchQuery ? searchSectionHtml(state) : discoverySectionHtml(state);
}

function searchSectionHtml(state: AppState): string {
  const head = `<div class="side-head">
      <h2>Results</h2>
      <button class="linklike" data-clear-search>clear</button>
    </div>
    <p class="side-sub">for “${escapeHtml(state.searchQuery)}”${
      state.semantic ? "" : " · keyword only (run <code>b2 init</code> for semantic)"
    }</p>`;
  if (state.loading) return head + `<p class="side-empty">Searching…</p>`;
  if (state.searchResults.length === 0)
    return head + `<p class="side-empty">No matches.</p>`;
  const items = state.searchResults
    .map(
      (r) => `<button class="card" data-open="${escapeHtml(r.path)}">
        <div class="card-title">${escapeHtml(r.title ?? r.path)}</div>
        <div class="card-path">${escapeHtml(r.path)} · ${r.score.toFixed(3)}</div>
        ${r.snippet ? `<div class="card-snip">${escapeHtml(r.snippet)}</div>` : ""}
      </button>`,
    )
    .join("");
  return head + `<div class="cards">${items}</div>`;
}

function discoverySectionHtml(state: AppState): string {
  if (!state.current) {
    return `<div class="side-head"><h2>Discovery</h2></div>
      <p class="side-empty">Open a note to see similar notes and its connections.</p>`;
  }
  return similarSectionHtml(state) + connectionsSectionHtml(state);
}

function similarSectionHtml(state: AppState): string {
  const head = `<div class="side-head"><h2>Similar &amp; unlinked</h2></div>`;
  if (state.similar.length === 0) {
    const hint = state.semantic
      ? "Nothing similar-but-unlinked, or the vault isn’t embedded yet (Reindex)."
      : "Semantic similarity is off — run <code>b2 init</code> then Reindex.";
    return head + `<p class="side-empty">${hint}</p>`;
  }
  const items = state.similar
    .map(
      (c) => `<div class="card candidate">
        <button class="card-open" data-open="${escapeHtml(c.path)}">
          <div class="card-title">${escapeHtml(c.title ?? c.path)}</div>
          <div class="card-path">${escapeHtml(c.path)} · ${c.score.toFixed(3)}</div>
          ${c.evidence ? `<div class="card-snip">${escapeHtml(c.evidence)}</div>` : ""}
        </button>
        <button class="btn small" data-link-path="${escapeHtml(c.path)}" data-link-title="${escapeHtml(
          c.title ?? "",
        )}">Link…</button>
      </div>`,
    )
    .join("");
  return head + `<div class="cards">${items}</div>`;
}

function connectionsSectionHtml(state: AppState): string {
  const head = `<div class="side-head"><h2>Connections</h2></div>`;
  if (state.connections.length === 0)
    return head + `<p class="side-empty">No connections yet.</p>`;
  const items = state.connections
    .map((c) => {
      const arrow = c.direction === "outbound" ? "→" : "←";
      const why = c.explanation
        ? `<div class="card-snip">${escapeHtml(c.explanation)}</div>`
        : "";
      return `<button class="card edge" data-open="${escapeHtml(c.path)}">
          <div class="card-title"><span class="edge-arrow">${arrow}</span> ${escapeHtml(
            c.label,
          )} <span class="edge-origin">${escapeHtml(c.origin)}</span></div>
          <div class="card-path">${escapeHtml(c.title ?? c.path)}</div>
          ${why}
        </button>`;
    })
    .join("");
  return head + `<div class="cards">${items}</div>`;
}

export function modalHtml(state: AppState): string {
  const t = state.linkTarget;
  if (!t) return "";
  const src = state.current;
  const opts = RELATION_VERBS.map(
    (v) => `<option value="${v}"${v === state.linkRelation ? " selected" : ""}>${v}</option>`,
  ).join("");
  // The backdrop carries no cancel attr (a click on it closes only when it is the
  // exact target — see main.ts); the Cancel button uses `data-cancel`. This keeps a
  // click *inside* the modal from bubbling into an accidental close.
  return `<div class="modal-backdrop">
      <div class="modal" role="dialog" aria-modal="true" aria-label="Link a connection">
        <h3>Link a connection</h3>
        <p class="modal-pair">
          <strong>${escapeHtml(src?.title ?? src?.path ?? "")}</strong>
          <span class="modal-verb" id="modal-verb">${escapeHtml(state.linkRelation)}</span>
          <strong>${escapeHtml(t.title ?? t.path)}</strong>
        </p>
        <label class="field">Relation
          <select id="link-relation">${opts}</select>
        </label>
        <label class="field">Explanation <span class="muted">(optional)</span>
          <input id="link-explanation" type="text" placeholder="why they connect" />
        </label>
        <div class="modal-actions">
          <button class="btn ghost" data-cancel>Cancel</button>
          <button class="btn primary" id="link-commit">Commit link</button>
        </div>
      </div>
    </div>`;
}
