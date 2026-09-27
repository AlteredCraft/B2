// The view: pane HTML builders as pure functions of state (no IPC, no DOM mutation), and
// the view layer's one import surface (other surfaces' modules are re-exported below).
//
// Safety (E5, GH #77): note content is untrusted. B2 HTML-escapes every value it
// interpolates, and the one Markdown→HTML path, `renderMarkdown`, sanitizes its own output.
// The webview CSP is a second layer, never the only one.

// Relative imports carry `.ts`: render.test.ts runs this under node's type-stripping,
// which resolves by real filename.
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

// Re-exported so this module stays the view layer's single import surface.
export { escapeHtml };
export { renderMarkdown };
export { cmdSheetHtml };
export { reindexDisabled, reindexLabel };

// --- file tree --------------------------------------------------------------------
//
// The tree's shape and row order live in treenav.ts, shared with arrow-key navigation
// (K1, GH #78) so the keys walk the order you see.
//
// A row is two fixed slots, fold chevron then thing icon, then its label, so a file's icon
// lines up under its folder's icon.

/** The fold chevron in its slot, for every foldable row in the app. */
function foldCaretHtml(open: boolean): string {
  return `<span class="tree-caret">${icon(foldChevron(open), { size: 12 })}</span>`;
}

/** The empty chevron slot for a row that doesn't fold, keeping icons aligned. */
const NO_CARET = `<span class="tree-caret"></span>`;

/** The "what is this" slot: a folder, a note, or a resource's type (icons.ts). */
function rowIconHtml(name: IconName): string {
  return `<span class="tree-icon">${icon(name)}</span>`;
}

/** The inline name input for a pending create, atop its target folder's children. The
 *  value lives only in the DOM (main.ts carries it across repaints). `role="none"`: a text
 *  field inside `role="tree"` is not a `treeitem`. */
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

/** The inline rename input, in place of the row being renamed; `treeCreateRowHtml`'s
 *  sibling. Keeps the row's own two slots. */
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
 * One folder's children (sub-folders, then files), recursively, in treenav.ts's order.
 * `roving` is the one row with `tabindex="0"`, so the tree is a single Tab stop.
 * `data-tree-row` is the row's keyboard identity, so main.ts never builds a selector out
 * of an arbitrary filename.
 */
function treeChildrenHtml(
  dir: TreeDir,
  state: AppState,
  depth: number,
  roving: string | null,
): string {
  const pad = (d: number) => `padding-left:${8 + d * 14}px`;
  // The DOM is flat, so `aria-level` is what tells a screen reader a row's depth.
  const level = ` aria-level="${depth + 1}"`;
  const tab = (path: string) => ` tabindex="${path === roving ? "0" : "-1"}"`;

  // startTreeCreate expanded the chain down to here, so a match is always visible.
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

/** The tree-head create icons. Both target the selection's folder, named in the tooltip. */
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
  // Resolved against visible rows, so collapsing a folder under the focused row never
  // leaves the tree with no tabbable row.
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

/** The tree's keyboard crib, from the live registry (⏎ is the button's own activation). */
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

// The note-pane top bar, a sibling before `<article class="note">` so its divider spans the
// pane: the frontmatter drawer toggle, then the graph, `</>` source and Edit toggles. The
// drawer and source toggles are state-controlled (not `<details>`) so they survive a
// full-pane repaint and stay sticky across notes.
//
// The drawer is also the frontmatter editor (GH #79): explicit Save/Cancel, no autosave,
// since half-typed YAML is not a save. While `fmEditing` the pane is under main.ts's render
// carve-out, so the buffer lives in the DOM.
function noteBarHtml(state: AppState, note: NoteView): string {
  const open = state.frontmatterOpen;
  const editing = state.fmEditing;
  const source = state.sourceOpen;
  const fm = note.frontmatter ?? "";
  const yaml = fm.replace(/\s+$/, ""); // display trim only
  const unreadable = note.frontmatter !== null && !note.frontmatter_readable;
  // The chord comes from the live registry: ⇧⌘E is rebindable.
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
    // Seeded verbatim, not trimmed: what you edit is what's on disk (W3).
    const rows = Math.min(16, Math.max(4, fm.split("\n").length + 1));
    // The "\n" after the opening tag is sacrificial: HTML parsing strips one leading
    // newline from a textarea, which would drop a block's own leading blank line.
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
  // Every chip carries a stable `id`: pressing one repaints the pane, and `paintNote`
  // restores focus by id (GH #91).
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

// --- the resource card + its image viewer ------------------------------------------
//
// What an image is (extensions, `data:` URL, size bound) lives in embeds.ts.

/** "67 B", "1.4 KB", "3.2 MB". */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// The resource card (spec §6): metadata, backlinks, and *Open in system default*. `image`
// (bytes main.ts fetched) replaces only the "no viewer" line; it is null for other classes,
// a file over `IMAGE_VIEWER_MAX_BYTES`, or unreadable bytes.
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
  // Alt text is the filename: B2 has no truer description of the picture.
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
 * The right column: search results, or the open note's discovery. Follows the tree's ARIA
 * pattern (K1, GH #78): flat rows, `aria-level`, roving `tabindex`, `.cards` wrappers as
 * `role="none"`. Row keys come from sidenav.ts, shared with the arrow keys.
 */
export function sidePaneHtml(state: AppState): string {
  const roving = rovingSideKey(sideRows(state), state.sideFocus);
  // Chat owns the column when open; opening chat or running a search closes the other.
  if (state.chatOpen) return chatPaneHtml(state, roving);
  return state.searchQuery
    ? searchSectionHtml(state, roving)
    : discoverySectionHtml(state, roving);
}

// The search-ranking caveat (#26): how much semantic ranking is mixed into keyword search,
// so an unembedded vault never silently under-ranks.
function searchCaveat(state: AppState): string {
  const c = coverage(state);
  if (!c.model) return " · keyword only (run <code>b2 init</code> for semantic)";
  switch (c.embedded) {
    case "empty":
    case "all":
      return "";
    case "none":
      return ` · keyword-only for now (0/${c.m} embedded — Reindex)`;
    case "partial":
      return ` · keyword-first (${c.n}/${c.m} embedded)`;
  }
}

// The install banner (#26): with no model installed, semantic search and discovery are off
// with little visible sign, so say so. ✕ hides it for the session; the checkbox opts out
// for good.
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

// `clear` is the pane's one non-row focusable, so it needs a stable `id` for `paintSide`
// to restore focus (GH #91).
function searchSectionHtml(state: AppState, roving: string | null): string {
  const head = `<div class="side-head">
      <h2>Results</h2>
      <button id="clear-search" class="linklike" data-clear-search>clear</button>
    </div>
    <p class="side-sub">for “${escapeHtml(state.searchQuery)}”${searchCaveat(state)}</p>`;
  if (state.loading) return head + `<p class="side-empty">Searching…</p>`;
  if (state.searchResults.length === 0) return head + searchEmptyHtml(state);
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

// The two empty states (D2, GH #202): `searchVouched === false` means the vault holds no
// evidence for the query, so the copy says so; otherwise the list is just empty (e.g. an
// unbuilt index) and the copy makes no claim about the query.
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

// A collapsible discovery-section header. Collapsing is sticky (`collapsedSections`); the
// count shows only when non-zero.
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

// The card's fold chevron, a sibling of `.card-open` so folding doesn't open the note.
// Mouse-only (`tabindex="-1"`, `aria-hidden`): the row carries `aria-expanded` and →← folds
// (K1).
function cardFold(key: string, collapsed: boolean): string {
  return `<button class="card-fold" tabindex="-1" aria-hidden="true" data-fold-card="${escapeHtml(
    key,
  )}">
      ${foldCaretHtml(!collapsed)}
    </button>`;
}

/** A caveat about the list shown: not an empty state and not an error, so accent tones,
 *  never `--danger`, and not muted prose that gets skimmed past. */
function sideNoteHtml(text: string, title: string): string {
  return `<p class="side-note" title="${escapeHtml(title)}"><span class="side-note-icon" aria-hidden="true">${icon(
    "info-circle",
    { size: 14 },
  )}</span><span>${escapeHtml(text)}</span></p>`;
}

/** The ungraded caveat: candidates exist but none carries a z. Silence would read as
 *  "everything scored low". States the rule, since a small pool and a pool with no spread
 *  both land here, and names the bar ([`STRENGTH_MIN_CANDIDATES`]). */
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
    // The ranked list is always served (GH #197), so empty means no candidates at all.
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
      // The action buttons are `tabindex="-1"` to keep the card one Tab stop; the card
      // menu (⇧F10, K1) reaches them. `data-card-path`/`-title` feed the right-click menu.
      // Draggable only while editing (droplink.ts): with no buffer, nothing can accept it.
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

// Dangling outbound links (GH #12), shown as written so the user can fix them. Nothing to
// open, but still a keyboard row: a row you can see must be reachable (K1).
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

/** One menu row. `chord` is the direct shortcut, from the live registry (`displayKeys`). */
function contextItemHtml(attr: string, label: string, chord = "", danger = false): string {
  const hint = chord ? `<span class="context-chord">${escapeHtml(chord)}</span>` : "";
  return `<button class="context-item${
    danger ? " is-danger" : ""
  }" ${attr} role="menuitem" tabindex="-1">${label}${hint}</button>`;
}

// The right-click menu for a discovery card or the file tree (state.ts `ContextMenuState`),
// anchored at the cursor. The coords are numbers clamped in main.ts, so need no escaping.
export function contextMenuHtml(state: AppState): string {
  const m = state.contextMenu;
  if (!m) return "";
  let items: string;
  if (m.kind === "tree") {
    // Over a row the menu targets that node; the create items target the folder context.
    // Delete stays last. The copy items are the only way to get either path (copypath.ts).
    const node = m.node
      ? `<div class="context-label">${escapeHtml(m.node.path)}</div>
        ${contextItemHtml("data-ctx-rename", "Rename", displayKeys(["tree.rename"]))}
        ${contextItemHtml("data-ctx-move", "Move…")}
        ${contextItemHtml("data-ctx-copy-vault-path", "Copy vault path")}
        ${contextItemHtml("data-ctx-copy-system-path", "Copy system path")}
        ${contextItemHtml("data-ctx-delete", "Delete", displayKeys(["delete.focused"]), true)}
        <div class="context-sep" role="separator"></div>`
      : `<div class="context-label">${escapeHtml(m.dir ? `${m.dir}/` : "vault root")}</div>`;
    // Import files… is the Finder drop's keyboard half (K1).
    items = `${node}
        ${contextItemHtml("data-ctx-new-note", "New note", displayKeys(["tree.new-note"]))}
        ${contextItemHtml("data-ctx-new-folder", "New folder", displayKeys(["tree.new-folder"]))}
        ${contextItemHtml("data-ctx-import", "Import files…")}`;
  } else {
    // *Insert link at cursor* is the card drag's keyboard half (K1); only while editing.
    items = `${contextItemHtml("data-ctx-open", "Open note", "⏎")}
        ${state.editing ? contextItemHtml("data-ctx-insert", "Insert link at cursor") : ""}
        ${contextItemHtml("data-ctx-link", "Link…")}
        ${contextItemHtml("data-ctx-explain", "Explain this suggestion")}
        ${contextItemHtml("data-ctx-why", "Why was this suggested?")}`;
  }
  // main.ts focuses the first item on open and traps ↑↓/⏎/Esc inside.
  return `<div class="context-menu" style="left:${m.x}px;top:${m.y}px" role="menu" tabindex="-1">${items}</div>`;
}

/** The Move… modal. An invalid destination renders disabled with the reason, teaching the
 *  rule the host enforces. */
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

/** The folder-delete confirm, the one delete that asks first: a whole subtree leaves disk. */
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

// The overlay layer, in the same precedence as main.ts's `currentOverlay`. Settings is
// first: it paints over a Move/Delete/Link target left set behind it.
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
  // The backdrop carries no cancel attr, so a click inside the modal can't bubble into a
  // close (main.ts closes only when the backdrop is the exact target).
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
