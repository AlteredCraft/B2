// The view: the pane HTML builders, and the view layer's one import surface. Pure
// functions of state — no IPC, no DOM mutation (main.ts writes the output in).
//
// This module paints the tree, the note pane, the discovery column, the menus and the
// dialogs; the bigger surfaces have modules of their own — chatview.ts, graphview.ts,
// explainview.ts, settingsview.ts, keysview.ts — and the small shared pieces are
// widgets.ts's. What the rest of the app imports is re-exported below, so a caller never
// has to know which module a surface lives in.
//
// Safety (invariant E5 — **note content is untrusted input**): authorship is not trust, a
// `.md` can come from anyone (a shared vault, a downloaded or clipped note), so every note
// body is hostile. Two rules hold together: B2 HTML-escapes every value *it* interpolates
// (titles, paths, snippets) so UI chrome can't be broken by note content, and the one
// Markdown→HTML path, `renderMarkdown` (markdown.ts, re-exported here), sanitizes its
// output as `marked`'s `postprocess` hook — so *every* caller is covered by construction
// rather than by remembering. The webview CSP (`default-src 'self'`, no inline scripts)
// is a second, independent layer, never the only one (crates/b2-desktop/CLAUDE.md, GH #77).

// Relative imports carry their `.ts` here (the idiom highlight.ts and reconcile.ts already
// use) because render.test.ts runs this module off the source under node's type-stripping,
// which resolves by real filename — a bundler-style extensionless value import doesn't
// resolve there. tsc rewrites nothing (noEmit).
import { escapeHtml } from "./escape.ts";
import { RELATION_VERBS, openDocPath, type AppState, type SideSection } from "./state.ts";
import { allDirs, baseName, moveRefusal, renamePrefill } from "./move.ts";
import { shouldPromptEmbedInstall } from "./embedreminder.ts";
import { coverage } from "./coverage.ts";
import { STRENGTH_MIN_CANDIDATES, strengthBand } from "./strength.ts";
import { displayKeys } from "./bindings.ts";
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
  type IconName,
} from "./icons.ts";
import { cardKey, cardRowKey, rovingSideKey, sectionRowKey, sideRows } from "./sidenav.ts";
import type { NoteView, ResourceExplainView } from "./types.ts";
import { renderMarkdown } from "./markdown.ts";
import { sideTab, strengthHtml } from "./widgets.ts";
import { chatPaneHtml } from "./chatview.ts";
import { editToggleHtml, graphPaneHtml, graphToggleHtml } from "./graphview.ts";
import { explainPaneHtml } from "./explainview.ts";
import { cmdSheetHtml } from "./keysview.ts";
import { reindexDisabled, reindexLabel, settingsScreenHtml } from "./settingsview.ts";

// Re-exported so this module stays the view layer's single import surface (main.ts and
// the suite reach for them here); the definitions live in the modules named.
export { escapeHtml };
export { renderMarkdown };
export { cmdSheetHtml };
export { reindexDisabled, reindexLabel };

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
  const note = escapeHtml(`New note ${ctx} (${displayKeys(["tree.new-note"])})`);
  const folder = escapeHtml(`New folder ${ctx} (${displayKeys(["tree.new-folder"])})`);
  return `<span class="tree-actions">
      <button class="tree-action" data-new-note title="${note}" aria-label="New note">
        ${icon("file-earmark-plus")}
      </button>
      <button class="tree-action" data-new-folder title="${folder}" aria-label="New folder">
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
    openDocPath(state),
  );
  const body = treeChildrenHtml(tree, state, 0, roving);
  if (!body)
    return head + `<p class="tree-empty">No files indexed yet — Reindex to populate.</p>`;
  return (
    head +
    `<div class="tree" role="tree" aria-label="Vault files"
       title="${escapeHtml(treeTitle())}">${body}</div>`
  );
}

/** The tree's keyboard crib, out of the live registry — every chord in it is rebindable
 *  (⏎ is the row button's own activation, so it stays a word). */
function treeTitle(): string {
  return [
    `${displayKeys(["tree.row.prev", "tree.row.next"], "/")} move`,
    `${displayKeys(["tree.row.in", "tree.row.out"], "/")} expand/collapse`,
    "⏎ open",
    `${displayKeys(["tree.rename"])} rename`,
    `${displayKeys(["menu.open"])} menu`,
  ].join(" · ");
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
          <span class="fm-hint">This block is yours — B2 changes nothing in it · ${escapeHtml(
            displayKeys(["fm.save"]),
          )} saves · ${escapeHtml(displayKeys(["dismiss"]))} cancels</span>
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
          ${editToggleHtml(state.loading || editing, editing ? "Finish the frontmatter edit first" : undefined)}
        </div>
      </div>
      ${body}
    </div>`;
}

// --- the resource card + its image viewer (file-type slice 2) ------------------------
//
// What an image *is* — the extension table, the `data:` URL, the size bound — moved to
// embeds.ts when the reading view grew an inline viewer of its own: one answer, three
// surfaces (the card, the reading view, the editor's live preview).

/** Human-readable byte count for the card ("67 B", "1.4 KB", "3.2 MB"). */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// The resource **card** (spec §6): selecting any file in the tree opens *something* —
// filename, class, size, modified, content hash — plus the backlinks panel (which
// notes reference this file, with their authored captions) and one action, *Open in
// system default* (an OS handoff performed host-side).
//
// `image` is the one class with a viewer so far: `image` is the bytes main.ts fetched
// for it, and it *replaces* the "no viewer" line rather than the card, so the metadata,
// the backlinks and the OS handoff stay put — the handoff is still how you get to a real
// image app, and it is the whole card when the file is too large to hold on screen
// (`IMAGE_VIEWER_MAX_BYTES`, embeds.ts) or its bytes could not be read. Every other
// class is still the fallback card, and `binary` is its permanent catch-all.
function resourceCardHtml(r: ResourceExplainView, image: string | null): string {
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
  const name = baseName(r.path);
  // The alt text is the filename: the `<h1>` right above already says it, so a screen
  // reader that reads both is repeating itself rather than being told nothing — and B2
  // has no description of the picture to offer that would be truer than its name.
  const viewer = image
    ? `<img class="resource-image" src="${escapeHtml(image)}" alt="${escapeHtml(name)}">`
    : `<p class="resource-no-viewer">No viewer available for this file type yet.</p>`;
  return `<article class="note resource-card">
      <header class="note-head">
        <h1>${escapeHtml(name)}</h1>
        <div class="note-meta">${escapeHtml(r.path)} · ${escapeHtml(r.class)} · ${formatSize(
          r.size,
        )} · modified ${escapeHtml(modified)}</div>
      </header>
      <div class="resource-card-body">
        ${viewer}
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
  if (state.currentResource) return resourceCardHtml(state.currentResource, state.resourceImage);
  const n = state.current;
  if (n && state.explainCard?.anchor === n.path && !state.editing) return explainPaneHtml(state);
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
      : renderMarkdown(n.body, state.embedImages);
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

// The honest search-ranking caveat (#26). Search always answers over the keyword (BM25)
// index; this says how much *semantic* ranking is mixed in, so a projected-but-unembedded
// vault never silently under-ranks:
//   • no real model            → "keyword only (run `b2 init`)"
//   • model, nothing embedded  → "keyword-only for now (0/M embedded — Reindex)"
//   • model, partly embedded   → "keyword-first (N/M embedded)" (vector half still filling)
//   • model, fully embedded    → "" (ranking is fully semantic; no caveat)
function searchCaveat(state: AppState): string {
  const c = coverage(state);
  if (!c.model) return " · keyword only (run <code>b2 init</code> for semantic)";
  switch (c.embedded) {
    case "empty":
    case "all":
      return ""; // empty vault, or every note embedded — semantic is live
    case "none":
      return ` · keyword-only for now (0/${c.m} embedded — Reindex)`;
    case "partial":
      return ` · keyword-first (${c.n}/${c.m} embedded)`;
  }
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
    if (!coverage(state).model)
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
            <div class="card-actions">
              <button class="card-why linklike" tabindex="-1" data-explain="${escapeHtml(
                c.path,
              )}" title="See the passages behind this suggestion (no model)">Explain</button>
              <button class="card-why linklike" tabindex="-1" data-why="${escapeHtml(
                c.path,
              )}" data-why-title="${escapeHtml(
                c.title ?? "",
              )}" title="Ask chat why this note was suggested">Why?</button>
            </div>
          </div>`;
      // *Why?* hands the pair to chat (main.ts's `askWhy`): the snippet above is the
      // passage that matched, and this is the question it raises. `tabindex="-1"` keeps
      // the card one Tab stop; the keyboard reaches the same action through the card
      // menu's *Why was this suggested?* (⇧F10 — K1).
      //
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

/** One menu row. `chord` names the direct shortcut where one exists — a menu is where
 *  a keyboard user *learns* the chord that lets them skip the menu next time — so it
 *  comes out of the live registry (`displayKeys`), never spelled here. */
function contextItemHtml(attr: string, label: string, chord = "", danger = false): string {
  const hint = chord ? `<span class="context-chord">${escapeHtml(chord)}</span>` : "";
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
        ${contextItemHtml("data-ctx-rename", "Rename", displayKeys(["tree.rename"]))}
        ${contextItemHtml("data-ctx-move", "Move…")}
        ${contextItemHtml("data-ctx-copy-vault-path", "Copy vault path")}
        ${contextItemHtml("data-ctx-copy-system-path", "Copy system path")}
        ${contextItemHtml("data-ctx-delete", "Delete", displayKeys(["delete.focused"]), true)}
        <div class="context-sep" role="separator"></div>`
      : `<div class="context-label">${escapeHtml(m.dir ? `${m.dir}/` : "vault root")}</div>`;
    // Import files… is the drop gesture's keyboard half (K1): dragging a file in from
    // Finder is a pointer-only action, so the same import runs from here — via ⇧F10,
    // the keyboard's right-click — with an OS picker instead of a drag. It targets the
    // folder context like the create pair, so it reads as the third way to put
    // something in this folder.
    items = `${node}
        ${contextItemHtml("data-ctx-new-note", "New note", displayKeys(["tree.new-note"]))}
        ${contextItemHtml("data-ctx-new-folder", "New folder", displayKeys(["tree.new-folder"]))}
        ${contextItemHtml("data-ctx-import", "Import files…")}`;
  } else {
    // *Insert link at cursor* is the card drag's keyboard half (K1), the same shape
    // *Import files…* is the Finder drop's: dragging a card onto a line is pointer-only,
    // so the identical insertion runs from here — via ⇧F10 — aimed at the line the caret
    // is already on. Only while editing, because that is the only time there is a buffer
    // to insert into; the drag is withheld on the same condition (the card's `draggable`).
    items = `${contextItemHtml("data-ctx-open", "Open note", "⏎")}
        ${state.editing ? contextItemHtml("data-ctx-insert", "Insert link at cursor") : ""}
        ${contextItemHtml("data-ctx-link", "Link…")}
        ${contextItemHtml("data-ctx-explain", "Explain this suggestion")}
        ${contextItemHtml("data-ctx-why", "Why was this suggested?")}`;
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
      const refusal = moveRefusal(t.path, t.nodeKind, dir);
      if (refusal !== null) {
        const why = refusal === "inside-itself" ? "inside the folder being moved" : "current folder";
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
