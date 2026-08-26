// The view: Markdown → HTML (with clickable wikilinks) and the pane HTML builders.
// Pure functions of state — no IPC, no DOM mutation (main.ts writes the output in).
//
// Safety (invariant E5 — **note content is untrusted input**): authorship is not trust, a
// `.md` can come from anyone (a shared vault, a downloaded or clipped note), so this
// module treats every note body as hostile. Two rules hold together here: B2 HTML-escapes
// every value *it* interpolates (titles, paths, snippets) so UI chrome can't be broken by
// note content, and the one Markdown→HTML path sanitizes its output — `sanitize.ts`, wired
// below as `marked`'s `postprocess` hook, so *every* caller is covered by construction
// rather than by remembering. The webview CSP (`default-src 'self'`, no inline scripts)
// is a second, independent layer, never the only one
// (crates/b2-desktop/CLAUDE.md, GH #77).

import { marked, type Tokens, type TokenizerAndRendererExtension } from "marked";
// Relative imports carry their `.ts` here (the idiom highlight.ts and reconcile.ts already
// use) because render.test.ts runs this module off the source under node's type-stripping,
// which resolves by real filename — a bundler-style extensionless value import doesn't
// resolve there. tsc rewrites nothing (noEmit).
import { escapeHtml } from "./escape.ts";
import { sanitizeHtml } from "./sanitize.ts";
import { RELATION_VERBS, type AppState, type SideSection } from "./state.ts";
import { allDirs, canMoveInto, renamePrefill } from "./move.ts";
import { shouldPromptEmbedInstall } from "./embedreminder.ts";
import { STRENGTH_MIN_CANDIDATES, strengthBand } from "./strength.ts";
import { type ShortcutKey, shortcuts } from "./shortcuts.ts";
import {
  DEFAULT_BINDINGS,
  activeBindings,
  displayChord,
  displayKeys,
  findBinding,
} from "./bindings.ts";
import { customized, refused } from "./keymap.ts";
import { SETTINGS_TABS, type SettingsTabId } from "./settingstabs.ts";
import {
  buildTree,
  rovingPath,
  sortedFiles,
  sortedSubdirs,
  visibleRows,
  type TreeDir,
} from "./treenav.ts";
import {
  directionIcon,
  foldChevron,
  folderIcon,
  icon,
  NOTE_ICON,
  resourceIcon,
  sceneIcon,
  type IconName,
} from "./icons.ts";
import {
  cardKey,
  cardRowKey,
  rovingSideKey,
  sectionRowKey,
  sideRows,
} from "./sidenav.ts";
import {
  type ChatMessage,
  OLLAMA_CLOUD_URL,
  OLLAMA_QUICKSTART_URL,
  PULL_PLACEHOLDER,
  STREAMING_ROW_KEY,
  chatEmptyState,
  citationRowKey,
  formatModelSize,
  pullCommand,
  retrievalNote,
  turnRowKey,
} from "./chat.ts";
import type { ChatSetup, NoteView, ResourceExplainView } from "./types.ts";
import {
  buildScene,
  NODE_R,
  VIEW_H,
  VIEW_W,
  type Category,
  type GraphEdge,
  type GraphNode,
  type GraphScene,
} from "./graph.ts";

// Re-exported so this module stays the view layer's single import surface (main.ts
// reaches for it here); the definition lives in escape.ts.
export { escapeHtml };

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

// Wrap each table in a scroll box so a wide one scrolls *within* its column instead of
// stretching the pane. The table itself must stay a real `display: table` (the wrapper
// is what's `display: block; overflow-x: auto`) — a `display: block` table splits
// marked's whitespace-separated `<thead>`/`<tbody>` into two anonymous tables, so
// `border-collapse` can't join the header row onto the body (the gap bug). marked
// escapes cell content, so these are the only literal `<table>` tags in the output.
function wrapTables(html: string): string {
  return html
    .replace(/<table>/g, '<div class="md-table"><table>')
    .replace(/<\/table>/g, "</table></div>");
}

// The postprocess hook is where the trust boundary sits (E5, GH #77). Two properties come
// from putting it *here* rather than inside `renderMarkdown`: sanitizing is the last thing
// that happens to the HTML — nothing, not even B2's own table wrapper, is spliced in
// afterwards — and it holds for every `marked.parse` in the app, so a future second call
// site cannot render an unsanitized note by forgetting a step.
marked.use({
  extensions: [wikilink],
  gfm: true,
  breaks: false,
  hooks: { postprocess: (html: string) => sanitizeHtml(wrapTables(html)) },
});

/** Note body → the HTML the panes write into the DOM. Sanitized (see the hook above). */
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
//
// The tree's shape and its *row order* live in treenav.ts — pure, tested, and shared
// with the arrow-key navigation (K1, GH #78), because a tree you can arrow through in
// a different order than you can see is worse than no arrows at all.

// A row is two fixed slots and then its label: the **fold chevron** (does this row open?)
// and the **thing icon** (what is this row?). Keeping them apart is what lets a file line
// its icon up under its folder's icon rather than under the folder's chevron, and it is why
// a folder shows both — the chevron is the affordance, the folder is the thing, and one
// glyph doing both jobs makes you learn which job it is doing today (icons.ts).

/** The fold chevron in its slot. Every foldable row in the app calls this one function. */
function foldCaretHtml(open: boolean): string {
  return `<span class="tree-caret">${icon(foldChevron(open), { size: 12 })}</span>`;
}

/** The chevron slot, held open for a row that doesn't fold — so the icons of a folder's
 *  files align with the folder's own icon rather than stepping left. */
const NO_CARET = `<span class="tree-caret"></span>`;

/** The "what is this" slot: a folder, a note, or a resource's type (icons.ts). */
function rowIconHtml(name: IconName): string {
  return `<span class="tree-icon">${icon(name)}</span>`;
}

/** The inline name input for a pending create (new note / new folder), rendered at
 *  the top of its target folder's children. The typed value lives only in the DOM —
 *  main.ts commits on Enter/blur, cancels on Escape, and carries it across an
 *  unrelated tree repaint.
 *
 *  `role="none"` because this row sits inside the `role="tree"` container but is *not*
 *  a `treeitem` — it's a text field. Without it the tree owns a child of no known
 *  role, and it does so exactly while a screen-reader user is naming a note. The role
 *  is not inherited by the focusable input inside, which keeps its own `aria-label`. */
function treeCreateRowHtml(kind: "note" | "folder", pad: string): string {
  const folder = kind === "folder";
  return `<div class="tree-row tree-create" role="none" style="${pad}">
      ${folder ? foldCaretHtml(false) : NO_CARET}
      ${rowIconHtml(folder ? folderIcon(false) : NOTE_ICON)}
      <input id="tree-create-input" class="tree-create-input" type="text"
        placeholder="${kind === "note" ? "New note…" : "New folder…"}"
        aria-label="${kind === "note" ? "New note name" : "New folder name"}"
        autocomplete="off" spellcheck="false" />
    </div>`;
}

/** The inline rename input, rendered in place of the row being renamed — the
 *  rename sibling of `treeCreateRowHtml` (same commit/cancel wiring in main.ts,
 *  same value-carrying across repaints in paintTree). It keeps the row's own two
 *  slots so the input reads as "this row, editable". `role="none"` for the same
 *  reason as the create row above: a text field is not a `treeitem`. */
function treeRenameRowHtml(prefill: string, caret: string, name: IconName, pad: string): string {
  return `<div class="tree-row tree-create" role="none" style="${pad}">
      ${caret}
      ${rowIconHtml(name)}
      <input id="tree-rename-input" class="tree-create-input" type="text"
        value="${escapeHtml(prefill)}"
        aria-label="Rename" autocomplete="off" spellcheck="false" />
    </div>`;
}

/**
 * Render one folder's children (its sub-folders, then its files), recursively — in
 * treenav.ts's order, which is also the order the arrow keys walk.
 *
 * `roving` is the one row that carries `tabindex="0"`: every other row is `-1`, so
 * the tree is a *single* Tab stop and the arrow keys move within it (the ARIA tree
 * pattern — a 1500-note vault is not a tab sequence). `data-tree-row` is the row's
 * keyboard identity: main.ts looks a row up by path to move focus after a repaint,
 * without having to build a CSS selector out of an arbitrary filename.
 */
function treeChildrenHtml(
  dir: TreeDir,
  state: AppState,
  depth: number,
  roving: string | null,
): string {
  // Indent by depth. Every row spends the same two slots — chevron, then icon — so a
  // folder's files line up under the folder's icon, with only the folder's chevron filled.
  const pad = (d: number) => `padding-left:${8 + d * 14}px`;
  // ARIA levels are 1-based; the DOM is flat (rows are siblings, not nested lists),
  // so `aria-level` is what tells a screen reader how deep a row sits.
  const level = ` aria-level="${depth + 1}"`;
  const tab = (path: string) => ` tabindex="${path === roving ? "0" : "-1"}"`;

  // An open create input renders first in its target folder (startTreeCreate
  // expanded the chain down to here, so a match is always visible).
  const create =
    state.treeCreate && state.treeCreate.dir === dir.path
      ? treeCreateRowHtml(state.treeCreate.kind, pad(depth))
      : "";

  const dirHtml = sortedSubdirs(dir)
    .map((sub) => {
      const open = state.expandedDirs.has(sub.path);
      const selected = state.selectedDir === sub.path ? " is-selected" : "";
      const header =
        state.treeRename?.path === sub.path
          ? treeRenameRowHtml(
              renamePrefill(sub.path, "folder"),
              foldCaretHtml(open),
              folderIcon(open),
              pad(depth),
            )
          : `<button class="tree-row tree-dir${selected}" role="treeitem"${level}${tab(
              sub.path,
            )} data-tree-row="${escapeHtml(sub.path)}" data-dir="${escapeHtml(
              sub.path,
            )}" style="${pad(depth)}" aria-expanded="${open}" draggable="true">
          ${foldCaretHtml(open)}
          ${rowIconHtml(folderIcon(open))}
          <span class="tree-label">${escapeHtml(sub.name)}</span>
        </button>`;
      const body = open ? treeChildrenHtml(sub, state, depth + 1, roving) : "";
      return header + body;
    })
    .join("");

  const fileHtml = sortedFiles(dir)
    .map((file) => {
      if (state.treeRename?.path === file.path) {
        return treeRenameRowHtml(
          renamePrefill(file.path, file.kind),
          NO_CARET,
          file.icon,
          pad(depth),
        );
      }
      if (file.kind === "resource") {
        const active = state.currentResource?.path === file.path ? " is-active" : "";
        return `<button class="tree-row tree-file tree-resource${active}" role="treeitem"${level}${tab(
          file.path,
        )} aria-selected="${state.currentResource?.path === file.path}" data-tree-row="${escapeHtml(
          file.path,
        )}" data-open-resource="${escapeHtml(
          file.path,
        )}" style="${pad(depth)}" title="${escapeHtml(file.path)}" draggable="true">
            ${NO_CARET}
            ${rowIconHtml(file.icon)}
            <span class="tree-label">${escapeHtml(file.label)}</span>
          </button>`;
      }
      const active = state.current?.path === file.path ? " is-active" : "";
      return `<button class="tree-row tree-file${active}" role="treeitem"${level}${tab(
        file.path,
      )} aria-selected="${state.current?.path === file.path}" data-tree-row="${escapeHtml(
        file.path,
      )}" data-open="${escapeHtml(
        file.path,
      )}" style="${pad(depth)}" title="${escapeHtml(file.path)}" draggable="true">
          ${NO_CARET}
          ${rowIconHtml(file.icon)}
          <span class="tree-label">${escapeHtml(file.label)}</span>
        </button>`;
    })
    .join("");

  return create + dirHtml + fileHtml;
}

/** The tree-head create icons (new note / new folder). Contextual: both target the
 *  selection's folder, named in the tooltip so ⌘N is never a surprise. */
function treeActionsHtml(state: AppState): string {
  const ctx = state.selectedDir ? `in ${state.selectedDir}/` : "in the vault root";
  return `<span class="tree-actions">
      <button class="tree-action" data-new-note title="New note ${escapeHtml(ctx)} (⌘N)" aria-label="New note">
        ${icon("file-earmark-plus")}
      </button>
      <button class="tree-action" data-new-folder title="New folder ${escapeHtml(ctx)} (⇧⌘N)" aria-label="New folder">
        ${icon("folder-plus")}
      </button>
    </span>`;
}

export function treePaneHtml(state: AppState): string {
  const total = state.notes.length + state.resources.length;
  const head = `<div class="tree-head">
      <h2>Files</h2>
      <span class="tree-head-right">
        <span class="tree-count">${total || ""}</span>
        ${state.vaultRoot === null ? "" : treeActionsHtml(state)}
      </span>
    </div>`;
  if (state.vaultRoot === null)
    return head + `<p class="tree-empty">No vault open.</p>`;
  const tree = buildTree(state.notes, state.resources, state.dirs);
  // The roving tabstop is resolved against the *visible* rows, so a folder collapsing
  // under the focused row hands the tabstop back to something reachable rather than
  // leaving the tree with no tabbable row at all.
  const roving = rovingPath(
    visibleRows(tree, state.expandedDirs),
    state.treeFocus,
    state.current?.path ?? state.currentResource?.path ?? null,
  );
  const body = treeChildrenHtml(tree, state, 0, roving);
  if (!body)
    return head + `<p class="tree-empty">No files indexed yet — Reindex to populate.</p>`;
  return (
    head +
    `<div class="tree" role="tree" aria-label="Vault files"
       title="↑↓ move · →← expand/collapse · ⏎ open · F2 rename · ⇧F10 menu">${body}</div>`
  );
}

// --- pane builders --------------------------------------------------------------

// The note-pane top bar: a full-bleed strip across the top of the note pane (above the
// centered reading column, not inside it). Its head row carries the frontmatter drawer
// toggle on the left and, grouped on the right, the `</>` view-source toggle and the
// **Edit** toggle (crates/b2-desktop/CLAUDE.md — entering edit mode hands the whole pane to
// the CodeMirror editor, so this bar isn't rendered again until edit mode exits). Sits
// as a sibling *before* `<article class="note">` so its divider spans the pane edge to
// edge, like the file tree's "Files" header.
//
// The frontmatter drawer is a collapsible peek at the note's raw YAML (verbatim, as on
// disk — `b2_relations:` and any unmodeled keys included). The `</>` toggle flips the note
// body between rendered Markdown and its raw source. Both are state-controlled (not
// native `<details>`) so their open state survives the full-pane re-render a toast timer
// or tree toggle triggers, and both stay sticky across notes. The bar is always
// rendered, so the note pane's chrome is stable; a note with no frontmatter unfolds to
// an explicit empty state.
//
// The drawer is also the frontmatter's *editing* surface (GH #79): Edit swaps the peek
// for a raw-YAML textarea with explicit Save/Cancel (no autosave — half-typed YAML is
// not a body sentence, so an autosaved keystroke is not a save).
// While `fmEditing`, the pane is under the render carve-out (main.ts), so this HTML is
// built once on entry and the buffer lives in the DOM. A block B2 can't read as YAML
// gets a non-blocking warning (`frontmatter_readable`) — the same flag an external
// hand-edit raises, since every read carries it.
function noteBarHtml(state: AppState, note: NoteView): string {
  const open = state.frontmatterOpen;
  const editing = state.fmEditing;
  const source = state.sourceOpen;
  const fm = note.frontmatter ?? "";
  const yaml = fm.replace(/\s+$/, ""); // display trim only — the edit buffer seeds verbatim
  const unreadable = note.frontmatter !== null && !note.frontmatter_readable;
  // What the `</>` chip says it will do next, plus its chord out of the *live* registry —
  // `graphToggleHtml`'s rule (⇧⌘E is rebindable, so a tooltip naming the shipped default
  // would be wrong for the user who moved it). The editor's own copy of this chip
  // (`editorSourceTitle`, main.ts) says "live preview" where this says "rendered
  // Markdown": one sticky flag, two surfaces, each naming what *it* shows when it's off.
  const sourceLabel = source ? "Show rendered Markdown" : "Show Markdown source";
  const sourceChord = escapeHtml(displayKeys(["source.toggle"]));
  const flag = unreadable
    ? ` <span class="fm-flag" role="img" aria-label="Unreadable frontmatter" title="B2 can't read this frontmatter as YAML">${icon(
        "exclamation-triangle",
        { size: 12 },
      )}</span>`
    : "";
  let body = "";
  if (open && editing) {
    // Seeded VERBATIM (not the display-trimmed `yaml`): what you edit is what's on
    // disk. Every key shows, including any B2 doesn't model — the block is the
    // human's, and B2 owns no line inside it (W3; the `b2id` guard went with the
    // stamp, GH #170).
    const rows = Math.min(16, Math.max(4, fm.split("\n").length + 1));
    // The extra "\n" right after the opening tag is sacrificial: HTML parsing strips
    // exactly one leading newline from a textarea's content, so without it a block
    // that *starts* with a blank line would seed one byte short of disk.
    body = `<div class="fm-editor">
        <textarea id="fm-editor" class="fm-input" rows="${rows}" spellcheck="false" aria-label="Frontmatter YAML">\n${escapeHtml(fm)}</textarea>
        <div id="fm-error" class="fm-error" hidden>
          <span id="fm-error-text"></span>
          <span id="fm-conflict-actions" class="conflict-actions" hidden>
            <button id="fm-reload" class="btn small" title="Discard my frontmatter edit and load the note from disk">Reload</button>
            <button id="fm-keep" class="btn small" title="Overwrite the note's frontmatter on disk with my edit">Keep mine</button>
          </span>
        </div>
        <div class="fm-actions">
          <span class="fm-hint">This block is yours — B2 changes nothing in it · ⌘⏎ saves · Esc cancels</span>
          <button id="fm-cancel" class="btn small">Cancel</button>
          <button id="fm-save" class="btn small primary">Save</button>
        </div>
      </div>`;
  } else if (open) {
    const warning = unreadable
      ? `<p class="fm-warning">B2 can't read this frontmatter as YAML — its metadata and <code>b2_relations:</code> stay unprojected until it's fixed (the bytes are kept exactly as written).</p>`
      : "";
    const peek = yaml
      ? `<pre class="frontmatter-block">${escapeHtml(yaml)}</pre>`
      : `<p class="frontmatter-empty">No frontmatter.</p>`;
    body = `${warning}${peek}
      <div class="fm-actions">
        <button id="fm-edit" class="btn small" data-fm-edit title="${
          yaml ? "Edit the raw frontmatter YAML" : "Add frontmatter to this note"
        }">${yaml ? "Edit" : "Add"}</button>
      </div>`;
  }
  // Every chip here carries a stable `id`: pressing one *is* a note-pane repaint, and the
  // `innerHTML` swap destroys the button under the keyboard — the id is the identity
  // `paintNote` puts focus back by (GH #91, crates/b2-desktop/CLAUDE.md). Each is unique
  // because the pane paints exactly one of its three modes at a time.
  return `<div class="frontmatter-bar">
      <div class="note-bar-head">
        <button id="fm-toggle" class="frontmatter-toggle" data-toggle-frontmatter aria-expanded="${open}"${
          editing ? " disabled" : ""
        }>
          ${foldCaretHtml(open)}
          <span class="frontmatter-label">Frontmatter</span>${flag}
        </button>
        <div class="note-bar-actions">
          ${graphToggleHtml(false)}
          <!-- An icon, not the angle-bracket-slash text this used to print: it sits shoulder
               to shoulder with the graph toggle, which has always been an SVG, so a text
               glyph beside it never quite lined up. An icon carries no accessible name,
               hence the aria-label the visible characters used to supply (K1, "reachable"). -->
          <button id="source-toggle" class="source-toggle${source ? " is-active" : ""}" data-toggle-source
            aria-pressed="${source}" aria-label="${sourceLabel}"
            title="${sourceLabel} — ${sourceChord}">${icon("code-slash")}</button>
          <button id="edit-toggle" class="edit-toggle" data-toggle-edit${
            state.loading || editing ? " disabled" : ""
          } title="${
            editing
              ? "Finish the frontmatter edit first"
              : "Edit this note — ⌘E (autosaves as you type)"
          }">Edit</button>
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
        <button id="resource-open" class="resource-open" data-open-system="${escapeHtml(r.path)}">
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
    return `${noteBarHtml(state, n)}
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

/**
 * The right column: search results, or the open note's discovery.
 *
 * Its rows follow the file tree's ARIA `tree` pattern (K1, GH #78) — `role="treeitem"`,
 * `aria-level`, and a **roving `tabindex`**, so the pane is one Tab stop and ↑↓→← move
 * within it. Like the tree, the DOM is flat: rows are siblings and `aria-level` is what
 * tells a screen reader that a card sits under its section head, so the `.cards`
 * wrappers are `role="none"` rather than groups. The row keys come from sidenav.ts,
 * which both this paint and the arrow keys derive from the same state — the one place
 * their order can't drift apart.
 */
export function sidePaneHtml(state: AppState): string {
  const roving = rovingSideKey(sideRows(state), state.sideFocus);
  // Chat owns the column outright when it's open (chat.ts's header says why it lives
  // here at all): one column, one thing in it, and opening chat or running a search
  // closes the other (main.ts).
  if (state.chatOpen) return chatPaneHtml(state, roving);
  return state.searchQuery
    ? searchSectionHtml(state, roving)
    : discoverySectionHtml(state, roving);
}

/** A row's slot in the roving tabstop: exactly one row is tabbable, the rest are -1. */
function sideTab(key: string, roving: string | null): string {
  return ` tabindex="${key === roving ? "0" : "-1"}"`;
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

// The install banner — the prominent, persistent counterpart to the small search caveat
// above (#26). On a fresh install with no model, the vault still gets its keyword index,
// but embedding is silently skipped (`autoIndexOnOpen` bails on `!semantic`), so semantic
// ranking and discovery are off with almost no visible sign — the reported gap. This
// surfaces that state as a dismissible strip under the top bar, pointing at Settings →
// Download. Gating is the pure, tested `shouldPromptEmbedInstall`; the controls are wired
// in main.ts:
//   • Open Settings         → opens the model picker + Download (the in-app `b2 init`)
//   • ✕                     → hide for this session (returns next launch — a gentle nag)
//   • Don't remind me again → persist the opt-out (a keyword-only user, for good)
export function embedBannerHtml(state: AppState): string {
  const show = shouldPromptEmbedInstall({
    hasVault: state.vaultRoot !== null,
    semantic: state.semantic,
    notesTotal: state.notesTotal,
    provisioning: state.provisioning,
    dismissed: state.embedReminderDismissed,
  });
  if (!show) return "";
  return `<div class="install-banner" role="status">
      <span class="install-banner-icon" aria-hidden="true">${icon("stars")}</span>
      <p class="install-banner-text">
        <strong>Semantic search is off.</strong>
        Your notes are indexed for keyword search, but the embedding model isn't installed —
        so similar-note discovery and semantic ranking are unavailable. Download it in
        Settings to turn them on.
      </p>
      <div class="install-banner-actions">
        <button class="btn small primary" data-install-open-settings>Open Settings</button>
        <label class="install-banner-optout">
          <input type="checkbox" data-install-remind-off />
          Don’t remind me again
        </label>
        <button class="install-banner-close" data-install-dismiss aria-label="Dismiss for now" title="Dismiss for now">✕</button>
      </div>
    </div>`;
}

// Search mode. `clear` is the one focusable in this pane that is *not* a row, so it carries
// a stable `id` — `paintSide` restores a row by its row key and everything else by id, and
// without one the button was dropped to `<body>` by any repaint (GH #91).
function searchSectionHtml(state: AppState, roving: string | null): string {
  const head = `<div class="side-head">
      <h2>Results</h2>
      <button id="clear-search" class="linklike" data-clear-search>clear</button>
    </div>
    <p class="side-sub">for “${escapeHtml(state.searchQuery)}”${searchCaveat(state)}</p>`;
  if (state.loading) return head + `<p class="side-empty">Searching…</p>`;
  if (state.searchResults.length === 0) return head + searchEmptyHtml(state);
  // A result card is the whole button, so it *is* the row — no fold, one level.
  const items = state.searchResults
    .map((r, i) => {
      const key = cardRowKey("search", i, r.path);
      return `<button class="card" role="treeitem" aria-level="1"${sideTab(
        key,
        roving,
      )} data-side-row="${escapeHtml(key)}" data-open="${escapeHtml(r.path)}">
        <div class="card-title">${escapeHtml(r.title ?? r.path)}</div>
        <div class="card-path">${escapeHtml(r.path)} · ${r.score.toFixed(3)}</div>
        ${r.snippet ? `<div class="card-snip">${escapeHtml(r.snippet)}</div>` : ""}
      </button>`;
    })
    .join("");
  return (
    head + `<div class="cards" role="tree" aria-label="Search results">${items}</div>`
  );
}

// The search pane's two empty states — the same blank list, two different reasons,
// and saying so is the whole of D2's honesty on this surface (GH #202).
//
//   • `searchVouched === false` — the vault holds no evidence for this query: no
//     lexical anchor, and nothing near enough by meaning at the active model's
//     calibrated bar. The engine had rows and `doSearch` dropped them, strictly, so
//     there is nothing to reveal and nothing to count. The copy says "no matches"
//     because that is a claim about the *query*, which is the claim we can support.
//   • anything else — the list is simply empty (an unbuilt index, a `null` verdict
//     with nothing retrieved). That is not a judgment about the query, so the copy
//     doesn't make one.
function searchEmptyHtml(state: AppState): string {
  return state.searchVouched === false
    ? `<p class="side-empty">No matches. Nothing in this vault matches “${escapeHtml(
        state.searchQuery,
      )}”.</p>`
    : `<p class="side-empty">No matches.</p>`;
}

function discoverySectionHtml(state: AppState, roving: string | null): string {
  if (!state.current) {
    return `<div class="side-head"><h2>Discovery</h2></div>
      <p class="side-empty">Open a note to see similar notes and its connections.</p>`;
  }
  return `<div class="side-nav" role="tree" aria-label="Discovery">${connectionsSectionHtml(
    state,
    roving,
  )}${similarSectionHtml(state, roving)}</div>`;
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
  roving: string | null,
): string {
  const key = sectionRowKey(section);
  return `<button class="side-head side-fold" role="treeitem" aria-level="1"${sideTab(
    key,
    roving,
  )} data-side-row="${escapeHtml(
    key,
  )}" data-fold-section="${section}" aria-expanded="${!collapsed}">
      ${foldCaretHtml(!collapsed)}
      <span class="side-title">${label}</span>
      ${count ? `<span class="side-count">${count}</span>` : ""}
    </button>`;
}

// The card's own fold chevron. Cards default expanded (the snippet is the signal you
// link on); this collapses the body (path + snippet) to just the title row. Kept out of
// the `.card-open` button so a click on the chevron folds without opening the note
// (nested buttons aren't allowed — the chevron and the open-region are siblings).
//
// A **mouse** affordance only, hence `tabindex="-1"` + `aria-hidden`: the row it sits on
// is the treeitem, it carries the `aria-expanded` a screen reader reads, and →← is the
// keyboard's fold (K1) — exactly as the file tree's caret is a span, not a tab stop.
function cardFold(key: string, collapsed: boolean): string {
  return `<button class="card-fold" tabindex="-1" aria-hidden="true" data-fold-card="${escapeHtml(
    key,
  )}">
      ${foldCaretHtml(!collapsed)}
    </button>`;
}

// The discovery card's strength cell: a banded read of the candidate's z
// (`strength.ts`), replacing the raw negated-L2 the card used to print (GH #150).
// No z → no cell: a statistic that wasn't computed isn't claimed (raw mode, tiny
// pools). The glyph is decorative; the band name is the accessible content.
function strengthHtml(z: number | undefined): string {
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

/** A caveat *about the list you are looking at* — not an empty state and not an error.
 *  The install banner already settled the tone for this class of message: accent tones,
 *  never `--danger`, because "B2 didn't grade these" is a nudge about what the numbers
 *  mean, not a fault to fix. Muted body prose (`.side-empty`) was the opposite failure —
 *  it read as chrome and got skimmed past, leaving the bare cards to imply a judgement.
 *  One caller since GH #197 retired raw mode's banner: the ungraded caveat. */
function sideNoteHtml(text: string, title: string): string {
  return `<p class="side-note" title="${escapeHtml(title)}"><span class="side-note-icon" aria-hidden="true">${icon(
    "info-circle",
    { size: 14 },
  )}</span><span>${escapeHtml(text)}</span></p>`;
}

/** The ungraded caveat: candidates exist, but none carries a z, so no band is shown on
 *  any of them. Left silent that reads as "everything here scored low" rather than the
 *  truth — that no statistic was computed. It states the *rule* rather than diagnosing
 *  this vault, because two different conditions land here: a candidate pool below
 *  `STATS_MIN_POPULATION` (the starter-vault posture) and a population with no spread
 *  at all. "Not enough" without a number leaves the reader with nothing to do, so the
 *  bar is named — [`STRENGTH_MIN_CANDIDATES`], which mirrors that Rust constant. */
function ungradedHtml(state: AppState): string {
  if (state.similar.length === 0) return "";
  if (state.similar.some((c) => strengthBand(c.z))) return "";
  return sideNoteHtml(
    `Ungraded — ranked by nearness, not strength. Grading needs ${STRENGTH_MIN_CANDIDATES} or more notes in the vault to compare against.`,
    `A strength band says how far a candidate stands above this note's other candidates. Under ${STRENGTH_MIN_CANDIDATES} of them — or with no spread between them — there is no distribution to measure against, so B2 claims no strength rather than guessing one.`,
  );
}

function similarSectionHtml(state: AppState, roving: string | null): string {
  const collapsed = state.collapsedSections.has("similar");
  const head = sideFoldHead(
    "similar",
    "Similar &amp; unlinked",
    state.similar.length,
    collapsed,
    roving,
  );
  if (collapsed) return head;
  if (state.similar.length === 0) {
    if (state.discoveringSimilar)
      return (
        head +
        `<div class="side-empty" role="status" aria-label="Finding similar notes"><span class="spinner"></span></div>`
      );
    if (!state.semantic)
      return (
        head +
        `<p class="side-empty">Semantic similarity is off — run <code>b2 init</code> then Reindex.</p>`
      );
    // The honest empty state (GH #197): the ranked list is always served, so an
    // empty pane can only mean the candidate set is genuinely empty — never a
    // verdict that nothing relates.
    return (
      head +
      `<p class="side-empty">Nothing unlinked has stored vectors to compare — Reindex may still be filling them.</p>`
    );
  }
  const items = state.similar
    .map((c, i) => {
      const key = cardKey("similar", c.path);
      const rowKey = cardRowKey("similar", i, c.path);
      const folded = state.collapsedCards.has(key);
      const body = folded
        ? ""
        : `<div class="card-body">
            <div class="card-path">${escapeHtml(c.path)}</div>
            ${c.evidence ? `<div class="card-snip">${escapeHtml(c.evidence)}</div>` : ""}
          </div>`;
      // `data-card-path`/`-title` on the root feed the right-click menu (Open / Link…);
      // the whole card is the target now that the inline Link button is gone. The card is
      // also the keyboard's *row* (`data-side-row`), which is why the whole box — title,
      // path, and snippet — is what a screen reader reads and what the ring wraps.
      //
      // Draggable **only while the note is being edited** (droplink.ts): the drop lands a
      // `[[wikilink]]` in the buffer, so with no buffer open there is nowhere for the drag
      // to go, and an affordance that starts a drag nothing can accept is a lie the OS
      // cursor has to walk back. The `title` says what the gesture does, since a drag is
      // the one affordance with no visible label; the keyboard's half of it is the card
      // menu's *Insert link at cursor* (⇧F10 — K1), below.
      return `<div class="card foldable candidate${
        folded ? " is-collapsed" : ""
      }"${
        state.editing
          ? ` draggable="true" title="Drag onto a line of the note to link it there"`
          : ""
      } role="treeitem" aria-level="2" aria-expanded="${!folded}"${sideTab(
        rowKey,
        roving,
      )} data-side-row="${escapeHtml(rowKey)}" data-card-path="${escapeHtml(
        c.path,
      )}" data-card-title="${escapeHtml(c.title ?? "")}">
          <div class="card-head">
            ${cardFold(key, folded)}
            <button class="card-open" tabindex="-1" data-open="${escapeHtml(c.path)}">
              <span class="card-title">${escapeHtml(c.title ?? c.path)}</span>
              ${strengthHtml(c.z)}
            </button>
          </div>
          ${body}
        </div>`;
    })
    .join("");
  return head + ungradedHtml(state) + `<div class="cards" role="none">${items}</div>`;
}

function connectionsSectionHtml(state: AppState, roving: string | null): string {
  const count = state.connections.length + state.unresolved.length;
  const collapsed = state.collapsedSections.has("connections");
  const head = sideFoldHead("connections", "Connections", count, collapsed, roving);
  if (collapsed) return head;
  if (count === 0)
    return (
      head +
      `<p class="side-empty">${
        state.discoveringConnections ? "Loading connections…" : "No connections yet."
      }</p>`
    );
  const items = state.connections
    .map((c, i) => {
      const arrow = icon(directionIcon(c.direction), { size: 12 });
      const key = cardKey("connections", c.path);
      const rowKey = cardRowKey("connections", i, c.path);
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
      return `<div class="card edge foldable${
        folded ? " is-collapsed" : ""
      }" role="treeitem" aria-level="2" aria-expanded="${!folded}"${sideTab(
        rowKey,
        roving,
      )} data-side-row="${escapeHtml(rowKey)}">
          <div class="card-head">
            ${cardFold(key, folded)}
            <button class="card-open" tabindex="-1" data-open="${escapeHtml(c.path)}">
              <span class="card-title"><span class="edge-arrow">${arrow}</span> ${escapeHtml(
                c.label,
              )} <span class="edge-origin">${escapeHtml(c.origin)}</span></span>
            </button>
          </div>
          ${body}
        </div>`;
    })
    .join("");
  return (
    head + `<div class="cards" role="none">${items}${unresolvedCardsHtml(state, roving)}</div>`
  );
}

// Dangling outbound links — a `[[folder]]` or a typo that resolves to no note or
// file (GH #12). Not clickable (nothing to open), so a plain `div`, flagged with a
// broken-link emblem so it reads as broken rather than silently missing. The target
// is shown as written (`[[Hermes]]`), which is what the user can fix in the note.
//
// A row all the same: nothing to open and nothing to fold, but a row you can *see* is a
// row you must be able to reach (K1), so ↑↓ walk through these too rather than jumping
// the last few cards in the section. ⏎ on one does nothing, which is the honest answer.
function unresolvedCardsHtml(state: AppState, roving: string | null): string {
  return state.unresolved
    .map((u, i) => {
      const key = cardRowKey("unresolved", i, u.target);
      const why = u.explanation
        ? `<div class="card-snip">${escapeHtml(u.explanation)}</div>`
        : "";
      return `<div class="card edge broken" role="treeitem" aria-level="2"${sideTab(
        key,
        roving,
      )} data-side-row="${escapeHtml(
        key,
      )}" title="This link points to nothing — no note or file named “${escapeHtml(
        u.target,
      )}”. A note is a single .md file, so a folder can’t be linked.">
          <div class="card-title"><span class="edge-broken" role="img" aria-label="Broken link">${icon(
            "exclamation-triangle",
            { size: 12 },
          )}</span> ${escapeHtml(
            u.relation,
          )} <span class="edge-origin">${escapeHtml(u.origin)}</span></div>
          <div class="card-path">[[${escapeHtml(u.target)}]] · unresolved</div>
          ${why}
        </div>`;
    })
    .join("");
}

// --- chat (flow ④, GH #151/#153/#155) -----------------------------------------------
//
// The right column's third mode: ask a question, watch the answer stream, click a
// citation to open the note behind it — *without* the conversation leaving the screen,
// which is why chat lives here rather than in the centre pane (chat.ts's header).
//
// Three rules this builder holds, all of them invariants rather than styling:
//
//   • **E5 — model output is untrusted content.** An answer is a string from a model that
//     was itself fed note content (which anyone can author), so it renders through the one
//     sanitizing `renderMarkdown` seam like every other document. The *streaming* half is
//     stronger still: it is written as `textContent` by `paintChatStream` (main.ts), so a
//     half-arrived answer is never parsed as markup at all.
//   • **A citation navigates in-app, never the webview.** Each one is a `data-open` button
//     — the same delegation search results and discovery cards use — so the mouse and ⏎
//     share one activation path (K1), and no `href` ever exists to be followed.
//   • **K1 — the pane is keyboard-complete.** Rows are `role="treeitem"` over chat.ts's
//     row order (the same order the arrows walk), with a roving tabstop; every control has
//     a stable `id` so `paintSide` can hand focus back across the repaint each token causes.
function chatPaneHtml(state: AppState, roving: string | null): string {
  const setup = state.chatSetup;
  const streaming = state.chatStreaming !== null;
  const model = setup
    ? `<span class="chat-model" title="${escapeHtml(
        `${setup.model} · ${setup.base_url}`,
      )}">${escapeHtml(setup.model)}</span>`
    : "";
  const head = `<div class="side-head chat-head">
      <h2>Chat</h2>
      ${model}
      <button id="chat-new" class="linklike"${
        state.chatMessages.length === 0 || streaming ? " disabled" : ""
      } data-chat-new title="Start a new conversation — nothing here is saved">new</button>
    </div>`;
  // No composer until there is something to answer with: a disabled field under a card
  // that says why is chrome with nothing behind it, and one more stop for a keyboard user
  // to walk past on the way to the fix the card is pointing at.
  const ready =
    chatEmptyState({ hasVault: state.vaultRoot !== null, setup: state.chatSetup }) === "ready";
  return head + chatStageHtml(state, roving) + (ready ? chatComposerHtml(state) : "");
}

/** The conversation, or the state that stands in for one (chat.ts's `chatEmptyState`
 *  makes that choice once, so the paint doesn't re-derive it branch by branch). */
function chatStageHtml(state: AppState, roving: string | null): string {
  switch (chatEmptyState({ hasVault: state.vaultRoot !== null, setup: state.chatSetup })) {
    case "no-vault":
      return `<p class="side-empty">Open a vault to chat with your notes.</p>`;
    case "loading":
      return `<p class="side-empty">Looking for a model…</p>`;
    case "no-server":
    case "no-model":
      return chatSetupCardHtml(state.chatSetup, false);
    case "ready":
      return chatLogHtml(state, roving);
  }
}

/** The transcript. Empty until the first question — and then it is what the pane is. */
function chatLogHtml(state: AppState, roving: string | null): string {
  if (state.chatMessages.length === 0 && state.chatStreaming === null) {
    const fake = state.chatSetup?.state === "fake";
    return `<div class="chat-log" id="chat-log">
        <div class="chat-intro">
          <p><strong>Ask your notes a question.</strong></p>
          <p class="muted">Answers come only from passages B2 retrieves from this vault, cited by
            [n]. Nothing here is written to your notes, and the conversation isn’t saved.</p>
          ${
            fake
              ? `<p class="muted">The fake chat provider is in use (<code>B2_LLM=fake</code>) — answers are deterministic test scaffolding, not a model.</p>`
              : ""
          }
        </div>
      </div>`;
  }
  const turns = state.chatMessages
    .map((m, i) => chatTurnHtml(m, i, roving))
    .join("");
  // The in-flight answer is a row of its own so the keyboard can sit on it while it
  // fills. `#chat-stream` is the element `paintChatStream` writes tokens into — as text,
  // never markup — which is what keeps a streaming answer off the full-render path.
  const live =
    state.chatStreaming === null
      ? ""
      : `<div class="chat-turn chat-answer chat-live" role="treeitem" aria-level="1"${sideTab(
          STREAMING_ROW_KEY,
          roving,
        )} data-side-row="${escapeHtml(STREAMING_ROW_KEY)}" aria-live="polite">
          <div class="chat-role">B2</div>
          <div class="chat-text" id="chat-stream">${escapeHtml(state.chatStreaming)}</div>
        </div>`;
  return `<div class="chat-log" id="chat-log" role="tree" aria-label="Conversation">${turns}${live}</div>`;
}

/** One turn: the question as typed, or the answer — rendered through the sanitizing
 *  Markdown seam (E5) — with its citations as rows beneath it. */
function chatTurnHtml(m: ChatMessage, index: number, roving: string | null): string {
  const key = turnRowKey(index);
  const row = (cls: string, role: string, body: string): string =>
    `<div class="chat-turn ${cls}" role="treeitem" aria-level="1"${sideTab(
      key,
      roving,
    )} data-side-row="${escapeHtml(key)}">
      <div class="chat-role">${role}</div>
      ${body}
    </div>`;
  if (m.role === "user") {
    return row("chat-question", "You", `<div class="chat-text">${escapeHtml(m.text)}</div>`);
  }
  if (m.error !== undefined) {
    return row(
      "chat-answer chat-failed",
      "B2",
      `<div class="chat-error">${escapeHtml(m.error)}</div>`,
    );
  }
  const stopped = m.cancelled
    ? `<p class="chat-stopped">Stopped — this answer is partial.</p>`
    : "";
  const cites = m.citations
    .map(
      (c) => `<button class="chat-cite" role="treeitem" aria-level="2"${sideTab(
        citationRowKey(index, c.marker, c.path),
        roving,
      )} data-side-row="${escapeHtml(
        citationRowKey(index, c.marker, c.path),
      )}" data-open="${escapeHtml(c.path)}" title="Open ${escapeHtml(c.path)}">
        <span class="chat-cite-marker">[${c.marker}]</span>
        <span class="chat-cite-path">${escapeHtml(c.path)}</span>
        ${c.excerpt ? `<span class="chat-cite-excerpt">${escapeHtml(c.excerpt)}</span>` : ""}
      </button>`,
    )
    .join("");
  return (
    row(
      "chat-answer",
      "B2",
      `<div class="chat-text">${renderMarkdown(m.text)}</div>${stopped}`,
    ) + cites
  );
}

/**
 * The composer. A `<textarea>` rather than an input because a question can be a
 * paragraph: ⏎ asks, ⇧⏎ is a newline (the platform's own reflex in a multi-line field,
 * which is why the registry marks the chord `fixed`).
 *
 * While an answer streams, Ask becomes **Stop** — the mouse's equal of Esc (K1: no action
 * reachable only by keyboard either). The field itself stays **enabled** throughout: you
 * can line up the next question while this answer arrives, and — the reason it matters —
 * disabling a focused control drops the keyboard to `<body>`, so the one gesture that
 * always precedes a stream would be the one that ejects you from the pane. `sendChat`
 * refuses a second turn instead, where it costs nobody their focus.
 */
function chatComposerHtml(state: AppState): string {
  const streaming = state.chatStreaming !== null;
  const note = retrievalNote(state);
  return `<form class="chat-composer" id="chat-composer">
      <textarea id="chat-input" class="chat-input" rows="2" placeholder="Ask your notes…"
        aria-label="Ask your notes"></textarea>
      <div class="chat-actions">
        ${
          streaming
            ? `<button type="button" class="btn small" id="chat-stop" data-chat-stop title="Stop this answer (${escapeHtml(
                displayKeys(["dismiss"]),
              )})">Stop</button>`
            : `<button type="submit" class="btn small primary" id="chat-send">Ask</button>`
        }
        ${note ? `<span class="chat-note muted">${escapeHtml(note)}</span>` : ""}
      </div>
    </form>`;
}

/**
 * The setup card — deliberately **Ollama-native** (GH #151: guided setup is a per-runtime
 * feature, and Ollama is the runtime B2 guides), shown both as the chat pane's empty state
 * and inside Settings → Chat.
 *
 * Three cards, one builder, because they are the same facts in a different order: no
 * server (start the daemon), no model (pull one — sized to this machine, illustrative and
 * non-binding), and the Settings copy of both. A non-Ollama endpoint gets the message and
 * nothing else: there is no honest instruction to give about pulling a model into LM
 * Studio or a cloud provider.
 */
function chatSetupCardHtml(setup: ChatSetup | null, inSettings: boolean): string {
  if (!setup) return "";
  const ollama = setup.ollama;
  const message = setup.message
    ? `<p class="chat-setup-message">${escapeHtml(setup.message)}</p>`
    : "";
  // What's installed, when the daemon answered — so "no model" can offer what *is* there
  // instead of only naming what isn't.
  //
  // The pane's card only. In Settings the same inventory *is* the Model field (a picker,
  // `chatModelFieldHtml`), and two controls setting one value is two things to keep in
  // step — the second of which would apply on click while the first waits for Save.
  const installed =
    !inSettings && ollama && ollama.running && ollama.installed.length > 0
      ? `<div class="chat-setup-block">
          <div class="settings-subhead">Installed models</div>
          <ul class="chat-models">${ollama.installed
            .map(
              (m) =>
                `<li><button type="button" class="linklike" data-chat-use-model="${escapeHtml(
                  m.name,
                )}">${escapeHtml(m.name)}</button> <span class="muted">${escapeHtml(
                  [m.parameters ?? "", formatModelSize(m.size)].filter(Boolean).join(" · "),
                )}</span></li>`,
            )
            .join("")}</ul>
        </div>`
      : "";
  // Pane-only for the same reason as the inventory: in Settings the **Local** section's
  // own copy already carries `ollama pull` sized to this machine (`localNoteHtml`), and
  // one command printed twice on one screen reads as two different instructions.
  const suggestion =
    !inSettings && ollama && ollama.suggested
      ? `<div class="chat-setup-block">
          <div class="settings-subhead">Suggested for this machine</div>
          <p class="settings-detail muted">${
            ollama.ram_gb ? `${ollama.ram_gb} GB of memory` : "This machine"
          } — a ${escapeHtml(ollama.suggested.size)} model. Illustrative, not a requirement.</p>
          <p class="chat-command"><code>${escapeHtml(
            pullCommand(ollama.suggested.model),
          )}</code></p>
        </div>`
      : "";
  const tiers =
    ollama && ollama.tiers.length > 0
      ? `<details class="chat-tiers"><summary>Sizes by memory</summary>
          <ul>${ollama.tiers
            .map(
              (t) =>
                `<li>${escapeHtml(t.ram)} → ${escapeHtml(t.size)} <code>${escapeHtml(
                  t.model,
                )}</code></li>`,
            )
            .join("")}</ul>
        </details>`
      : "";
  const install =
    ollama && !ollama.running
      ? `<p class="settings-note">B2 talks to any OpenAI-compatible model server; Ollama is the
          one it can walk you through. Start it with <code>ollama serve</code> — or, if it
          isn’t installed yet, the
          <a href="${OLLAMA_QUICKSTART_URL}">Ollama quickstart</a> is the install and the
          first pull, in that order.</p>`
      : "";
  // A retry, in the pane only: the card's whole job is to be looked at while the user
  // goes and fixes something (starts the daemon, pulls a model), so it must be able to
  // notice that they did — without making them close and reopen the pane. Settings has
  // its own re-probe, spelled *Save and test*.
  const recheck = inSettings
    ? ""
    : `<div class="settings-action"><button class="btn small" id="chat-recheck" data-chat-recheck>Check again</button></div>`;
  return `<div class="chat-setup${inSettings ? " chat-setup-inline" : ""}">
      ${inSettings ? "" : `<div class="chat-setup-head">Chat isn’t ready yet</div>`}
      ${message}
      ${recheck}
      ${install}
      ${suggestion}
      ${installed}
      ${tiers}
      ${
        inSettings
          ? ""
          : `<p class="settings-note">Everything else in B2 — search, discovery, editing —
              works exactly as it does with chat off.</p>`
      }
    </div>`;
}

// --- the anchored ghost graph (GH #22) ----------------------------------------------
//
// The center pane's third mode: the open note's typed neighborhood as hand-rolled,
// deterministic SVG — scene geometry from `graph.ts` (pure, unit-tested), markup
// here, clicks delegated in main.ts. The reading key: color = edge category, solid =
// authored / dashed teal = latent (`similar`), disc = note / square = resource /
// dashed hollow = dangling. Everything renders from state the note-open already
// fetched, so entering the graph costs no IPC.

/** The graph toggle chip, shared by the reading bar (off) and the graph bar (on).
 *
 *  Its chord comes out of the live registry rather than being spelled here: ⌘G is
 *  rebindable (#121), and a tooltip naming the shipped default would be wrong for exactly
 *  the user who changed it. */
function graphToggleHtml(active: boolean): string {
  const chord = escapeHtml(displayKeys(["graph.toggle"]));
  return `<button id="graph-toggle" class="source-toggle graph-toggle${active ? " is-active" : ""}" data-toggle-graph
      aria-pressed="${active}" aria-label="${active ? "Back to reading" : "Show the connection graph"}"
      title="${
        active
          ? `Back to reading — ${chord} or Esc`
          : `Show the connection graph — ${chord}; nodes are Tab-reachable, ⏎ opens`
      }">${icon("diagram-3")}</button>`;
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
        ${sceneIcon(resourceIcon(n.sub), n.x, n.y, 16, "gglyph")}`;
    }
    case "dangling":
      return `<circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R.dangling}"/>
        ${sceneIcon("exclamation-triangle", n.x, n.y, 16, "gglyph")}`;
    default:
      return `<circle class="gshape" cx="${x}" cy="${y}" r="${NODE_R[n.kind]}"/>`;
  }
}

/** The tooltip line(s) for a node — also the activation affordance's explanation.
 *  Phrased for both hands: an activatable node says "⏎", because the graph is
 *  reachable by Tab and arrow keys too (K1, GH #78), not by mouse alone. */
function nodeTitle(n: GraphNode): string {
  switch (n.kind) {
    case "anchor":
      return `${n.full} — the open note. Click or ⏎ to return to reading.`;
    case "ghost":
      // The strength figure when there is one; a bare "similar but not linked"
      // otherwise. "similarity ?" claimed a measurement existed and was merely
      // unavailable — an ungraded candidate was never measured at all.
      return `${n.full} — similar but not linked${
        n.sub ? ` (${n.sub} above this note's other candidates)` : ""
      }. Click or ⏎ to link it; right-click (or ⇧F10) for more.`;
    case "dangling":
      return `${n.full} resolves to no note or file — fix the link in the note.`;
    case "resource":
      return `${n.full} (${n.sub ?? "file"}) — click or ⏎ to open.`;
    default:
      return `${n.full} — click or ⏎ to open.`;
  }
}

/** The accessible name for a focusable node — what a screen reader announces, and
 *  what the node's own `<title>` can't be (that one is the mouse tooltip prose). */
function nodeAriaLabel(n: GraphNode): string {
  switch (n.kind) {
    case "anchor":
      return `${n.full} — the open note; back to reading`;
    case "ghost":
      return `${n.full} — similar but unlinked; link it`;
    case "resource":
      return `${n.full} — open this file`;
    default:
      return `${n.full} — open this note`;
  }
}

/**
 * One scene node as an interactive `<g>`, its incident edges inside it so a pure-CSS
 * hover lights the node *and* its edges while the rest of the scene dims. The click
 * affordance rides existing delegation: notes reuse `data-open`, resources
 * `data-open-resource`; ghosts get `data-ghost-link` (→ the link palette) plus the
 * `data-card-*` pair the right-click menu reads; the anchor toggles back to reading.
 *
 * An activatable node is also a **keyboard** control (K1, GH #78): `tabindex="0"` puts
 * it in the Tab order and `role="button"` says what it is, so a graph is walkable and
 * openable with no mouse. main.ts turns ⏎/Space on a focused node into the same click
 * these attributes already answer — one activation path, not two. A `dangling` node
 * stays inert: it opens nothing (there's nothing there), so it isn't a tab stop.
 *
 * It also carries `data-gnode` — the scene id (graph.ts), which is what the note pane's
 * focus restoration re-finds it *by* after an `innerHTML` swap destroys the element
 * itself (GH #91). The discovery row's `data-side-row` for the graph: an identity that
 * outlives the repaint, not a pointer into it.
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
  if (n.kind !== "dangling") {
    attrs.push(
      `tabindex="0"`,
      `role="button"`,
      `aria-label="${escapeHtml(nodeAriaLabel(n))}"`,
      `data-gnode="${escapeHtml(n.id)}"`,
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

/** The honest ghost-halo caveat (mirrors `searchCaveat`'s tiers, #26): why there are
 *  no ghosts right now, or null when there are (or when silence is the honest state). */
function ghostHintHtml(state: AppState): string {
  if (state.similar.length > 0) return "";
  if (state.discoveringSimilar)
    return `<div class="graph-hint is-scanning"><span class="spinner"></span>scanning for latent connections…</div>`;
  if (!state.semantic)
    return `<div class="graph-hint">ghost connections need the semantic model — run <code>b2 init</code>, then Reindex</div>`;
  if (state.notesTotal > 0 && state.notesEmbedded < state.notesTotal)
    return `<div class="graph-hint">ghosts appear once the vault is embedded — Reindex</div>`;
  return "";
}

/** The centered guidance when there's nothing to draw (the anchor always shows). */
function graphEmptyHtml(state: AppState, scene: GraphScene): string {
  if (scene.edges.length > 0) return "";
  if (state.discoveringSimilar) return "";
  return `<div class="graph-empty"><p>No connections yet.</p>
    <p class="muted">B2 floats similar-but-unlinked notes here as ghosts — click one to make the connection real.</p></div>`;
}

/** The reading key, one quiet strip: verb colors, edge states, node shapes. */
function graphLegendHtml(): string {
  const cats: [Category, string][] = [
    ["references", "references"],
    ["supports", "supports"],
    ["contradicts", "contradicts"],
  ];
  const dots = cats
    .map(([c, label]) => `<span class="leg"><span class="leg-dot cat-${c}"></span>${label}</span>`)
    .join("");
  return `<div class="graph-legend" aria-hidden="true">${dots}
      <span class="leg"><span class="leg-dash"></span>ghost (unlinked)</span>
      <span class="leg"><span class="leg-square"></span>file</span>
      <span class="leg"><span class="leg-broken">${icon("exclamation-triangle", {
        size: 11,
      })}</span>broken</span>
    </div>`;
}

/** Arrowhead markers, one per category (an SVG marker can't inherit its edge's
 *  stroke everywhere yet). */
function graphDefsHtml(): string {
  const cats: Category[] = ["references", "supports", "contradicts", "other"];
  const arrow = (id: string, cls: string) =>
    `<marker id="${id}" viewBox="0 0 10 10" refX="8.5" refY="5" markerWidth="7.5" markerHeight="7.5" orient="auto-start-reverse">
       <path d="M0 0.8 L9.5 5 L0 9.2 z" class="${cls}"/>
     </marker>`;
  return `<defs>${cats.map((c) => arrow(`garr-${c}`, `garr cat-${c}`)).join("")}</defs>`;
}

/**
 * The graph pane — the note pane's third mode (Reading / Editing / Graph). Bar:
 * the same action chips as reading; stage: the SVG scene (fills the pane,
 * `viewBox`-scaled) with overlay hints; footer: the reading key.
 */
function graphPaneHtml(state: AppState, n: NoteView): string {
  const scene = buildScene({
    anchor: { path: n.path, title: n.title },
    connections: state.connections,
    resources: state.resourceLinks,
    unresolved: state.unresolved,
    ghosts: state.similar,
  });

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
        <div class="note-bar-actions">
          ${graphToggleHtml(true)}
          <button id="edit-toggle" class="edit-toggle" data-toggle-edit${
            state.loading ? " disabled" : ""
          } title="Edit this note — ⌘E (autosaves as you type)">Edit</button>
        </div>
      </div>
      <div class="graph-stage">
        <svg class="graph-svg" viewBox="0 0 ${VIEW_W} ${VIEW_H}" preserveAspectRatio="xMidYMid meet"
             role="img" aria-label="Connection graph for ${escapeHtml(n.title ?? n.path)}">
          ${graphDefsHtml()}
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

// --- Settings (⌘,) --------------------------------------------------------------
//
// A tabbed surface over a rail (settingstabs.ts) — General, Index, Embedding, Chat,
// Keyboard — rather than the one scrolling column it grew out of, and since it outgrew a
// floating box too it takes the whole window (`settingsScreenHtml` below). It keeps the
// link modal's `.field` chrome, so a section is written as a form and nothing about the
// surface it lands on is a section's business.
//
// **Every control in here carries a stable `id`**, and that is load-bearing, not tidy:
// `#modal-root` is swapped wholesale on a repaint, so main.ts's `captureModalFocus` can
// only put the keyboard back on what it was on by re-finding it by id after the swap
// (crates/b2-desktop/CLAUDE.md, "Two things that bite"). A settings control with no id
// is a control that ejects the keyboard to `<body>` the moment it's used.

/** The panel for one section. Split per tab rather than one long builder so a new
 *  section is a `case` plus a builder, and the others can't shift under it. */
function settingsPanelHtml(state: AppState): string {
  switch (state.settingsTab) {
    case "general":
      return generalPanelHtml(state);
    case "index":
      return indexPanelHtml(state);
    case "embedding":
      return embeddingPanelHtml(state);
    case "chat":
      return chatPanelHtml(state);
    case "keyboard":
      return keyboardPanelHtml(state);
  }
}

// Chat — which model answers your questions, and where it runs. The spec's two named
// configurations (GH #151), and they are one setting rather than two: **Local** is a
// localhost endpoint (Ollama's, unless pointed elsewhere) and **Cloud models** is a
// provider's, so the segmented control below is a *view* of the URL, not a second piece
// of state to keep in step with it.
//
// The privacy copy sits beside the Cloud fields deliberately: **the consent moment is the
// configuration moment** (invariant M5 — note content never leaves the machine unbidden),
// informed where the decision is made rather than by a popup later.
//
// None of this is vault or index state. Changing the chat model costs no reindex — the
// contrast with M2 that makes "change models at any time" true by construction.
function chatPanelHtml(state: AppState): string {
  const setup = state.chatSetup;
  const cloud = state.chatCloud;
  const modes: { id: "local" | "cloud"; label: string }[] = [
    { id: "local", label: "Local" },
    { id: "cloud", label: "Cloud models" },
  ];
  const segments = modes
    .map((m) => {
      const on = cloud === (m.id === "cloud");
      return `<button type="button" class="seg${on ? " seg-on" : ""}" id="settings-chat-${
        m.id
      }" data-chat-mode="${m.id}" aria-pressed="${on}">${m.label}</button>`;
    })
    .join("");
  // The status line, in the setup card's own words when there's a problem — the same
  // sentence the chat pane shows, from the same probe.
  const status = ((): string => {
    if (!setup) return `<p class="settings-detail muted">Checking the model server…</p>`;
    if (setup.state === "ready")
      return `<p class="settings-detail">Connected · ${escapeHtml(setup.model)}</p>`;
    if (setup.state === "fake")
      return `<p class="settings-detail">${escapeHtml(setup.message ?? "")}</p>`;
    return chatSetupCardHtml(setup, true);
  })();
  const key = cloud ? cloudKeyHtml(setup) : localNoteHtml(setup);
  return `<div class="settings-subhead">Chat model</div>
      <p class="settings-detail muted">Grounded chat answers only from passages B2 retrieves
        from this vault. Changing the model costs no reindex — nothing about chat is stored.</p>
      <div class="field">
        <span class="field-label">Where it runs</span>
        <div class="segmented" role="group" aria-label="Chat model location">${segments}</div>
      </div>
      <label class="field">Endpoint
        <input id="settings-chat-url" type="text" autocomplete="off" spellcheck="false"
          value="${escapeHtml(setup?.base_url ?? "")}" placeholder="http://localhost:11434/v1" />
      </label>
      ${chatModelFieldHtml(state, setup)}
      ${key}
      <div class="settings-action">
        <button class="btn small primary" id="settings-chat-save">Save and test</button>
      </div>
      ${status}`;
}

/**
 * The **Model** field — a picker over what the daemon actually has, or a text box.
 *
 * Two shapes for one value, chosen by whether there is an inventory to pick from. A
 * typed model name is the commonest local-setup mistake there is (the daemon is up, the
 * name is just not one it has), and the fix was previously to read it off a card *after*
 * getting it wrong. When `/api/tags` answered, the list of installed models is simply
 * what the field offers.
 *
 * The text box stays reachable on purpose, and is not a fallback: a list of *installed*
 * models structurally cannot contain the one you are pulling right now, and naming it
 * before the pull finishes is a real thing to do. So the picker carries a way out
 * (`chatModelTyped`), and the way back is beside the box.
 *
 * Both shapes carry the **same id**, because the id is the contract: `saveChatConfig`
 * reads `.value` off it (a `<select>` and an `<input>` agree on that), and
 * `captureModalFocus` puts the keyboard back on it by id after the repaint.
 */
function chatModelFieldHtml(state: AppState, setup: ChatSetup | null): string {
  const ollama = setup?.ollama;
  // A **Local** control by definition: `/api/tags` is one daemon's inventory, which
  // says nothing about what a cloud provider serves. `chatCloud` and not `setup.cloud`
  // because the view flag turns over the instant *Cloud models* is pressed, while the
  // setup is whatever the last probe found — and nothing re-probes on that press (the
  // URL field is deliberately cleared, there being no default provider). Reading the
  // stale answer would leave a picker of this machine's models under an empty cloud
  // endpoint until Save and test, with *Type a model name* standing between the user
  // and the field they came to fill in.
  const installed = !state.chatCloud && ollama?.running ? ollama.installed : [];
  const current = setup?.model ?? "";
  if (state.chatModelTyped || installed.length === 0) {
    // The way back, offered only when there is something to go back *to*.
    const pick =
      installed.length > 0
        ? `<div class="settings-action"><button type="button" class="btn small"
             id="settings-chat-model-pick" data-chat-model-pick>Choose an installed model</button></div>`
        : "";
    return `<label class="field">Model
        <input id="settings-chat-model" type="text" autocomplete="off" spellcheck="false"
          value="${escapeHtml(current)}" placeholder="llama3.2" />
      </label>
      ${pick}`;
  }
  // The configured model leads the list when the daemon doesn't have it — dropping it
  // would silently re-point the configuration at whatever happened to be first, as a side
  // effect of *looking* at the field.
  const missing =
    current !== "" && !installed.some((m) => m.name === current)
      ? `<option value="${escapeHtml(current)}" selected>${escapeHtml(
          current,
        )} — not installed</option>`
      : "";
  const options = installed
    .map((m) => {
      const detail = [m.parameters ?? "", formatModelSize(m.size)].filter(Boolean).join(" · ");
      return `<option value="${escapeHtml(m.name)}"${
        m.name === current ? " selected" : ""
      }>${escapeHtml(m.name)}${detail ? ` — ${escapeHtml(detail)}` : ""}</option>`;
    })
    .join("");
  return `<label class="field">Model
      <select id="settings-chat-model">${missing}${options}</select>
    </label>
    <div class="settings-action"><button type="button" class="btn small"
      id="settings-chat-model-custom" data-chat-model-custom>Type a model name</button>
      <span class="muted">For one you haven’t pulled yet.</span></div>`;
}

// The **Local** configuration's whole note: nothing leaves, so there is no key field and
// no privacy warning to give — only the fact that makes the difference legible.
//
// Plus the one command that changes what the picker above can offer. Spelled with the
// suggested model when this machine's memory could be read and as a `<model-name>` shape
// when it couldn't — either way beside the quickstart, which is the page carrying both
// the install and the pull for someone who has neither.
function localNoteHtml(setup: ChatSetup | null): string {
  const suggested = setup?.ollama?.suggested?.model;
  return `<p class="settings-note">Local models keep everything on this machine — your
        question and the retrieved passages never leave it. B2 talks to any
        OpenAI-compatible server; Ollama is the one it can walk you through.</p>
      <p class="settings-note">Add a model with
        <code>${escapeHtml(pullCommand(PULL_PLACEHOLDER))}</code>${
          suggested ? ` — e.g. <code>${escapeHtml(pullCommand(suggested))}</code>` : ""
        }, then pick it above. The <a href="${OLLAMA_QUICKSTART_URL}">Ollama quickstart</a>
        has the whole sequence.</p>`;
}

// The **Cloud models** key field, and the sentence saying where that key lives.
//
// Four states, because a user has to be told *before* they wonder (GH #176). B2 remembers
// a key in the macOS Keychain — encrypted at rest, and there next launch — but two of the
// four are cases where what they just did isn't quite what they'd assume:
//
//   - `environment` — `B2_LLM_API_KEY` overrides anything saved here, so a key typed into
//     this field is stored and *not used*. Saying so is the difference between a documented
//     precedence and a field that silently does nothing.
//   - `session` — the Keychain refused, so the key works now and is gone at quit. The
//     degrade is deliberate (chat must not break on a locked keychain) but it is not
//     something to discover at the next launch.
//
// The field itself always paints empty: a password input that echoed its secret back would
// be a worse idea than not showing it at all. Which is what makes an empty save mean
// "keep", and leaves Remove as the only way back to a keyless configuration — without it a
// key could never be removed, and repointing the endpoint would send the old provider's
// token to the new one.
function cloudKeyHtml(setup: ChatSetup | null): string {
  const source = setup?.api_key_source ?? "none";
  const placeholder =
    source === "none"
      ? "sk-…"
      : source === "environment"
        ? "•••••••• (from your environment)"
        : source === "stored"
          ? "•••••••• (saved in your Keychain)"
          : "•••••••• (this session only)";
  const where = {
    none: "",
    environment: `<p class="settings-detail"><code>B2_LLM_API_KEY</code> is set in your
        environment, and that is the key in force — it overrides any key saved here.
        Unset it in your shell to go back to the one B2 remembers.</p>`,
    stored: `<p class="settings-detail">Saved in your macOS Keychain — encrypted at rest,
        and here the next time you open B2.</p>`,
    session: `<p class="settings-detail">Kept for this session only: B2 couldn’t save it to
        your Keychain, so it will be gone when you quit. Saving again will retry.</p>`,
  }[source];
  // The closing sentence is where the key *lives*, and it has to agree with `where` above.
  // Under `session` it cannot be the general "B2 saves it in your Keychain": this key is
  // precisely the one B2 could not save, and a paragraph that says both is worse than
  // either — a user reading "couldn't save it" and then "B2 saves the key" has no way to
  // know which sentence is about them.
  const storage =
    source === "session"
      ? `This key was <strong>not</strong> saved — B2 normally keeps it in your macOS Keychain,
         never in a plain file. Set <code>B2_LLM_API_KEY</code> in your environment to have one
         that persists regardless.`
      : `B2 saves the key in your macOS Keychain, never in a plain file — set
         <code>B2_LLM_API_KEY</code> in your environment to override it.`;
  // Offered whenever there is a key to remove. Under `environment` it still has work to
  // do — it clears the one B2 remembers — but it cannot touch a variable the app doesn't
  // own, so the label says which key it means.
  const remove =
    source === "none"
      ? ""
      : `<div class="settings-action"><button class="btn small" id="settings-chat-clear-key" data-chat-clear-key
           title="Forget the key B2 has saved">Remove key</button>
         <span class="muted">Removes the key B2 saved. A key set in <code>B2_LLM_API_KEY</code>
         is your environment's, and stays.</span></div>`;
  // Where to *get* an endpoint, since B2 ships no default cloud provider and never will
  // (picking one is the explicit act M5 is about — see `setChatMode`). Ollama's hosted
  // models are named because they are the one provider B2 already knows how to talk to
  // without a second thought: the same `/v1` surface, the same model names as the local
  // configuration. A link, though, not a pre-filled URL.
  const whereToGet = `<p class="settings-detail muted">Any OpenAI-compatible provider works —
        put its <code>/v1</code> URL above. Ollama’s hosted models are one:
        <a href="${OLLAMA_CLOUD_URL}">Ollama cloud</a>.</p>`;
  return `${whereToGet}
      <label class="field">API key
        <input id="settings-chat-key" type="password" autocomplete="off" spellcheck="false"
          placeholder="${placeholder}" />
      </label>
      ${where}
      ${remove}
      <p class="settings-note">
        <strong>Cloud models send your question and the retrieved note passages to the
        configured provider.</strong> Nothing else leaves your machine, and B2 still writes
        nothing to your notes. ${storage}
      </p>`;
}

// General — app-wide preferences that belong to no subsystem. Appearance is the only one
// today; this is the tab a vault or editor preference lands in rather than being wedged
// beside the embedding model, which it has nothing to do with.
function generalPanelHtml(state: AppState): string {
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
      return `<button type="button" class="seg${on ? " seg-on" : ""}" id="settings-theme-${
        t.id
      }" data-theme-choice="${t.id}" aria-pressed="${on}">${t.label}</button>`;
    })
    .join("");
  return `<div class="settings-subhead">Appearance</div>
      <p class="settings-detail muted">System follows macOS; Light and Dark pin B2 regardless.</p>
      <div class="field">
        <span class="field-label">Theme</span>
        <div class="segmented" role="group" aria-label="Appearance">${themeButtons}</div>
      </div>`;
}

// Index — the vault's projection into SQLite (index-engine.md §1), and the one button that
// rebuilds it by hand.
//
// Why the button is *here* and not in the top bar it shipped in: indexing is automatic now.
// The vault is brought up to date the moment it opens (#25, `autoIndexOnOpen`), the fs-watch
// pulse re-projects every external save, and a cancelled run heals off the DB-derived
// pending set on the next pass. A manual Reindex is therefore the exception — the thing you
// reach for after a model swap or a bulk edit outside B2 — and permanent top-bar chrome for
// an exception trains the eye to ignore the bar. It belongs where you go *looking* for it,
// next to the coverage numbers that say whether you need it.
//
// The *progress* meter stays in the top bar beside the vault it is indexing (main.ts
// `buildShell`): a run is watchable — and cancellable — with Settings shut, which is the
// whole point of the app staying usable while it runs. This panel paints a second one while
// a run is live, which it did not need to when Settings was a box floating over that bar —
// it takes the window now, so pointing at the top bar would be pointing at something the
// human cannot see. Two meters, but not two truths: `paintReindex` writes the same values
// into every meter on screen, and only one of them is ever visible.
function indexPanelHtml(state: AppState): string {
  const disabled = reindexDisabled(state);
  // The same honesty as the search caveat (#26): "indexed" and "embedded" are two different
  // states, and a projected-but-unembedded vault must never read as finished.
  const coverage = ((): string => {
    if (state.vaultRoot === null) return "No vault is open.";
    if (state.notesTotal === 0)
      return "Nothing indexed yet — B2 indexes a vault when you open it.";
    const n = state.notesTotal;
    if (!state.semantic)
      return `${n} note${n === 1 ? "" : "s"} indexed for keyword search. The embedding model isn’t installed, so none are embedded.`;
    return state.notesEmbedded >= n
      ? `${n} note${n === 1 ? "" : "s"} indexed, all embedded.`
      : `${n} note${n === 1 ? "" : "s"} indexed · ${state.notesEmbedded}/${n} embedded.`;
  })();
  // While a run is live the panel carries the meter itself. It used to point at the top
  // bar's ("Progress and Cancel are in the top bar"), which was true while Settings was a
  // box floating over the bar and became a lie the moment it took the window. Same markup
  // and the same painter as the shell's (`paintReindex` walks every `.reindex-progress` on
  // screen), so the two can't disagree about a run — only one of them is ever visible.
  // The Cancel carries an id for the reason every control in here does: the surface
  // repaints per progress batch, and focus is put back by id.
  const running = state.reindexing
    ? `<div class="reindex-progress" aria-live="polite">
         <div class="reindex-track"><div class="reindex-fill is-indeterminate"></div></div>
         <span class="reindex-label"></span>
         <button id="settings-cancel-reindex" class="btn ghost small" data-cancel-reindex>Cancel</button>
       </div>`
    : `<span class="muted">Rarely needed — B2 indexes on open and as you save.</span>`;
  return `<div class="settings-subhead">Vault index</div>
      <p class="settings-detail muted">The index is a disposable projection of your Markdown — delete it and a reindex rebuilds it identically.</p>
      <p class="settings-coverage">${escapeHtml(coverage)}</p>
      <div class="settings-action">
        <button class="btn small" id="reindex"${disabled ? " disabled" : ""}
          title="Re-project the vault into the index">${escapeHtml(reindexLabel(state))}</button>
        ${running}
      </div>
      <p class="settings-note">Reindex re-projects every note (notes, keyword index, and the
        typed graph), then embeds whatever is missing vectors. Reach for it after changing the
        embedding model, or after editing the vault with B2 closed.</p>`;
}

/** Whether the Reindex button is refused, and what it reads — pure, so the panel's paint
 *  and main.ts's targeted repaint (`paintReindex`, which runs on every streamed progress
 *  batch without a full render) can't drift apart on either. */
export function reindexDisabled(state: AppState): boolean {
  return state.loading || state.reindexing || state.vaultRoot === null;
}

export function reindexLabel(state: AppState): string {
  return state.reindexing ? "Indexing…" : "Reindex";
}

// Embedding — everything about the model: which one, where its files are, what device it
// runs on, whether it's downloaded, and how long it takes. The time ledger lives here
// rather than in a diagnostics tab of its own because its whole purpose is judging a
// model *swap*, which is the decision made two controls above it.
function embeddingPanelHtml(state: AppState): string {
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
  return `<div class="settings-subhead">Model</div>
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
      }`;
}

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
function keyboardPanelHtml(state: AppState): string {
  const changed = customized(DEFAULT_BINDINGS, state.keyOverrides).length;
  const resetAll = changed
    ? `<button class="btn small" id="keys-reset-all">Reset all (${changed})</button>`
    : "";
  return `<div class="settings-subhead">Keyboard shortcuts</div>
      <p class="settings-detail muted">B2 is fully operable from the keyboard — the mouse is an accelerator, never a requirement. Click a chord to change it.</p>
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

/** Every chord the app answers to, grouped, from the one table in shortcuts.ts — plus
 *  the menu bar's, which are the host's declaration (`state.menuChords`, #119) rather
 *  than B2 bindings. `null` until the boot fetch lands, and the sheet falls back to
 *  menukeys.ts's mirror for that window.
 *
 *  A chip that names a command is a `<button>`; everything else — the platform's own
 *  keys, the menu bar's, a chord two commands print alike — stays a `<kbd>`. That split
 *  is the whole affordance: what looks pressable is what B2 can actually move.
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
  const groups = shortcuts(state.menuChords ?? undefined)
    .map(
      (g) => `<section class="keys-group">
        <h4>${escapeHtml(g.title)}</h4>
        <dl class="keys-list">${g.items
          .map((s) => `<dt>${s.keys.map(chip).join(" ")}</dt><dd>${escapeHtml(s.action)}</dd>`)
          .join("")}</dl>
      </section>`,
    )
    .join("");
  return `<div class="keys-grid">${groups}</div>`;
}

/** The tab id's element id — the one both the rail and `aria-labelledby` derive from,
 *  and what main.ts re-focuses a tab by after the modal repaints. */
function tabDomId(id: SettingsTabId): string {
  return `settings-tab-${id}`;
}

/**
 * Settings: a vertical rail of sections beside the active panel, taking **the whole
 * window** rather than floating in a box.
 *
 * It was a floating dialog until it stopped fitting in one. Five sections, and the two
 * ends of the range don't want the same rectangle: Chat is a provider configuration with
 * a setup card and a page of privacy copy, Keyboard is forty rows of chord table, and
 * General is a three-button theme switch. A fixed box sized for the long ones leaves the
 * short ones mostly empty and *still* puts the rest below the fold. A surface that big
 * has stopped being an interruption you dismiss and become a place you go, so it says
 * so — it covers the app, and the panel gets the whole remaining rectangle.
 *
 * Modal semantics are unchanged (`role="dialog"` + `aria-modal`, the ⇥ trap, Escape, the
 * focus return in main.ts): the app is still underneath, and Done is still where you came
 * from. What goes with the box is the **backdrop** — there is no "outside" left to click,
 * so the ways out are Done and Escape, and main.ts's click handler dropped that branch to
 * match. The three rows are fixed header / scrolling panel / fixed footer, which is what
 * keeps Done and the key hints on screen no matter how long a section runs.
 *
 * DOM order is load-bearing: rail, then panel, then Done. `focusIntoOverlay` opens
 * Settings on `overlayFocusables()[0]` and documents that as "the selected tab", which is
 * true only while nothing focusable precedes the rail — hence a header that carries the
 * title alone and a Done button that stays in the footer.
 *
 * The rail is the ARIA `tabs` pattern (settingstabs.ts owns the moves): `role="tablist"`,
 * one `role="tab"` per section, and a **roving `tabindex`** so the whole rail is a single
 * Tab stop — a settings surface whose Tab sequence starts with N section buttons is one
 * you Tab *past*, not through. The panel carries `tabindex="0"` on purpose even when it
 * holds its own controls: it is the scroll container, and a region you can't focus is a
 * region you can't scroll without the mouse (the Keyboard section is a page of table and
 * nothing else, so this is the only way to read past the fold).
 */
function settingsScreenHtml(state: AppState): string {
  const active = state.settingsTab;
  const tabs = SETTINGS_TABS.map((t) => {
    const on = t.id === active;
    return `<button class="stab${on ? " stab-on" : ""}" id="${tabDomId(t.id)}" role="tab"
              aria-selected="${on}" aria-controls="settings-panel" tabindex="${on ? "0" : "-1"}"
              data-settings-tab="${t.id}" title="${escapeHtml(t.hint)}">${escapeHtml(t.label)}</button>`;
  }).join("");
  // `.settings-measure` caps the line length inside a panel that is now as wide as the
  // window: prose set across 1600px is prose nobody reads back to the start of. Keyboard
  // is the one section that isn't prose — a two-column reference read in columns — so it
  // takes the wider measure, and the choice is here rather than in the panel builder
  // because it is a fact about the *surface*, not about what the section says.
  const measure =
    active === "keyboard" ? "settings-measure settings-measure-wide" : "settings-measure";
  return `<div class="settings-screen" role="dialog" aria-modal="true" aria-label="Settings">
      <header class="settings-head"><h3>Settings</h3></header>
      <div class="settings-body">
        <div class="settings-tabs" role="tablist" aria-orientation="vertical"
             aria-label="Settings sections">${tabs}</div>
        <div class="settings-panel" id="settings-panel" role="tabpanel" tabindex="0"
             aria-labelledby="${tabDomId(active)}">
          <div class="${measure}">${settingsPanelHtml(state)}</div>
        </div>
      </div>
      <div class="settings-foot">
        <span class="modal-hint">↑↓ picks a section · ⌃Tab cycles · Esc closes</span>
        <button class="btn primary" id="settings-done" data-settings-close>Done</button>
      </div>
    </div>`;
}

/** One menu row. `chord` names the direct shortcut where one exists — a menu is where
 *  a keyboard user *learns* the chord that lets them skip the menu next time. */
function contextItemHtml(attr: string, label: string, chord = "", danger = false): string {
  const hint = chord ? `<span class="context-chord">${chord}</span>` : "";
  return `<button class="context-item${
    danger ? " is-danger" : ""
  }" ${attr} role="menuitem" tabindex="-1">${label}${hint}</button>`;
}

// The right-click menu — one overlay, two surfaces (state.ts `ContextMenuState`):
// a discovery card (Open note / Link…, replacing the old inline "Link…" button) or
// the file tree (New note / New folder in the folder under the cursor, named in a
// muted context line). Anchored at the cursor via inline left/top — the coords are
// set + clamped on-screen in main.ts, and are plain numbers, so no escaping is
// needed. Rendered into its own overlay root so it floats above the panes; an
// outside click / Escape / scroll dismisses it (main.ts).
export function contextMenuHtml(state: AppState): string {
  const m = state.contextMenu;
  if (!m) return "";
  let items: string;
  if (m.kind === "tree") {
    // Over a concrete row the menu targets that node (Rename / Move… — renaming
    // acts on the file path, never a frontmatter title); the create pair keeps
    // targeting the folder context either way.
    //
    // The two copy items are the row's *read* actions, sitting between the ones
    // that change the file and the one that destroys it (Delete stays last, where
    // a destructive item belongs). They copy the two paths that name this row —
    // vault-relative and absolute (copypath.ts says why both) — and they are the
    // only way to get either: the context line above shows the vault path but is
    // muted text, and the system path is nowhere in the UI at all.
    const node = m.node
      ? `<div class="context-label">${escapeHtml(m.node.path)}</div>
        ${contextItemHtml("data-ctx-rename", "Rename", "F2")}
        ${contextItemHtml("data-ctx-move", "Move…")}
        ${contextItemHtml("data-ctx-copy-vault-path", "Copy vault path")}
        ${contextItemHtml("data-ctx-copy-system-path", "Copy system path")}
        ${contextItemHtml("data-ctx-delete", "Delete", "⌘⌫", true)}
        <div class="context-sep" role="separator"></div>`
      : `<div class="context-label">${escapeHtml(m.dir ? `${m.dir}/` : "vault root")}</div>`;
    // Import files… is the drop gesture's keyboard half (K1): dragging a file in from
    // Finder is a pointer-only action, so the same import runs from here — via ⇧F10,
    // the keyboard's right-click — with an OS picker instead of a drag. It targets the
    // folder context like the create pair, so it reads as the third way to put
    // something in this folder.
    items = `${node}
        ${contextItemHtml("data-ctx-new-note", "New note", "⌘N")}
        ${contextItemHtml("data-ctx-new-folder", "New folder", "⇧⌘N")}
        ${contextItemHtml("data-ctx-import", "Import files…")}`;
  } else {
    // *Insert link at cursor* is the card drag's keyboard half (K1), the same shape
    // *Import files…* is the Finder drop's: dragging a card onto a line is pointer-only,
    // so the identical insertion runs from here — via ⇧F10 — aimed at the line the caret
    // is already on. Only while editing, because that is the only time there is a buffer
    // to insert into; the drag is withheld on the same condition (the card's `draggable`).
    items = `${contextItemHtml("data-ctx-open", "Open note", "⏎")}
        ${state.editing ? contextItemHtml("data-ctx-insert", "Insert link at cursor") : ""}
        ${contextItemHtml("data-ctx-link", "Link…")}`;
  }
  // `tabindex="-1"` on the menu itself makes the container focusable-by-script but not
  // by Tab: main.ts moves focus to the first item on open and traps ↑↓/⏎/Esc inside.
  return `<div class="context-menu" style="left:${m.x}px;top:${m.y}px" role="menu" tabindex="-1">${items}</div>`;
}

/** The Move… modal: pick a destination folder for the targeted tree node. Every
 *  folder the tree knows renders as a row; an invalid destination (the node's
 *  current folder, or a folder inside the folder being moved) renders disabled
 *  with the reason, so the modal teaches the same rule the host enforces. */
function moveModalHtml(state: AppState): string {
  const t = state.moveTarget;
  if (!t) return "";
  const dirs = allDirs(state.dirs);
  const rows = dirs
    .map((dir) => {
      const label = dir === "" ? "vault root" : `${dir}/`;
      if (!canMoveInto(t.path, t.nodeKind, dir)) {
        const why =
          t.nodeKind === "folder" && (dir === t.path || dir.startsWith(`${t.path}/`))
            ? "inside the folder being moved"
            : "current folder";
        return `<div class="move-dest is-disabled">${escapeHtml(label)}<span class="muted"> — ${why}</span></div>`;
      }
      return `<button class="move-dest" data-move-dest="${escapeHtml(dir)}">${escapeHtml(label)}</button>`;
    })
    .join("");
  return `<div class="modal-backdrop">
      <div class="modal" role="dialog" aria-modal="true" aria-label="Move to a folder">
        <h3>Move ${escapeHtml(t.label)} to…</h3>
        <div class="move-dest-list">${rows}</div>
        <div class="modal-actions">
          <span class="modal-hint">Tab / ↑↓ pick a folder · ⏎ moves · Esc cancels</span>
          <button class="btn ghost" data-cancel>Cancel</button>
        </div>
      </div>
    </div>`;
}

/** The folder-delete confirm — the one destructive gesture that asks first: a
 *  whole subtree (unindexed files included) leaves the disk. Files delete
 *  without a dialog; the tree gesture itself is the intent. */
function deleteModalHtml(state: AppState): string {
  const t = state.deleteTarget;
  if (!t) return "";
  return `<div class="modal-backdrop">
      <div class="modal" role="dialog" aria-modal="true" aria-label="Delete folder">
        <h3>Delete ${escapeHtml(t.label)}?</h3>
        <p class="muted">${escapeHtml(t.path)}/ and everything inside it will be deleted from the vault and the disk.</p>
        <div class="modal-actions">
          <span class="modal-hint">⏎ deletes · Esc cancels</span>
          <button class="btn ghost" data-cancel>Cancel</button>
          <button class="btn danger" id="delete-confirm">Delete folder</button>
        </div>
      </div>
    </div>`;
}

// The overlay layer, in precedence order (main.ts's `currentOverlay` ranks them the same
// way). Settings is the odd one out since it went full-window — same `role="dialog"` and
// the same trap, no backdrop and no box — so it is first here for the reason it is first
// there: it paints *over* a Move/Delete/Link target left set behind it.
export function modalHtml(state: AppState): string {
  if (state.settingsOpen) return settingsScreenHtml(state);
  if (state.moveTarget) return moveModalHtml(state);
  if (state.deleteTarget) return deleteModalHtml(state);
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
          <span class="modal-hint">⏎ commits · Esc cancels</span>
          <button class="btn ghost" data-cancel>Cancel</button>
          <button class="btn primary" id="link-commit">Commit link</button>
        </div>
      </div>
    </div>`;
}
