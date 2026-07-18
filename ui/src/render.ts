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
import { RELATION_VERBS, type AppState, type SideSection } from "./state";
import type { NoteSummary, NoteView, ResourceExplainView, ResourceSummary } from "./types";
import {
  buildScene,
  NODE_R,
  VIEW_H,
  VIEW_W,
  type Category,
  type GraphEdge,
  type GraphLens,
  type GraphNode,
  type GraphScene,
} from "./graph";

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
          <span class="tree-caret">${open ? "▼" : "▶"}</span>
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
          <span class="tree-caret">${open ? "▼" : "▶"}</span>
          <span class="frontmatter-label">Frontmatter</span>
        </button>
        <div class="note-bar-actions">
          ${graphToggleHtml(false)}
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
  if (n && state.graphOpen) return graphPaneHtml(state, n);
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

// The honest search-ranking caveat (#26). Search always answers over the keyword (BM25)
// index; this says how much *semantic* ranking is mixed in, so a projected-but-unembedded
// vault never silently under-ranks:
//   • no real model            → "keyword only (run `b2 init`)"
//   • model, nothing embedded  → "keyword-only for now (0/M embedded — Reindex)"
//   • model, partly embedded   → "keyword-first (N/M embedded)" (vector half still filling)
//   • model, fully embedded    → "" (ranking is fully semantic; no caveat)
function searchCaveat(state: AppState): string {
  if (!state.semantic)
    return " · keyword only (run <code>b2 init</code> for semantic)";
  const n = state.notesEmbedded;
  const m = state.notesTotal;
  if (m === 0 || n >= m) return ""; // empty vault, or every note embedded — semantic is live
  return n === 0
    ? ` · keyword-only for now (0/${m} embedded — Reindex)`
    : ` · keyword-first (${n}/${m} embedded)`;
}

function searchSectionHtml(state: AppState): string {
  const head = `<div class="side-head">
      <h2>Results</h2>
      <button class="linklike" data-clear-search>clear</button>
    </div>
    <p class="side-sub">for “${escapeHtml(state.searchQuery)}”${searchCaveat(state)}</p>`;
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

// A collapsible discovery-section header (chevron + title + count) — the same fold
// idiom the file tree and the Frontmatter drawer use, so the right column reads the
// same way. Collapsing is a sticky viewing preference (`collapsedSections`); the count
// is shown only when non-zero so an empty section stays quiet.
function sideFoldHead(
  section: SideSection,
  label: string,
  count: number,
  collapsed: boolean,
): string {
  return `<button class="side-head side-fold" data-fold-section="${section}" aria-expanded="${!collapsed}">
      <span class="tree-caret">${collapsed ? "▶" : "▼"}</span>
      <span class="side-title">${label}</span>
      ${count ? `<span class="side-count">${count}</span>` : ""}
    </button>`;
}

/** A card's per-note fold key — unique across the two sections a path can appear in. */
function cardKey(section: SideSection, path: string): string {
  return `${section}:${path}`;
}

// The card's own fold chevron. Cards default expanded (the snippet is the signal you
// link on); this collapses the body (path + snippet) to just the title row. Kept out of
// the `.card-open` button so a click on the chevron folds without opening the note
// (nested buttons aren't allowed — the chevron and the open-region are siblings).
function cardFold(key: string, collapsed: boolean): string {
  return `<button class="card-fold" data-fold-card="${escapeHtml(
    key,
  )}" aria-expanded="${!collapsed}" aria-label="Toggle details">
      <span class="tree-caret">${collapsed ? "▶" : "▼"}</span>
    </button>`;
}

function similarSectionHtml(state: AppState): string {
  const collapsed = state.collapsedSections.has("similar");
  const head = sideFoldHead(
    "similar",
    "Similar &amp; unlinked",
    state.similar.length,
    collapsed,
  );
  if (collapsed) return head;
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
    .map((c) => {
      const key = cardKey("similar", c.path);
      const folded = state.collapsedCards.has(key);
      const body = folded
        ? ""
        : `<div class="card-body">
            <div class="card-path">${escapeHtml(c.path)}</div>
            ${c.evidence ? `<div class="card-snip">${escapeHtml(c.evidence)}</div>` : ""}
          </div>`;
      // `data-card-path`/`-title` on the root feed the right-click menu (Open / Link…);
      // the whole card is the target now that the inline Link button is gone.
      return `<div class="card foldable candidate${folded ? " is-collapsed" : ""}" data-card-path="${escapeHtml(
        c.path,
      )}" data-card-title="${escapeHtml(c.title ?? "")}">
          <div class="card-head">
            ${cardFold(key, folded)}
            <button class="card-open" data-open="${escapeHtml(c.path)}">
              <span class="card-title">${escapeHtml(c.title ?? c.path)}</span>
              <span class="card-score">${c.score.toFixed(3)}</span>
            </button>
          </div>
          ${body}
        </div>`;
    })
    .join("");
  return head + `<div class="cards">${items}</div>`;
}

function connectionsSectionHtml(state: AppState): string {
  const count = state.connections.length + state.unresolved.length;
  const collapsed = state.collapsedSections.has("connections");
  const head = sideFoldHead("connections", "Connections", count, collapsed);
  if (collapsed) return head;
  if (count === 0)
    return (
      head +
      `<p class="side-empty">${
        state.discoveringConnections ? "Loading connections…" : "No connections yet."
      }</p>`
    );
  const items = state.connections
    .map((c) => {
      const arrow = c.direction === "outbound" ? "→" : "←";
      const key = cardKey("connections", c.path);
      const folded = state.collapsedCards.has(key);
      const why = c.explanation
        ? `<div class="card-snip">${escapeHtml(c.explanation)}</div>`
        : "";
      const body = folded
        ? ""
        : `<div class="card-body">
            <div class="card-path">${escapeHtml(c.title ?? c.path)}</div>
            ${why}
          </div>`;
      return `<div class="card edge foldable${folded ? " is-collapsed" : ""}">
          <div class="card-head">
            ${cardFold(key, folded)}
            <button class="card-open" data-open="${escapeHtml(c.path)}">
              <span class="card-title"><span class="edge-arrow">${arrow}</span> ${escapeHtml(
                c.label,
              )} <span class="edge-origin">${escapeHtml(c.origin)}</span></span>
            </button>
          </div>
          ${body}
        </div>`;
    })
    .join("");
  return head + `<div class="cards">${items}${unresolvedCardsHtml(state)}</div>`;
}

// Dangling outbound links — a `[[folder]]` or a typo that resolves to no note or
// file (GH #12). Not clickable (nothing to open), so a plain `div`, flagged with a
// broken-link emblem so it reads as broken rather than silently missing. The target
// is shown as written (`[[Hermes]]`), which is what the user can fix in the note.
function unresolvedCardsHtml(state: AppState): string {
  return state.unresolved
    .map((u) => {
      const why = u.explanation
        ? `<div class="card-snip">${escapeHtml(u.explanation)}</div>`
        : "";
      return `<div class="card edge broken" title="This link points to nothing — no note or file named “${escapeHtml(
        u.target,
      )}”. A note is a single .md file, so a folder can’t be linked.">
          <div class="card-title"><span class="edge-broken" aria-label="Broken link">⚠</span> ${escapeHtml(
            u.relation,
          )} <span class="edge-origin">${escapeHtml(u.origin)}</span></div>
          <div class="card-path">[[${escapeHtml(u.target)}]] · unresolved</div>
          ${why}
        </div>`;
    })
    .join("");
}

// --- the anchored ghost graph (GH #22) ----------------------------------------------
//
// The center pane's third mode: the open note's typed neighborhood as hand-rolled,
// deterministic SVG — scene geometry from `graph.ts` (pure, unit-tested), markup
// here, clicks delegated in main.ts. The reading key: color = edge category, solid =
// authored / dashed teal = latent (`similar`), disc = note / square = resource /
// dashed hollow = dangling. Everything renders from state the note-open already
// fetched, so entering the graph (and switching lenses) costs no IPC.

/** The lens selector's entries, in display order. */
const LENSES: { id: GraphLens; label: string; blurb: string }[] = [
  { id: "all", label: "All", blurb: "Every authored edge, plus latent (ghost) candidates" },
  { id: "lineage", label: "Lineage", blurb: "supersedes / derived-from on a time axis" },
  { id: "argument", label: "Argument", blurb: "supports / refutes / contradicts around the claim" },
];

/** The small node-and-edges glyph on the graph toggle (both bars). */
const GRAPH_ICON = `<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M8 5.4 4 10.6M8 5.4l4 5.2" stroke="currentColor" stroke-width="1.4" fill="none"/>
    <circle cx="8" cy="3.4" r="2.1" fill="currentColor"/>
    <circle cx="3.4" cy="12.4" r="2.1" fill="currentColor"/>
    <circle cx="12.6" cy="12.4" r="2.1" fill="currentColor"/>
  </svg>`;

/** The graph toggle chip, shared by the reading bar (off) and the graph bar (on). */
function graphToggleHtml(active: boolean): string {
  return `<button class="source-toggle graph-toggle${active ? " is-active" : ""}" data-toggle-graph
      aria-pressed="${active}" title="${active ? "Back to reading" : "Show the connection graph"}">${GRAPH_ICON}</button>`;
}

/** Fixed-point SVG coordinate — keeps the markup compact and diff-stable. */
function px(v: number): string {
  return (Math.round(v * 10) / 10).toString();
}

/** One edge's path (a straight segment, or the parallel-separating quadratic). */
function edgePathD(e: GraphEdge): string {
  return e.cx === null || e.cy === null
    ? `M ${px(e.x1)} ${px(e.y1)} L ${px(e.x2)} ${px(e.y2)}`
    : `M ${px(e.x1)} ${px(e.y1)} Q ${px(e.cx)} ${px(e.cy)} ${px(e.x2)} ${px(e.y2)}`;
}

function edgeHtml(e: GraphEdge): string {
  if (e.ghost) {
    return `<path class="gedge is-ghost" d="${edgePathD(e)}"/>`;
  }
  const verb = e.label.replace(/[^a-z0-9-]/gi, "");
  const marker = e.arrow ? ` marker-end="url(#garr-${e.category})"` : "";
  const label = `<text class="gedge-label cat-${e.category}" x="${px(e.lx)}" y="${px(
    e.ly - 6,
  )}">${escapeHtml(e.label)}</text>`;
  return `<path class="gedge cat-${e.category} verb-${verb}" d="${edgePathD(e)}"${marker}/>${label}`;
}

/** A node's shape + glyph, by kind (labels are added by the group builder). */
function nodeShapeHtml(n: GraphNode): string {
  const x = px(n.x);
  const y = px(n.y);
  switch (n.kind) {
    case "anchor":
      return `<circle class="gring" cx="${x}" cy="${y}" r="${NODE_R.anchor}"/>
        <circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R.anchor - 7}"/>
        <circle class="gcore" cx="${x}" cy="${y}" r="7"/>`;
    case "resource": {
      const s = NODE_R.resource - 2;
      return `<rect class="gshape" x="${px(n.x - s)}" y="${px(n.y - s)}" width="${2 * s}" height="${2 * s}" rx="9"/>
        <text class="gglyph" x="${x}" y="${px(n.y + 5)}">${CLASS_GLYPHS[n.sub ?? ""] ?? CLASS_GLYPHS.binary}</text>`;
    }
    case "dangling":
      return `<circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R.dangling}"/>
        <text class="gglyph" x="${x}" y="${px(n.y + 5)}">⚠</text>`;
    default:
      return `<circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R[n.kind]}"/>`;
  }
}

/** The tooltip line(s) for a node — also the click affordance's explanation. */
function nodeTitle(n: GraphNode): string {
  switch (n.kind) {
    case "anchor":
      return `${n.full} — the open note. Click to return to reading.`;
    case "ghost":
      return `${n.full} — similar but not linked (similarity ${n.sub ?? "?"}). Click to link it; right-click for more.`;
    case "dangling":
      return `${n.full} resolves to no note or file — fix the link in the note.`;
    case "resource":
      return `${n.full} (${n.sub ?? "file"}) — click to open.`;
    default:
      return `${n.full} — click to open.`;
  }
}

/**
 * One scene node as an interactive `<g>`, its incident edges inside it so a pure-CSS
 * hover lights the node *and* its edges while the rest of the scene dims. The click
 * affordance rides existing delegation: notes reuse `data-open`, resources
 * `data-open-resource`; ghosts get `data-ghost-link` (→ the link palette) plus the
 * `data-card-*` pair the right-click menu reads; the anchor toggles back to reading.
 */
function nodeGroupHtml(n: GraphNode, edges: GraphEdge[], order: number): string {
  const attrs: string[] = [`class="gnode is-${n.kind}"`, `style="--i:${order}"`];
  if (n.kind === "note" && n.path) attrs.push(`data-open="${escapeHtml(n.path)}"`);
  if (n.kind === "anchor") attrs.push(`data-toggle-graph="1"`);
  if (n.kind === "resource" && n.path) attrs.push(`data-open-resource="${escapeHtml(n.path)}"`);
  if (n.kind === "ghost" && n.path) {
    attrs.push(
      `data-ghost-link="${escapeHtml(n.path)}"`,
      `data-card-path="${escapeHtml(n.path)}"`,
      `data-card-title="${escapeHtml(n.title ?? "")}"`,
    );
  }
  const r = NODE_R[n.kind];
  // Text goes on the side of the node facing *away* from the anchor (above for the
  // upper half of the scene), so a label never sits in its own edge's path.
  const above = n.kind !== "anchor" && n.y < VIEW_H / 2 - 20;
  const label = `<text class="gnode-label" x="${px(n.x)}" y="${px(
    above ? n.y - r - 14 : n.y + r + 18,
  )}">${escapeHtml(n.label)}</text>`;
  const sub = n.sub
    ? `<text class="gnode-sub" x="${px(n.x)}" y="${px(
        above ? n.y - r - 29 : n.y + r + 33,
      )}">${escapeHtml(n.sub)}</text>`
    : "";
  return `<g ${attrs.join(" ")}>
      <title>${escapeHtml(nodeTitle(n))}</title>
      ${edges.map(edgeHtml).join("")}
      ${nodeShapeHtml(n)}
      ${label}${sub}
    </g>`;
}

/** Per-lens chrome drawn under the scene: the lineage time axis, the argument
 *  zones + fault line. Pure annotation — no hit targets. */
function lensChromeHtml(lens: GraphLens, scene: GraphScene): string {
  if (lens === "lineage") {
    const y = VIEW_H - 34;
    return `<g class="gaxis" aria-hidden="true">
        <line x1="150" y1="${y}" x2="850" y2="${y}" marker-end="url(#garr-axis)"/>
        <text x="150" y="${y - 10}" text-anchor="start">older</text>
        <text x="850" y="${y - 10}" text-anchor="end">newer</text>
      </g>`;
  }
  if (lens === "argument") {
    const fault = scene.edges.some((e) => e.label === "contradicts")
      ? `<line class="gfault" x1="${VIEW_W / 2}" y1="56" x2="${VIEW_W / 2}" y2="${VIEW_H - 56}"/>`
      : "";
    return `<g class="gzone" aria-hidden="true">${fault}
        <text x="195" y="42" text-anchor="middle">supports →</text>
        <text x="805" y="42" text-anchor="middle">← refutes</text>
      </g>`;
  }
  return "";
}

/** The honest ghost-halo caveat (mirrors `searchCaveat`'s tiers, #26): why there are
 *  no ghosts right now, or null when there are (or when silence is the honest state). */
function ghostHintHtml(state: AppState): string {
  if (state.graphLens !== "all" || state.similar.length > 0) return "";
  if (state.discoveringSimilar)
    return `<div class="graph-hint is-scanning"><span class="spinner"></span>scanning for latent connections…</div>`;
  if (!state.semantic)
    return `<div class="graph-hint">ghost connections need the semantic model — run <code>b2 init</code>, then Reindex</div>`;
  if (state.notesTotal > 0 && state.notesEmbedded < state.notesTotal)
    return `<div class="graph-hint">ghosts appear once the vault is embedded — Reindex</div>`;
  return "";
}

/** The centered guidance when a lens has nothing to draw (the anchor always shows). */
function graphEmptyHtml(state: AppState, scene: GraphScene): string {
  if (scene.edges.length > 0) return "";
  if (state.graphLens === "lineage")
    return `<div class="graph-empty"><p>No lineage yet.</p>
      <p class="muted">Link with <code>supersedes</code> or <code>derived-from</code> to see this idea's history on a time axis.</p></div>`;
  if (state.graphLens === "argument")
    return `<div class="graph-empty"><p>No argument yet.</p>
      <p class="muted"><code>supports</code>, <code>refutes</code>, and <code>contradicts</code> edges map a claim's evidence here.</p></div>`;
  if (state.discoveringSimilar) return "";
  return `<div class="graph-empty"><p>No connections yet.</p>
    <p class="muted">B2 floats similar-but-unlinked notes here as ghosts — click one to make the connection real.</p></div>`;
}

/** The reading key, one quiet strip: category colors, edge states, node shapes. */
function graphLegendHtml(): string {
  const cats: [Category, string][] = [
    ["referential", "referential"],
    ["expository", "expository"],
    ["evidential", "evidential"],
    ["structural", "structural"],
    ["versioning", "versioning"],
  ];
  const dots = cats
    .map(([c, label]) => `<span class="leg"><span class="leg-dot cat-${c}"></span>${label}</span>`)
    .join("");
  return `<div class="graph-legend" aria-hidden="true">${dots}
      <span class="leg"><span class="leg-dash"></span>ghost (unlinked)</span>
      <span class="leg"><span class="leg-square"></span>file</span>
      <span class="leg"><span class="leg-broken">⚠</span>broken</span>
    </div>`;
}

/** Arrowhead markers, one per category (an SVG marker can't inherit its edge's
 *  stroke everywhere yet), plus the muted lineage-axis arrow. */
function graphDefsHtml(): string {
  const cats: Category[] = ["referential", "expository", "evidential", "structural", "versioning", "other"];
  const arrow = (id: string, cls: string) =>
    `<marker id="${id}" viewBox="0 0 10 10" refX="8.5" refY="5" markerWidth="7.5" markerHeight="7.5" orient="auto-start-reverse">
       <path d="M0 0.8 L9.5 5 L0 9.2 z" class="${cls}"/>
     </marker>`;
  return `<defs>${cats.map((c) => arrow(`garr-${c}`, `garr cat-${c}`)).join("")}
    ${arrow("garr-axis", "garr is-axis")}</defs>`;
}

/**
 * The graph pane — the note pane's third mode (Reading / Editing / Graph). Bar:
 * the typed-lens selector + the same action chips as reading; stage: the SVG scene
 * (fills the pane, `viewBox`-scaled) with overlay hints; footer: the reading key.
 */
function graphPaneHtml(state: AppState, n: NoteView): string {
  const scene = buildScene(state.graphLens, {
    anchor: { path: n.path, title: n.title, created: n.created },
    connections: state.connections,
    resources: state.resourceLinks,
    unresolved: state.unresolved,
    ghosts: state.similar,
  });

  const lenses = LENSES.map((l) => {
    const on = state.graphLens === l.id;
    return `<button type="button" class="seg${on ? " seg-on" : ""}" data-graph-lens="${l.id}"
        aria-pressed="${on}" title="${escapeHtml(l.blurb)}">${l.label}</button>`;
  }).join("");

  // Edges live inside their node's group (hover affordance); the anchor renders
  // last so it always paints on top of edge crossings.
  const byNode = new Map<string, GraphEdge[]>();
  for (const e of scene.edges) {
    const owner = e.from === "anchor" ? e.to : e.from;
    const list = byNode.get(owner) ?? [];
    list.push(e);
    byNode.set(owner, list);
  }
  // Paint order: ghosts lowest (their long dashed spokes must pass *under* the
  // authored orbit), authored above them, the anchor on top of everything. The
  // stagger index is narrative, not paint, order: authored pops first, ghosts after.
  const authoredNodes = scene.nodes.filter((node) => node.kind !== "anchor" && node.kind !== "ghost");
  const ghostNodes = scene.nodes.filter((node) => node.kind === "ghost");
  const anchor = scene.nodes.find((node) => node.kind === "anchor");
  const groups = [
    ...ghostNodes.map((node, i) =>
      nodeGroupHtml(node, byNode.get(node.id) ?? [], authoredNodes.length + 1 + i),
    ),
    ...authoredNodes.map((node, i) => nodeGroupHtml(node, byNode.get(node.id) ?? [], i + 1)),
    ...(anchor ? [nodeGroupHtml(anchor, [], 0)] : []),
  ].join("");

  return `<div class="graph-view">
      <div class="graph-bar">
        <div class="segmented graph-lenses" role="group" aria-label="Graph lens">${lenses}</div>
        <div class="note-bar-actions">
          ${graphToggleHtml(true)}
          <button class="edit-toggle" data-toggle-edit${
            state.loading ? " disabled" : ""
          } title="Edit this note (autosaves as you type)">Edit</button>
        </div>
      </div>
      <div class="graph-stage">
        <svg class="graph-svg" viewBox="0 0 ${VIEW_W} ${VIEW_H}" preserveAspectRatio="xMidYMid meet"
             role="img" aria-label="Connection graph for ${escapeHtml(n.title ?? n.path)}">
          ${graphDefsHtml()}
          ${lensChromeHtml(state.graphLens, scene)}
          ${groups}
        </svg>
        ${graphEmptyHtml(state, scene)}
        ${ghostHintHtml(state)}
      </div>
      ${graphLegendHtml()}
    </div>`;
}

/** A cumulative-duration label from milliseconds: "3h 25m", "12m 04s", "45s", "0s". */
function formatDuration(ms: number): string {
  const totalSec = Math.round(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  if (m > 0) return `${m}m ${String(s).padStart(2, "0")}s`;
  return `${s}s`;
}

// The per-model embedding-time ledger (b2-desktop stats.rs): a running total per model,
// summed across every reindex since you selected it, so a model swap can be judged on
// real speed. Switching to a model restarts its total (the swap re-embeds the whole
// corpus), so each row covers only that model's current stint — the copy says so. One row
// per model that has history: total time, chunks, and derived throughput, current marked.
function embedStatsHtml(state: AppState): string {
  const byModel = new Map(state.embedStats.map((s) => [s.model, s]));
  // Order by the picker so rows are stable; only models with recorded time appear.
  const rows = state.models
    .map((m) => ({ model: m, stat: byModel.get(m.id) }))
    .filter((r) => r.stat && r.stat.chunks > 0);
  const head =
    `<div class="settings-subhead">Embedding time</div>` +
    `<p class="settings-detail muted">Running total per model, summed across every reindex since you selected it. Switching models restarts the total.</p>`;
  if (rows.length === 0) {
    return (
      head +
      `<p class="settings-detail muted">No embedding runs recorded yet — Reindex to start measuring.</p>`
    );
  }
  const list = rows
    .map(({ model, stat }) => {
      const s = stat!;
      const perSec = s.total_ms > 0 ? (s.chunks / (s.total_ms / 1000)).toFixed(1) : "—";
      const marker = model.current ? ` <span class="settings-current">current</span>` : "";
      return `<div class="settings-stat">
          <span class="settings-stat-model">${escapeHtml(model.label)}${marker}</span>
          <span class="settings-stat-nums">${formatDuration(s.total_ms)} · ${s.chunks.toLocaleString()} chunks · ${perSec} chunks/sec</span>
        </div>`;
    })
    .join("");
  return head + `<div class="settings-stats">${list}</div>`;
}

// The Settings modal (⌘,). Reuses the link modal's `.modal-*`/`.field` chrome. Today it
// holds the embedding model picker + the per-model embedding-time ledger; built to hold
// more settings later. Mutually exclusive with the link modal in practice, so it takes
// precedence in `modalHtml`.
function settingsModalHtml(state: AppState): string {
  const models = state.models;
  const current = models.find((m) => m.current) ?? models[0];
  const options = models
    .map(
      (m) =>
        `<option value="${escapeHtml(m.id)}"${m.current ? " selected" : ""}>${escapeHtml(
          m.label,
        )}${m.installed ? "" : " — not installed"}</option>`,
    )
    .join("");
  const detail = current
    ? `<p class="settings-detail">${escapeHtml(current.description)} · ${current.dim}-dim · ${
        current.installed ? "installed" : "not installed"
      }</p>`
    : `<p class="settings-detail muted">Loading models…</p>`;
  // Subtle badge: which compute device the build embeds on (GH #40). Metal gets the accent
  // pill + a ⚡ cue; CPU is a neutral pill. Hidden until the async read resolves.
  const device = state.embedDevice;
  const deviceRow = device
    ? `<p class="settings-device">Embedding on <span class="settings-badge${
        device === "Metal" ? " settings-badge-metal" : ""
      }">${device === "Metal" ? "⚡ " : ""}${escapeHtml(device)}</span></p>`
    : "";
  // In-app `b2 init`: a Download button appears when the selected model isn't installed,
  // and a spinner while it downloads (network-bound, can take minutes).
  const provisionRow =
    current && !current.installed
      ? state.provisioning
        ? `<div class="settings-provision"><span class="spinner"></span><span class="muted">Downloading ${escapeHtml(
            current.label,
          )}… this can take a few minutes.</span></div>`
        : `<div class="settings-provision"><button class="btn small primary" id="settings-provision">Download model</button><span class="muted">Required before this model can embed.</span></div>`
      : "";
  // Appearance: System (follow the OS) / Light / Dark. A segmented control rather than a
  // <select> so the three mutually-exclusive choices read at a glance.
  const themes: { id: "system" | "light" | "dark"; label: string }[] = [
    { id: "system", label: "System" },
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
  ];
  const themeButtons = themes
    .map((t) => {
      const on = state.theme === t.id;
      return `<button type="button" class="seg${on ? " seg-on" : ""}" data-theme-choice="${
        t.id
      }" aria-pressed="${on}">${t.label}</button>`;
    })
    .join("");
  return `<div class="modal-backdrop" data-settings-backdrop>
      <div class="modal" role="dialog" aria-modal="true" aria-label="Settings">
        <h3>Settings</h3>
        <div class="field">
          <span class="field-label">Appearance</span>
          <div class="segmented" role="group" aria-label="Appearance">${themeButtons}</div>
        </div>
        <label class="field">Embedding model
          <select id="settings-model"${
            models.length && !state.provisioning ? "" : " disabled"
          }>${options}</select>
        </label>
        ${detail}
        ${deviceRow}
        ${provisionRow}
        <p class="settings-note">Changing the model re-embeds the whole vault on the next
          Reindex. A newly-chosen model is downloaded with the button above.</p>
        ${embedStatsHtml(state)}
        ${
          state.modelsDir
            ? `<div class="settings-subhead">Model files</div>
               <p class="settings-path" title="${escapeHtml(state.modelsDir)}">${escapeHtml(
                 state.modelsDir,
               )}</p>`
            : ""
        }
        <div class="modal-actions">
          <button class="btn primary" data-settings-close>Done</button>
        </div>
      </div>
    </div>`;
}

// The discovery card right-click menu (replaces the inline "Link…" button on Similar
// cards). Anchored at the cursor via inline left/top — the coords are set + clamped
// on-screen in main.ts, and are plain numbers, so no escaping is needed. Rendered into
// its own overlay root so it floats above the panes; an outside click / Escape / scroll
// dismisses it (main.ts).
export function contextMenuHtml(state: AppState): string {
  const m = state.contextMenu;
  if (!m) return "";
  return `<div class="context-menu" style="left:${m.x}px;top:${m.y}px" role="menu">
      <button class="context-item" data-ctx-open role="menuitem">Open note</button>
      <button class="context-item" data-ctx-link role="menuitem">Link…</button>
    </div>`;
}

export function modalHtml(state: AppState): string {
  if (state.settingsOpen) return settingsModalHtml(state);
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
