// The controller: build the shell once, wire delegated events, run the actions that mutate
// `state` and re-render (a full-pane innerHTML swap; no framework). Backend access goes
// through `api`, the one IPC seam; no engine logic lives here.

import "../style.css";
import { autocompletion } from "@codemirror/autocomplete";
import { history } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxHighlighting } from "@codemirror/language";
import {
  Compartment,
  StateEffect,
  type Extension,
} from "@codemirror/state";
import {
  EditorView,
  type KeyBinding,
  keymap,
  tooltips,
} from "@codemirror/view";
import { api, errText, isWriteConflict } from "./api";
import {
  openDocPath,
  state,
  type AppState,
  type ContextMenuState,
  type SideSection,
  type ThemePref,
  type TreeNodeRef,
} from "./state";
import { dirChain, joinPath, normalizeName, parentDir } from "./newentry";
import { systemPath } from "./copypath";
import { bytesToBase64, importSummary, planImport } from "./importfiles";
import {
  baseName,
  canMoveInto,
  folderContext,
  isWithin,
  moveDestination,
  type NodeKind,
  refKind,
  remapPath,
  renameDestination,
} from "./move";
import {
  arrowMove,
  buildTree,
  neighborPath,
  rowIndex,
  treeNavFor,
  typeaheadTarget,
  visibleRows,
  type TreeRow,
} from "./treenav";
import { cardRowKey, sideArrowMove, sideNavFor, sideRowIndex, sideRows } from "./sidenav";
import {
  answerMessage,
  chatHistory,
  chatReady,
  errorMessage,
  LOCAL_CHAT_ENDPOINT,
  toolCapInput,
  userMessage,
  whyQuestion,
} from "./chat";
import {
  isSettingsTab,
  tabDomId,
  tabMove,
  tabNavFor,
  tabStep,
  type SettingsTabId,
} from "./settingstabs";
import { reindexMeterHtml } from "./widgets";
import { isThemePref, loadThemePref, saveThemePref, themeAttr } from "./theme";
import { loadReminderOptOut, saveReminderOptOut } from "./embedreminder";
import { externalUrl, isInPageAnchor } from "./links";
import { embedImagesField, livePreview, setEmbedImages, wikilink } from "./livepreview";
import {
  imageDataUrl,
  imageEmbedTargets,
  IMAGE_VIEWER_MAX_BYTES,
  inlineImagePlan,
} from "./embeds";
import { b2Highlighter, highlightCodeBlocks, resolveLang } from "./highlight";
import { noteTarget } from "./wikicomplete";
import {
  CARD_DRAG_MIME,
  cardDrop,
  type DraggedCard,
  insertDrop,
  planDrop,
  setDropTarget,
  withoutCard,
} from "./droplink";
import { FORMATS } from "./format";
import { indentList, outdentList } from "./list";
import {
  activeBindings,
  canonicalKey,
  chordFor,
  DEFAULT_BINDINGS,
  displayKeys,
  findBinding,
  isBound,
  setActiveBindings,
  type BindingId,
} from "./bindings";
import {
  applyOverrides,
  chordProblems,
  loadOverrides,
  type Overrides,
  refused,
  saveOverrides,
  withOverride,
} from "./keymap";
import { capture, PROBE_AFTER_MS, silenceHint } from "./recorder";
import { STOCK_EDITOR_KEYMAP } from "./editorkeys";
import { menuDrift } from "./menukeys";
import { HOLD_MS, type HoldEvent, type HoldPhase, holdStep } from "./cmdhold";
import {
  richPaste,
  runFormat,
  runInsertTable,
  runListShift,
  wikiCompletionSource,
} from "./editorcmds";
import { icon } from "./icons";
import { editDoneTitle, shellHints } from "./hints";
import { activeAfter, countLabel, FIND_CAP, findMatches, locate, stepActive, type Match } from "./findbar";
import { findField, setFindEffect } from "./findfield";
import { BOUNDS, initPanes, visiblePanes } from "./panes";
import {
  DEFAULT_ZOOM,
  hiddenNotice,
  loadZoom,
  saveZoom,
  stepZoom,
  type Direction,
} from "./zoom";
import { reconcileIndex } from "./reconcile";
import type {
  AnswerView,
  ChatSetup,
  ExplainView,
  ResourceExplainView,
  ResourceSummary,
  VaultInfo,
} from "./types";
import {
  cmdSheetHtml,
  contextMenuHtml,
  embedBannerHtml,
  escapeHtml,
  modalHtml,
  notePaneHtml,
  reindexDisabled,
  reindexLabel,
  sidePaneHtml,
  treePaneHtml,
} from "./render";

// --- render ---------------------------------------------------------------------

function el(id: string): HTMLElement {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node;
}

/**
 * Where the keyboard is in `pane`, as a thunk that restores it after an `innerHTML` swap
 * (WebKit drops focus to `<body>`; crates/b2-desktop/CLAUDE.md). Holds a stable identity
 * (side-row key, graph node id, element id) and falls back to the pane. Null when focus
 * was elsewhere: a repaint gives focus back, never takes it.
 */
function capturePaneFocus(pane: HTMLElement): (() => void) | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement || active instanceof SVGElement)) return null;
  if (!pane.contains(active)) return null;
  const row = active.closest<HTMLElement>("[data-side-row]");
  if (row) {
    // A row that didn't survive the repaint falls back to the roving tabstop.
    const key = row.dataset.sideRow ?? null;
    return () => (sideRowEl(key) ?? rovingSideRowEl() ?? pane).focus();
  }
  const gnode = active.closest<SVGElement>("[data-gnode]");
  if (gnode) {
    const id = gnode.dataset.gnode ?? null;
    return () => (gnodeEl(id) ?? pane).focus();
  }
  const id = active.id;
  if (id) return () => (document.getElementById(id) ?? pane).focus();
  return () => pane.focus();
}

/**
 * `capturePaneFocus` for the overlay layer, restored by `id` (every modal control carries
 * one). Null when focus was elsewhere or on a control with no id. If the control can no
 * longer take focus (gone, or disabled like a pressed Reindex button), falls back to the
 * overlay's first stop: focus must never land on `<body>` behind a backdrop.
 */
function captureModalFocus(root: HTMLElement): (() => void) | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !root.contains(active) || !active.id) return null;
  const id = active.id;
  // Carry the focused field's uncommitted value (typed text, or an unsaved `<select>`
  // choice) across a repaint the user didn't cause, e.g. a chat probe landing. Only the
  // focused one, so a repaint meant to rewrite another field still does.
  const typed =
    active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement ||
    active instanceof HTMLSelectElement
      ? {
          value: active.value,
          start: active instanceof HTMLSelectElement ? null : active.selectionStart,
          end: active instanceof HTMLSelectElement ? null : active.selectionEnd,
        }
      : null;
  return () => {
    const stops = overlayFocusables();
    const back = document.getElementById(id);
    const target = back && stops.includes(back) ? back : stops[0];
    if (
      typed &&
      target === back &&
      (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)
    ) {
      target.value = typed.value;
      // `selectionStart` is null on inputs without selection (`type="number"`).
      target.setSelectionRange?.(
        typed.start ?? typed.value.length,
        typed.end ?? typed.value.length,
      );
    }
    // Only a choice the list still offers: an absent value would set it to `""`, which
    // saves as "no model".
    if (typed && target === back && target instanceof HTMLSelectElement) {
      const offered = Array.from(target.options).some((o) => o.value === typed.value);
      if (offered) target.value = typed.value;
    }
    target?.focus();
  };
}

// The overlay memo: a modal's typed state lives only in the DOM, so an identical repaint
// must not swap it away.
let lastModalHtml: string | null = null;

function paintModal(): void {
  const html = modalHtml(state);
  if (html === lastModalHtml) return;
  const root = el("modal-root");
  const restore = captureModalFocus(root);
  root.innerHTML = html;
  lastModalHtml = html;
  restore?.();
}

// The note pane's memo. Cleared whenever edit mode owns the pane's DOM, so exiting repaints.
let lastNotePaneHtml: string | null = null;
// The tree pane's memo: an open create/rename input's typed name lives only in the DOM, so
// an identical repaint must not rebuild it under the cursor.
let lastTreePaneHtml: string | null = null;

/** Repaint the tree pane (memoized), carrying an open create/rename input across the swap.
 *  A fresh rename input gets its name selected, the platform rename affordance. */
function paintTree(): void {
  const html = treePaneHtml(state);
  if (html === lastTreePaneHtml) return;
  const carry = (id: string, open: boolean, selectAllOnFresh: boolean) => {
    const prev = document.getElementById(id) as HTMLInputElement | null;
    const saved =
      prev && open ? { value: prev.value, start: prev.selectionStart, end: prev.selectionEnd } : null;
    return () => {
      const input = document.getElementById(id) as HTMLInputElement | null;
      if (!input) return;
      if (saved) {
        input.value = saved.value;
        input.setSelectionRange(saved.start ?? saved.value.length, saved.end ?? saved.value.length);
      } else if (selectAllOnFresh) {
        input.select();
      }
      input.focus();
    };
  };
  const restoreCreate = carry("tree-create-input", state.treeCreate !== null, false);
  const restoreRename = carry("tree-rename-input", state.treeRename !== null, true);
  // Keep a keyboard user in the tree across the swap (K1), restored by path.
  const hadRowFocus = focusedTreeRow() !== null;
  el("tree-pane").innerHTML = html;
  lastTreePaneHtml = html;
  restoreCreate();
  restoreRename();
  if (hadRowFocus && !state.treeCreate && !state.treeRename) rovingRowEl()?.focus();
}

// The side pane's memo: identical HTML skips the swap, so scroll survives. Focus is carried
// by `capturePaneFocus` (K1, GH #91).
let lastSidePaneHtml: string | null = null;

function paintSide(): void {
  const html = sidePaneHtml(state);
  if (html === lastSidePaneHtml) return;
  const restore = capturePaneFocus(el("side-pane"));
  // The chat composer's half-typed question lives only in the DOM; carry it across.
  const carryInput = captureChatInput();
  el("side-pane").innerHTML = html;
  lastSidePaneHtml = html;
  carryInput();
  restore?.();
}

/** The chat composer's value and caret, as a restore thunk (a no-op when empty). */
function captureChatInput(): () => void {
  const prev = document.getElementById("chat-input") as HTMLTextAreaElement | null;
  if (!prev || prev.value === "") return () => {};
  const saved = { value: prev.value, start: prev.selectionStart, end: prev.selectionEnd };
  return () => {
    const next = document.getElementById("chat-input") as HTMLTextAreaElement | null;
    if (!next) return;
    next.value = saved.value;
    next.setSelectionRange(saved.start, saved.end);
  };
}

/**
 * Repaint the note pane (memoized, so scroll survives and the graph's entrance animation
 * plays only on real changes), restoring focus (GH #91). Returns whether it swapped: find
 * Ranges and syntax highlighting are re-derived only then.
 */
function paintNote(): boolean {
  const html = notePaneHtml(state);
  if (html === lastNotePaneHtml) return false;
  const restore = capturePaneFocus(el("note-pane"));
  el("note-pane").innerHTML = html;
  lastNotePaneHtml = html;
  restore?.();
  return true;
}

function render(): void {
  paintTree();
  // While editing (or in the frontmatter editor, GH #79) the pane belongs to the live
  // editor; a rebuild would destroy it mid-keystroke (crates/b2-desktop/CLAUDE.md).
  let noteSwapped = false;
  if (!state.editing && !state.fmEditing) noteSwapped = paintNote();
  else lastNotePaneHtml = null;
  // Graph mode owns the pane's box (no padding or scroll; the stage flexes to fill).
  el("note-pane").classList.toggle(
    "is-graph",
    state.graphOpen && !state.editing && state.current !== null && state.currentResource === null,
  );
  paintSide();
  // The "install the model" banner; empty (collapsed) when embedreminder.ts says not to.
  el("embed-banner").innerHTML = embedBannerHtml(state);
  el("menu-root").innerHTML = contextMenuHtml(state);
  paintModal();
  paintCmdSheet();
  el("vault-root").textContent = state.vaultRoot ?? "no vault";
  document.body.classList.toggle("is-loading", state.loading);
  paintReindex();
  paintNav();
  // Enabled with no vault open (it picks the first one) and during a reindex (the host
  // cancels the run), but not mid-op, to avoid re-entrant switches.
  (el("switch-vault") as HTMLButtonElement).disabled = state.loading;

  const toast = el("toast");
  if (state.status) {
    toast.textContent = state.status;
    toast.hidden = false;
  } else {
    toast.hidden = true;
  }
  syncOverlayFocus();
  syncFind(noteSwapped);
  if (noteSwapped) void paintCodeHighlights();
  // The open note's `![[image.png]]` embeds; while editing, `scheduleImageScan` owns this.
  if (!state.editing) void syncNoteImages(state.current?.path ?? null, state.current?.body ?? "");
}

/** Reading-view syntax highlighting (highlight.ts), a post-render pass. Grammars load
 *  lazily, so fences paint plain first; an open find bar re-derives its Ranges after. */
async function paintCodeHighlights(): Promise<void> {
  if (!(await highlightCodeBlocks(el("note-pane")))) return;
  if (findOpen && !state.editing) applyReadingFind();
}

// Paint just the reindex affordance, on every render and each streamed progress batch, so
// progress never rebuilds the panes. Writes every `.reindex-progress` on screen (top bar and
// Settings → Index) from one computation. The Reindex button exists only while Settings →
// Index is open, hence the null-tolerant lookup; auto-index runs repaint only this, so it
// must keep the button's state current.
function paintReindex(): void {
  const btn = document.getElementById("reindex") as HTMLButtonElement | null;
  if (btn) {
    btn.disabled = reindexDisabled(state);
    btn.textContent = reindexLabel(state);
  }

  const meters = [...document.querySelectorAll<HTMLElement>(".reindex-progress")];
  for (const wrap of meters) wrap.hidden = !state.reindexing;
  if (!state.reindexing) return;

  // Determinate only once embedding has a known denominator; before that the bar sweeps.
  const p = state.reindexProgress;
  const embedding = p && p.notes_to_embed > 0 ? p : null;
  const done = embedding ? `${embedding.notes_embedded}/${embedding.notes_to_embed}` : "";
  const label = state.reindexCancelling
    ? "Cancelling…"
    : embedding
      ? `Embedding ${done} · ${embedding.note_path.replace(/\.md$/, "")}`
      : "Indexing…";

  for (const wrap of meters) {
    const fill = wrap.querySelector<HTMLElement>(".reindex-fill");
    if (fill) {
      if (embedding) {
        const ratio = embedding.notes_embedded / embedding.notes_to_embed;
        const pct = Math.min(100, Math.round(ratio * 100));
        fill.classList.remove("is-indeterminate");
        fill.style.width = `${pct}%`;
      } else {
        fill.classList.add("is-indeterminate");
        fill.style.width = "";
      }
    }
    const text = wrap.querySelector<HTMLElement>(".reindex-label");
    if (text) text.textContent = label;
    const cancelBtn = wrap.querySelector<HTMLButtonElement>("[data-cancel-reindex]");
    if (cancelBtn) {
      cancelBtn.disabled = state.reindexCancelling;
      cancelBtn.textContent = state.reindexCancelling ? "Cancelling…" : "Cancel";
    }
  }
}

let statusTimer: number | undefined;
function flash(msg: string): void {
  state.status = msg;
  render();
  if (statusTimer) clearTimeout(statusTimer);
  statusTimer = window.setTimeout(() => {
    state.status = null;
    render();
  }, 4500);
}

// --- actions --------------------------------------------------------------------

/** Unfold every folder down to and including `dir` in the file tree. */
function revealDir(dir: string): void {
  for (const d of dirChain(dir)) state.expandedDirs.add(d);
}

// Load the file tree's listings, all fetched before any commit so a failure can't leave the
// tree half-updated. On failure, toasts and resolves false so callers don't flash success.
async function loadNotes(): Promise<boolean> {
  try {
    const notes = await api.listNotes();
    const resources = await api.listResources();
    const dirs = await api.listDirs();
    state.notes = notes;
    state.resources = resources;
    state.dirs = dirs;
    return true;
  } catch (e) {
    state.notes = [];
    state.resources = [];
    state.dirs = [];
    flash(errText(e));
    return false;
  }
}

/**
 * Load a note into the center pane: the core of `openNote` and back/forward (#52). `commit`
 * updates history with the canonical path as soon as the read succeeds, before discovery,
 * so rapid navigations stay ordered. Resolves false on a failed read (already toasted).
 */
async function loadNote(ref: string, commit: (path: string) => void): Promise<boolean> {
  state.loading = true;
  render();
  try {
    const note = await api.readNote(ref);
    state.current = note;
    state.explainCard = null; // Explain is about one card of the note it was opened on
    state.currentResource = null; // one document owns the pane
    state.resourceImage = null;
    state.fmEditing = false; // a new document ends any drawer edit (guards ran upstream)
    // Paint the note now; discovery is a slower, independent side-pane read.
    enterDocument(note.path, commit);
    state.loading = false;
    state.discoveringSimilar = true;
    state.discoveringConnections = true;
    render();
    await refreshDiscovery();
    return true;
  } catch (e) {
    flash(errText(e));
    return false;
  } finally {
    // Discovery flags belong to refreshDiscovery; clearing them here would race a newer
    // open.
    state.loading = false;
    render();
  }
}

/** What `loadNote` and `loadResource` share on entering a document: history, tree reveal,
 *  create context, and clearing the previous document's side pane. */
function enterDocument(path: string, commit: (path: string) => void): void {
  commit(path);
  revealDir(parentDir(path));
  state.selectedDir = parentDir(path); // the create context follows the selection
  resetSearch();
  clearDiscovery();
  state.collapsedCards.clear(); // per-note fold state belongs to the note we just left
  state.contextMenu = null;
}

/** Forget the side pane's discovery — the Similar list and the `explain` read. */
function clearDiscovery(): void {
  state.similar = [];
  clearConnections();
}

/** Forget the `explain` half of discovery: connections, resource links, unresolved. */
function clearConnections(): void {
  state.connections = [];
  state.resourceLinks = [];
  state.unresolved = [];
}

/** Adopt a note's `explain` read into the Connections section. */
function adoptExplain(explain: ExplainView): void {
  state.connections = explain.connections;
  state.resourceLinks = explain.resources;
  state.unresolved = explain.unresolved;
}

/** Put a resource card and its picture in the pane together, so the card never shows
 *  another file's picture. */
function adoptResource(resource: ResourceExplainView, picture: string | null): void {
  state.currentResource = resource;
  state.resourceImage = picture;
}

// User navigation to a note. Mid-edit, flushes and leaves edit mode first (a conflict keeps
// the editor instead). Records history (#52); back/forward call `loadNote` directly.
async function openNote(ref: string): Promise<void> {
  if (!(await leaveEdits())) return;
  await loadNote(ref, (path) => navPush({ kind: "note", path }));
}

// Follow a wikilink. A target can name a resource (`[[report.pdf]]`), routed by the same
// extension rule the core uses (`refKind`/`doc_kind`). Targets are vault-root, so minus any
// `#fragment` it is the resource's path; the host re-validates.
async function followWikilink(target: string): Promise<void> {
  if (refKind(target) === "resource") {
    await openResource(target.split("#")[0].trim());
  } else {
    await openNote(target);
  }
}

/**
 * The open resource's picture, or null: not an image, over `IMAGE_VIEWER_MAX_BYTES`, or the
 * read failed. A failed read still shows the card (with *Open in system default*) rather
 * than failing the navigation.
 */
async function loadResourceImage(r: ResourceExplainView): Promise<string | null> {
  if (r.class !== "image" || r.size > IMAGE_VIEWER_MAX_BYTES) return null;
  try {
    return imageDataUrl(r.path, await api.readResource(r.path));
  } catch {
    return null;
  }
}

// --- the note's inline pictures (`![[image.png]]`) ---------------------------------
//
// Reconciles which pictures the open document should hold (bytes via `read_resource`).
// Driven from the tail of `render()` rather than from each way a body reaches the pane, and
// memoized on the body, so it is cheap. `inlineImagePlan` (embeds.ts) bounds what is held.

/** The memo: owner, body and inventory the held pictures were planned from. The inventory
 *  is included because the file list can arrive after the first note (`loadVault`). */
let imagesOwner: string | null = null;
let imagesBody: string | null = null;
let imagesInventory: readonly ResourceSummary[] | null = null;
/** The current plan's generation. Reads store only if still current, or a picture removed
 *  mid-read would come back and escape the budget. */
let imagesGeneration = 0;
/** Debounce for the buffer scan while editing. */
let imageScanTimer: number | undefined;
const IMAGE_SCAN_MS = 400;

/** Hand the live editor the pictures the note holds now (a no-op when not editing). */
function pushNoteImages(): void {
  // A copy: editor state must not change under CodeMirror without a transaction.
  editorView?.dispatch({ effects: setEmbedImages.of(new Map(state.embedImages)) });
}

/**
 * Reconcile `state.embedImages` against `body` (the note's, or the live buffer). A null
 * `owner` drops every picture. Repaints only after a picture arrives, so it is safe from
 * the tail of `render()`.
 */
async function syncNoteImages(owner: string | null, body: string): Promise<void> {
  if (imagesOwner === owner && imagesBody === body && imagesInventory === state.resources) return;
  imagesOwner = owner;
  imagesBody = owner === null ? null : body;
  imagesInventory = state.resources;
  const generation = ++imagesGeneration;
  const plan = owner === null ? [] : inlineImagePlan(imageEmbedTargets(body), state.resources);
  const keep = new Set(plan);
  for (const path of [...state.embedImages.keys()]) {
    if (!keep.has(path)) state.embedImages.delete(path);
  }
  const missing = plan.filter((path) => !state.embedImages.has(path));
  if (missing.length === 0) return;
  // A failed read stays silent: the embed still reads as a working link.
  const loaded = await Promise.all(
    missing.map(async (path) => {
      try {
        return [path, imageDataUrl(path, await api.readResource(path))] as const;
      } catch {
        return [path, null] as const;
      }
    }),
  );
  // Superseded while reading; the newer plan requested its own.
  if (imagesGeneration !== generation) return;
  let arrived = false;
  for (const [path, url] of loaded) {
    if (url !== null) {
      state.embedImages.set(path, url);
      arrived = true;
    }
  }
  if (!arrived) return;
  pushNoteImages();
  render();
}

/** While editing, reconcile pictures against the buffer rather than the saved body. */
function scheduleImageScan(): void {
  window.clearTimeout(imageScanTimer);
  imageScanTimer = window.setTimeout(() => {
    const n = state.current;
    if (!n || !editorView) return;
    void syncNoteImages(n.path, editorView.state.doc.toString());
  }, IMAGE_SCAN_MS);
}

/** `loadNote` for resources. Resources have no discovery yet, so the side pane clears. */
async function loadResource(path: string, commit: (path: string) => void): Promise<boolean> {
  state.loading = true;
  render();
  try {
    const resource = await api.explainResource(path);
    adoptResource(resource, await loadResourceImage(resource));
    state.current = null;
    enterDocument(resource.path, commit);
    state.discoveringSimilar = false;
    state.discoveringConnections = false;
    return true;
  } catch (e) {
    flash(errText(e));
    return false;
  } finally {
    state.loading = false;
    render();
  }
}

// Open a resource's fallback card (spec §6): `openNote`'s sibling.
async function openResource(path: string): Promise<void> {
  if (!(await leaveEdits())) return;
  await loadResource(path, (p) => navPush({ kind: "resource", path: p }));
}

// --- navigation history (#52) -----------------------------------------------------
//
// Browser-style back/forward over the center pane's documents. Session-scoped: never
// persisted, cleared on vault switch. Kept out of AppState; the buttons repaint via
// `paintNav`. In-place updates (save, external edit) bypass `openNote`, so add no entries.

/** One center-pane document: what `loadNote`/`loadResource` can bring back. */
interface NavEntry {
  kind: "note" | "resource";
  path: string;
}

/** Cap the stack so an all-day browse can't grow it unbounded. */
const NAV_MAX = 100;

let navStack: NavEntry[] = [];
/** Index of the pane's current document in `navStack`; -1 while it's empty. */
let navCursor = -1;

// Record a navigation: drop the forward branch, then append. Called after a successful
// read with the canonical path, so failed targets never enter and duplicates collapse.
function navPush(entry: NavEntry): void {
  const cur = navStack[navCursor];
  if (cur && cur.kind === entry.kind && cur.path === entry.path) return;
  navStack.splice(navCursor + 1);
  navStack.push(entry);
  if (navStack.length > NAV_MAX) navStack.shift();
  navCursor = navStack.length - 1;
  paintNav();
}

/** Vault switch: the stack's paths are meaningless in the new vault. */
function navClear(): void {
  navStack = [];
  navCursor = -1;
  paintNav();
}

/** True while a text field owns the keyboard, where ⌘←/⌘→ move the caret, not history. */
function inTextEntry(): boolean {
  const a = document.activeElement;
  return (
    a instanceof HTMLInputElement ||
    a instanceof HTMLTextAreaElement ||
    (a instanceof HTMLElement && a.isContentEditable)
  );
}

// --- keyboard: focus plumbing (invariant K1, GH #78) --------------------------------
//
// B2 is fully operable from the keyboard (K1): the tree is an ARIA `tree`, overlays take and
// return focus, and every mouse gesture has a key. Chords are wired in `wireChords`; this
// is their shared focus bookkeeping.

/** Every row the tree currently paints, in paint order — the list the arrows walk. */
function treeRows(): TreeRow[] {
  return visibleRows(buildTree(state.notes, state.resources, state.dirs), state.expandedDirs);
}

/** A file-tree row; each carries its vault path in `data-tree-row` (render.ts). */
const TREE_ROW = ".tree-row[data-tree-row]";

/** The DOM row for a vault path. */
function treeRowEl(path: string | null): HTMLElement | null {
  return findByData<HTMLElement>(el("tree-pane"), TREE_ROW, "treeRow", path);
}

/** The first `selector` match under `root` whose `data-*` field `key` is `value`. Iterated,
 *  not a selector, because the value holds a path, which may contain anything. */
function findByData<E extends HTMLElement | SVGElement>(
  root: Element,
  selector: string,
  key: string,
  value: string | null,
): E | null {
  if (value === null) return null;
  for (const node of root.querySelectorAll<E>(selector)) if (node.dataset[key] === value) return node;
  return null;
}

/** The row carrying the roving tabstop, as painted (treenav.ts `rovingPath`). */
function rovingRowEl(): HTMLElement | null {
  return el("tree-pane").querySelector<HTMLElement>('.tree-row[tabindex="0"]');
}

/** The tree row the keyboard is on right now, or null when focus is elsewhere. */
function focusedTreeRow(): HTMLElement | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return null;
  return active.closest<HTMLElement>(`#tree-pane ${TREE_ROW}`);
}

/** A tree row element as the node ref that rename / move / delete all speak. */
function treeRowRef(row: HTMLElement): TreeNodeRef {
  const path = row.dataset.treeRow ?? "";
  const nodeKind: NodeKind =
    row.dataset.dir !== undefined
      ? "folder"
      : row.dataset.openResource !== undefined
        ? "resource"
        : "note";
  return { path, nodeKind, label: baseName(path) };
}

/** Focus a tree row: state first (so the roving tabstop moves), repaint, then the DOM. */
function focusTreeRow(path: string): void {
  state.treeFocus = path;
  paintTree();
  const row = treeRowEl(path);
  row?.focus();
  row?.scrollIntoView({ block: "nearest" });
}

// --- keyboard: the discovery pane's rows (sidenav.ts) --------------------------------

/** The DOM row for a `sidenav.ts` row key. */
function sideRowEl(key: string | null): HTMLElement | null {
  return findByData<HTMLElement>(el("side-pane"), "[data-side-row]", "sideRow", key);
}

/** The graph node for a scene id (graph.ts `GraphNode.id`). */
function gnodeEl(id: string | null): SVGElement | null {
  return findByData<SVGElement>(el("note-pane"), "[data-gnode]", "gnode", id);
}

/** The row carrying the roving tabstop, as painted (sidenav.ts `rovingSideKey`). */
function rovingSideRowEl(): HTMLElement | null {
  return el("side-pane").querySelector<HTMLElement>('[data-side-row][tabindex="0"]');
}

/** Focus a discovery row; `focusTreeRow`'s counterpart. */
function focusSideRow(key: string): void {
  state.sideFocus = key;
  paintSide();
  const row = sideRowEl(key);
  row?.focus();
  row?.scrollIntoView({ block: "nearest" });
}

/** Put the keyboard in the file tree (⌘1) — on the row it last left off at. */
function focusTreePane(): void {
  const row = rovingRowEl();
  if (row) row.focus();
  else el("tree-pane").focus();
}

/** Put the keyboard in the note (⌘2): the editor while editing, else the scrolling pane. */
function focusNotePane(): void {
  if (state.editing && editorView) {
    editorView.focus();
    return;
  }
  el("note-pane").focus();
}

/** Put the keyboard in discovery (⌘3): the roving row, else the first button, else the
 *  pane. */
function focusSidePane(): void {
  const pane = el("side-pane");
  const row = rovingSideRowEl();
  const first = pane.querySelector<HTMLElement>("button:not([disabled])");
  (row ?? first ?? pane).focus();
}

// --- keyboard: overlay focus (K1) ---------------------------------------------------
//
// Each overlay takes focus on open, traps Tab, and returns focus on close. Focus moves only
// on the open/close edge; moving it on every render would fight the user's own Tab.

type OverlayKind = "settings" | "move" | "delete" | "link" | "menu" | null;

/**
 * Which overlay is up, in `modalHtml`'s precedence; global chords check it before acting.
 * Exactly one is ever up (a menu → Move… replaces, not covers), so this is a value, not a
 * stack.
 */
function currentOverlay(): OverlayKind {
  if (state.settingsOpen) return "settings";
  if (state.moveTarget) return "move";
  if (state.deleteTarget) return "delete";
  if (state.linkTarget) return "link";
  if (state.contextMenu) return "menu";
  return null;
}

/**
 * Take down every overlay so the caller's is the only one up. ⌘, is unguarded, so without
 * this a hidden Move… would reappear when Settings closes. Clears targets only; nothing is
 * committed.
 */
function dismissOverlays(): void {
  state.contextMenu = null;
  state.settingsOpen = false;
  clearRecorder(); // the chord recorder lives inside Settings and goes with it
  state.moveTarget = null;
  state.deleteTarget = null;
  state.linkTarget = null;
}

// --- the ⌘-hold sheet ----------------------------------------------------------------
//
// The DOM half of cmdhold.ts: its timer, and listeners feeding its inputs. They listen in
// the capture phase on `window` and never `preventDefault`: a spectator that sees even
// events other handlers stop, and never affects whether a chord fires.

let holdPhase: HoldPhase = "idle";
let holdTimer: number | null = null;

/** The ⌘ sheet's paint (nothing in it is focusable, so no memo or focus handling). The hold
 *  calls only this: a full `render()` in a capture listener would swap the DOM under an
 *  event still in flight. */
function paintCmdSheet(): void {
  el("cmdhold-root").innerHTML = cmdSheetHtml(state);
}

/** Feed the machine one event, manage the timer, and repaint only if `open` changed. */
function cmdHoldEvent(e: HoldEvent): void {
  const step = holdStep(holdPhase, e);
  if (step.timer !== "keep" && holdTimer !== null) {
    clearTimeout(holdTimer);
    holdTimer = null;
  }
  if (step.timer === "start") {
    holdTimer = window.setTimeout(() => {
      holdTimer = null;
      cmdHoldEvent({ kind: "elapsed" });
    }, HOLD_MS);
  }
  holdPhase = step.phase;
  const open = step.phase === "open";
  if (state.cmdSheet === open) return;
  state.cmdSheet = open;
  paintCmdSheet();
}

function wireCmdHold(): void {
  window.addEventListener(
    "keydown",
    (e) => {
      // A bare ⌘ (⇧⌘ is the start of a chord). Refused while an overlay is up: Settings
      // already is the reference, and the recorder is listening for chords.
      if (e.key === "Meta" && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        if (currentOverlay() === null) cmdHoldEvent({ kind: "hold", repeat: e.repeat });
        return;
      }
      cmdHoldEvent({ kind: "other" });
    },
    true,
  );
  window.addEventListener(
    "keyup",
    (e) => {
      if (e.key === "Meta") cmdHoldEvent({ kind: "release" });
    },
    true,
  );
  // A ⌘-click or ⌘-drag is a gesture, not a hold.
  window.addEventListener("pointerdown", () => cmdHoldEvent({ kind: "other" }), true);
  // macOS never delivers the ⌘ keyup after ⌘⇥, Spotlight or Hide, so treat losing the
  // window as a release. Kept separate from `wireWindowBlur`'s listener.
  window.addEventListener("blur", () => cmdHoldEvent({ kind: "release" }));
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) cmdHoldEvent({ kind: "release" });
  });
}

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

/** The overlay's focusable controls in DOM order: the Tab cycle, first one focused on open.
 *  Menu items are `tabindex=-1` (arrow-navigated), so they're collected by class. */
function overlayFocusables(): HTMLElement[] {
  // `[role="dialog"]` covers both the `.modal` box and Settings' `.settings-screen`.
  const modal = document.querySelector<HTMLElement>('#modal-root [role="dialog"]');
  // `tabIndex >= 0` keeps roving tabstops (Settings' rail) to one Tab stop; the selector
  // alone matches `tabindex="-1"` buttons.
  if (modal) {
    return [...modal.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((n) => n.tabIndex >= 0);
  }
  const menu = document.querySelector<HTMLElement>("#menu-root .context-menu");
  return menu ? [...menu.querySelectorAll<HTMLElement>(".context-item")] : [];
}

/**
 * The last element that received focus, tracked via `focusin` (`wireFocusMemory`). By the
 * time `render()` handles an overlay's open edge, the trigger's DOM is gone and
 * `activeElement` is `<body>`.
 */
let lastFocused: HTMLElement | SVGElement | null = null;

/**
 * A thunk that returns focus to where it was before an overlay opened. The trigger rarely
 * survives the repaint, so restore by a stable identity (tree path, side-row key, graph
 * node id, element id), and only last by the element itself.
 */
function captureReturnFocus(): (() => void) | null {
  const active = lastFocused;
  if (active === null || active === document.body) return null;
  // Unscoped: `active` is usually detached by now, so `#tree-pane …` would match nothing.
  const row = active.closest<HTMLElement>(TREE_ROW);
  if (row) {
    const path = row.dataset.treeRow ?? null;
    return () => (treeRowEl(path) ?? rovingRowEl())?.focus();
  }
  // A discovery row, by key (committing a link re-runs discovery and detaches it).
  const side = active.closest<HTMLElement>("[data-side-row]");
  if (side) {
    const key = side.dataset.sideRow ?? null;
    return () => (sideRowEl(key) ?? rovingSideRowEl())?.focus();
  }
  // A graph node, by id. A linked ghost (`ghost:<path>`) becomes the authored node `<path>`,
  // so fall back to that, then to the pane.
  const gnode = active.closest<SVGElement>("[data-gnode]");
  if (gnode) {
    const id = gnode.dataset.gnode ?? null;
    const linked = id?.startsWith("ghost:") === true ? id.slice("ghost:".length) : null;
    return () => (gnodeEl(id) ?? gnodeEl(linked) ?? el("note-pane")).focus();
  }
  if (active.id) {
    const id = active.id;
    return () => document.getElementById(id)?.focus();
  }
  return () => {
    if (active.isConnected) active.focus();
  };
}

// The return target, captured when the overlay chain starts (menu → Move… → back to row).
let overlayShowing: OverlayKind = null;
let overlayReturn: (() => void) | null = null;

/** Move focus into the overlay that just opened. */
function focusIntoOverlay(kind: OverlayKind): void {
  // The link modal opens on its explanation field; Settings on its selected tab.
  const preferred = kind === "link" ? document.getElementById("link-explanation") : null;
  (preferred ?? overlayFocusables()[0])?.focus();
}

/** Called at the end of every `render()`; acts only on open/close edges, so a repaint
 *  never yanks focus mid-Tab (`paintModal` handles repaints while open). */
function syncOverlayFocus(): void {
  const overlay = currentOverlay();
  if (overlay === overlayShowing) return;
  if (overlayShowing === null) overlayReturn = captureReturnFocus();
  overlayShowing = overlay;
  if (overlay === null) {
    const back = overlayReturn;
    overlayReturn = null;
    // Not when the menu just opened an inline tree input (Rename, New note).
    if (!state.treeCreate && !state.treeRename) back?.();
  } else {
    focusIntoOverlay(overlay);
  }
}

// Paint just the Back/Forward buttons' enabled state (disabled at the ends and mid-op).
function paintNav(): void {
  const back = document.getElementById("nav-back") as HTMLButtonElement | null;
  const forward = document.getElementById("nav-forward") as HTMLButtonElement | null;
  if (back) back.disabled = state.loading || navCursor <= 0;
  if (forward) forward.disabled = state.loading || navCursor >= navStack.length - 1;
}

// Back (-1) / Forward (+1), through the same edit-mode guard as any navigation. The cursor
// commits on read success, like a push. A dead target is toasted and dropped from the stack.
async function navGo(delta: -1 | 1): Promise<void> {
  if (state.loading) return;
  // Guards first (they can await a save); cursor math against the stack after.
  if (!(await leaveEdits())) return;
  const target = navCursor + delta;
  if (target < 0 || target >= navStack.length) return;
  const entry = navStack[target];
  const commit = () => {
    navCursor = target;
    paintNav();
  };
  const ok =
    entry.kind === "note"
      ? await loadNote(entry.path, commit)
      : await loadResource(entry.path, commit);
  if (!ok) {
    // By identity: the stack may have shifted during the await.
    const i = navStack.indexOf(entry);
    if (i !== -1) {
      navStack.splice(i, 1);
      if (i < navCursor) navCursor -= 1;
    }
    paintNav();
  }
}

function toggleDir(path: string): void {
  if (state.expandedDirs.has(path)) state.expandedDirs.delete(path);
  else state.expandedDirs.add(path);
  state.selectedDir = path; // clicking a folder also makes it the create context
  render();
}

function toggleFrontmatter(): void {
  if (state.fmEditing) return; // the toggle is disabled while the mini-editor is live
  state.frontmatterOpen = !state.frontmatterOpen;
  render();
}

// --- frontmatter mini-editor (GH #79) ---------------------------------------------
//
// Raw YAML in a textarea with explicit Save/Cancel (no autosave: half-typed YAML is
// invalid). The pane is under the render carve-out while live, so the buffer lives in the
// DOM. The façade refuses a `---` line (E3); unreadable YAML saves and is flagged.

function enterFmEdit(): void {
  const n = state.current;
  if (!n || state.editing || state.fmEditing || state.loading) return;
  state.fmEditing = true;
  state.frontmatterOpen = true; // the editor lives in the open drawer
  // One explicit paint with the editor; render() then leaves the pane alone.
  el("note-pane").innerHTML = notePaneHtml(state);
  render();
  // render() skips the highlight pass for a carved-out pane, so run it here.
  void paintCodeHighlights();
  (document.getElementById("fm-editor") as HTMLTextAreaElement | null)?.focus();
}

/** The live buffer, or null when the mini-editor isn't mounted. */
function fmBuffer(): string | null {
  const ta = document.getElementById("fm-editor");
  return ta instanceof HTMLTextAreaElement ? ta.value : null;
}

/** Cancel (Esc, button, or Reload): drop the buffer and re-read disk, since reconcile
 *  deferred any external edit while the editor was live. */
async function cancelFmEdit(): Promise<void> {
  if (!state.fmEditing) return;
  state.fmEditing = false;
  render(); // instant: back to the read-only peek
  const n = state.current;
  if (!n) return;
  try {
    const fresh = await api.readNote(n.path);
    // Adopt only if this note still owns the pane and no new edit began meanwhile.
    if (state.current?.path === n.path && !state.fmEditing && !state.editing) {
      state.current = fresh;
      render();
    }
  } catch {
    // The note may be gone (an external delete) — the watcher's reconcile owns that.
  }
}

/** Resolve the mini-editor before anything repaints the note pane: a pristine buffer
 *  closes, a dirty one blocks (typed YAML is never silently discarded). */
function fmEditGuard(): boolean {
  if (!state.fmEditing) return true;
  const buf = fmBuffer();
  if (buf === null || buf === (state.current?.frontmatter ?? "")) {
    state.fmEditing = false;
    return true;
  }
  flash("Finish editing the frontmatter first — Save it, or press Esc to discard.");
  return false;
}

async function saveFmEdit(): Promise<void> {
  const n = state.current;
  const buf = fmBuffer();
  if (!n || !state.fmEditing || buf === null) return;
  try {
    await api.writeFrontmatter(n.path, buf, n.revision);
    await finishFmSave(n.path);
  } catch (e) {
    showFmError(errText(e), isWriteConflict(e));
  }
}

// After a save: leave edit mode, re-read from disk (revision, metadata, readability), then
// refresh discovery, since `b2_relations:` may have changed.
async function finishFmSave(path: string): Promise<void> {
  state.fmEditing = false;
  try {
    const fresh = await api.readNote(path);
    if (state.current?.path === path) state.current = fresh;
  } catch (e) {
    flash(errText(e));
    render();
    return;
  }
  render();
  flash("Frontmatter saved.");
  void refreshDiscovery();
}

// The conflict bar's "Keep mine": re-read for the fresh revision, then overwrite it.
async function fmConflictKeepMine(): Promise<void> {
  const n = state.current;
  const buf = fmBuffer();
  if (!n || !state.fmEditing || buf === null) return;
  try {
    const fresh = await api.readNote(n.path);
    await api.writeFrontmatter(n.path, buf, fresh.revision);
    await finishFmSave(n.path);
  } catch (e) {
    showFmError(errText(e), isWriteConflict(e));
  }
}

/** Paint the inline error under the buffer; `conflict` also reveals Reload/Keep mine. */
function showFmError(msg: string, conflict: boolean): void {
  const box = document.getElementById("fm-error");
  const text = document.getElementById("fm-error-text");
  const actions = document.getElementById("fm-conflict-actions");
  if (!box || !text || !actions) return;
  text.textContent = msg;
  actions.hidden = !conflict;
  box.hidden = false;
}

/** Typing again clears the failed save's message. */
function hideFmError(): void {
  const box = document.getElementById("fm-error");
  if (box) box.hidden = true;
}

// Fold a whole discovery section; sticky across notes.
function toggleSection(section: SideSection): void {
  if (state.collapsedSections.has(section)) state.collapsedSections.delete(section);
  else state.collapsedSections.add(section);
  render();
}

// Fold a card to its title row. Per-note, keyed `"<section>:<path>"`.
function toggleCard(key: string): void {
  if (state.collapsedCards.has(key)) state.collapsedCards.delete(key);
  else state.collapsedCards.add(key);
  render();
}

// --- context menus (discovery cards + the file tree) ------------------------------
//
// Anchored at the cursor and clamped to the viewport. The heights below feed only the
// clamp; they may over-estimate but must not under-estimate.
const CTX_MENU_W = 168;
const CARD_MENU_H = 140; // Open note / Link… / Explain this suggestion / Why was this suggested?
/** Plus *Insert link at cursor*, added while editing. */
const CARD_EDIT_MENU_H = 172;
const TREE_MENU_H = 132; // the context line + three items
// + Rename / Move… / Copy vault path / Copy system path / Delete and their separator.
const TREE_NODE_MENU_H = 300;

function clampMenu(clientX: number, clientY: number, height: number): { x: number; y: number } {
  const x = Math.min(clientX, window.innerWidth - CTX_MENU_W - 8);
  const y = Math.min(clientY, window.innerHeight - height - 8);
  return { x: Math.max(8, x), y: Math.max(8, y) };
}

function openCardMenu(clientX: number, clientY: number, path: string, title: string): void {
  const { x, y } = clampMenu(clientX, clientY, state.editing ? CARD_EDIT_MENU_H : CARD_MENU_H);
  state.contextMenu = { kind: "card", x, y, path, title: title || null };
  render();
}

function openTreeMenu(
  clientX: number,
  clientY: number,
  dir: string,
  node: TreeNodeRef | null = null,
): void {
  const { x, y } = clampMenu(clientX, clientY, node ? TREE_NODE_MENU_H : TREE_MENU_H);
  state.contextMenu = { kind: "tree", x, y, dir, node };
  render();
}

function closeContextMenu(): void {
  if (!state.contextMenu) return;
  state.contextMenu = null;
  render();
}

/**
 * A right-click menu item. `closeFirst` closes the menu before acts that don't open a
 * surface of their own (a copy, an OS picker, a navigation); the others clear it themselves.
 */
interface MenuItem<A> {
  readonly attr: string;
  readonly closeFirst: boolean;
  readonly act: (arg: A) => void;
}

type CardMenu = Extract<ContextMenuState, { kind: "card" }>;

/** The tree menu's items that act on the row it was opened over. */
const TREE_ROW_ITEMS: readonly MenuItem<TreeNodeRef>[] = [
  { attr: "data-ctx-rename", closeFirst: false, act: startTreeRename },
  { attr: "data-ctx-move", closeFirst: false, act: openMoveModal },
  { attr: "data-ctx-copy-vault-path", closeFirst: true, act: (node) => void copyPath(node.path) },
  {
    attr: "data-ctx-copy-system-path",
    closeFirst: true,
    // The menu needs a vault to open, but never invent a root.
    act: (node) => {
      if (state.vaultRoot !== null) void copyPath(systemPath(state.vaultRoot, node.path));
    },
  },
  { attr: "data-ctx-delete", closeFirst: false, act: requestDelete },
];

/** The tree menu's items that act on the folder it was opened in. */
const TREE_DIR_ITEMS: readonly MenuItem<string>[] = [
  { attr: "data-ctx-new-note", closeFirst: false, act: (dir) => startTreeCreate("note", dir) },
  { attr: "data-ctx-new-folder", closeFirst: false, act: (dir) => startTreeCreate("folder", dir) },
  // The OS picker is modal, so the menu goes first.
  { attr: "data-ctx-import", closeFirst: true, act: (dir) => void pickAndImport(dir) },
];

/** A discovery card's (or graph ghost's) items. */
const CARD_ITEMS: readonly MenuItem<CardMenu>[] = [
  { attr: "data-ctx-open", closeFirst: true, act: (m) => void openNote(m.path) },
  // The drag's keyboard half — aimed at the caret's line.
  { attr: "data-ctx-insert", closeFirst: true, act: (m) => insertCardLink(m.path) },
  { attr: "data-ctx-link", closeFirst: true, act: (m) => openLinkModal(m.path, m.title ?? "") },
  { attr: "data-ctx-explain", closeFirst: true, act: (m) => void openExplain(m.path) },
  { attr: "data-ctx-why", closeFirst: true, act: (m) => void askWhy({ path: m.path, title: m.title }) },
];

/** A click while a menu is up: its items act; any other click dismisses it. */
function contextMenuClick(target: HTMLElement): void {
  const menu = state.contextMenu;
  if (!menu) return;
  const chose = <A>(items: readonly MenuItem<A>[], arg: A): boolean => {
    const item = items.find((i) => target.closest(`[${i.attr}]`) !== null);
    if (!item) return false;
    if (item.closeFirst) closeContextMenu();
    item.act(arg);
    return true;
  };
  if (menu.kind === "tree") {
    if (menu.node && chose(TREE_ROW_ITEMS, menu.node)) return;
    if (chose(TREE_DIR_ITEMS, menu.dir)) return;
  } else if (chose(CARD_ITEMS, menu)) {
    return;
  }
  closeContextMenu();
}

// --- tree creation: new note / new folder (left nav) ------------------------------
//
// The tree-head icons, ⌘N / ⇧⌘N and the tree menu create in `state.selectedDir` via an
// inline input (Enter commits, Escape cancels, blur commits a non-empty name). A note is
// written and projected at once; its vectors fill via autosave's trailing embed. A folder
// is a real `mkdir`.

function startTreeCreate(kind: "note" | "folder", dir: string): void {
  if (state.vaultRoot === null) return;
  state.contextMenu = null;
  state.treeCreate = { kind, dir };
  revealDir(dir); // reveal the target folder
  render(); // paintTree focuses the fresh input
}

function cancelTreeCreate(): void {
  if (!state.treeCreate) return;
  state.treeCreate = null;
  render();
}

/** Commit the inline input's name. `open` (Enter) opens the note in edit mode; a blur
 *  commit creates quietly and leaves the click's navigation alone. */
async function commitTreeCreate(raw: string, open: boolean): Promise<void> {
  const create = state.treeCreate;
  if (!create) return;
  const name = normalizeName(raw);
  if (name === null) {
    cancelTreeCreate(); // an empty (or traversal) name is a back-out, not an error
    return;
  }
  const path = joinPath(create.dir, name);
  state.treeCreate = null;
  if (create.kind === "folder") {
    try {
      const report = await api.createDir(path);
      const refreshed = await loadNotes(); // re-lists structure from disk — the folder is real
      revealDir(report.dir);
      state.selectedDir = report.dir; // the natural next step is a note inside it
      if (refreshed) flash(`Created ${report.dir}/.`);
      else render(); // the refresh failure already toasted; still repaint the expansion
    } catch (e) {
      // Refused: keep the input open with the typed name; the toast explains.
      state.treeCreate = create;
      flash(errText(e));
    }
    return;
  }
  try {
    const report = await api.createNote(path);
    const refreshed = await loadNotes(); // the tree lists it now — create_note already projected it
    void refreshEmbedStatus(state.vaultRoot); // the N/M denominator grew (#26)
    if (open) {
      await openNote(report.path); // sets selectedDir to the new note's folder
      enterEdit(); // a fresh, empty note wants a cursor, not a reading view
    } else if (refreshed) {
      flash(`Created ${report.path}.`); // a failed refresh already toasted — don't overwrite it
    }
  } catch (e) {
    // Refused: keep the input open (the unchanged tree HTML keeps the typed name).
    state.treeCreate = create;
    flash(errText(e));
  }
}

// --- tree import: files from outside the vault ------------------------------------
//
// Drag files onto a folder row, or pick them from the tree menu (the keyboard path, K1).
// Both copy verbatim and project via `Vault::import_file`/`import_path`. A drop yields
// bytes (WebKit gives no path), sent as capped base64 (importfiles.ts); the picker yields
// paths the host reads itself.

/** An import is running — further gestures are ignored (no queueing, like moves). */
let importInFlight = false;

/** One dropped entry: the plan's metadata plus the handle to read it with. */
interface DroppedFile {
  name: string;
  size: number;
  isDirectory: boolean;
  file: File | null;
}

/**
 * Read a drop's entries synchronously: the DataTransfer is neutered once the handler
 * returns. `items` rather than `files`, to detect folders (`webkitGetAsEntry`), which the
 * plan refuses by name.
 */
function droppedFiles(dt: DataTransfer | null): DroppedFile[] {
  if (!dt) return [];
  const out: DroppedFile[] = [];
  for (const item of Array.from(dt.items)) {
    if (item.kind !== "file") continue;
    const entry = item.webkitGetAsEntry?.() ?? null;
    if (entry?.isDirectory) {
      out.push({ name: entry.name, size: 0, isDirectory: true, file: null });
      continue;
    }
    const file = item.getAsFile();
    if (file) out.push({ name: file.name, size: file.size, isDirectory: false, file });
  }
  // If `items` yielded nothing, fall back to the file list.
  if (out.length === 0) {
    for (const file of Array.from(dt.files)) {
      out.push({ name: file.name, size: file.size, isDirectory: false, file });
    }
  }
  return out;
}

/** Shared refusal gate: an import writes, so it queues behind the same runs a move does. */
function canImportNow(): boolean {
  if (importInFlight || state.vaultRoot === null) return false;
  return !refusedWhileIndexing("import");
}

/** The drop half: send each accepted file's bytes, then report once. */
async function importDroppedFiles(dir: string, dropped: DroppedFile[]): Promise<void> {
  if (!canImportNow()) return;
  const plan = planImport(dropped);
  const refused = [...plan.refused];
  const imported: string[] = [];
  importInFlight = true;
  try {
    // Sequential: one refusal must not cancel the rest, and reports keep drop order.
    for (const entry of plan.accepted) {
      if (!entry.file) continue;
      try {
        const bytes = new Uint8Array(await entry.file.arrayBuffer());
        const report = await api.importFile(dir, entry.name, bytesToBase64(bytes));
        imported.push(report.path);
      } catch (e) {
        refused.push(`${entry.name}: ${errText(e)}`);
      }
    }
    // Inside the gate, so two imports can't re-list and toast out of order.
    await finishImport(dir, imported, refused);
  } finally {
    importInFlight = false;
  }
}

/** The keyboard half: an OS picker, then the same import by path. */
async function pickAndImport(dir: string): Promise<void> {
  if (!canImportNow()) return;
  let picked: string[];
  try {
    picked = await api.pickImportFiles();
  } catch (e) {
    flash(errText(e));
    return;
  }
  if (picked.length === 0) return; // cancelled — say nothing
  const refused: string[] = [];
  const imported: string[] = [];
  importInFlight = true;
  try {
    for (const source of picked) {
      try {
        imported.push((await api.importPath(dir, source)).path);
      } catch (e) {
        refused.push(`${baseName(source)}: ${errText(e)}`);
      }
    }
    await finishImport(dir, imported, refused); // inside the gate, as above
  } finally {
    importInFlight = false;
  }
}

/** After an import: reveal, re-list, and toast once. Schedules the trailing embed, since
 *  imported notes are projected but unembedded. */
async function finishImport(dir: string, imported: string[], refused: string[]): Promise<void> {
  if (imported.length > 0) {
    revealDir(dir);
    state.selectedDir = dir;
    await loadNotes();
    void refreshEmbedStatus(state.vaultRoot); // the N/M denominator grew (#26)
    scheduleTrailingEmbed();
  }
  flash(importSummary(dir, imported, refused));
}

// --- tree move / rename (context menu, Move… modal, drag-and-drop) -----------------
//
// All three gestures resolve a destination (move.ts) and call one executor; the host
// rewrites inbound links. Rename changes the file path, like `b2 mv` (data-model.md §1).
// The open document is re-pointed before the watcher's pulse arrives, or reconcile would
// report our own move as "moved or removed".

/** Refuse (and say so) a write that would race a live index run; true when refused. */
function refusedWhileIndexing(what: string): boolean {
  if (!state.reindexing) return false;
  flash(`Indexing is running — try the ${what} again when it finishes.`);
  return true;
}

/** A move/rename is in flight — further gestures are ignored (no queueing in v1). */
let moveInFlight = false;

function startTreeRename(node: TreeNodeRef): void {
  state.contextMenu = null;
  state.treeRename = node;
  revealDir(parentDir(node.path));
  render(); // paintTree focuses the input and selects the prefilled name
}

function cancelTreeRename(): void {
  if (!state.treeRename) return;
  state.treeRename = null;
  render();
}

async function commitTreeRename(raw: string): Promise<void> {
  const node = state.treeRename;
  if (!node) return;
  const dest = renameDestination(node.path, node.nodeKind, raw);
  if (dest === null) {
    cancelTreeRename(); // empty / traversal / unchanged — a back-out, not an error
    return;
  }
  // The input stays open while the move runs, so a refusal keeps the typed name.
  const ok = await executeMove(node, dest);
  if (ok) {
    state.treeRename = null;
    render();
  }
}

function openMoveModal(node: TreeNodeRef): void {
  state.contextMenu = null;
  state.moveTarget = node;
  render();
}

/** The shared move executor; true on success. Refuses during a reindex (it would load a
 *  second model) and while another move is in flight. */
async function executeMove(node: TreeNodeRef, to: string): Promise<boolean> {
  if (moveInFlight) return false;
  if (refusedWhileIndexing("move")) return false;
  // Close an affected editor first so no save targets the old path (conflict aborts).
  const curPath = openDocPath(state);
  const affected =
    curPath !== null &&
    (node.nodeKind === "folder" ? remapPath(curPath, node.path, to) !== null : curPath === node.path);
  // Not `leaveEdits`: with no editor open this must not yield before the in-flight flag
  // is set below, or a second gesture could slip past it.
  if (affected && !fmEditGuard()) return false;
  if (affected && state.editing && !(await closeEditor())) return false;

  moveInFlight = true;
  if (node.nodeKind === "folder") flash(`Moving ${node.path}/…`);
  try {
    // One report shape for all three kinds.
    const move =
      node.nodeKind === "note"
        ? api.moveNote
        : node.nodeKind === "resource"
          ? api.moveResource
          : api.moveDir;
    const r = await move(node.path, to);
    const from = r.from;
    to = r.to; // the host normalizes (e.g. appends .md)
    const rewritten = r.links_rewritten;

    // Re-point open/tree state through the move before the watcher pulse re-reads it.
    state.expandedDirs = new Set(
      [...state.expandedDirs].map((d) => remapPath(d, from, to) ?? d),
    );
    state.selectedDir = remapPath(state.selectedDir, from, to) ?? state.selectedDir;
    revealDir(parentDir(to));
    const openNotePath = state.current ? remapPath(state.current.path, from, to) : null;
    const openResourcePath = state.currentResource
      ? remapPath(state.currentResource.path, from, to)
      : null;
    if (openNotePath !== null) {
      state.current = await api.readNote(openNotePath);
    }
    if (openResourcePath !== null) {
      // Adopt the new path before reading the picture, ahead of the watcher's pulse.
      const moved = await api.explainResource(openResourcePath);
      adoptResource(moved, null);
      const picture = await loadResourceImage(moved);
      if (state.currentResource === moved) state.resourceImage = picture;
    }
    await loadNotes();
    if (openNotePath !== null) await refreshDiscovery(); // backlinks may show new paths
    flash(
      rewritten > 0
        ? `Moved ${from} → ${to} (${rewritten} link${rewritten === 1 ? "" : "s"} rewritten).`
        : `Moved ${from} → ${to}.`,
    );
    return true;
  } catch (e) {
    flash(errText(e));
    return false;
  } finally {
    moveInFlight = false;
    render();
  }
}

// --- tree delete (context menu, ⌘⌫, the folder confirm modal) ---------------------
//
// Deletes remove from disk. Files go immediately; folders confirm first. Inbound links are
// left dangling, as an external delete would leave them.

/** A delete is in flight — further gestures are ignored (the move posture). */
let deleteInFlight = false;

/** Files delete immediately; folders always confirm, since even an empty-looking one may
 *  hold unindexed files. */
function requestDelete(node: TreeNodeRef): void {
  state.contextMenu = null;
  if (node.nodeKind !== "folder") {
    render();
    void executeDelete(node);
    return;
  }
  state.deleteTarget = node;
  render();
}

/** Commit the folder-delete confirm (button or ⏎). */
function confirmDelete(): void {
  const node = state.deleteTarget;
  if (!node) return;
  state.deleteTarget = null;
  void executeDelete(node);
}

/** Forget tree state pointing into a deleted folder subtree. */
function dropDirState(dir: string): void {
  const gone = (d: string) => isWithin(d, dir);
  state.expandedDirs = new Set([...state.expandedDirs].filter((d) => !gone(d)));
  if (gone(state.selectedDir)) state.selectedDir = parentDir(dir);
}

/** The shared delete executor. Refuses mid-reindex so two writers never race the index. */
async function executeDelete(node: TreeNodeRef): Promise<void> {
  if (deleteInFlight) return;
  if (refusedWhileIndexing("delete")) return;
  // Close an affected editor first (a conflict aborts, keeping the buffer).
  const curPath = openDocPath(state);
  const affected =
    curPath !== null &&
    (node.nodeKind === "folder" ? isWithin(curPath, node.path) : curPath === node.path);
  // Not `leaveEdits`, as in `executeMove`.
  if (affected && !fmEditGuard()) return;
  if (affected && state.editing && !(await closeEditor())) return;

  // Where the keyboard lands after (K1): the next visible row, else the previous. Computed
  // while the row is still listed.
  const nextFocus = neighborPath(treeRows(), node.path);

  deleteInFlight = true;
  try {
    let what: string;
    let dangled: number;
    if (node.nodeKind === "folder") {
      const r = await api.deleteDir(node.path);
      what = `${r.dir}/`;
      dangled = r.dangled.length;
    } else {
      // A note and a resource report alike.
      const r = await (node.nodeKind === "note" ? api.deleteNote : api.deleteResource)(node.path);
      what = r.path;
      dangled = r.dangled.length;
    }

    // Clear state pointing into the deleted subtree before the watcher's pulse re-reads it.
    if (node.nodeKind === "folder") dropDirState(node.path);
    state.treeFocus = nextFocus;
    if (affected) {
      state.current = null;
      state.currentResource = null;
      state.resourceImage = null;
      clearDiscovery();
      state.discoveringSimilar = false;
      state.discoveringConnections = false;
    }
    await loadNotes();
    void refreshEmbedStatus(state.vaultRoot); // the N/M denominator shrank (#26)
    flash(
      dangled > 0
        ? `Deleted ${what} — links in ${dangled} note${dangled === 1 ? "" : "s"} now unresolved.`
        : `Deleted ${what}.`,
    );
  } catch (e) {
    flash(errText(e));
  } finally {
    deleteInFlight = false;
    render();
  }
}

// --- the anchored ghost graph (GH #22) --------------------------------------------

/** Flip the pane between reading and the graph (no IPC; sticky across notes). */
function toggleGraph(): void {
  if (!state.current) return; // the graph anchors on an open note
  if (!fmEditGuard()) return; // the graph takes the pane the mini-editor holds
  state.explainCard = null; // the graph and Explain share the pane; the toggle picks the graph
  state.graphOpen = !state.graphOpen;
  render();
}

// The `</>` toggle (spec §3 "Escape hatch"). Reading view: a full re-render. Editing: the
// carve-out forbids a rebuild, so it reconfigures the live-preview compartment in place,
// keeping cursor and undo.
function toggleSource(): void {
  if (!fmEditGuard()) return; // a reading-view flip would rebuild the pane
  state.sourceOpen = !state.sourceOpen;
  if (state.editing) {
    editorView?.dispatch({ effects: lpCompartment.reconfigure(livePreviewConf()) });
    paintEditor();
  } else {
    render();
  }
}

async function refreshDiscovery(): Promise<void> {
  const n = state.current;
  if (!n) return;
  // Two independent reads, each painting when it settles (`explain` is fast, `similar`
  // slow). Each drops its result if the user navigated away.
  const stale = () => state.current?.path !== n.path;
  const connections = api
    .explain(n.path)
    .then((explain) => {
      if (!stale()) adoptExplain(explain);
    })
    .catch((e) => {
      if (!stale()) {
        clearConnections();
        flash(errText(e));
      }
    })
    .finally(() => {
      if (stale()) return;
      state.discoveringConnections = false;
      render();
    });
  const similar = api
    .similar(n.path, SIMILAR_LIMIT)
    .then((cands) => {
      if (!stale()) state.similar = cands;
    })
    .catch((e) => {
      if (!stale()) {
        state.similar = [];
        flash(errText(e));
      }
    })
    .finally(() => {
      if (stale()) return;
      state.discoveringSimilar = false;
      render();
    });
  await Promise.all([connections, similar]);
}

// Search-request counter: has a newer search taken over? (A reset is checked separately.)
let searchSeq = 0;

// Search, and where D2's verdict becomes what the pane shows (GH #202):
//   • `false`: no evidence. Rows are dropped here, in state, so render.ts and sidenav.ts
//     agree by construction.
//   • `true`: serve them.
//   • `null`: no calibrated bar for this model (M2). Serve them; reading it as "no
//     matches" would blank every dev vault.
async function doSearch(raw: string): Promise<void> {
  const query = raw.trim();
  if (!query) {
    resetSearch();
    render();
    return;
  }
  state.loading = true;
  state.searchQuery = query;
  // Search takes the right column from chat; ⌘J brings the conversation back.
  state.chatOpen = false;
  render();
  // Staleness guards. Results are dropped if a newer search or a reset took over (a stale
  // `false` would claim "no evidence" for a query nobody judged, against D2). The global
  // `state.loading` is released unless a newer search owns it, or a mid-flight clear
  // would strand the window loading.
  const seq = ++searchSeq;
  const superseded = () => seq !== searchSeq;
  const abandoned = () => superseded() || state.searchQuery !== query;
  try {
    const view = await api.search(query);
    if (abandoned()) return;
    state.searchVouched = view.vouched;
    state.searchResults = view.vouched === false ? [] : view.results;
  } catch (e) {
    if (abandoned()) return;
    state.searchResults = [];
    state.searchVouched = null;
    flash(errText(e));
  } finally {
    if (!superseded()) {
      state.loading = false;
      render();
    }
  }
}

// Back to discovery: query, rows and verdict reset together.
function resetSearch(): void {
  state.searchQuery = "";
  state.searchResults = [];
  state.searchVouched = null;
}

/** The top bar's vault-search box. */
function searchInput(): HTMLInputElement | null {
  return document.getElementById("search-input") as HTMLInputElement | null;
}

function clearSearch(): void {
  resetSearch();
  const input = searchInput();
  if (input) input.value = "";
  render();
}

// --- chat (flow ④, GH #151/#153/#155) -----------------------------------------------
//
// The streaming turn and its cancellation; the paint is chatview.ts, the logic chat.ts.
// Tokens do not go through `render()` (a swap per token would lose caret, scroll and
// focus): they are painted into one element by `paintChatStream`. One full render at each
// end of a turn.

/** Show or hide the chat pane. Opening probes the model server and focuses the composer. */
function toggleChat(): void {
  if (state.chatOpen) {
    closeChat();
    return;
  }
  openChat();
  focusChatInput();
  void refreshChatSetup();
}

/** Open the pane, replacing search. Renders explicitly: callers focus the composer next. */
function openChat(): void {
  state.chatOpen = true;
  clearSearch();
  render();
}

/** Close the pane, stopping a streaming answer (its partial text is kept). The
 *  conversation survives until the window closes (S4). */
function closeChat(): void {
  // A failed cancel is harmless: the turn resolves on its own.
  if (state.chatStreaming !== null) void api.cancelAsk().catch(() => {});
  state.chatOpen = false;
  render();
}

function focusChatInput(): void {
  (document.getElementById("chat-input") as HTMLTextAreaElement | null)?.focus();
}

/** Probe the chat provider for the setup card. A rejected IPC leaves it "loading". */
async function refreshChatSetup(): Promise<void> {
  try {
    adoptChatSetup(await api.chatSetup());
  } catch (e) {
    flash(errText(e));
    return;
  }
  render();
}

/** One typed turn. A failed turn stays in the transcript; `chatHistory` omits it. */
async function sendChat(question: string): Promise<void> {
  const q = question.trim();
  if (!q || state.chatStreaming !== null) return;
  // History is taken before this question joins the transcript.
  const history = chatHistory(state.chatMessages);
  await runChatTurn(q, true, "Searching your notes…", (onToken) =>
    api.ask(q, history, onToken),
  );
}

/** The *Similar & unlinked* list length, also passed to `whySimilar`/`explainSimilar` so
 *  the rank they name matches the card's. */
const SIMILAR_LIMIT = 10;

/**
 * A card's *Why?*: opens chat and asks, as one turn, why `candidate` is in the open note's
 * *Similar & unlinked* list. The host runs it as a tool-using turn (`Vault::why_similar`);
 * follow-ups are ordinary asks.
 */
async function askWhy(candidate: { path: string; title: string | null }): Promise<void> {
  const anchor = state.current;
  if (!anchor || state.chatStreaming !== null) {
    if (state.chatStreaming !== null) flash("B2 is still answering. Press Esc to stop it.");
    return;
  }
  if (!state.chatOpen) openChat();
  // Probe first: with no model, the setup card is what to show.
  await refreshChatSetup();
  if (!chatReady(state) || state.current?.path !== anchor.path) return;
  // `false`: not typed, so leave the composer's draft alone.
  await runChatTurn(
    whyQuestion(candidate, anchor),
    false,
    "Looking things up with B2 tools…",
    (onToken) => api.whySimilar(anchor.path, candidate.path, SIMILAR_LIMIT, onToken),
  );
}

/**
 * A card's model-free *Explain* (GH #236): the centre pane compares the open note with
 * `candidate` via `Vault::explain_similar`. Paints a spinner at once; a superseded read
 * is dropped.
 */
async function openExplain(candidate: string): Promise<void> {
  const anchor = state.current;
  if (!anchor) return;
  // The pane belongs to the editor while editing; say so rather than drop the click.
  if (state.editing) {
    flash(`Leave edit mode (${displayKeys(["edit.toggle"])}) to see Explain.`);
    return;
  }
  if (!fmEditGuard()) return; // the view takes the pane the mini-editor holds
  const current: NonNullable<AppState["explainCard"]> = {
    anchor: anchor.path,
    candidate,
    view: null,
    error: null,
    allPairs: false,
    help: false,
  };
  state.explainCard = current;
  render();
  document.getElementById("explain-close")?.focus();
  try {
    const view = await api.explainSimilar(anchor.path, candidate, SIMILAR_LIMIT);
    if (state.explainCard !== current) return; // superseded or closed meanwhile
    current.view = view;
  } catch (e) {
    if (state.explainCard !== current) return;
    current.error = errText(e);
  }
  render();
}

/** Back out of the Explain view, handing the keyboard back to the card it explained. */
function closeExplain(): void {
  const ec = state.explainCard;
  if (!ec) return;
  state.explainCard = null;
  render();
  const i = state.similar.findIndex((c) => c.path === ec.candidate);
  if (i >= 0) focusSideRow(cardRowKey("similar", i, ec.candidate));
}

/** The shared body of a chat turn: `shown` joins the transcript, `call` streams into the
 *  live row, and the answer (or failure) replaces it. */
async function runChatTurn(
  shown: string,
  clearComposer: boolean,
  waiting: string,
  call: (onToken: (token: string) => void) => Promise<AnswerView>,
): Promise<void> {
  if (state.chatStreaming !== null) return;
  // A vault switch mid-answer clears the transcript, so a late answer is dropped.
  const askedIn = state.vaultRoot;
  state.chatMessages.push(userMessage(shown));
  state.chatStreaming = "";
  state.chatWaiting = waiting;
  render();
  const input = document.getElementById("chat-input") as HTMLTextAreaElement | null;
  if (input && clearComposer) input.value = "";
  scrollChatToEnd();
  try {
    const view = await call((token) => {
      // A token after the turn ended would resurrect the live row.
      if (state.chatStreaming === null) return;
      state.chatStreaming += token;
      paintChatStream();
    });
    if (state.vaultRoot === askedIn) state.chatMessages.push(answerMessage(view));
  } catch (e) {
    if (state.vaultRoot === askedIn) state.chatMessages.push(errorMessage(errText(e)));
  } finally {
    // Read focus before the repaint swaps the pane.
    const active = document.activeElement;
    const composerHeld =
      active === document.body || (active instanceof HTMLElement && active.id === "chat-input");
    state.chatStreaming = null;
    render();
    scrollChatToEnd();
    // Back to the composer (K1), but only if focus was there: a repaint gives focus back,
    // never takes it (crates/b2-desktop/CLAUDE.md).
    if (composerHeld) focusChatInput();
  }
}

/** Paint the streaming answer as `textContent`, never `innerHTML`: model output is
 *  untrusted (E5); the finished answer goes through sanitizing `renderMarkdown`. */
function paintChatStream(): void {
  const live = document.getElementById("chat-stream");
  if (!live || state.chatStreaming === null) return;
  live.textContent = state.chatStreaming;
  scrollChatToEnd();
}

/** Keep the newest text in view. */
function scrollChatToEnd(): void {
  const log = document.getElementById("chat-log");
  if (log) log.scrollTop = log.scrollHeight;
}

/** Esc while an answer streams: stop it. False when idle, so `dismiss` falls through. */
function stopChatAnswer(): boolean {
  if (state.chatStreaming === null) return false;
  void api.cancelAsk().catch((e) => flash(errText(e)));
  return true;
}

/** Start over. The transcript is session state only; dropping it writes nothing. */
function newChat(): void {
  state.chatMessages = [];
  state.sideFocus = null;
  render();
  focusChatInput();
}

/** Switch Settings → Chat between Local (seeds the local endpoint) and Cloud (clears it:
 *  there is no default cloud provider, M5). Neither saves. */
function setChatMode(cloud: boolean): void {
  state.chatCloud = cloud;
  render();
  const url = document.getElementById("settings-chat-url") as HTMLInputElement | null;
  if (url) {
    url.value = cloud ? "" : LOCAL_CHAT_ENDPOINT;
    url.focus();
  }
}

/** Swap Settings → Chat's Model field between picker and text box, and focus it: the swap
 *  button is replaced by the repaint, so focus would otherwise fall to `<body>`. */
function setChatModelTyped(typed: boolean): void {
  state.chatModelTyped = typed;
  render();
  document.getElementById("settings-chat-model")?.focus();
}

/** Settings → Chat: save endpoint/model/key and re-probe ("Save and test"). */
async function saveChatConfig(): Promise<void> {
  const value = (id: string): string | null => {
    const el = document.getElementById(id) as HTMLInputElement | null;
    const v = el?.value.trim() ?? "";
    return v === "" ? null : v;
  };
  const url = value("settings-chat-url");
  const model = value("settings-chat-model");
  // Empty is `null` (keep): the field paints empty even when a key is set. Removal is
  // `clearChatKey`.
  const key = value("settings-chat-key");
  // Validate the tool-call cap before sending anything.
  const capField = document.getElementById("settings-chat-tool-cap") as HTMLInputElement | null;
  const cap =
    capField && state.chatSetup
      ? toolCapInput(capField.value, state.chatSetup.tool_calls)
      : { send: null };
  if ("error" in cap) {
    flash(cap.error);
    capField?.focus();
    return;
  }
  await applyChatConfig(url, model, key, cap.send, (setup) =>
    setup.state === "ready"
      ? `Chat model saved — connected to ${setup.model}.`
      : (setup.message ?? "Chat settings saved."),
  );
}

/** The setup card's installed-model list: pick one to configure it. */
async function useChatModel(model: string): Promise<void> {
  // Send the endpoint explicitly: `null` would reset it to the default. The key's `null`
  // means keep.
  await applyChatConfig(
    state.chatSetup?.base_url ?? null,
    model,
    null,
    null,
    () => `Chat model set to ${model}.`,
  );
}

/**
 * Forget B2's API key, in memory and the Keychain. Sends `""` (the host's clear signal, vs
 * `null`'s keep). A `B2_LLM_API_KEY` in the environment outlives it.
 */
async function clearChatKey(): Promise<void> {
  await applyChatConfig(
    state.chatSetup?.base_url ?? null,
    state.chatSetup?.model ?? null,
    "",
    null,
    // The returned source is the outcome: still stored/session means the Keychain
    // refused, and the key would return at next launch.
    (setup) =>
      setup.api_key_source === "stored" || setup.api_key_source === "session"
        ? "Couldn’t remove the key — your Keychain refused. It is still saved."
        : "API key removed.",
  );
}

/** Save a chat configuration, re-probe, and flash `said(setup)` (or the host's refusal). */
async function applyChatConfig(
  baseUrl: string | null,
  model: string | null,
  apiKey: string | null,
  maxToolCalls: string | null,
  said: (setup: ChatSetup) => string,
): Promise<void> {
  try {
    const setup = await api.setChatConfig(baseUrl, model, apiKey, maxToolCalls);
    adoptChatSetup(setup);
    flash(said(setup));
  } catch (e) {
    flash(errText(e));
  }
  render();
}

/** Adopt the host's chat setup; the Local/Cloud switch follows it. */
function adoptChatSetup(setup: ChatSetup): void {
  state.chatSetup = setup;
  state.chatCloud = setup.cloud;
}

function openLinkModal(path: string, title: string): void {
  // A link rewrites the frontmatter the mini-editor holds, so resolve that first.
  if (!fmEditGuard()) return;
  state.linkTarget = { path, title: title || null };
  state.linkRelation = "references";
  render();
}

function closeModal(): void {
  state.linkTarget = null;
  state.moveTarget = null;
  state.deleteTarget = null;
  render();
}

async function commitLink(): Promise<void> {
  const target = state.linkTarget;
  const src = state.current;
  if (!target || !src) return;
  const relation =
    (document.getElementById("link-relation") as HTMLSelectElement | null)?.value ??
    state.linkRelation;
  const explanationRaw =
    (document.getElementById("link-explanation") as HTMLInputElement | null)?.value ?? "";
  const explanation = explanationRaw.trim() || null;

  state.loading = true;
  render();
  try {
    // Mid-edit: flush first, then chain the post-link revision, or the next autosave
    // would conflict with our own link write.
    if (state.editing) await saveNow();
    const report = await api.link(src.path, target.path, relation, explanation);
    if (state.editing && !state.editConflict && state.current?.path === src.path) {
      // Not under the conflict bar: a fresh revision would let a save clobber the edit.
      const fresh = await api.readNote(src.path);
      state.current.revision = fresh.revision;
      state.current.frontmatter = fresh.frontmatter;
    }
    closeModal();
    await refreshDiscovery();
    flash(
      report.created
        ? `Linked ${report.src_path} —${report.relation}→ ${report.dst_path}.`
        : `Already linked —${report.relation}→ ${report.dst_path}. Nothing changed.`,
    );
  } catch (e) {
    // Keep the modal open so the user can adjust and retry.
    flash(errText(e));
  } finally {
    state.loading = false;
    render();
  }
}

// --- settings (⌘,) ----------------------------------------------------------------
//
// The tabbed preferences dialog (rail: settingstabs.ts; paint: settingsview.ts).

/** Open Settings, optionally at a section; otherwise where it was left. */
async function openSettings(tab?: SettingsTabId): Promise<void> {
  // Read before `dismissOverlays` clears it: open or jump?
  const wasOpen = state.settingsOpen;
  dismissOverlays();
  if (tab) state.settingsTab = tab;
  state.settingsOpen = true;
  render(); // show the dialog shell immediately; the model list fills when it resolves
  if (wasOpen) {
    // A jump between sections: move focus with the selection (`paintModal` would restore
    // the old tab). Skip the reads; nothing changed host-side.
    if (tab) document.getElementById(tabDomId(tab))?.focus();
    return;
  }
  try {
    // Models, embedding stats, models dir and compute device, in parallel.
    const [models, stats, dir, device] = await Promise.all([
      api.listModels(),
      api.embedStats(),
      api.modelsDir(),
      api.embedDevice(),
    ]);
    state.models = models;
    state.embedStats = stats;
    state.modelsDir = dir;
    state.embedDevice = device;
  } catch (e) {
    flash(errText(e));
  }
  // Not in the `Promise.all`: a network probe can take seconds and would hold the dialog.
  void refreshChatSetup();
  // Focus is already on the selected tab, and `paintModal` keeps it there.
  render();
}

function closeSettings(): void {
  state.settingsOpen = false;
  // The recorder must not outlive the dialog, or it swallows every keystroke.
  stopRecording();
}

/**
 * Show a section. `focusTab` (arrow or ⌃Tab) moves focus with the selection, which
 * `paintModal` would otherwise restore to the previous tab. A click passes false: WebKit
 * doesn't focus buttons on click, and forcing it would show a focus ring.
 */
function selectSettingsTab(tab: SettingsTabId, focusTab: boolean): void {
  if (state.settingsTab === tab && !focusTab) return;
  state.settingsTab = tab;
  render();
  if (focusTab) document.getElementById(tabDomId(tab))?.focus();
}

/** Copy a path to the clipboard and say so. WebKit can refuse the write, so the failure
 *  toast shows the path instead. */
async function copyPath(path: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(path);
    flash(`Copied ${path}`);
  } catch {
    flash(`Couldn't copy — the path is ${path}`);
  }
}

// --- appearance (light/dark) ------------------------------------------------------
//
// "system" follows `prefers-color-scheme`; "light"/"dark" set `data-theme` on <html>.
// Persisted in localStorage: a viewing choice, never vault state.

/** Reflect `state.theme` onto <html>: absent attribute ⇒ follow the OS. */
function applyTheme(): void {
  const root = document.documentElement;
  const attr = themeAttr(state.theme);
  if (attr === null) root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", attr);
}

/** Read the saved preference into state and apply it (once, first thing on boot). */
function loadTheme(): void {
  state.theme = loadThemePref();
  applyTheme();
}

/** Persist + apply an appearance choice from the Settings control. */
function setTheme(theme: ThemePref): void {
  if (state.theme === theme) return;
  state.theme = theme;
  saveThemePref(theme);
  applyTheme();
  render();
}

// --- text size (View ▸ Zoom In / Zoom Out / Actual Size) ---------------------------
//
// A viewing choice stored like the theme, applied as WebKit page zoom (zoom.ts). ⌘= / ⌘- /
// ⌘0 belong to the View menu (crates/b2-desktop/src/menu.rs), whose accelerators fire before
// any keydown reaches here; `initMenuCommands` gets the item id, so they work everywhere,
// mid-edit included. Module-local: nothing renders from it.

let zoom = DEFAULT_ZOOM;

/** Hand a size to the host and remember it. Resolves once the window is scaled and never
 *  rejects: a refusal (window closing, no Tauri) isn't worth a toast or a failed boot. */
function pushZoom(next: number): Promise<void> {
  zoom = next;
  saveZoom(next);
  return api.setZoom(next).catch(() => {
    // Non-fatal: the app is fully usable at whatever size it is currently drawn.
  });
}

/** Apply a requested size, then say so if a breakpoint hid a pane (zoom.ts `hiddenNotice`).
 *  Measured, not predicted, so the breakpoints live only in style.css: after the round trip
 *  plus two frames, since WebKit's layout can lag `getComputedStyle` by one. */
function applyZoom(next: number): void {
  const before = visiblePanes();
  void pushZoom(next).then(() =>
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const notice = hiddenNotice(before, visiblePanes());
        if (notice) flash(notice);
      }),
    ),
  );
}

/** One rung in `dir`; silent at the ends. */
function nudgeZoom(dir: Direction): void {
  const next = stepZoom(zoom, dir);
  if (next !== zoom) applyZoom(next);
}

/** Read the saved size, apply it, and wait: the rest of `boot` needs the final viewport,
 *  or the first paint jumps and `initPanes` sizes columns against a stale width. No
 *  column notice at boot. */
async function loadZoomPref(): Promise<void> {
  const saved = loadZoom();
  zoom = saved;
  if (saved !== DEFAULT_ZOOM) await pushZoom(saved);
}

/** Listen for B2's own menu items; unknown ids are ignored. */
function initMenuCommands(): void {
  void api.onMenuCommand((id) => {
    if (id === "view.zoom-in") nudgeZoom(1);
    else if (id === "view.zoom-out") nudgeZoom(-1);
    else if (id === "view.zoom-reset" && zoom !== DEFAULT_ZOOM) applyZoom(DEFAULT_ZOOM);
  });
}

// --- the customizable keyboard (GH #121) ------------------------------------------
//
// Stored in localStorage like the theme (never vault state). keymap.ts owns the logic;
// this installs the table, runs the recorder, and persists the result.

/** Lay the stored rebindings over the shipped table as the live registry. CodeMirror keeps
 *  its own copy of the chords, so it is reconfigured via its compartment too. */
function installKeymap(): void {
  setActiveBindings(applyOverrides(DEFAULT_BINDINGS, state.keyOverrides));
  editorView?.dispatch({ effects: keysCompartment.reconfigure(editorKeymap()) });
}

/** Read the saved keyboard and install it (before `buildShell`, which prints chords).
 *  Returns dropped entries for the caller to report, since there is no shell to flash yet. */
function loadKeymap(): string[] {
  const { overrides, dropped } = loadOverrides();
  state.keyOverrides = overrides;
  installKeymap();
  return dropped;
}

/** Adopt rebindings: persist, install, repaint. The one write path. */
function setOverrides(next: Overrides): void {
  state.keyOverrides = next;
  saveOverrides(next);
  installKeymap();
  render();
  // The two surfaces `render()` doesn't rebuild: the shell, and the live editor's bar.
  paintShellHints();
  paintEditor();
}

// The chord recorder; its state is `state.recorder`.

/** When the recorder opened, for the silence probe (recorder.ts). */
let recorderOpenedAt = 0;
let recorderTimer: number | null = null;

function startRecording(id: BindingId): void {
  const b = findBinding(activeBindings(), id);
  if (!b || b.fixed !== undefined) return; // a chip for a fixed chord isn't a button at all
  state.recorder = { id, candidate: null, problems: [], hint: null, blurred: false };
  recorderOpenedAt = Date.now();
  cancelProbe();
  // The silence probe (recorder.ts).
  recorderTimer = window.setTimeout(() => {
    recorderTimer = null;
    if (!state.recorder || state.recorder.candidate !== null) return;
    const { blurred } = state.recorder; // what the session has observed, not what this tick assumes
    state.recorder.hint = silenceHint({ elapsedMs: Date.now() - recorderOpenedAt, blurred });
    render();
  }, PROBE_AFTER_MS);
  render();
  // Take focus off CodeMirror, which would otherwise type the chord into the buffer.
  document.getElementById("keys-recorder")?.focus();
}

/** Stop the silence probe: on close, a chord, or a window blur (which sets a stronger
 *  hint the timer would otherwise overwrite, GH #125). */
function cancelProbe(): void {
  if (recorderTimer !== null) {
    clearTimeout(recorderTimer);
    recorderTimer = null;
  }
}

/** Tear the recorder down without painting, for callers that paint themselves. */
function clearRecorder(): void {
  cancelProbe();
  state.recorder = null;
}

function stopRecording(): void {
  clearRecorder();
  render();
}

/** A keydown while the recorder is open; true when consumed. Esc cancels, ⏎ accepts (both
 *  `fixed`, so unrecordable anyway); any other chord replaces the candidate. */
function recorderKeydown(e: KeyboardEvent): boolean {
  const rec = state.recorder;
  if (!rec) return false;
  // Nothing should happen while a chord is pressed. (CodeMirror runs first, which is why
  // `startRecording` moves focus off it.)
  e.preventDefault();
  if (canonicalKey(e.key) === "Escape") {
    stopRecording();
    return true;
  }
  if (canonicalKey(e.key) === "Enter" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
    if (rec.candidate !== null && !refused(rec.problems)) commitRecording();
    return true;
  }
  const got = capture(e);
  if (got.kind === "modifier") return true; // still reaching for the chord
  if (got.kind === "unbindable") {
    rec.candidate = null;
    rec.problems = [];
    rec.hint = got.message;
    render();
    return true;
  }
  cancelProbe(); // a chord arrived, so there is no silence left to read
  rec.candidate = got.spec;
  rec.problems = chordProblems(rec.id, got.spec, DEFAULT_BINDINGS, state.keyOverrides);
  rec.hint = null;
  render();
  return true;
}

/** Save the captured chord. */
function commitRecording(): void {
  const rec = state.recorder;
  if (!rec || rec.candidate === null || refused(rec.problems)) return;
  const next = withOverride(state.keyOverrides, rec.id, [rec.candidate]);
  clearRecorder(); // the strip goes; `setOverrides` does the one repaint
  setOverrides(next);
}

/** Put one command back on its shipped chord (removes its override). */
function resetChord(id: BindingId): void {
  clearRecorder();
  setOverrides(withOverride(state.keyOverrides, id, []));
}

/** Put the whole keyboard back. */
function resetAllChords(): void {
  clearRecorder();
  setOverrides({});
}

// --- install reminder (the "semantic search is off" banner) -----------------------
//
// Gating is embedreminder.ts; this is dismissal. ✕ hides it for the session; "Don't remind
// me again" persists the opt-out in localStorage.

/** Read the persisted "don't remind me" opt-out into state (once, on boot). */
function loadEmbedReminderPref(): void {
  state.embedReminderDismissed = loadReminderOptOut();
}

/** Turn the banner off; `persist` keeps it off across launches. */
function dismissEmbedReminder(persist: boolean): void {
  state.embedReminderDismissed = true;
  if (persist) saveReminderOptOut();
  render();
}

// Download and verify the selected model in-app (`b2 init`). Single-flight via
// `state.provisioning`.
async function provisionModel(): Promise<void> {
  if (state.provisioning) return;
  state.provisioning = true;
  render();
  try {
    state.models = await api.provisionModel();
    const now = state.models.find((m) => m.current);
    // Re-read coverage so the banner and search caveat clear now.
    await refreshEmbedStatus(state.vaultRoot);
    flash(`Downloaded ${now?.label ?? "model"}. Embedding your vault now…`);
    // Embed the vault now so semantic search turns on (#25).
    trackIndexing(autoIndexOnOpen(state.vaultRoot));
  } catch (e) {
    flash(errText(e));
  } finally {
    state.provisioning = false;
    render();
  }
}

// Persist a model choice and say what the swap still needs (download, then Reindex).
async function changeModel(model: string): Promise<void> {
  if (state.models.find((m) => m.current)?.id === model) return;
  try {
    state.models = await api.setModel(model);
    const now = state.models.find((m) => m.current);
    const label = now?.label ?? model;
    flash(
      now && !now.installed
        ? `Model set to ${label}. Download it with \`b2 init\`, then Reindex to re-embed.`
        : `Model set to ${label}. Reindex to re-embed your vault with it.`,
    );
  } catch (e) {
    // Refused: re-sync the picker to the unchanged config.
    flash(errText(e));
    try {
      state.models = await api.listModels();
    } catch {
      /* leave the stale list; the toast already explains */
    }
  }
  render();
}

// Switch vault via the host's folder picker, resetting everything that belonged to the
// old vault. A cancel is a no-op.
async function switchVault(): Promise<void> {
  // Leave edit mode, then drop the pending trailing embed (it heals on that vault's next
  // run).
  if (!(await leaveEdits())) return;
  if (embedTimer !== undefined) {
    clearTimeout(embedTimer);
    embedTimer = undefined;
  }
  try {
    const info = await api.chooseVault();
    if (!info) return; // cancelled — leave the current vault untouched
    // The host cancelled the old index run; chain the new auto-index after it settles, or
    // it would see a stale `reindexing` flag. Not awaited: the reset mustn't block.
    const departing = indexingRun;
    state.vaultRoot = info.root; // set now so the departing run's guards bail promptly
    adoptCoverage(info);
    state.current = null;
    state.currentResource = null;
    state.resourceImage = null;
    clearDiscovery();
    resetSearch();
    // The conversation's citations are paths in the old vault (L1): stop and drop it.
    if (state.chatStreaming !== null) void api.cancelAsk().catch(() => {});
    state.chatMessages = [];
    state.chatStreaming = null;
    state.expandedDirs = new Set<string>();
    state.selectedDir = ""; // the create context belongs to the vault we left
    state.dirs = []; // loadNotes below re-lists the new vault's structure
    state.treeCreate = null;
    navClear(); // history is per-vault: the old stack's paths mean nothing here
    const input = searchInput();
    if (input) input.value = "";
    state.loading = true;
    render();
    await loadNotes(); // catches its own errors → toast; empty tree on an unindexed vault
    state.loading = false;
    flash(`Switched to ${info.root}.`);
    // Auto-index the new vault (#25), after the departing run winds down.
    trackIndexing(
      (async () => {
        if (departing) await departing;
        await autoIndexOnOpen(info.root);
      })(),
    );
  } catch (e) {
    state.loading = false;
    flash(errText(e));
  }
}

// Re-read embedding coverage (#26). Best-effort, and dropped if the vault switched.
async function refreshEmbedStatus(forRoot: string | null): Promise<void> {
  try {
    const info = await api.vaultInfo();
    if (state.vaultRoot !== forRoot) return;
    adoptCoverage(info);
  } catch {
    // ignore — coverage is a hint, never worth surfacing an error over
  }
}

/** Adopt a `VaultInfo`'s embedding coverage (the root is the caller's to set). */
function adoptCoverage(info: VaultInfo): void {
  state.semantic = info.semantic;
  state.notesEmbedded = info.notes_embedded;
  state.notesTotal = info.notes_total;
}

// The in-flight background index run (reindex, auto-index, or trailing embed), or null.
// A vault switch chains its auto-index after this. One run at a time, so one slot.
let indexingRun: Promise<void> | null = null;

/** Register a background-index run; it settles after `state.reindexing` is cleared. */
function trackIndexing(run: Promise<void>): void {
  const done = run.finally(() => {
    if (indexingRun === done) indexingRun = null;
  });
  indexingRun = done;
}

/** Start an index run's state; the caller picks the repaint. */
function beginIndexRun(): void {
  state.reindexing = true;
  state.reindexProgress = null;
  state.reindexCancelling = false;
}

/** End an index run (every run's `finally`). */
function endIndexRun(): void {
  state.reindexing = false;
  state.reindexProgress = null;
  state.reindexCancelling = false;
  render();
}

/** After a run: re-read the open note (unless an editor holds it, where a fresh revision
 *  would break the save chain) and refresh discovery. */
async function refreshOpenNoteAfterIndex(): Promise<void> {
  if (!state.current) return;
  if (!state.editing && !state.fmEditing) {
    state.current = await api.readNote(state.current.path);
  }
  await refreshDiscovery();
}

// Reindex as project → embed (Shape A, docs/index-engine.md): the fast `project` paints the
// tree, then the cancellable `embed` streams progress. Doesn't set `state.loading`, so the
// app stays usable.
async function doReindex(): Promise<void> {
  if (state.reindexing) return; // single-in-flight (the host also guards embed)
  const startedRoot = state.vaultRoot; // guard against a vault switch mid-run
  beginIndexRun();
  render();
  try {
    // Phase 1: projection (fast, no model): notes, keyword index, graph.
    const p = await api.project();
    // A vault switch committed and owns the UI (spec §6).
    if (state.vaultRoot !== startedRoot) return;
    // Unreadable files are skipped, not fatal; every flash below names them.
    const skipped = p.skipped.length
      ? ` — skipped ${p.skipped.length} unreadable file(s): ${p.skipped
          .map((s) => `${s.path} (${s.reason})`)
          .join(", ")}`
      : "";
    // The tree paints now; the vault is browsable while embedding runs.
    await loadNotes();
    // The search caveat shows 0/M embedded while vectors fill (#26).
    await refreshEmbedStatus(startedRoot);
    render();
    if (state.reindexCancelling) {
      // Cancelled during projection: don't start embedding (the host would clear the
      // flag and run to completion).
      flash(
        `Indexed ${p.indexed} note(s) — cancelled before embedding. Re-run to embed.${skipped}`,
      );
      return;
    }
    // Phase 2: embedding (real model), metered and cancellable.
    const r = await embedWithProgress(startedRoot);
    if (state.vaultRoot !== startedRoot) return;
    // The host frees the embed slot before the switch returns, so the check above usually
    // misses a switch. A cancel we didn't initiate can only be a switch (main.rs
    // `cancel_and_wait_for_reindex`): leave the departing vault alone. A user cancel falls
    // through and is reported.
    if (r.cancelled && !state.reindexCancelling) return;
    // Refresh coverage for the search caveat (#26).
    await refreshEmbedStatus(startedRoot);
    flash(
      r.cancelled
        ? `Embedded ${r.embedded}/${p.indexed} note(s) — cancelled. Re-run to finish the rest.${skipped}`
        : `Indexed ${p.indexed} note(s) — ${r.embedded} embedded.${skipped}`,
    );
    await refreshOpenNoteAfterIndex();
  } catch (e) {
    if (state.vaultRoot === startedRoot) flash(errText(e));
  } finally {
    endIndexRun();
  }
}

// Auto-index on open (#25), driven by `VaultInfo`'s coverage (#26):
//   • notesTotal === 0        → never projected: project, then embed.
//   • notesEmbedded < total   → embedding unfinished: fill the pending vectors (§7.2).
//   • embedded === total (>0) → complete: nothing to do.
// Embeds only with the real model installed. Silent (meter and Cancel only), with
// doReindex's vault-switch guards (spec §6).
async function autoIndexOnOpen(startedRoot: string | null): Promise<void> {
  if (state.reindexing || state.vaultRoot === null) return; // a run is live, or no vault
  const projected = state.notesTotal > 0;
  if (projected && state.notesEmbedded >= state.notesTotal) return; // index already complete
  const needsProject = !projected;
  // Without a model, only a never-projected vault has work (project is model-free).
  if (!needsProject && !state.semantic) return;

  beginIndexRun();
  render();
  try {
    if (needsProject) {
      await api.project();
      if (state.vaultRoot !== startedRoot) return; // a switch took over — it owns the UI
      await loadNotes(); // the tree paints HERE; keyword search is live
      await refreshEmbedStatus(startedRoot); // caveat reads "keyword-only for now (0/M)"
      render();
    }
    // Embed only with a model, no pending Cancel, and no vault switch.
    if (state.vaultRoot !== startedRoot || !state.semantic || state.reindexCancelling) return;
    const r = await embedWithProgress(startedRoot);
    if (state.vaultRoot !== startedRoot) return;
    // A cancel we didn't initiate is a vault switch (see doReindex).
    if (r.cancelled && !state.reindexCancelling) return;
    await refreshEmbedStatus(startedRoot);
    // Unlike doReindex, an unasked-for run leaves an open editor's pane entirely alone.
    if (!state.editing && !state.fmEditing) await refreshOpenNoteAfterIndex();
  } catch {
    // Silent (§7.2): the pending set heals on the next run.
  } finally {
    endIndexRun();
  }
}

// Ask the host to stop the embed at its next batch; the run resolves with `cancelled`.
// During projection, `doReindex` sees `reindexCancelling` and skips embedding.
async function cancelReindex(): Promise<void> {
  if (!state.reindexing || state.reindexCancelling) return;
  state.reindexCancelling = true;
  paintReindex();
  try {
    await api.cancelReindex();
  } catch (e) {
    flash(errText(e));
  }
}

// --- editing (crates/b2-desktop/CLAUDE.md) -------------------------------------------
//
// Edit mode hands the note pane to CodeMirror 6 and autosaves on idle via `write_note`.
// State that never drives a render is module-local, not AppState.

let editorView: EditorView | null = null;
let autosaveTimer: number | undefined;
let embedTimer: number | undefined;
// Live preview sits in a Compartment (spec §5) so `</>` swaps to raw source with no remount.
// Both use `b2Highlighter` (highlight.ts); CSS scopes its reach: `.src-body` colors the whole
// document, live preview only `.lp-fence`. Not `defaultHighlightStyle`: it is light-only.
const lpCompartment = new Compartment();
function livePreviewConf(): Extension {
  return state.sourceOpen
    ? [syntaxHighlighting(b2Highlighter), EditorView.contentAttributes.of({ class: "src-body" })]
    : [livePreview((target) => void followWikilink(target)), syntaxHighlighting(b2Highlighter)];
}
/** The in-flight save chain — resolves only when it settles (trailing saves included). */
let inFlight: Promise<void> | null = null;
/** A save arrived while one was in flight; run one more against the latest buffer. */
let trailingDirty = false;
/** Set on WriteConflict: no save fires until the conflict bar's action resumes. */
let autosavePaused = false;

const AUTOSAVE_MS = 1000;
const TRAILING_EMBED_MS = 2000;

// Enter edit mode: render (now skipping the note pane), then own the pane until exit.
function enterEdit(): void {
  const n = state.current;
  if (!n || state.editing || state.loading) return;
  if (!fmEditGuard()) return; // one editor at a time — resolve the drawer first
  state.explainCard = null; // editing takes the pane back to the note
  state.editing = true;
  state.editConflict = false;
  render();
  mountEditor(n.body);
}

// `[[` completion over the vault's lists as they stand (editorcmds.ts).
const wikiSource = wikiCompletionSource(() => state);


/** B2's chords in the editor, read from the live registry each call. A function, not a
 *  const, or it would freeze the defaults before boot loads rebindings (GH #125). */
function b2EditorKeymap(): KeyBinding[] {
  return [
    ...FORMATS.map((f) => ({
      key: chordFor(`format.${f.id}`),
      run: (view: EditorView) => runFormat(view, f),
    })),
    { key: chordFor("editor.table"), run: runInsertTable },
    {
      key: chordFor("editor.list.indent"),
      run: (view: EditorView) => runListShift(view, indentList),
    },
    {
      key: chordFor("editor.list.outdent"),
      run: (view: EditorView) => runListShift(view, outdentList),
    },
    {
      key: chordFor("editor.paste-plain"),
      run: (view: EditorView) => {
        void pastePlain(view);
        return true;
      },
    },
  ];
}

// In a Compartment so a rebinding reaches a mounted editor (Settings opens mid-edit).
// B2's chords and the stock ones share one array, B2's first: that order makes ⌘I italic
// rather than `selectParentSyntax` (editorkeys.test.ts pins it).
const keysCompartment = new Compartment();
const editorKeymap = (): Extension => keymap.of([...b2EditorKeymap(), ...STOCK_EDITOR_KEYMAP]);



/**
 * ⌘⇧V: paste as plain text, bypassing `richPaste`. WebKit fires no paste event for ⌘⇧V,
 * and gates `navigator.clipboard` reads behind a prompt, so the host reads the clipboard
 * (crates/b2-desktop/CLAUDE.md). The binding claims the chord so no webview pastes twice.
 */
async function pastePlain(view: EditorView): Promise<void> {
  try {
    const text = await api.clipboardText();
    if (!text) return;
    view.dispatch({
      ...view.state.replaceSelection(text),
      scrollIntoView: true,
      userEvent: "input.paste",
    });
  } catch (e) {
    flash(errText(e));
  }
}

function mountEditor(body: string): void {
  const n = state.current;
  if (!n) return;
  el("note-pane").innerHTML = `
    <div class="editor-chrome">
      <div class="editor-bar">
        <span class="editor-title">Editing · ${escapeHtml(n.path)}</span>
        <div class="note-bar-actions">
          <button id="edit-source" class="source-toggle${
            state.sourceOpen ? " is-active" : ""
          }" data-toggle-source aria-pressed="${state.sourceOpen}" title="${escapeHtml(
            editorSourceTitle(),
          )}">&lt;/&gt;</button>
          <button id="edit-done" class="btn small primary" title="${escapeHtml(editDoneTitle())}">Done</button>
        </div>
      </div>
      <div id="edit-conflict" class="conflict-bar" hidden>
        <span>This note changed on disk.</span>
        <span class="conflict-actions">
          <button id="conflict-reload" class="btn small" title="Discard my edits and load the note from disk">Reload</button>
          <button id="conflict-keep" class="btn small" title="Overwrite the note on disk with my edits">Keep mine</button>
        </span>
      </div>
    </div>
    <div id="editor-host" class="editor-host"></div>`;
  editorView = new EditorView({
    doc: body,
    extensions: [
      // GFM (the default base is CommonMark-only, with no `Strikethrough`) plus wikilinks.
      // `codeLanguages` parses fences with highlight.ts's resolver, as the reading view does.
      markdown({ base: markdownLanguage, extensions: [wikilink], codeLanguages: resolveLang }),
      history(),
      // Registry chords (CodeMirror syntax), ahead of the stock ones (`editorKeymap`).
      keysCompartment.of(editorKeymap()),
      EditorView.lineWrapping,
      // Web-page formatting survives the clipboard (editorcmds.ts `richPaste`).
      richPaste,
      // A discovery card dropped into the buffer (droplink.ts); other drags are untouched.
      wikilinkDrop,
      // `[[` completion, in both modes. Its keymap outranks defaultKeymap, so Enter
      // accepts.
      autocompletion({ override: [wikiSource], icons: false }),
      // Tooltips on <body>, so the scrolling pane can't clip them.
      tooltips({ position: "fixed", parent: document.body }),
      // The note's loaded pictures. Outside `lpCompartment` so `</>` keeps them, and
      // ahead of it because live preview's `blockField` reads this field, and a field can
      // only read fields defined before it.
      embedImagesField,
      lpCompartment.of(livePreviewConf()),
      // Find-in-note (⌘F) match decorations.
      findField,
      EditorView.updateListener.of((u) => {
        if (u.docChanged) {
          scheduleAutosave();
          scheduleImageScan(); // a typed or deleted embed changes what to hold
          // Keep the find bar's count in step.
          if (findOpen) syncEditorFind(u.view);
        }
      }),
    ],
    parent: el("editor-host"),
  });
  editorView.focus();
  pushNoteImages(); // whatever the reading view already loaded, without a second read
  paintEditor();
  // An open find bar carries across the mount.
  if (findOpen) setFindQuery(findInput().value);
}

/** The editor's `</>` tooltip, with the live (rebindable, #121) chord. */
function editorSourceTitle(): string {
  const what = state.sourceOpen ? "Show live preview" : "Show Markdown source";
  return `${what} — ${displayKeys(["source.toggle"])}`;
}

// Repaint just the editor's conflict bar and `</>` button, never the pane.
function paintEditor(): void {
  const bar = document.getElementById("edit-conflict");
  if (bar) bar.hidden = !state.editConflict;
  const src = document.getElementById("edit-source");
  if (src) {
    src.classList.toggle("is-active", state.sourceOpen);
    src.setAttribute("aria-pressed", String(state.sourceOpen));
    src.title = editorSourceTitle();
  }
}

function scheduleAutosave(): void {
  if (autosavePaused) return; // the conflict bar is up — the user decides first
  if (autosaveTimer !== undefined) clearTimeout(autosaveTimer);
  autosaveTimer = window.setTimeout(() => {
    autosaveTimer = undefined;
    void saveNow();
  }, AUTOSAVE_MS);
}

/** Flush now (skipping the debounce). Single-flight with at most one trailing save; the
 *  promise resolves when the whole chain settles. */
function saveNow(): Promise<void> {
  if (autosaveTimer !== undefined) {
    clearTimeout(autosaveTimer);
    autosaveTimer = undefined;
  }
  if (!state.editing || !editorView || !state.current || autosavePaused)
    return Promise.resolve();
  if (inFlight) {
    trailingDirty = true; // the trailing save reads the latest buffer when it fires
    return inFlight;
  }
  inFlight = runSaveChain().finally(() => {
    inFlight = null;
  });
  return inFlight;
}

async function runSaveChain(): Promise<void> {
  do {
    trailingDirty = false;
    const cur = state.current;
    const view = editorView;
    if (!cur || !view || autosavePaused) return;
    const buffer = view.state.doc.toString();
    if (buffer === cur.body) continue; // nothing new since the last save — settle
    try {
      const report = await api.writeNote(cur.path, buffer, cur.revision);
      // Chain the returned revision so our own saves never conflict (spec §3); mirroring
      // `body` lets exit render without a re-read.
      cur.revision = report.revision;
      cur.body = buffer;
      scheduleTrailingEmbed();
      void refreshConnections(); // a body edit can add/remove [[wikilink]] edges
    } catch (e) {
      if (isWriteConflict(e)) {
        // Pause and let the user decide; never re-fire or silently clobber.
        autosavePaused = true;
        state.editConflict = true;
        paintEditor();
      } else {
        flash(errText(e)); // real errors surface; autosave *success* stays silent
      }
      return;
    }
  } while (trailingDirty);
}

// Post-save connection refresh (spec §6). Silent on failure; the next open corrects it.
async function refreshConnections(): Promise<void> {
  const cur = state.current;
  if (!cur) return;
  try {
    const explain = await api.explain(cur.path);
    if (state.current?.path !== cur.path) return; // navigated away meanwhile
    adoptExplain(explain);
    render();
  } catch {
    // deliberately silent
  }
}

// After the save chain settles (~2s), fill the invalidated vectors. Keyword search and the
// graph are current from the save; `similar` lags by these seconds (spec §6).
function scheduleTrailingEmbed(): void {
  if (embedTimer !== undefined) clearTimeout(embedTimer);
  embedTimer = window.setTimeout(() => {
    embedTimer = undefined;
    trackIndexing(runTrailingEmbed());
  }, TRAILING_EMBED_MS);
}

async function runTrailingEmbed(): Promise<void> {
  if (inFlight) {
    scheduleTrailingEmbed(); // the chain hasn't settled — come back after it has
    return;
  }
  // A live run covers our note; the pending set heals on any later run (split §7.2).
  if (state.reindexing || state.vaultRoot === null) return;
  const startedRoot = state.vaultRoot;
  beginIndexRun();
  paintReindex();
  try {
    await embedWithProgress(startedRoot);
    // Vectors are fresh — let `similar` rank with them.
    if (state.vaultRoot === startedRoot) await refreshDiscovery();
  } catch {
    // Silent: the user didn't ask for this run, and the pending set heals.
  } finally {
    endIndexRun();
  }
}

// Embed with progress into the meter, ignoring events from a vault we've left.
function embedWithProgress(startedRoot: string | null) {
  return api.embed((prog) => {
    if (state.vaultRoot !== startedRoot) return;
    state.reindexProgress = prog;
    paintReindex();
  });
}

// Conflict bar: Reload. Discard the buffer, remount on disk's version, resume autosave.
async function conflictReload(): Promise<void> {
  const cur = state.current;
  if (!cur) return;
  try {
    const fresh = await api.readNote(cur.path);
    state.current = fresh;
    teardownEditor();
    state.editConflict = false;
    mountEditor(fresh.body);
    void refreshConnections(); // the external edit may have changed edges too
  } catch (e) {
    flash(errText(e));
  }
}

// Conflict bar: Keep mine. Write the buffer against the current revision through the same
// guarded op (there is no force flag).
async function conflictKeepMine(): Promise<void> {
  const cur = state.current;
  if (!cur || !editorView) return;
  try {
    const fresh = await api.readNote(cur.path);
    // Adopt disk's revision and frontmatter (the save preserves disk frontmatter).
    state.current = fresh;
    autosavePaused = false;
    state.editConflict = false;
    paintEditor();
    await saveNow();
  } catch (e) {
    flash(errText(e));
  }
}

/** Destroy the editor and reset its save-chain flags (remount and close share this). */
function teardownEditor(): void {
  editorView?.destroy();
  editorView = null;
  trailingDirty = false;
  autosavePaused = false;
}

/** Does the editor hold text not on disk (a failed flush)? */
function bufferUnsaved(): boolean {
  return (
    editorView !== null &&
    state.current !== null &&
    editorView.state.doc.toString() !== state.current.body
  );
}

/** Resolve both editors before the pane changes. False means the caller must abandon
 *  the action rather than drop an edit. */
async function leaveEdits(): Promise<boolean> {
  if (!fmEditGuard()) return false;
  return closeEditor();
}

/** Flush and leave edit mode; false when the buffer couldn't be saved (conflict or
 *  failure), so the caller abandons rather than drop edits. */
async function closeEditor(): Promise<boolean> {
  if (!state.editing) return true;
  await saveNow();
  if (state.editConflict) return false;
  // The flush failed (its error already toasted) — keep the buffer alive.
  if (bufferUnsaved()) return false;
  if (autosaveTimer !== undefined) {
    clearTimeout(autosaveTimer);
    autosaveTimer = undefined;
  }
  teardownEditor();
  state.editing = false;
  state.editConflict = false;
  return true;
}

async function exitEdit(): Promise<void> {
  if (await closeEditor()) render(); // shows the saved text — no re-read needed
}

// --- discovery card → wikilink (droplink.ts) ---------------------------------------
//
// Drag a Similar card onto a line being edited to append a `[[wikilink]]` there. Placement
// and preview are droplink.ts's; this holds the payload, the save, and the pane refresh.
// Cards are draggable only in edit mode. Keyboard half: *Insert link at cursor* (K1).

/** The card being dragged, or null. Can go stale (see `carriesCard`). */
let cardDrag: DraggedCard | null = null;

/** Is this drag a discovery card? Asked of the payload's MIME type: a mid-drag repaint can
 *  destroy the card and its `dragend`, stranding `cardDrag`. */
function carriesCard(e: DragEvent): boolean {
  return e.dataTransfer?.types.includes(CARD_DRAG_MIME) ?? false;
}

/** The editor extension, built once; reads the drag through this closure. */
const wikilinkDrop = cardDrop({
  dragged: (e) => (carriesCard(e) ? cardDrag : null),
  onDrop: (card) => void commitDroppedLink(card),
});

/** Clear the editor's drop preview as the pointer leaves the buffer. */
function clearDropPreview(): void {
  editorView?.dispatch({ effects: setDropTarget.of(null) });
}

/** Mark the discovery column as the drag's cancel target (the global dragover's
 *  `dropEffect = "none"` makes AppKit refuse the drop). */
function markSideCancel(on: boolean): void {
  el("side-pane").classList.toggle("is-drag-cancel", on);
}

/**
 * After the link is in the buffer: flush it, then drop the card from Similar. Safe once the
 * write succeeded, since discovery excludes 1-hop neighbours (`discover::candidates`);
 * waiting for the trailing embed's refresh would leave it "unlinked" for seconds.
 */
async function commitDroppedLink(card: DraggedCard): Promise<void> {
  const src = state.current;
  if (!src) return;
  await saveNow();
  // Not saved (conflict or failure): the link stays in the buffer, the card stays listed.
  if (state.editConflict) return;
  if (bufferUnsaved()) return;
  if (state.current?.path !== src.path) return; // navigated away while the save ran
  // Pass the card, not a string: `withoutCard` compares paths, not targets (PR #185).
  state.similar = withoutCard(state.similar, card);
  render();
  flash(`Linked [[${card.target}]].`);
}

/** The keyboard half (K1): insert the link at the caret's line, via the same
 *  `planDrop`/`insertDrop` path as the drop. */
function insertCardLink(path: string): void {
  const view = editorView;
  if (!state.editing || !view || !path) return;
  // The same `DraggedCard` shape the drag carries.
  const card: DraggedCard = { path, target: noteTarget(path) };
  const plan = planDrop(view.state, view.state.selection.main.head, card.target);
  if (plan === null) {
    // The drop refuses silently; a menu item must say why.
    flash("The cursor is inside code — a wikilink there would stay literal. Move it out first.");
    return;
  }
  insertDrop(view, plan);
  view.focus();
  void commitDroppedLink(card);
}

// --- find in note (⌘F) ------------------------------------------------------------
//
// One bar, two engines, one pure core (findbar.ts). The bar is static shell chrome with
// module-local state, so typing never renders. The reading view paints with the CSS Custom
// Highlight API (no DOM mutation, so memo, scroll and delegation are untouched); the
// editor uses findfield.ts's StateField. ⇧⌘F just focuses vault search.

let findOpen = false;
let findQuery = "";
/** The current match set + active index, whichever engine produced it. */
let findMatchesList: Match[] = [];
let findActive = -1;
/** Reading-mode DOM anchors, parallel to findMatchesList; stale after any pane swap. */
let findRanges: globalThis.Range[] = [];
/** The doc the bar is bound to — navigating anywhere else closes it (syncFind). */
let findDocKey: string | null = null;

/** The note pane's document identity; null when nothing is findable (empty, graph). */
function findableDocKey(): string | null {
  if (state.currentResource) return `res:${state.currentResource.path}`;
  if (!state.current) return null;
  if (state.graphOpen && !state.editing) return null;
  return `note:${state.current.path}`;
}

const findInput = () => el("find-input") as HTMLInputElement;

function openFind(): void {
  const key = findableDocKey();
  if (!key) return;
  // Seed from a one-line selection; otherwise keep the last query, preselected.
  const sel =
    state.editing && editorView
      ? editorView.state.sliceDoc(
          editorView.state.selection.main.from,
          editorView.state.selection.main.to,
        )
      : (window.getSelection()?.toString() ?? "");
  findOpen = true;
  findDocKey = key;
  el("find-bar").hidden = false;
  if (sel && !sel.includes("\n")) findInput().value = sel;
  setFindQuery(findInput().value);
  findInput().focus();
  findInput().select();
}

function closeFind(): void {
  if (!findOpen) return;
  findOpen = false;
  findDocKey = null;
  findMatchesList = [];
  findActive = -1;
  el("find-bar").hidden = true;
  clearReadingFind();
  if (state.editing && editorView) {
    editorView.dispatch({ effects: setFindEffect.of(null) });
    editorView.focus();
  }
}

function clearReadingFind(): void {
  findRanges = [];
  if ("highlights" in CSS) {
    CSS.highlights.delete("b2-find");
    CSS.highlights.delete("b2-find-active");
  }
}

/** Recompute matches and Ranges over the rendered note and repaint. Also runs after any
 *  pane swap, which invalidates the Ranges. */
function applyReadingFind(): void {
  const anchor = findMatchesList[findActive]?.from ?? 0;
  clearReadingFind();
  // Only the article is searched; the bars above it are chrome.
  const article = document.querySelector("#note-pane article.note");
  if (!article) {
    findMatchesList = [];
    findActive = -1;
    paintFindBar();
    return;
  }
  const nodes: Text[] = [];
  const walker = document.createTreeWalker(article, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) nodes.push(n as Text);
  const segs = nodes.map((n) => n.data.length);
  findMatchesList = findMatches(nodes.map((n) => n.data).join(""), findQuery);
  findRanges = findMatchesList.map((m) => {
    const s = locate(segs, m.from, "start");
    const e = locate(segs, m.to, "end");
    const r = document.createRange();
    r.setStart(nodes[s.seg], s.off);
    r.setEnd(nodes[e.seg], e.off);
    return r;
  });
  findActive = activeAfter(findMatchesList, anchor);
  paintReadingFind();
  paintFindBar();
}

/** Paint the two highlight layers (all matches + the active one) from findRanges. */
function paintReadingFind(): void {
  if (!("highlights" in CSS)) return; // unsupported: navigation/scroll still work, unpainted
  if (findRanges.length === 0) {
    CSS.highlights.delete("b2-find");
    CSS.highlights.delete("b2-find-active");
    return;
  }
  CSS.highlights.set("b2-find", new Highlight(...findRanges));
  const active = findRanges[findActive];
  if (active) CSS.highlights.set("b2-find-active", new Highlight(active));
  else CSS.highlights.delete("b2-find-active");
}

function scrollFindActiveIntoView(): void {
  const range = findRanges[findActive];
  if (!range) return;
  const pane = el("note-pane");
  const rect = range.getBoundingClientRect();
  const box = pane.getBoundingClientRect();
  // Leave it if already comfortably visible; else bring it to the upper third.
  if (rect.top >= box.top + 96 && rect.bottom <= box.bottom - 40) return;
  pane.scrollTop += rect.top - box.top - pane.clientHeight * 0.35;
}

/** Mirror the editor field's match state into the bar (count pill, button states). */
function syncEditorFind(view: EditorView): void {
  const f = view.state.field(findField, false) ?? null;
  findMatchesList = f?.matches ?? [];
  findActive = f?.active ?? -1;
  paintFindBar();
}

function setFindQuery(q: string): void {
  findQuery = q;
  if (state.editing && editorView) {
    const view = editorView;
    const matches = findMatches(view.state.doc.toString(), q);
    // Anchor on the previous match, else the caret, so typing doesn't jump to the top.
    const anchor = findMatchesList[findActive]?.from ?? view.state.selection.main.from;
    const active = activeAfter(matches, anchor);
    const effects: StateEffect<unknown>[] = [setFindEffect.of({ query: q, active })];
    const m = matches[active];
    if (m) effects.push(EditorView.scrollIntoView(m.from, { y: "center" }));
    view.dispatch({ effects });
    syncEditorFind(view);
  } else {
    applyReadingFind();
    scrollFindActiveIntoView();
  }
}

function findStep(delta: 1 | -1): void {
  if (!findOpen || findMatchesList.length === 0) return;
  if (state.editing && editorView) {
    const view = editorView;
    const f = view.state.field(findField, false);
    if (!f || f.matches.length === 0) return;
    const active = stepActive(f.matches.length, f.active, delta);
    const m = f.matches[active];
    // Select the match (editor convention); focus stays in the bar so Enter keeps stepping.
    view.dispatch({
      selection: { anchor: m.from, head: m.to },
      effects: [
        setFindEffect.of({ query: f.query, active }),
        EditorView.scrollIntoView(m.from, { y: "center" }),
      ],
    });
    syncEditorFind(view);
  } else {
    findActive = stepActive(findMatchesList.length, findActive, delta);
    paintReadingFind();
    scrollFindActiveIntoView();
    paintFindBar();
  }
}

function paintFindBar(): void {
  const pill = el("find-count");
  pill.hidden = findQuery === "";
  pill.textContent = countLabel(
    findMatchesList.length,
    findActive,
    findMatchesList.length === FIND_CAP,
  );
  const none = findMatchesList.length === 0;
  (el("find-prev") as HTMLButtonElement).disabled = none;
  (el("find-next") as HTMLButtonElement).disabled = none;
}

/** render()'s hook: close when the pane shows a different doc; re-derive highlights when
 *  the pane was rebuilt (old Ranges point into detached nodes). */
function syncFind(noteSwapped: boolean): void {
  if (!findOpen) return;
  if (findableDocKey() !== findDocKey) {
    closeFind();
    return;
  }
  if (!state.editing && noteSwapped) applyReadingFind();
}

/** ⇧⌘F: hand the keyboard to the global vault-search box in the top bar. */
function focusGlobalSearch(): void {
  const input = searchInput();
  input?.focus();
  input?.select();
}

// --- external-edit reconciliation (crates/b2-desktop/CLAUDE.md / #14) --------------------
//
// The host emits a debounced `vault-changed` pulse on external disk changes. We reconcile
// by re-reading through the façade, never trusting event paths. Our own saves also pulse,
// but the revision compare below makes them no-ops.

let reconcileInFlight = false;
let reconcilePending = false;

// Serialize reconciles, coalescing overlapping pulses into one trailing run.
async function onVaultChanged(): Promise<void> {
  if (reconcileInFlight) {
    reconcilePending = true;
    return;
  }
  reconcileInFlight = true;
  try {
    do {
      reconcilePending = false;
      await reconcileExternalChange();
    } while (reconcilePending);
  } finally {
    reconcileInFlight = false;
  }
}

async function reconcileExternalChange(): Promise<void> {
  if (state.vaultRoot === null) return;
  // The tree first: project, then re-list, since the tree lists are index-first (#65;
  // reconcile.ts). Safe while editing: projection reads disk, and render() skips the pane.
  await reconcileIndex({
    reindexing: state.reindexing,
    project: api.project,
    list: loadNotes,
    // Re-chunking an edited note drops its vectors, so heal them with the save path's
    // trailing embed. Gated on the N/M coverage read (#26), which can over-fire but never
    // miss.
    vectorsPending: async () => {
      await refreshEmbedStatus(state.vaultRoot);
      return state.notesTotal > 0 && state.notesEmbedded < state.notesTotal;
    },
    healVectors: scheduleTrailingEmbed,
  });

  // The open note, except:
  //   • while editing: never clobber the buffer or adopt a revision under it; the save
  //     guard surfaces the conflict (crates/b2-desktop/CLAUDE.md).
  //   • while reindexing: the run owns the open note's refresh.
  if (state.current && !state.editing && !state.fmEditing && !state.reindexing) {
    const cur = state.current;
    try {
      const fresh = await api.readNote(cur.path);
      // Apply only if the note still owns the pane in reading mode.
      if (state.current?.path === cur.path && !state.editing) {
        // Unchanged bytes (e.g. our own save's echo): skip.
        if (fresh.revision !== cur.revision) {
          state.current = fresh;
          await refreshDiscovery(); // the edit may have changed similar/edges
          flash("Reloaded — this note changed on disk.");
        }
      }
    } catch {
      // Moved or removed: keep the stale pane, but say so.
      if (state.current?.path === cur.path) {
        flash("This note is no longer on disk — it was moved or removed.");
      }
    }
  }

  // The open resource card, same posture.
  if (state.currentResource && !state.reindexing) {
    const cur = state.currentResource;
    try {
      const fresh = await api.explainResource(cur.path);
      // Re-read the bytes too: the picture may have changed in place.
      const picture = await loadResourceImage(fresh);
      if (state.currentResource?.path === cur.path) adoptResource(fresh, picture);
    } catch {
      if (state.currentResource?.path === cur.path) {
        flash("This file is no longer on disk — it was moved or removed.");
      }
    }
  }
  render();
}

// --- shell + events -------------------------------------------------------------

function buildShell(): void {
  el("app").innerHTML = `
    <header class="topbar">
      <div class="brand">B2</div>
      <div class="nav-history">
        <button id="nav-back" class="btn ghost icon-btn" aria-label="Back" disabled>
          ${icon("chevron-left", { size: 15 })}
        </button>
        <button id="nav-forward" class="btn ghost icon-btn" aria-label="Forward" disabled>
          ${icon("chevron-right", { size: 15 })}
        </button>
      </div>
      <form id="search-form" class="search" autocomplete="off">
        <input id="search-input" type="search" aria-label="Search" />
      </form>
      <div class="topbar-right">
        <!-- The vault and its indexing state, as one group: a progress meter is *about*
             a vault, so it reads beside the name of the one being indexed rather than
             floating at the far end of the bar. Hidden between runs, so this is just the
             path almost all of the time. The Reindex button that used to stand here has
             moved into Settings → Index — indexing is automatic now, and permanent chrome
             for an exception trains the eye to skip the bar (settingsview.ts, indexPanelHtml).
             What stays is the live meter and the Cancel that belongs with it — visible
             wherever you are in the app, except behind Settings, which covers the bar and
             so paints a second meter of its own. -->
        <div class="vault-status">
          <span id="vault-root" class="vault-root" title="Active vault"></span>
          <!-- Classes, not ids: this is one of those two meters, and paintReindex writes
               the same values into every one on screen. -->
          ${reindexMeterHtml({ hidden: true, indeterminate: false })}
        </div>
        <button id="open-chat" class="btn ghost icon-btn" aria-label="Ask your notes">
          ${icon("chat-dots", { size: 15 })}
        </button>
        <button id="switch-vault" class="btn ghost icon-btn" title="Switch vault — choose another folder" aria-label="Switch vault">
          ${icon("folder", { size: 15 })}
        </button>
        <button id="open-settings" class="btn ghost icon-btn" aria-label="Settings">
          ${icon("gear", { size: 16 })}
        </button>
      </div>
    </header>
    <div id="embed-banner"></div>
    <main id="layout" class="layout">
      <!-- The three panes carry tabindex="-1" so ⌘1/⌘2/⌘3 can put the keyboard *in* a
           pane (K1) without adding three more stops to the Tab order. The note pane is
           also the scroll container, so focusing it is what lets the arrows read a note. -->
      <nav id="tree-pane" class="tree-pane" tabindex="-1"></nav>
      <div id="gutter-tree" class="gutter" role="separator" aria-orientation="vertical"
           aria-label="Resize the file tree" aria-controls="tree-pane" tabindex="0"
           title="Drag, or ←/→ to resize (⇧ for a bigger step, Home/End for the limits, ⏎ to reset)"
           aria-valuemin="${BOUNDS.tree.min}" aria-valuemax="${BOUNDS.tree.max}"></div>
      <section id="note-pane" class="note-pane" tabindex="-1"></section>
      <div id="gutter-side" class="gutter" role="separator" aria-orientation="vertical"
           aria-label="Resize the discovery pane" aria-controls="side-pane" tabindex="0"
           title="Drag, or ←/→ to resize (⇧ for a bigger step, Home/End for the limits, ⏎ to reset)"
           aria-valuemin="${BOUNDS.side.min}" aria-valuemax="${BOUNDS.side.max}"></div>
      <aside id="side-pane" class="side-pane" tabindex="-1"></aside>
      <div id="find-bar" class="find-bar" role="search" aria-label="Find in note" hidden>
        <div class="find-field">
          ${icon("search", { size: 13, class: "find-glass" })}
          <input id="find-input" type="text" placeholder="Find…" autocomplete="off" spellcheck="false" aria-label="Find in note" />
          <span id="find-count" class="find-count" aria-live="polite" hidden></span>
        </div>
        <button id="find-prev" class="btn ghost icon-btn" aria-label="Previous match">
          ${icon("chevron-up", { size: 15 })}
        </button>
        <button id="find-next" class="btn ghost icon-btn" aria-label="Next match">
          ${icon("chevron-down", { size: 15 })}
        </button>
        <button id="find-close" class="btn ghost icon-btn" aria-label="Close find">
          ${icon("x-lg", { size: 13 })}
        </button>
      </div>
    </main>
    <div id="menu-root"></div>
    <div id="modal-root"></div>
    <!-- Above the overlay layer in DOM order and in the stylesheet's z-index, because it
         is the one thing that is never *under* anything: it appears only when no overlay
         is up — see wireCmdHold — and it must not be painted behind the note pane's own
         stacking contexts. -->
    <div id="cmdhold-root"></div>
    <div id="toast" class="toast" role="status" hidden></div>`;
  paintShellHints();
}

/** Write the shell's chord hints (hints.ts). The shell paints once, so this keeps its
 *  tooltips true after a rebind. */
function paintShellHints(): void {
  for (const [id, hint] of Object.entries(shellHints())) {
    const node = document.getElementById(id);
    if (!node) continue;
    if (hint.title !== undefined) node.title = hint.title;
    if (hint.placeholder !== undefined && node instanceof HTMLInputElement)
      node.placeholder = hint.placeholder;
  }
}

/** A click inside Settings; the caller does nothing else with it. */
function settingsClick(target: HTMLElement): void {
  const tab = target.closest<HTMLElement>("[data-settings-tab]");
  if (tab) {
    const id = tab.dataset.settingsTab ?? null;
    if (isSettingsTab(id)) selectSettingsTab(id, false);
    return;
  }
  if (target.closest("#settings-provision")) {
    void provisionModel();
    return;
  }
  // Settings → Chat's Local/Cloud segments rewrite the URL field (M5; settingsview.ts).
  const chatMode = target.closest<HTMLElement>("[data-chat-mode]");
  if (chatMode) {
    setChatMode(chatMode.dataset.chatMode === "cloud");
    return;
  }
  if (target.closest("#settings-chat-save")) {
    void saveChatConfig();
    return;
  }
  // The Model field's two shapes (settingsview.ts `chatModelFieldHtml`); neither saves.
  if (target.closest("[data-chat-model-custom]")) {
    setChatModelTyped(true);
    return;
  }
  if (target.closest("[data-chat-model-pick]")) {
    setChatModelTyped(false);
    return;
  }
  const useModel = target.closest<HTMLElement>("[data-chat-use-model]");
  if (useModel) {
    void useChatModel(useModel.dataset.chatUseModel ?? "");
    return;
  }
  if (target.closest("[data-chat-clear-key]")) {
    void clearChatKey();
    return;
  }
  // Settings → Index: the manual Reindex. The dialog stays open to show the result.
  if (target.closest("#reindex")) {
    trackIndexing(doReindex());
    return;
  }
  // …and its Cancel (the top bar's, repeated here).
  if (target.closest("[data-cancel-reindex]")) {
    void cancelReindex();
    return;
  }
  const themeBtn = target.closest<HTMLElement>("[data-theme-choice]");
  if (themeBtn) {
    const choice = themeBtn.dataset.themeChoice ?? null;
    if (isThemePref(choice)) setTheme(choice);
    return;
  }
  // Settings → Keyboard: a chord chip opens the recorder; checked before close so a click
  // in the strip never closes the dialog.
  const chip = target.closest<HTMLElement>("[data-rebind]");
  if (chip) {
    const id = chip.dataset.rebind ?? "";
    if (findBinding(activeBindings(), id)) startRecording(id as BindingId);
    return;
  }
  if (target.closest("#keys-save")) {
    commitRecording();
    return;
  }
  if (target.closest("#keys-cancel")) {
    stopRecording();
    return;
  }
  if (target.closest("#keys-reset-one")) {
    if (state.recorder) resetChord(state.recorder.id);
    return;
  }
  if (target.closest("#keys-reset-all")) {
    resetAllChords();
    return;
  }
  if (target.closest("[data-settings-close]")) closeSettings();
}

/** Every listener the app registers. Order matters where two share an event and target
 *  (`wireCmdHold`'s keydown before `wireChords`'). */
function wireEvents(): void {
  wireCmdHold(); // a spectator, so it goes on first
  wireFocusMemory();
  wireFmErrorClear();
  wireClicks();
  wireContextMenu();
  wireTreeKeys();
  wireSideKeys();
  wireMenuDismissal();
  wireFindBar();
  wireForms();
  wireChords();
  wireMouseHistory();
  wireWindowBlur();
  wireDrags();
}

function wireFocusMemory(): void {
  // Track focus continuously for `lastFocused` (K1); `<body>` is not a real target.
  document.addEventListener("focusin", (e) => {
    const t = e.target;
    if ((t instanceof HTMLElement || t instanceof SVGElement) && t !== document.body) {
      lastFocused = t;
    }
  });
}

function wireFmErrorClear(): void {
  // Typing in the frontmatter editor clears its error (delegated: rendered dynamically).
  document.addEventListener("input", (e) => {
    if (state.fmEditing && e.target instanceof HTMLTextAreaElement && e.target.id === "fm-editor") {
      hideFmError();
    }
  });
}

function wireClicks(): void {
  // Delegated clicks for everything that renders dynamically.
  document.addEventListener("click", (e) => {
    const target = e.target as HTMLElement;

    // An open menu owns the next click (a dismissing click isn't also a card click).
    if (state.contextMenu) {
      contextMenuClick(target);
      return;
    }

    // A web link opens in the system browser (`open_external`; links.ts decides which):
    // navigating the webview would replace the app with no way back. Any other href is
    // cancelled with a toast, except in-page anchors, which fall through so wikilinks
    // (`href="#"`) and `#heading` links keep working.
    const anchor = target.closest<HTMLAnchorElement>("a[href]");
    if (anchor) {
      const href = anchor.getAttribute("href");
      const url = externalUrl(href);
      if (url) {
        e.preventDefault();
        api.openExternal(url).catch((err) => flash(errText(err)));
        return;
      }
      if (!isInPageAnchor(href)) {
        e.preventDefault();
        flash("B2 doesn't follow this link — web links open in your browser, [[wikilinks]] open notes.");
      }
    }

    // The tree-head create icons — contextual on the selection's folder.
    if (target.closest("[data-new-note]")) {
      startTreeCreate("note", state.selectedDir);
      return;
    }
    if (target.closest("[data-new-folder]")) {
      startTreeCreate("folder", state.selectedDir);
      return;
    }

    if (target.closest("#open-settings")) {
      void openSettings();
      return;
    }
    if (target.closest("#open-chat")) {
      toggleChat();
      return;
    }
    // The chat pane's chrome; `data-chat-stop` is Esc's mouse equivalent.
    if (target.closest("[data-chat-stop]")) {
      stopChatAnswer();
      return;
    }
    if (target.closest("[data-chat-new]")) {
      newChat();
      return;
    }
    // The setup card's retry, after the user starts the daemon or pulls a model.
    if (target.closest("[data-chat-recheck]")) {
      void refreshChatSetup();
      return;
    }
    // The install banner: open Settings → Embedding, or ✕ for this session. ("Don't remind
    // me again" is in the `change` delegation.)
    if (target.closest("[data-install-open-settings]")) {
      void openSettings("embedding");
      return;
    }
    if (target.closest("[data-install-dismiss]")) {
      dismissEmbedReminder(false);
      return;
    }
    // Settings, before the modal backdrop branch. It fills the window, so there is no
    // click-outside; the ways out are Done and Escape.
    if (state.settingsOpen) {
      settingsClick(target);
      return; // clicks inside Settings do nothing else
    }

    const cancel = target.closest<HTMLElement>("[data-cancel]");
    if (cancel) {
      closeModal();
      return;
    }
    if (target.classList.contains("modal-backdrop")) {
      closeModal();
      return;
    }
    if (target.closest("#link-commit")) {
      void commitLink();
      return;
    }
    // The folder-delete confirm: the Delete button commits and closes it.
    if (target.closest("#delete-confirm") && state.deleteTarget) {
      confirmDelete();
      return;
    }
    // The Move… modal: clicking a destination row commits the move and closes it.
    const moveDest = target.closest<HTMLElement>("[data-move-dest]");
    if (moveDest && state.moveTarget) {
      const node = state.moveTarget;
      const dest = moveDest.dataset.moveDest ?? "";
      state.moveTarget = null;
      void executeMove(node, moveDestination(node.path, dest));
      return;
    }

    const wiki = target.closest<HTMLElement>(".wikilink");
    if (wiki) {
      e.preventDefault();
      const t = wiki.dataset.target;
      if (t) void followWikilink(t);
      return;
    }

    // A click moves the roving tabstop too (WebKit doesn't focus buttons on click). Ahead
    // of the fold/open handlers, which return.
    const sideRow = target.closest<HTMLElement>("#side-pane [data-side-row]");
    if (sideRow) state.sideFocus = sideRow.dataset.sideRow ?? null;

    const foldSection = target.closest<HTMLElement>("[data-fold-section]");
    if (foldSection) {
      const s = foldSection.dataset.foldSection;
      if (s === "similar" || s === "connections") toggleSection(s);
      return;
    }
    const foldCard = target.closest<HTMLElement>("[data-fold-card]");
    if (foldCard) {
      toggleCard(foldCard.dataset.foldCard ?? "");
      return;
    }

    if (target.closest("[data-fm-edit]")) {
      enterFmEdit();
      return;
    }
    if (target.closest("#fm-save")) {
      void saveFmEdit();
      return;
    }
    if (target.closest("#fm-cancel")) {
      void cancelFmEdit();
      return;
    }
    if (target.closest("#fm-reload")) {
      void cancelFmEdit(); // Reload = discard the buffer and adopt disk
      return;
    }
    if (target.closest("#fm-keep")) {
      void fmConflictKeepMine();
      return;
    }

    if (target.closest("[data-toggle-frontmatter]")) {
      toggleFrontmatter();
      return;
    }

    if (target.closest("[data-toggle-source]")) {
      toggleSource();
      return;
    }

    if (target.closest("[data-toggle-graph]")) {
      toggleGraph();
      return;
    }
    // Clicking a ghost opens the link modal; committing solidifies it into a typed edge.
    const ghostNode = target.closest<HTMLElement>("[data-ghost-link]");
    if (ghostNode) {
      openLinkModal(ghostNode.dataset.ghostLink ?? "", ghostNode.dataset.cardTitle ?? "");
      return;
    }

    if (target.closest("[data-toggle-edit]")) {
      enterEdit();
      return;
    }
    if (target.closest("#edit-done")) {
      void exitEdit();
      return;
    }
    if (target.closest("#conflict-reload")) {
      void conflictReload();
      return;
    }
    if (target.closest("#conflict-keep")) {
      void conflictKeepMine();
      return;
    }

    // A click moves the tree's roving tabstop too (WebKit doesn't focus buttons on click).
    const treeRow = target.closest<HTMLElement>("#tree-pane .tree-row[data-tree-row]");
    if (treeRow) state.treeFocus = treeRow.dataset.treeRow ?? null;

    const dir = target.closest<HTMLElement>("[data-dir]");
    if (dir) {
      toggleDir(dir.dataset.dir ?? "");
      return;
    }

    const openRes = target.closest<HTMLElement>("[data-open-resource]");
    if (openRes) {
      const p = openRes.dataset.openResource;
      if (p) void openResource(p);
      return;
    }

    const openSystem = target.closest<HTMLElement>("[data-open-system]");
    if (openSystem) {
      const p = openSystem.dataset.openSystem;
      if (p) api.openResource(p).catch((e) => flash(errText(e)));
      return;
    }

    // *Explain* and its view's controls, before the card's own `data-open`.
    const explain = target.closest<HTMLElement>("[data-explain]");
    if (explain) {
      const p = explain.dataset.explain;
      if (p) void openExplain(p);
      return;
    }
    if (target.closest("[data-explain-close]")) {
      closeExplain();
      return;
    }
    if (target.closest("[data-explain-help]")) {
      if (state.explainCard) state.explainCard.help = !state.explainCard.help;
      render();
      return;
    }
    if (target.closest("[data-explain-all]")) {
      if (state.explainCard) state.explainCard.allPairs = !state.explainCard.allPairs;
      render();
      return;
    }

    // *Why?*, before the card's own `data-open`.
    const why = target.closest<HTMLElement>("[data-why]");
    if (why) {
      const p = why.dataset.why;
      if (p) void askWhy({ path: p, title: why.dataset.whyTitle || null });
      return;
    }

    const open = target.closest<HTMLElement>("[data-open]");
    if (open) {
      const p = open.dataset.open;
      if (p) void openNote(p);
      return;
    }

    if (target.closest("[data-clear-search]")) {
      clearSearch();
      return;
    }
    if (target.closest("#nav-back")) {
      void navGo(-1);
      return;
    }
    if (target.closest("#nav-forward")) {
      void navGo(1);
      return;
    }
    if (target.closest("#switch-vault")) {
      void switchVault();
      return;
    }
    // The top bar's Cancel (Settings' own is in `settingsClick`).
    if (target.closest("[data-cancel-reindex]")) {
      void cancelReindex();
      return;
    }
  });
}

function wireContextMenu(): void {
  // The tree's menu targets the row's folder (or the root) and moves the selection
  // context; Similar cards and graph ghosts get the card menu. Elsewhere the webview's
  // menu is untouched.
  document.addEventListener("contextmenu", (e) => {
    const target = e.target as HTMLElement;
    if (target.closest("#tree-pane") && state.vaultRoot !== null) {
      e.preventDefault();
      // Over a row, the menu also targets that node (Rename / Move…).
      const row = target.closest<HTMLElement>(TREE_ROW);
      const node = row ? treeRowRef(row) : null;
      const dir = node ? folderContext(node.path, node.nodeKind) : "";
      state.selectedDir = dir;
      openTreeMenu(e.clientX, e.clientY, dir, node && node.path ? node : null);
      return;
    }
    const card = target.closest<HTMLElement>(".card.candidate, .gnode.is-ghost");
    if (!card) return;
    e.preventDefault();
    openCardMenu(e.clientX, e.clientY, card.dataset.cardPath ?? "", card.dataset.cardTitle ?? "");
  });
}

function wireTreeKeys(): void {
  // The tree's ARIA `tree` keyboard (K1, GH #78), on the pane so it runs before the global
  // chords. Moves are treenav.ts's. ⏎/Space are absent: rows are buttons, so the platform
  // already clicks them.
  el("tree-pane").addEventListener("keydown", (e) => {
    // The inline create/rename inputs own their keys.
    if (e.target instanceof HTMLInputElement) return;
    const row = (e.target as HTMLElement).closest<HTMLElement>(TREE_ROW);
    if (!row) return;
    const path = row.dataset.treeRow ?? "";
    const rows = treeRows();

    const nav = treeNavFor(e);
    const move = nav ? arrowMove(rows, rowIndex(rows, path), nav) : null;
    if (move) {
      e.preventDefault();
      if (move.kind === "focus") {
        focusTreeRow(move.path);
      } else {
        // Folding keeps focus in place.
        if (move.kind === "expand") state.expandedDirs.add(move.path);
        else state.expandedDirs.delete(move.path);
        state.selectedDir = move.path; // folding a folder makes it the create context, as a click does
        state.treeFocus = move.path;
        render();
        treeRowEl(move.path)?.focus();
      }
      return;
    }

    // First-letter typeahead, on bare printable keys only.
    if (e.key.length === 1 && e.key !== " " && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const hit = typeaheadTarget(rows, rowIndex(rows, path), e.key);
      if (hit !== null) {
        e.preventDefault();
        focusTreeRow(hit);
      }
    }
  });
}

function wireSideKeys(): void {
  // Discovery's ARIA `tree` keyboard (K1, GH #78); moves are sidenav.ts's. A card row is a
  // `<div>` containing its open button, so ⏎/Space click that button
  // (crates/b2-desktop/CLAUDE.md); button rows use the platform's activation.
  el("side-pane").addEventListener("keydown", (e) => {
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-side-row]");
    if (!row) return;
    const key = row.dataset.sideRow ?? "";
    const rows = sideRows(state);

    const nav = sideNavFor(e);
    const move = nav ? sideArrowMove(rows, sideRowIndex(rows, key), nav) : null;
    if (move) {
      e.preventDefault();
      if (move.kind === "focus") {
        focusSideRow(move.key);
        return;
      }
      // Folding keeps focus in place. State tracks what's collapsed.
      const open = move.kind === "expand";
      if (move.fold.kind === "section") {
        if (open) state.collapsedSections.delete(move.fold.section);
        else state.collapsedSections.add(move.fold.section);
      } else if (open) {
        state.collapsedCards.delete(move.fold.key);
      } else {
        state.collapsedCards.add(move.fold.key);
      }
      state.sideFocus = move.key;
      render();
      sideRowEl(move.key)?.focus();
      return;
    }

    if (e.key === "Enter" || e.key === " ") {
      // Only when a non-button row itself holds focus, or the note opens twice.
      if (e.target !== row || row instanceof HTMLButtonElement) return;
      // Before the lookup: an unresolved row has no button, and Space would scroll (PR #90).
      e.preventDefault();
      const open = row.querySelector<HTMLElement>(".card-open");
      if (!open) return; // an unresolved link points at nothing — ⏎/Space do nothing
      open.click();
    }
  });
}

function wireMenuDismissal(): void {
  // The menu is at fixed viewport coords, so a scroll or resize dismisses it. Capture
  // phase, since scroll doesn't bubble.
  document.addEventListener("scroll", closeContextMenu, true);
  window.addEventListener("resize", closeContextMenu);
}

function wireFindBar(): void {
  // Static chrome, so direct listeners. Preventing mousedown keeps focus in the input.
  findInput().addEventListener("input", (e) => setFindQuery((e.target as HTMLInputElement).value));
  const findButtons: [string, () => void][] = [
    ["find-prev", () => findStep(-1)],
    ["find-next", () => findStep(1)],
    ["find-close", closeFind],
  ];
  for (const [id, act] of findButtons) {
    const btn = el(id);
    btn.addEventListener("mousedown", (e) => e.preventDefault());
    btn.addEventListener("click", act);
  }
}

function wireForms(): void {
  // Search on submit (Enter).
  document.addEventListener("submit", (e) => {
    if ((e.target as HTMLElement).id === "search-form") {
      e.preventDefault();
      void doSearch(searchInput()?.value ?? "");
    }
    // The chat composer is a form, so its Ask button submits (`chat.send` is fixed).
    if ((e.target as HTMLElement).id === "chat-composer") {
      e.preventDefault();
      const input = document.getElementById("chat-input") as HTMLTextAreaElement | null;
      void sendChat(input?.value ?? "");
    }
  });

  // Keep the modal's verb preview in sync with the relation select.
  document.addEventListener("change", (e) => {
    const t = e.target as HTMLElement;
    if (t.id === "link-relation") {
      state.linkRelation = (t as HTMLSelectElement).value;
      const preview = document.getElementById("modal-verb");
      if (preview) preview.textContent = state.linkRelation;
    }
    if (t.id === "settings-model") {
      void changeModel((t as HTMLSelectElement).value);
    }
    // The install banner's "Don't remind me again": persist and dismiss.
    if (t instanceof HTMLInputElement && t.matches("[data-install-remind-off]")) {
      dismissEmbedReminder(true);
    }
  });

  // The inline create/rename inputs commit on blur (VS Code-style; empty or unchanged backs
  // out). `isConnected` skips an input torn down by a repaint or its own commit.
  document.addEventListener("focusout", (e) => {
    const t = e.target as HTMLElement;
    if (t.id === "tree-create-input" && t.isConnected && state.treeCreate) {
      void commitTreeCreate((t as HTMLInputElement).value, false);
    }
    if (t.id === "tree-rename-input" && t.isConnected && state.treeRename) {
      void commitTreeRename((t as HTMLInputElement).value);
    }
  });
}

function wireChords(): void {
  // The app's chords. `isBound` asks the registry (bindings.ts) which chord is which; this
  // handler owns the order surfaces get their turn (innermost first) and each command's
  // guard.
  document.addEventListener("keydown", (e) => {
    // The recorder first: while open, keystrokes are chords to record, not commands.
    if (recorderKeydown(e)) return;
    // The inline create input owns its keys; nothing leaks to the chords below.
    if (state.treeCreate && (e.target as HTMLElement).id === "tree-create-input") {
      if (isBound(e, "create.commit")) {
        e.preventDefault();
        void commitTreeCreate((e.target as HTMLInputElement).value, true);
      } else if (isBound(e, "create.cancel")) {
        e.preventDefault();
        cancelTreeCreate();
      }
      return;
    }
    // The rename input owns its keys the same way.
    if (state.treeRename && (e.target as HTMLElement).id === "tree-rename-input") {
      if (isBound(e, "rename.commit")) {
        e.preventDefault();
        void commitTreeRename((e.target as HTMLInputElement).value);
      } else if (isBound(e, "rename.cancel")) {
        e.preventDefault();
        cancelTreeRename();
      }
      return;
    }
    // The chat composer's ⏎. Other keys fall through, so ⌘J, ⌘, and Esc work from it;
    // ⇧⏎ stays the textarea's newline.
    if ((e.target as HTMLElement).id === "chat-input" && isBound(e, "chat.send")) {
      e.preventDefault();
      void sendChat((e.target as HTMLTextAreaElement).value);
      return;
    }
    // Settings' rail (K1, ARIA tabs; settingstabs.ts), above the Tab trap, which would
    // swallow ⌃Tab. ⌃Tab cycles sections from anywhere in the dialog; ↑↓/Home/End only
    // on a tab, since elsewhere they belong to the panel.
    if (state.settingsOpen) {
      const forward = isBound(e, "settings.section.next");
      if (forward || isBound(e, "settings.section.prev")) {
        e.preventDefault();
        selectSettingsTab(tabStep(state.settingsTab, forward ? 1 : -1), true);
        return;
      }
      const onRail =
        document.activeElement instanceof HTMLElement &&
        document.activeElement.closest("[data-settings-tab]") !== null;
      const nav = onRail ? tabNavFor(e) : null;
      if (nav) {
        e.preventDefault();
        selectSettingsTab(tabMove(state.settingsTab, nav), true);
        return;
      }
    }
    // An open overlay traps Tab (K1), or focus walks the page behind the backdrop.
    if (isBound(e, "overlay.focus.step") && currentOverlay() !== null) {
      // `Any-Tab`, swallowed unconditionally: no Tab may reach the page, whatever the
      // modifiers or focusables.
      e.preventDefault();
      const items = overlayFocusables();
      if (items.length > 0) {
        const i = items.indexOf(document.activeElement as HTMLElement);
        const step = e.shiftKey ? -1 : 1;
        const next =
          i < 0 ? (e.shiftKey ? items.length - 1 : 0) : (i + step + items.length) % items.length;
        items[next].focus();
      }
      return;
    }
    // ↑/↓ walk an open context menu (⏎ needs no binding: the items are buttons).
    if (state.contextMenu) {
      const down = isBound(e, "menu.item.next");
      if (down || isBound(e, "menu.item.prev")) {
        const items = overlayFocusables();
        if (items.length > 0) {
          e.preventDefault();
          const i = items.indexOf(document.activeElement as HTMLElement);
          const n = items.length;
          const next = i < 0 ? (down ? 0 : n - 1) : (i + (down ? 1 : -1) + n) % n;
          items[next].focus();
        }
        return;
      }
    }
    // The find input: Enter steps (⇧Enter back), Escape closes; the rest falls through.
    if (findOpen && (e.target as HTMLElement).id === "find-input") {
      const forward = isBound(e, "find.input.next");
      if (forward || isBound(e, "find.input.prev")) {
        e.preventDefault();
        findStep(forward ? 1 : -1);
        return;
      }
      if (isBound(e, "find.input.close")) {
        e.preventDefault();
        closeFind();
        return;
      }
    }
    // ⌘F — find in the open note; ⇧⌘F — jump to the global vault-search box.
    const vaultSearch = isBound(e, "search.focus");
    if (vaultSearch || isBound(e, "find.open")) {
      if (currentOverlay() !== null) return;
      e.preventDefault();
      if (vaultSearch) focusGlobalSearch();
      else openFind();
      return;
    }
    // ⌘G / ⇧⌘G — the classic find-next/previous chords, live while the bar is open.
    if (findOpen) {
      const forward = isBound(e, "find.next");
      if (forward || isBound(e, "find.prev")) {
        e.preventDefault();
        findStep(forward ? 1 : -1);
        return;
      }
    }
    // ⌘J: chat. Allowed while editing, since it doesn't take the editor's pane.
    if (isBound(e, "chat.toggle")) {
      if (currentOverlay() !== null) return;
      e.preventDefault();
      toggleChat();
      return;
    }
    // ⌘G: the graph. Below the find branch, where ⌘G is Find Next (bindings.test.ts pins
    // the shadow). Refused while editing: the editor owns the pane.
    if (isBound(e, "graph.toggle")) {
      if (currentOverlay() !== null || state.editing) return;
      e.preventDefault();
      toggleGraph();
      return;
    }
    const newFolder = isBound(e, "tree.new-folder");
    if (newFolder || isBound(e, "tree.new-note")) {
      if (currentOverlay() !== null) return; // an overlay owns the keyboard
      e.preventDefault();
      startTreeCreate(newFolder ? "folder" : "note", state.selectedDir);
      return;
    }
    // ⇧F10 / Menu key: the keyboard's right-click, anchored under the focused tree row or
    // card / graph ghost.
    if (isBound(e, "menu.open")) {
      if (currentOverlay() !== null || state.vaultRoot === null) return;
      const row = focusedTreeRow();
      if (row) {
        e.preventDefault();
        const node = treeRowRef(row);
        const dir = folderContext(node.path, node.nodeKind);
        state.selectedDir = dir;
        const box = row.getBoundingClientRect();
        openTreeMenu(box.left + 12, box.bottom, dir, node.path ? node : null);
        return;
      }
      const active = document.activeElement;
      const card =
        active instanceof HTMLElement || active instanceof SVGElement
          ? active.closest<HTMLElement>(".card.candidate, .gnode.is-ghost")
          : null;
      if (card) {
        e.preventDefault();
        const box = card.getBoundingClientRect();
        openCardMenu(
          box.left + 12,
          box.bottom,
          card.dataset.cardPath ?? "",
          card.dataset.cardTitle ?? "",
        );
      }
      return;
    }
    // F2: rename the focused tree row.
    if (isBound(e, "tree.rename")) {
      const row = focusedTreeRow();
      if (!row) return;
      e.preventDefault();
      startTreeRename(treeRowRef(row));
      return;
    }
    // ?: toggle Settings → Keyboard. Bare `?`, so never in a text surface; other overlays
    // own the keyboard first.
    if (isBound(e, "help.keyboard")) {
      if (state.editing || inTextEntry()) return;
      const overlay = currentOverlay();
      if (overlay !== null && overlay !== "settings") return;
      e.preventDefault();
      if (state.settingsOpen && state.settingsTab === "keyboard") closeSettings();
      else void openSettings("keyboard");
      return;
    }
    // ⌘1 / ⌘2 / ⌘3: put the keyboard in the files, the note, or discovery (K1).
    const focusPane = isBound(e, "pane.tree")
      ? focusTreePane
      : isBound(e, "pane.note")
        ? focusNotePane
        : isBound(e, "pane.discovery")
          ? focusSidePane
          : null;
    if (focusPane) {
      if (currentOverlay() !== null) return;
      e.preventDefault();
      focusPane();
      return;
    }
    // Enter commits the link modal from anywhere inside it.
    if (state.linkTarget && isBound(e, "link.commit")) {
      e.preventDefault();
      void commitLink();
      return;
    }
    // Enter commits the folder-delete confirm.
    if (state.deleteTarget && isBound(e, "delete.confirm")) {
      e.preventDefault();
      confirmDelete();
      return;
    }
    // ⏎ / Space on a graph node: SVG has no button activation, so dispatch a click.
    if (isBound(e, "graph.activate")) {
      const active = document.activeElement;
      const node =
        active instanceof SVGElement || active instanceof HTMLElement
          ? active.closest(".gnode[tabindex]")
          : null;
      if (node) {
        e.preventDefault();
        node.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        return;
      }
    }
    // ⌘⌫: delete the focused tree row, else the open document. Never in a text field or
    // while editing, where it is delete-to-line-start.
    if (isBound(e, "delete.focused")) {
      if (currentOverlay() !== null) return;
      if (state.editing || inTextEntry()) return;
      const row = focusedTreeRow();
      const node: TreeNodeRef | null = row
        ? treeRowRef(row)
        : state.current
          ? { path: state.current.path, nodeKind: "note", label: baseName(state.current.path) }
          : state.currentResource
            ? {
                path: state.currentResource.path,
                nodeKind: "resource",
                label: baseName(state.currentResource.path),
              }
            : null;
      if (!node) return;
      e.preventDefault();
      requestDelete(node);
      return;
    }
    if (isBound(e, "settings.toggle")) {
      e.preventDefault();
      if (state.settingsOpen) closeSettings();
      else void openSettings();
      return;
    }
    // ⌘E toggles edit mode (CodeMirror leaves Mod-e unbound, so it works while editing).
    if (isBound(e, "edit.toggle")) {
      if (currentOverlay() !== null) return;
      if (state.editing) {
        e.preventDefault();
        void exitEdit();
      } else if (state.current && !state.currentResource) {
        e.preventDefault();
        enterEdit();
      }
      return;
    }
    // ⇧⌘E: the `</>` toggle. Works while editing (unbound in CodeMirror; editorkeys.test.ts);
    // refused with the graph up, where the flip would be invisible.
    if (isBound(e, "source.toggle")) {
      if (currentOverlay() !== null || state.graphOpen) return;
      if (!state.current || state.currentResource) return;
      e.preventDefault();
      toggleSource();
      return;
    }
    if (isBound(e, "dismiss")) {
      // Innermost first.
      if (state.contextMenu) {
        closeContextMenu();
        return;
      }
      if (state.settingsOpen) {
        closeSettings();
        return;
      }
      if (state.linkTarget || state.moveTarget || state.deleteTarget) {
        closeModal();
        return;
      }
      // The frontmatter mini-editor: Esc discards.
      if (state.fmEditing) {
        void cancelFmEdit();
        return;
      }
      // The find bar, from anywhere.
      if (findOpen) {
        closeFind();
        return;
      }
      // Chat: the first Esc stops a streaming answer, a second closes the pane. Gated on
      // the pane being open, since `chatStreaming` outlives `closeChat` by a tick.
      if (state.chatOpen) {
        if (stopChatAnswer()) return;
        closeChat();
        return;
      }
      // Then back out of Explain, then the graph.
      if (state.explainCard && !state.editing) {
        closeExplain();
        return;
      }
      if (state.graphOpen && state.current && !state.editing) toggleGraph();
      return;
    }
    if (state.editing && isBound(e, "editor.save")) {
      e.preventDefault();
      void saveNow();
      return;
    }
    // ⌘⏎ / ⌘S save the frontmatter mini-editor; plain Enter stays a newline.
    if (state.fmEditing && isBound(e, "fm.save")) {
      e.preventDefault();
      void saveFmEdit();
      return;
    }
    // ⌘[ / ⌘] (and ⌘←/⌘→) walk history (#52). Not while editing (CodeMirror owns them) or
    // under a modal; in an input only the arrows mean caret movement, so brackets still
    // navigate.
    const back = isBound(e, "nav.back");
    if ((back || isBound(e, "nav.forward")) && !state.editing) {
      if (currentOverlay() !== null) return;
      if (canonicalKey(e.key).startsWith("Arrow") && inTextEntry()) return;
      e.preventDefault();
      void navGo(back ? -1 : 1);
    }
  });
}

function wireMouseHistory(): void {
  // Mouse back/forward buttons (3 back, 4 forward). `auxclick` is non-primary only.
  document.addEventListener("auxclick", (e) => {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    void navGo(e.button === 3 ? -1 : 1);
  });
}

function wireWindowBlur(): void {
  // Losing window focus flushes the buffer. It is also the recorder's one positive signal
  // (recorder.ts): something outside B2 took the chord.
  window.addEventListener("blur", () => {
    if (state.editing) void saveNow();
    if (state.recorder && state.recorder.candidate === null) {
      cancelProbe(); // the blur answered; see `cancelProbe`
      state.recorder.blurred = true;
      state.recorder.hint = silenceHint({ elapsedMs: Date.now() - recorderOpenedAt, blurred: true });
      render();
    }
  });
}

function wireDrags(): void {
  // --- tree drag-and-drop ---------------------------------------------------------
  //
  // Two drags, told apart by `treeDrag`: a tree row being moved, and a file from outside
  // being imported.
  //
  // Both need `dragDropEnabled: false` (tauri.conf.json), or wry swallows the DOM drag
  // events; so imports take bytes, not paths. It also means an unhandled file drop makes
  // WebKit navigate to the file, replacing the app, so every file drag is cancelled.
  //
  // A drop on a file row lands in its folder; the background is the root. Validity is
  // `canMoveInto` (move.ts). The highlight is imperative: a render() per dragover would
  // fight the drag.
  let treeDrag: TreeNodeRef | null = null;
  let dropHighlight: Element | null = null;

  const clearDropHighlight = () => {
    dropHighlight?.classList.remove("is-drop-target");
    dropHighlight = null;
  };
  /** The drop context under the cursor: a row element + its destination folder. */
  const dropTargetOf = (target: HTMLElement): { el: Element; dir: string } | null => {
    const pane = target.closest("#tree-pane");
    if (!pane) return null;
    const row = target.closest<HTMLElement>(TREE_ROW);
    if (!row) return { el: pane, dir: "" };
    const node = treeRowRef(row);
    return { el: row, dir: folderContext(node.path, node.nodeKind) };
  };

  document.addEventListener("dragstart", (e) => {
    const target = e.target as HTMLElement;
    // A discovery card (droplink.ts), under a private MIME: CodeMirror would insert
    // `text/plain` on its own.
    const card = target.closest<HTMLElement>("#side-pane .card.candidate");
    if (card) {
      const path = card.dataset.cardPath ?? "";
      if (!state.editing || !path) {
        e.preventDefault(); // nothing to drop into — don't start a drag that can't land
        return;
      }
      cardDrag = { path, target: noteTarget(path) };
      if (e.dataTransfer) {
        e.dataTransfer.setData(CARD_DRAG_MIME, path);
        e.dataTransfer.effectAllowed = "copy";
      }
      return;
    }
    if (!target.closest("#tree-pane")) return;
    const row = target.closest<HTMLElement>(TREE_ROW);
    const node = row ? treeRowRef(row) : null;
    treeDrag = node && node.path ? node : null;
    if (!treeDrag) return;
    if (e.dataTransfer) {
      e.dataTransfer.setData("text/plain", treeDrag.path);
      e.dataTransfer.effectAllowed = "move";
    }
  });

  /** Is this drag carrying files from outside the app (as opposed to text, or a row)? */
  const carriesFiles = (e: DragEvent) => e.dataTransfer?.types.includes("Files") ?? false;

  /** Is the pointer over the editor's buffer — the one surface that accepts a card? */
  const overBuffer = (e: DragEvent) =>
    e.target instanceof Element && e.target.closest(".cm-content") !== null;

  /** Would WebKit navigate (losing the app) if this were dropped unhandled? Files and
   *  dragged links. */
  const navigatesWebview = (e: DragEvent) =>
    carriesFiles(e) || (e.dataTransfer?.types.includes("text/uri-list") ?? false);

  /** Where an external file drag may land: a tree target, or nowhere. */
  const importTargetOf = (e: DragEvent) =>
    !carriesFiles(e) || state.vaultRoot === null
      ? null
      : dropTargetOf(e.target as HTMLElement);

  // Some engines need dragenter cancelled too before treating an element as a drop zone.
  document.addEventListener("dragenter", (e) => {
    if (!treeDrag && navigatesWebview(e)) e.preventDefault();
  });

  document.addEventListener("dragover", (e) => {
    // The card drag. The editor handles the buffer; elsewhere, clear the preview and let
    // the OS refuse the drop (a cancel).
    if (carriesCard(e)) {
      if (overBuffer(e)) {
        markSideCancel(false);
        return;
      }
      clearDropPreview();
      markSideCancel(e.target instanceof Element && e.target.closest("#side-pane") !== null);
      if (e.dataTransfer) e.dataTransfer.dropEffect = "none";
      return;
    }
    if (!treeDrag) {
      if (!navigatesWebview(e)) return; // plain text: the editor's business, not ours
      // Unconditional, over every pane: this is what stops WebKit navigating.
      e.preventDefault();
      const drop = importTargetOf(e);
      if (drop?.el !== dropHighlight) clearDropHighlight();
      // "none" outside the tree, so the cursor shows it and AppKit refuses the drop.
      if (e.dataTransfer) e.dataTransfer.dropEffect = drop ? "copy" : "none";
      if (drop && drop.el !== dropHighlight) {
        dropHighlight = drop.el;
        drop.el.classList.add("is-drop-target");
      }
      return;
    }
    const drop = dropTargetOf(e.target as HTMLElement);
    const valid = drop !== null && canMoveInto(treeDrag.path, treeDrag.nodeKind, drop.dir);
    if (drop?.el !== dropHighlight) clearDropHighlight();
    if (!valid) return;
    e.preventDefault(); // this is what makes the target droppable
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    if (drop.el !== dropHighlight) {
      dropHighlight = drop.el;
      drop.el.classList.add("is-drop-target");
    }
  });

  document.addEventListener("drop", (e) => {
    // The editor already handled a card dropped in the buffer; elsewhere it's a cancel.
    if (carriesCard(e)) {
      cardDrag = null;
      markSideCancel(false);
      clearDropPreview();
      return;
    }
    const drag = treeDrag;
    treeDrag = null;
    clearDropHighlight();
    if (!drag) {
      if (!navigatesWebview(e)) return;
      e.preventDefault(); // belt and braces — a drop must never navigate the webview
      const drop = importTargetOf(e);
      if (drop === null) return; // a link, or a pane that imports nothing
      // Read the transfer *now*: it is neutered the moment this handler returns.
      void importDroppedFiles(drop.dir, droppedFiles(e.dataTransfer));
      return;
    }
    const drop = dropTargetOf(e.target as HTMLElement);
    if (drop === null || !canMoveInto(drag.path, drag.nodeKind, drop.dir)) return;
    e.preventDefault();
    void executeMove(drag, moveDestination(drag.path, drop.dir));
  });

  // An external drag leaving the window fires no dragend; `relatedTarget` is null then.
  document.addEventListener("dragleave", (e) => {
    if (!treeDrag && e.relatedTarget === null) clearDropHighlight();
  });

  document.addEventListener("dragend", () => {
    treeDrag = null;
    clearDropHighlight();
    // Escape mid-drag, or a release the OS refused, ends here rather than at `drop`.
    cardDrag = null;
    markSideCancel(false);
    clearDropPreview();
  });
}

// --- boot -----------------------------------------------------------------------

/**
 * Check `menukeys.ts`'s offline mirror of `menu.rs` against the host (#119); this is the
 * mirror's only check. Drift is a developer's bug, so it goes to the console.
 */
async function checkMenuDrift(): Promise<void> {
  try {
    const drift = menuDrift(await api.menuChords());
    if (drift.length > 0) {
      console.error(
        `[b2] ui/src/menukeys.ts no longer matches the host's menu:\n  ${drift.join("\n  ")}`,
      );
    }
  } catch (e) {
    console.error(`[b2] could not read the menu bar's chords: ${errText(e)}`);
  }
}

async function boot(): Promise<void> {
  loadTheme(); // stamp the saved appearance onto <html> before the first paint
  await loadZoomPref(); // awaited: it changes the viewport
  initMenuCommands(); // View ▸ Zoom In / Zoom Out / Actual Size arrive from the host
  const lostChords = loadKeymap(); // the user's chords, before anything paints or dispatches one
  loadEmbedReminderPref(); // before the banner can paint
  buildShell();
  initPanes(el("layout")); // restore the saved column widths before the paint
  wireEvents();
  // Auto-reload on external edits (#14). The host re-points its watch on a vault switch,
  // so one subscription suffices.
  void api.onVaultChanged(() => void onVaultChanged());
  void checkMenuDrift(); // menukeys.ts vs. the host's own menu — never blocks the paint
  try {
    const info = await api.vaultInfo();
    state.vaultRoot = info.root;
    adoptCoverage(info);
    // Populate the file tree so the vault is navigable before anything is opened.
    await loadNotes();
  } catch (e) {
    // No vault (or another startup failure): the note pane shows the actionable state.
    state.vaultRoot = null;
    flash(errText(e));
  }
  if (lostChords.length > 0) {
    // Reported now that there is a shell, after any startup failure's notice.
    flash(`${lostChords.length} saved shortcut(s) couldn't be applied — those are back to their defaults.`);
  }
  render();
  // Auto-index on launch (#25); a no-op with no vault or a complete index.
  trackIndexing(autoIndexOnOpen(state.vaultRoot));
}

void boot();
