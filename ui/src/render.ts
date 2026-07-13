// The view: Markdown → HTML (with clickable wikilinks) and the pane HTML builders.
// Pure functions of state — no IPC, no DOM mutation (main.ts writes the output in).
//
// Safety: output comes from the user's *own* local notes, and the webview CSP
// (script-src 'self', no 'unsafe-inline') neutralizes inline `<script>`/`onclick`
// from a note and blocks remote script/style loads (specs/completed/desktop-ui-mvp.md §6). We
// still HTML-escape every value B2 itself interpolates (titles, paths, snippets) so
// UI chrome can't be broken by note content. A DOMPurify pass is a later hardening,
// not needed for a local-first, own-content MVP.

import { marked, type Tokens, type TokenizerAndRendererExtension } from "marked";
import { RELATION_VERBS, type AppState } from "./state";
import type { NoteSummary, ResourceExplainView, ResourceSummary } from "./types";

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

// --- file tree --------------------------------------------------------------------
//
// The navigation pane. `list_notes` + `list_resources` hand us *flat*, path-ordered
// per-kind lists (research §9b #10 — two contracts, composed here); arranging them
// into one folder tree is pure presentation, so it lives here in `ui/` (not the host —
// the host stays a dumb adapter). Note rows reuse the `[data-open]` delegation that
// search/discovery cards already use; resource rows get `[data-open-resource]`, which
// opens the fallback card.

/** One tree leaf — a note or a resource, normalized for display. */
interface TreeFile {
  kind: "note" | "resource";
  path: string;
  label: string;
  /** The resource class glyph slot ("" for notes). */
  glyph: string;
}

interface TreeDir {
  name: string;
  /** Vault-relative folder path, no trailing slash ("" for the root). */
  path: string;
  dirs: Map<string, TreeDir>;
  files: TreeFile[];
}

/** A small, unobtrusive per-class marker so a resource reads as "not a note". */
const CLASS_GLYPHS: Record<string, string> = {
  image: "▣",
  media: "▶",
  pdf: "▤",
  html: "◇",
  text: "≡",
  binary: "◆",
};

/** Fold the flat, path-ordered note + resource lists into one nested folder tree. */
function buildTree(notes: NoteSummary[], resources: ResourceSummary[]): TreeDir {
  const root: TreeDir = { name: "", path: "", dirs: new Map(), files: [] };
  const insert = (file: TreeFile) => {
    const parts = file.path.split("/");
    let dir = root;
    for (const seg of parts.slice(0, -1)) {
      const full = dir.path ? `${dir.path}/${seg}` : seg;
      let child = dir.dirs.get(seg);
      if (!child) {
        child = { name: seg, path: full, dirs: new Map(), files: [] };
        dir.dirs.set(seg, child);
      }
      dir = child;
    }
    dir.files.push(file);
  };
  for (const note of notes) {
    insert({ kind: "note", path: note.path, label: fileLabel(note), glyph: "" });
  }
  for (const r of resources) {
    insert({
      kind: "resource",
      path: r.path,
      label: r.path.split("/").pop() ?? r.path,
      glyph: CLASS_GLYPHS[r.class] ?? CLASS_GLYPHS.binary,
    });
  }
  return root;
}

/** A note's display label: its title, else the filename without the `.md`. */
function fileLabel(note: NoteSummary): string {
  if (note.title) return note.title;
  const base = note.path.split("/").pop() ?? note.path;
  return base.replace(/\.md$/i, "");
}

/** Render one folder's children (its sub-folders, then its files), recursively. */
function treeChildrenHtml(dir: TreeDir, state: AppState, depth: number): string {
  const subdirs = [...dir.dirs.values()].sort((a, b) => a.name.localeCompare(b.name));
  const files = [...dir.files].sort((a, b) => a.label.localeCompare(b.label));
  // Indent by depth; a folder's own chevron occupies the same slot a file's icon does,
  // so files sit one notch deeper than the folder header above them.
  const pad = (d: number) => `padding-left:${8 + d * 14}px`;

  const dirHtml = subdirs
    .map((sub) => {
      const open = state.expandedDirs.has(sub.path);
      const header = `<button class="tree-row tree-dir" data-dir="${escapeHtml(
        sub.path,
      )}" style="${pad(depth)}" aria-expanded="${open}">
          <span class="tree-caret">${open ? "▾" : "▸"}</span>
          <span class="tree-label">${escapeHtml(sub.name)}</span>
        </button>`;
      const body = open ? treeChildrenHtml(sub, state, depth + 1) : "";
      return header + body;
    })
    .join("");

  const fileHtml = files
    .map((file) => {
      if (file.kind === "resource") {
        const active = state.currentResource?.path === file.path ? " is-active" : "";
        return `<button class="tree-row tree-file tree-resource${active}" data-open-resource="${escapeHtml(
          file.path,
        )}" style="${pad(depth)}" title="${escapeHtml(file.path)}">
            <span class="tree-caret tree-glyph">${file.glyph}</span>
            <span class="tree-label">${escapeHtml(file.label)}</span>
          </button>`;
      }
      const active = state.current?.path === file.path ? " is-active" : "";
      return `<button class="tree-row tree-file${active}" data-open="${escapeHtml(
        file.path,
      )}" style="${pad(depth)}" title="${escapeHtml(file.path)}">
          <span class="tree-caret"></span>
          <span class="tree-label">${escapeHtml(file.label)}</span>
        </button>`;
    })
    .join("");

  return dirHtml + fileHtml;
}

export function treePaneHtml(state: AppState): string {
  const total = state.notes.length + state.resources.length;
  const head = `<div class="tree-head">
      <h2>Files</h2>
      <span class="tree-count">${total || ""}</span>
    </div>`;
  if (state.vaultRoot === null)
    return head + `<p class="tree-empty">No vault open.</p>`;
  if (total === 0)
    return head + `<p class="tree-empty">No files indexed yet — Reindex to populate.</p>`;
  return (
    head +
    `<div class="tree">${treeChildrenHtml(buildTree(state.notes, state.resources), state, 0)}</div>`
  );
}

// --- pane builders --------------------------------------------------------------

// The note-pane top bar: a full-bleed strip across the top of the note pane (above the
// centered reading column, not inside it). Its head row carries the frontmatter drawer
// toggle on the left and, grouped on the right, the `</>` view-source toggle and the
// **Edit** toggle (desktop-editing.md §6 — entering edit mode hands the whole pane to
// the CodeMirror editor, so this bar isn't rendered again until edit mode exits). Sits
// as a sibling *before* `<article class="note">` so its divider spans the pane edge to
// edge, like the file tree's "Files" header.
//
// The frontmatter drawer is a collapsible peek at the note's raw YAML (verbatim, as on
// disk — `relations:` and any unmodeled keys included). The `</>` toggle flips the note
// body between rendered Markdown and its raw source. Both are state-controlled (not
// native `<details>`) so their open state survives the full-pane re-render a toast timer
// or tree toggle triggers, and both stay sticky across notes. The bar is always
// rendered, so the note pane's chrome is stable; a note with no frontmatter unfolds to
// an explicit empty state.
function noteBarHtml(state: AppState, frontmatter: string | null): string {
  const open = state.frontmatterOpen;
  const source = state.sourceOpen;
  const yaml = frontmatter?.replace(/\s+$/, "") ?? "";
  const body = !open
    ? ""
    : yaml
      ? `<pre class="frontmatter-block">${escapeHtml(yaml)}</pre>`
      : `<p class="frontmatter-empty">No frontmatter.</p>`;
  return `<div class="frontmatter-bar">
      <div class="note-bar-head">
        <button class="frontmatter-toggle" data-toggle-frontmatter aria-expanded="${open}">
          <span class="tree-caret">${open ? "▾" : "▸"}</span>
          <span class="frontmatter-label">Frontmatter</span>
        </button>
        <div class="note-bar-actions">
          <button class="source-toggle${source ? " is-active" : ""}" data-toggle-source aria-pressed="${source}" title="${
            source ? "Show rendered Markdown" : "Show Markdown source"
          }">&lt;/&gt;</button>
          <button class="edit-toggle" data-toggle-edit${
            state.loading ? " disabled" : ""
          } title="Edit this note (autosaves as you type)">Edit</button>
        </div>
      </div>
      ${body}
    </div>`;
}

/** Human-readable byte count for the card ("67 B", "1.4 KB", "3.2 MB"). */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// The resource **fallback card** (file-type slice 1, spec §6): selecting any file in
// the tree opens *something*. Slice 1 shows the card for every resource class —
// filename, class, size, modified, content hash — plus the backlinks panel (which
// notes reference this file, with their authored captions) and one action, *Open in
// system default* (an OS handoff performed host-side). Per-class viewers replace the
// card's body in slice 2; the card remains the `binary` catch-all.
function resourceCardHtml(r: ResourceExplainView): string {
  const modified = r.mtime ? new Date(r.mtime * 1000).toLocaleString() : "—";
  const backlinks = r.backlinks.length
    ? `<div class="cards">${r.backlinks
        .map((b) => {
          const context = [
            b.type + (b.embed ? " (embed)" : ""),
            b.caption ? `“${b.caption}”` : "",
          ]
            .filter(Boolean)
            .join(" — ");
          return `<button class="card" data-open="${escapeHtml(b.path)}">
              <div class="card-title">${escapeHtml(b.title ?? b.path)}</div>
              <div class="card-path">${escapeHtml(b.path)}</div>
              <div class="card-snip">${escapeHtml(context)}</div>
            </button>`;
        })
        .join("")}</div>`
    : `<p class="side-empty">No notes link to this file yet.</p>`;
  const name = r.path.split("/").pop() ?? r.path;
  return `<article class="note resource-card">
      <header class="note-head">
        <h1>${escapeHtml(name)}</h1>
        <div class="note-meta">${escapeHtml(r.path)} · ${escapeHtml(r.class)} · ${formatSize(
          r.size,
        )} · modified ${escapeHtml(modified)}</div>
      </header>
      <div class="resource-card-body">
        <p class="resource-no-viewer">No viewer available for this file type yet.</p>
        <button class="resource-open" data-open-system="${escapeHtml(r.path)}">
          Open in system default
        </button>
        <div class="resource-hash" title="${escapeHtml(r.content_hash)}">
          blake3 ${escapeHtml(r.content_hash.slice(0, 16))}…
        </div>
        <h2 class="resource-backlinks-head">Backlinks</h2>
        ${backlinks}
      </div>
    </article>`;
}

export function notePaneHtml(state: AppState): string {
  if (state.currentResource) return resourceCardHtml(state.currentResource);
  const n = state.current;
  if (n) {
    const metaBits = [n.type, n.created].filter(Boolean).map((s) => escapeHtml(s as string));
    const meta = [escapeHtml(n.path), ...metaBits].join(" · ");
    const tags = n.tags.length
      ? `<div class="tags">${n.tags
          .map((t) => `<span class="tag">${escapeHtml(t)}</span>`)
          .join("")}</div>`
      : "";
    const body = state.sourceOpen
      ? `<pre class="note-source">${escapeHtml(n.body)}</pre>`
      : renderMarkdown(n.body);
    return `${noteBarHtml(state, n.frontmatter)}
      <article class="note">
        <header class="note-head">
          <h1>${escapeHtml(n.title ?? n.path)}</h1>
          <div class="note-meta">${meta}</div>
          ${tags}
        </header>
        <div class="note-body">${body}</div>
      </article>`;
  }
  if (state.loading) return `<div class="empty"><p>Loading…</p></div>`;
  if (state.vaultRoot === null) {
    return `<div class="empty">
        <h2>No vault open</h2>
        <p>Click the folder icon in the top bar to choose a vault, or launch B2 with a vault path (or set <code>B2_VAULT_PATH</code>).</p>
      </div>`;
  }
  return `<div class="empty">
      <h2>Read → discover → link</h2>
      <p>Pick a note from the file tree on the left, or search above. B2 will surface its similar-but-unlinked notes on the right, so you can connect them.</p>
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
    if (state.discoveringSimilar)
      return (
        head +
        `<div class="side-empty" role="status" aria-label="Finding similar notes"><span class="spinner"></span></div>`
      );
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
    return (
      head +
      `<p class="side-empty">${
        state.discoveringConnections ? "Loading connections…" : "No connections yet."
      }</p>`
    );
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
