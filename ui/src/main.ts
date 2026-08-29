// The controller: build the shell once, wire events (delegated), run the actions
// that mutate `state` and re-render. No framework — the app is small enough that a
// full-pane innerHTML swap on each change is instant and keeps the model honest.
// All backend access goes through `api` (the one IPC seam); this file holds the UI
// flow, never engine logic.

import "../style.css";
import {
  autocompletion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import { history } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxHighlighting, syntaxTree } from "@codemirror/language";
import {
  Compartment,
  EditorSelection,
  type EditorState,
  StateEffect,
  StateField,
  type Extension,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  type KeyBinding,
  keymap,
  tooltips,
} from "@codemirror/view";
import { api, errText, isWriteConflict } from "./api";
import { state, type SideSection, type ThemePref, type TreeNodeRef } from "./state";
import { dirChain, joinPath, normalizeName, parentDir } from "./newentry";
import { systemPath } from "./copypath";
import { bytesToBase64, importSummary, planImport } from "./importfiles";
import {
  baseName,
  canMoveInto,
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
import { sideArrowMove, sideNavFor, sideRowIndex, sideRows } from "./sidenav";
import { answerMessage, chatHistory, errorMessage, userMessage } from "./chat";
import { isSettingsTab, tabMove, tabNavFor, tabStep, type SettingsTabId } from "./settingstabs";
import { externalUrl, isInPageAnchor } from "./links";
import { livePreview, wikilink } from "./livepreview";
import { b2Highlighter, highlightCodeBlocks, resolveLang } from "./highlight";
import { noteTarget, wikiCandidates, wikiInsertion, wikiQueryAt } from "./wikicomplete";
import {
  CARD_DRAG_MIME,
  cardDrop,
  type DraggedCard,
  inCodeAt,
  insertDrop,
  planDrop,
  setDropTarget,
  withoutCard,
} from "./droplink";
import { FORMATS, insertTable, toggleInline, type InlineFormat } from "./format";
import { indentList, outdentList, type ListEdit } from "./list";
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
import { markdownForPaste } from "./paste";
import { icon } from "./icons";
import { activeAfter, countLabel, FIND_CAP, findMatches, locate, stepActive, type Match } from "./findbar";
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
import type { ResourceExplainView } from "./types";
import {
  contextMenuHtml,
  embedBannerHtml,
  escapeHtml,
  imageDataUrl,
  IMAGE_VIEWER_MAX_BYTES,
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
 * Where the keyboard is inside `pane`, as a thunk that finds it again after the pane's
 * `innerHTML` is swapped — the one mechanism every repaint here owes the keyboard
 * (crates/b2-desktop/CLAUDE.md, "Two things that bite"). The swap destroys the focused
 * element and WebKit silently drops the keyboard to `<body>`, so the element itself is
 * never the thing to hold on to: what survives is an identity the next paint re-emits —
 * a discovery row's key (sidenav.ts), a graph node's scene id (graph.ts), or a control's
 * stable `id`. Null when the keyboard was somewhere else entirely, because a repaint
 * must only ever *give back* focus, never take it.
 *
 * One capture serves both panes: the two row attributes are pane-specific by
 * construction (`data-side-row` is painted only by the side pane, `data-gnode` only by
 * the note pane). What a pane paints *without* an identity — a wikilink in a note's
 * body, a backlink card — falls back to the pane itself, which at least leaves the
 * keyboard in the column it was in (⌘1/⌘2/⌘3's own landing spot) instead of at the top
 * of the window.
 */
function capturePaneFocus(pane: HTMLElement): (() => void) | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement || active instanceof SVGElement)) return null;
  if (!pane.contains(active)) return null;
  const row = active.closest<HTMLElement>("[data-side-row]");
  if (row) {
    // A row that didn't survive the repaint (its section folded, a new note replaced
    // discovery wholesale) hands off to the roving tabstop rather than to nothing.
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
 * `capturePaneFocus`'s counterpart for the overlay layer. `#modal-root` is swapped
 * wholesale on a repaint, so a modal control holding the keyboard — a Settings tab, the
 * theme segment you just pressed, the link modal's verb — is destroyed by a toast timer,
 * a watcher pulse, or the dialog's own state change, and WebKit drops focus to `<body>`.
 *
 * Restored by **`id`**: a modal control's id is the identity that outlives the swap
 * (render.ts's Settings builder says so out loud, which is why every control in there
 * carries one). Null when the keyboard was somewhere else entirely, or on a control with
 * no id — a repaint must only ever *give back* focus, never take it, and guessing a
 * replacement for an unidentifiable control is taking it.
 *
 * When the named control genuinely didn't survive — Settings' Download button *becomes*
 * a spinner the moment you press it — the floor is the overlay's first stop rather than
 * nothing, the way `capturePaneFocus`'s is the pane itself. Dropping the keyboard on
 * `<body>` behind a backdrop is the one outcome an overlay may never produce.
 *
 * "Didn't survive" means *can't take the keyboard*, not merely "is gone": Settings →
 * Index's Reindex button is still there after you press it and **disabled**, which
 * `.focus()` silently declines — the same `<body>` outcome, arrived at through an element
 * that exists. Hence the membership test against `overlayFocusables()` (whose selector
 * already excludes `[disabled]`) rather than a null check.
 */
function captureModalFocus(root: HTMLElement): (() => void) | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || !root.contains(active) || !active.id) return null;
  const id = active.id;
  // What the human has *typed* into the control the keyboard is in, if it is a field.
  // A modal's fields are painted from state (`value="…"`), so a repaint the user didn't
  // cause — Settings → Chat's probe landing seconds after the dialog opened — otherwise
  // discards the endpoint they were half-way through typing. This is `captureChatInput`'s
  // rule for the overlay layer, and deliberately narrower than "restore every field":
  // only the focused one is carried, so a repaint that is *meant* to rewrite a field the
  // user is not in (picking an installed model rewrites the model field) still does.
  //
  // A `<select>` is the same promise about a different gesture: Settings → Chat's Model
  // field is a picker over what the daemon has, and a choice made and not yet saved is
  // exactly as uncommitted as a half-typed endpoint. Without this it reverts to the
  // configured model on the next repaint and *Save and test* saves what was already
  // there — a button that appears to do nothing.
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
      // `selectionStart` is null on an input type that doesn't support selection
      // (`type="number"`, and some engines for `type="password"`); fall back to the end
      // of the text rather than throwing on the way to restoring focus.
      target.setSelectionRange?.(
        typed.start ?? typed.value.length,
        typed.end ?? typed.value.length,
      );
    }
    // Only a choice the repainted list still offers. Assigning an absent value to a
    // `<select>` sets `selectedIndex` to -1 and its value to `""` — which would reach the
    // save as "no model" and reset the configuration, a far worse outcome than the
    // reversion this is preventing. So a model that vanished from the inventory between
    // the two paints simply loses the pending choice, and the field shows what is true.
    if (typed && target === back && target instanceof HTMLSelectElement) {
      const offered = Array.from(target.options).some((o) => o.value === typed.value);
      if (offered) target.value = typed.value;
    }
    target?.focus();
  };
}

// The overlay layer's memo, and the reason it is worth having beyond the focus contract
// above: a modal's *typed* state lives only in the DOM (the link modal's explanation
// field), so an unrelated repaint with identical HTML must not swap it away mid-sentence.
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

// The note pane's last-written HTML, for the render memo below. Cleared whenever the
// pane is owned imperatively (edit mode writes its own DOM) so exiting always repaints.
let lastNotePaneHtml: string | null = null;
// The tree pane's memo — same idea, different reason: while an inline create input
// is open (`state.treeCreate`), its typed name lives only in the DOM, so an
// unrelated repaint (a toast timer, streamed progress) must not rebuild the pane
// under the user's cursor. Identical HTML skips the swap entirely; a real tree
// change swaps and then restores the input's value, caret, and focus below.
let lastTreePaneHtml: string | null = null;

/** Repaint the tree pane (memoized), carrying an open create/rename input across
 *  the swap. A fresh rename input (first paint) gets its prefilled name selected,
 *  so typing replaces it wholesale — the platform rename affordance. */
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
  // Whether the keyboard is *in* the tree right now (K1): the innerHTML swap destroys
  // the focused row and focus silently falls back to <body>, so an unrelated repaint —
  // a toast timer, a watcher pulse, streamed reindex progress — would eject a keyboard
  // user from the tree mid-navigation. Restored by path, not by element, since the
  // element the focus was on no longer exists after the swap.
  const hadRowFocus = focusedTreeRow() !== null;
  el("tree-pane").innerHTML = html;
  lastTreePaneHtml = html;
  restoreCreate();
  restoreRename();
  if (hadRowFocus && !state.treeCreate && !state.treeRename) rovingRowEl()?.focus();
}

// The side pane's memo, and the same focus contract the tree has — discovery is a keyboard
// surface too now (sidenav.ts, K1): an `innerHTML` swap destroys the row holding focus, so
// an unrelated repaint — a toast timer, a watcher pulse, the *other* discovery read landing
// — would silently eject a keyboard user to `<body>` mid-list. Restored by row key, since
// the element it was on no longer exists — and by `id` for the pane's one focusable that
// *isn't* a row, search mode's `clear` (GH #91). Memoized for the reason the note pane is:
// identical HTML skips the swap, so the pane's scroll position survives a repaint that
// changes nothing about it.
let lastSidePaneHtml: string | null = null;

function paintSide(): void {
  const html = sidePaneHtml(state);
  if (html === lastSidePaneHtml) return;
  const restore = capturePaneFocus(el("side-pane"));
  // The chat composer's half-typed question lives only in the DOM (`paintTree`'s inline
  // create input, in the right column): a repaint this pane doesn't know about — a toast
  // timer, a watcher pulse, an answer landing — would otherwise wipe a question mid-word.
  // Carried by value and caret, since the element itself does not survive the swap.
  const carryInput = captureChatInput();
  el("side-pane").innerHTML = html;
  lastSidePaneHtml = html;
  carryInput();
  restore?.();
}

/** The chat composer's contents across a side-pane repaint — value, caret and all.
 *  A no-op thunk when the composer isn't up or is empty, so nothing is restored over a
 *  pane that has moved on to something else. */
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
 * Repaint the note pane, putting the keyboard back on what it was on — a graph node by
 * its scene id, a bar chip by its `id` (GH #91).
 *
 * Memoized: an unrelated render (a toast timer, streamed progress) with identical pane
 * HTML skips the swap entirely, so reading scroll position survives and the graph view's
 * entrance animation plays on real changes only. That memo is also why this pane's focus
 * bug was the narrow one — a swap here means the pane *genuinely* changed: discovery
 * landing while the graph is open and focused, or the drawer/source/graph chip you just
 * pressed rebuilding the bar it lives in. Both dropped the keyboard to `<body>`.
 *
 * Reports whether the swap actually happened — the find bar's Ranges and the syntax
 * highlight pass are re-derived off that, not off every render.
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
  // The carve-out (crates/b2-desktop/CLAUDE.md): while editing, the note pane belongs to
  // the live EditorView — rebuilding it here (e.g. from a toast timer) would destroy
  // the editor mid-keystroke. The frontmatter mini-editor (GH #79) gets the same
  // deal: its buffer is a live textarea in the pane. Everything else keeps rendering.
  let noteSwapped = false;
  if (!state.editing && !state.fmEditing) noteSwapped = paintNote();
  else lastNotePaneHtml = null;
  // Graph mode owns the pane's box: padding off, scrolling off, column flex on
  // (the stage flexes to fill; the SVG viewBox scales the scene into it).
  el("note-pane").classList.toggle(
    "is-graph",
    state.graphOpen && !state.editing && state.current !== null && state.currentResource === null,
  );
  paintSide();
  // The "semantic search is off — install the model" banner, under the top bar. Empty
  // string when the gate (embedreminder.ts) says not to prompt, so the strip collapses.
  el("embed-banner").innerHTML = embedBannerHtml(state);
  el("menu-root").innerHTML = contextMenuHtml(state);
  paintModal();
  el("vault-root").textContent = state.vaultRoot ?? "no vault";
  document.body.classList.toggle("is-loading", state.loading);
  paintReindex();
  paintNav();
  // The vault switcher stays enabled with no vault open — it's the in-app way to pick
  // the first one — but not mid-op, to avoid re-entrant switches. It stays live during
  // a reindex: switching cancels the in-flight run first (handled host-side).
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
}

/** The reading view's half of syntax highlighting (highlight.ts): a post-render pass over
 *  the pane's `<pre><code>` blocks. Async — a language grammar is a lazily loaded chunk —
 *  so a fence paints plain first and gains its colours a tick later. Find-in-note anchors
 *  Ranges into the pane's text nodes, so a repaint under an open bar re-derives them. */
async function paintCodeHighlights(): Promise<void> {
  if (!(await highlightCodeBlocks(el("note-pane")))) return;
  if (findOpen && !state.editing) applyReadingFind();
}

// Paint just the reindex affordance — the progress bar/label/Cancel, and the Reindex
// button's state *if it is on screen*. Called on every full render AND on each streamed
// progress batch, so progress updates never rebuild the panes (which would fight scrolling
// and churn on a large vault).
//
// There are two meters and one painter. The shell's lives in the top bar; Settings →
// Index paints a second while a run is live, because Settings took the whole window and a
// meter behind an opaque surface is no meter (render.ts). So this walks *every*
// `.reindex-progress` on screen and writes the same values into each — one computation,
// so the two can't disagree about a run, and adding a third costs nothing here.
//
// The button does not: it is Settings → Index's now (render.ts `indexPanelHtml`), so it
// exists only while that dialog is open on that section — hence the null-tolerant lookup
// rather than `el`. `settingsPanelHtml` paints it in the right state to begin with; this
// keeps it there through the runs that *don't* full-render, which is every auto-index
// (`autoIndexOnOpen`, `trailingEmbed`) — those repaint the affordance alone, and a stale
// "Reindex" you can click into a no-op is the failure that reads as a broken button.
function paintReindex(): void {
  const btn = document.getElementById("reindex") as HTMLButtonElement | null;
  if (btn) {
    btn.disabled = reindexDisabled(state);
    btn.textContent = reindexLabel(state);
  }

  const meters = [...document.querySelectorAll<HTMLElement>(".reindex-progress")];
  for (const wrap of meters) wrap.hidden = !state.reindexing;
  if (!state.reindexing) return;

  // Determinate only once embedding starts and the denominator is known; before that
  // (the fast projection phase) the bar sweeps rather than showing a bogus fraction.
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

// Expand every folder on the way to `path` so the file tree reveals it — used when a
// note is opened from search/wikilink/discovery, not just by clicking it in the tree.
function expandAncestors(path: string): void {
  const parts = path.split("/");
  let dir = "";
  for (const seg of parts.slice(0, -1)) {
    dir = dir ? `${dir}/${seg}` : seg;
    state.expandedDirs.add(dir);
  }
}

// Load the vault listing for the file tree — all three lists fetched before any
// state commit, so a mid-refresh failure can't leave the tree half-updated.
// Non-fatal on failure (e.g. no vault open): the tree shows its empty state and
// the reason surfaces as a toast; resolves false so callers don't overwrite that
// toast with a success flash.
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
 * Load a note into the center pane — the shared core of `openNote` and back/forward
 * (#52): everything after the edit-mode guard. `commit` runs the history-stack
 * mutation the moment the read succeeds — before the slower discovery tail, so rapid
 * navigations can't interleave stack updates out of order — and receives the
 * canonical vault-relative path (the ref may be a wikilink target, so `.md`-less).
 * Resolves false when the read failed (its error already toasted), so back/forward
 * can prune a dead entry.
 */
async function loadNote(ref: string, commit: (path: string) => void): Promise<boolean> {
  state.loading = true;
  render();
  try {
    const note = await api.readNote(ref);
    state.current = note;
    state.currentResource = null; // one document owns the pane
    state.resourceImage = null;
    state.fmEditing = false; // a new document ends any drawer edit (guards ran upstream)
    commit(note.path);
    expandAncestors(note.path);
    state.selectedDir = parentDir(note.path); // the create context follows the selection
    resetSearch();
    // Paint the note the instant its body is read — the body is already in hand.
    // Discovery (`similar` + `explain`) is a slower, independent side-pane read; gating
    // the middle pane on it made note-open feel as slow as the whole discovery scan.
    // Clear the prior note's discovery so its cards don't linger under the new note.
    state.similar = [];
    state.connections = [];
    state.resourceLinks = [];
    state.unresolved = [];
    state.collapsedCards.clear(); // per-note fold state belongs to the note we just left
    state.contextMenu = null;
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
    // The discovery flags are owned by refreshDiscovery (it clears each section's when
    // that read settles, guarded against a superseding open) — clearing them here would
    // race a newer note's in-flight load, so only the middle-pane spinner is ours.
    state.loading = false;
    render();
  }
}

// User navigation to a note (tree, wikilink, backlink, similar card, search result).
// Mid-edit navigation flushes the buffer and leaves edit mode first; a conflict keeps
// the editor — and the user's buffer — alive instead. A successful load records the
// document in the history stack (#52); back/forward call `loadNote` directly.
async function openNote(ref: string): Promise<void> {
  if (!fmEditGuard()) return;
  if (!(await closeEditor())) return;
  await loadNote(ref, (path) => navPush({ kind: "note", path }));
}

// Follow a wikilink from the reading view or the editor's mod-click. A `[[link]]`
// target can name a resource just as readily as a note (`[[report.pdf]]`), so route
// by the target's shape — the same extension-only rule the core resolves the edge on
// (`refKind`/`doc_kind`) — to the resource card rather than failing a note read. A
// wikilink target is vault-root, so it *is* the resource's vault-relative path (minus
// any `#fragment`); the host re-validates either way.
async function followWikilink(target: string): Promise<void> {
  if (refKind(target) === "resource") {
    await openResource(target.split("#")[0].trim());
  } else {
    await openNote(target);
  }
}

/**
 * The open resource's picture, or null when there isn't one to show.
 *
 * Null for every class without an in-app viewer, for an image too large to hold on
 * screen (`IMAGE_VIEWER_MAX_BYTES` — the card's size is already in hand, so the decision
 * costs no IPC), and for a read that failed. That last one is deliberate: the card is the
 * truth about the file whether or not its bytes can be read, so a failed read falls back
 * to *Open in system default* rather than failing the navigation and leaving the pane on
 * the previous document.
 */
async function loadResourceImage(r: ResourceExplainView): Promise<string | null> {
  if (r.class !== "image" || r.size > IMAGE_VIEWER_MAX_BYTES) return null;
  try {
    return imageDataUrl(r.path, await api.readResource(r.path));
  } catch {
    return null;
  }
}

/** The resource sibling of `loadNote` — same core/commit split, for `openResource`
 *  and back/forward. Discovery doesn't apply (resources have no chunks until file-type
 *  slice 3), so the side pane clears. */
async function loadResource(path: string, commit: (path: string) => void): Promise<boolean> {
  state.loading = true;
  render();
  try {
    const resource = await api.explainResource(path);
    state.currentResource = resource;
    state.resourceImage = await loadResourceImage(resource);
    state.current = null;
    commit(resource.path);
    expandAncestors(resource.path);
    state.selectedDir = parentDir(resource.path); // the create context follows the selection
    resetSearch();
    state.similar = [];
    state.connections = [];
    state.resourceLinks = [];
    state.unresolved = [];
    state.collapsedCards.clear();
    state.contextMenu = null;
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

// Select a resource in the tree → the fallback card (file-type slice 1, spec §6):
// metadata + backlinks + *Open in system default*. The note-pane sibling of
// openNote — same edit-mode flush, same one-document-owns-the-pane rule, same
// history push.
async function openResource(path: string): Promise<void> {
  if (!fmEditGuard()) return;
  if (!(await closeEditor())) return;
  await loadResource(path, (p) => navPush({ kind: "resource", path: p }));
}

// --- navigation history (#52) -----------------------------------------------------
//
// Browser-style back/forward over the center pane's document. The stack holds every
// document the pane has shown — notes and resources alike, regardless of how each was
// reached — with a cursor at the current one. Session-scoped by design: it starts
// empty on launch, is never persisted, and clears on vault switch. Module-locals like
// the editor's timers (nothing here is rendered from, so it stays out of AppState);
// the two chrome buttons repaint through the targeted `paintNav` (the `paintReindex`
// pattern). In-place content updates (a save's re-read, a write report, external-edit
// reconciliation) mutate `state.current` directly without passing through
// `openNote`/`openResource`, so they never create entries.

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

// Record a genuine navigation: truncate the forward branch (the browser model —
// navigating after going back discards it), then append. Called from the load cores
// *after* a successful read with the canonical vault-relative path in hand, so a
// wikilink followed by title and a tree click on the same note dedupe, and a target
// that fails to load never enters the stack. Re-opening the already-current document
// is a history no-op (consecutive-duplicate suppression).
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

/** True while a text-entry surface owns the keyboard (the search field, a modal
 *  input) — ⌘←/⌘→ mean caret-to-line-edge there, never history. */
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
// B2 is fully operable from the keyboard; the mouse is an accelerator, never a
// requirement (docs/invariants.md K1). Three things make that true and all three
// live here: the file tree navigates by arrow key (the ARIA `tree` pattern, walking the
// row order treenav.ts also paints), every overlay takes focus on open and gives it back
// on close, and every mouse-only gesture — the right-click menu above all — has a key
// that reaches it. The chords themselves are wired in the global keydown handler at the
// bottom of `wireEvents`; what's here is the focus bookkeeping they share.

/** Every row the tree currently paints, in paint order — the list the arrows walk. */
function treeRows(): TreeRow[] {
  return visibleRows(buildTree(state.notes, state.resources, state.dirs), state.expandedDirs);
}

/** The DOM row for a vault path. Looked up by data attribute rather than by CSS
 *  selector because a filename may contain anything a selector would choke on. */
function treeRowEl(path: string | null): HTMLElement | null {
  if (path === null) return null;
  const rows = el("tree-pane").querySelectorAll<HTMLElement>(".tree-row[data-tree-row]");
  for (const row of rows) if (row.dataset.treeRow === path) return row;
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
  return active.closest<HTMLElement>("#tree-pane .tree-row[data-tree-row]");
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

/**
 * Move keyboard focus to a tree row: state first (so the roving tabstop travels with
 * it), then the repaint, then the DOM. `scrollIntoView` is what keeps a long vault
 * navigable — arrowing off the bottom of the viewport must bring the row into view,
 * exactly as a mouse-driven scroll would.
 */
function focusTreeRow(path: string): void {
  state.treeFocus = path;
  paintTree();
  const row = treeRowEl(path);
  row?.focus();
  row?.scrollIntoView({ block: "nearest" });
}

// --- keyboard: the discovery pane's rows (sidenav.ts) --------------------------------
//
// The tree's helpers above, for the right column. Rows are keyed by `data-side-row` rather
// than looked up by CSS selector for the same reason: a row key carries a note path, and a
// path may contain anything a selector would choke on.

/** The DOM row for a `sidenav.ts` row key. */
function sideRowEl(key: string | null): HTMLElement | null {
  if (key === null) return null;
  const rows = el("side-pane").querySelectorAll<HTMLElement>("[data-side-row]");
  for (const row of rows) if (row.dataset.sideRow === key) return row;
  return null;
}

/** The graph node for a scene id (graph.ts `GraphNode.id`) — the note pane's `sideRowEl`,
 *  and iterating rather than selecting for the same reason: a node id carries a vault
 *  path, and a path may contain anything a selector would choke on. */
function gnodeEl(id: string | null): SVGElement | null {
  if (id === null) return null;
  const nodes = el("note-pane").querySelectorAll<SVGElement>("[data-gnode]");
  for (const node of nodes) if (node.dataset.gnode === id) return node;
  return null;
}

/** The row carrying the roving tabstop, as painted (sidenav.ts `rovingSideKey`). */
function rovingSideRowEl(): HTMLElement | null {
  return el("side-pane").querySelector<HTMLElement>('[data-side-row][tabindex="0"]');
}

/** Move keyboard focus to a discovery row — state first (so the roving tabstop travels
 *  with it), then the repaint, then the DOM. `focusTreeRow`'s counterpart. */
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

/** Put the keyboard in the note (⌘2): the live editor while editing, else the pane
 *  itself — which is the scroll container, so the arrows read the note from there. */
function focusNotePane(): void {
  if (state.editing && editorView) {
    editorView.focus();
    return;
  }
  el("note-pane").focus();
}

/** Put the keyboard in discovery (⌘3) — on the row it last left off at (the roving
 *  tabstop, like ⌘1), else whatever chrome the pane has, else the empty pane itself. */
function focusSidePane(): void {
  const pane = el("side-pane");
  const row = rovingSideRowEl();
  const first = pane.querySelector<HTMLElement>("button:not([disabled])");
  (row ?? first ?? pane).focus();
}

// --- keyboard: overlay focus (K1) ---------------------------------------------------
//
// An overlay that opens without taking focus is a mouse-only control: the keyboard is
// still on the page behind it, ⏎ hits whatever was focused before, and Tab walks the
// page *under* the modal. So each overlay takes focus on open, keeps it (the Tab trap in
// the keydown handler), and hands it back on close. One transition hook rather than a
// per-modal dance: `render()` paints overlays declaratively, so the open/close *edge*
// is the only honest place to move focus — moving it on every render would fight the
// user's own Tab while a modal is up.

type OverlayKind = "settings" | "move" | "delete" | "link" | "menu" | null;

/**
 * Which overlay is up, in the same precedence `modalHtml` renders them — the guard
 * every global chord asks before acting, and what the focus transition below keys on.
 *
 * **Exactly one is ever up**, which is what keeps this a single value rather than a
 * stack: an overlay that hands off to another (a menu → Move…) is *replaced*, not
 * covered, so closing returns focus to whatever opened the menu and never to a menu item
 * that no longer exists. The `?` sheet used to be the one exception — it rendered *over*
 * Settings, because a button there was one of its two entry points — and it is now the
 * Keyboard section of the Settings dialog itself (settingstabs.ts), so the second layer,
 * and the pair of return-focus slots it needed, are gone.
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
 * Take down every overlay, so the caller's own can be the one that is up.
 *
 * This is what makes "exactly one is ever up" true rather than aspirational. ⌘, is a
 * deliberately **unguarded** toggle — you can hit it from anywhere, editing included —
 * so it is the path that opens an overlay while another is already on screen.
 * `currentOverlay` and `modalHtml` both rank Settings above the rest, so the newcomer
 * *paints*; but a `moveTarget` left set is not a dismissed modal, it is a **hidden**
 * one, and it comes back the moment the newcomer closes. ⌘, over Move… then Esc used to
 * put the Move modal on screen with nothing having asked for it.
 *
 * Discarding beats deferring here, and beats refusing: pressing ⌘, is an unambiguous
 * "take me to Settings", and a modal you have to dismiss twice is worse than one that
 * closed when you looked away from it. Only the *targets* are cleared — no side effects,
 * so nothing is committed on the way out.
 */
function dismissOverlays(): void {
  state.contextMenu = null;
  state.settingsOpen = false;
  clearRecorder(); // the chord recorder lives inside Settings and goes with it
  state.moveTarget = null;
  state.deleteTarget = null;
  state.linkTarget = null;
}

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

/** The overlay's own focusable controls, in DOM order — what Tab cycles through, and
 *  whose first entry receives focus on open. Menu items are deliberately `tabindex=-1`
 *  (the menu is one stop, arrow-navigated), so they're collected by class instead. */
function overlayFocusables(): HTMLElement[] {
  // `[role="dialog"]`, not a class: the overlay layer has two shapes now — the `.modal`
  // box the link/move/delete dialogs paint into, and Settings' full-window
  // `.settings-screen` (render.ts) — and what they have in common is the semantics the
  // trap exists to serve, not the chrome.
  const modal = document.querySelector<HTMLElement>('#modal-root [role="dialog"]');
  // The `tabIndex >= 0` filter is what makes a **roving tabstop inside a modal** work:
  // `button:not([disabled])` matches a `tabindex="-1"` button regardless of the last
  // clause, so without it Settings' rail would put every section in the Tab cycle —
  // which is precisely the "Tab past N buttons to reach the controls" the roving
  // tabindex exists to prevent (settingstabs.ts).
  if (modal) {
    return [...modal.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((n) => n.tabIndex >= 0);
  }
  const menu = document.querySelector<HTMLElement>("#menu-root .context-menu");
  return menu ? [...menu.querySelectorAll<HTMLElement>(".context-item")] : [];
}

/**
 * The last element that actually received focus.
 *
 * Tracked continuously rather than read on demand, because by the time an overlay's
 * open edge is handled — at the end of `render()` — the DOM that held focus is already
 * gone: `render()` swaps `#modal-root`/`#menu-root` wholesale, so `document.activeElement`
 * has fallen back to `<body>` and the trigger is unrecoverable. `focusin` fires long
 * before that, and *destroying* a focused node fires no `focusin` (only blur), so this
 * still names the trigger at the moment we come to remember it. Set in `wireEvents`.
 */
let lastFocused: HTMLElement | SVGElement | null = null;

/**
 * A thunk that puts focus back where it was before an overlay opened. A *thunk*, not
 * the element, because the element usually doesn't survive: opening a menu repaints
 * the tree, which swaps the row that triggered it, and opening the `?` sheet swaps the
 * Settings button that opened it. Restore by identity that outlives a repaint — a tree
 * row by its path (falling back to the roving row when the path is gone, e.g. it was
 * just deleted), anything else by its element id, and only then by the element itself.
 */
function captureReturnFocus(): (() => void) | null {
  const active = lastFocused;
  if (active === null || active === document.body) return null;
  // Unscoped on purpose: `active` is usually *detached* by now, so an ancestor-matching
  // selector (`#tree-pane .tree-row`) would find nothing. `data-tree-row` is emitted by
  // the tree and nowhere else, so it identifies a row on its own.
  const row = active.closest<HTMLElement>(".tree-row[data-tree-row]");
  if (row) {
    const path = row.dataset.treeRow ?? null;
    return () => (treeRowEl(path) ?? rovingRowEl())?.focus();
  }
  // A discovery row, by its key — the ⇧F10 → Link… path a keyboard user takes, where
  // committing the link re-runs discovery and so detaches the row that opened the menu.
  const side = active.closest<HTMLElement>("[data-side-row]");
  if (side) {
    const key = side.dataset.sideRow ?? null;
    return () => (sideRowEl(key) ?? rovingSideRowEl())?.focus();
  }
  // A graph node, by its scene id — the same ⇧F10 → Link… path taken from a *ghost*, where
  // committing re-runs discovery and repaints the graph out from under the node. The
  // committed ghost is exactly the authored node for the same path (graph.ts ids a ghost
  // `ghost:<path>`), so the solidified node is where the keyboard belongs; failing that,
  // the pane, which keeps the keyboard in the graph rather than at the top of the window.
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

// The one return target, captured when the *chain* starts, so menu → Move… → close
// lands back on the tree row rather than on the menu item that handed off.
let overlayShowing: OverlayKind = null;
let overlayReturn: (() => void) | null = null;

/** Move focus into the overlay that just opened. */
function focusIntoOverlay(kind: OverlayKind): void {
  // The link modal opens on its explanation field — the one thing you came here to type.
  // Settings opens on its rail: `overlayFocusables()[0]` is the *selected* tab, since the
  // unselected ones are the roving tabstop's `tabindex="-1"`.
  const preferred = kind === "link" ? document.getElementById("link-explanation") : null;
  (preferred ?? overlayFocusables()[0])?.focus();
}

/**
 * Called at the end of every `render()`: acts only on open/close *edges*, never on a
 * plain repaint — otherwise a toast timer would yank focus back to an overlay's first
 * control while the user is mid-Tab. (A repaint *while* an overlay is up is
 * `paintModal`'s job, which restores what the keyboard was actually on.)
 */
function syncOverlayFocus(): void {
  const overlay = currentOverlay();
  if (overlay === overlayShowing) return;
  if (overlayShowing === null) overlayReturn = captureReturnFocus();
  overlayShowing = overlay;
  if (overlay === null) {
    const back = overlayReturn;
    overlayReturn = null;
    // Not when an inline tree input just took the keyboard: Rename and New note are
    // reached *through* the menu, so the menu closing must not yank focus back out of
    // the input it just opened.
    if (!state.treeCreate && !state.treeRename) back?.();
  } else {
    focusIntoOverlay(overlay);
  }
}

// Paint just the Back/Forward buttons' enabled state — never a pane rebuild. Disabled
// at the stack's ends, and mid-op like the vault switcher (navGo also guards, for the
// keyboard/mouse paths that don't go through a disabled button).
function paintNav(): void {
  const back = document.getElementById("nav-back") as HTMLButtonElement | null;
  const forward = document.getElementById("nav-forward") as HTMLButtonElement | null;
  if (back) back.disabled = state.loading || navCursor <= 0;
  if (forward) forward.disabled = state.loading || navCursor >= navStack.length - 1;
}

// Back (-1) / Forward (+1): move the cursor and load the entry there, through the
// same edit-mode guard as any navigation — flush + leave edit mode first, abort (and
// keep the buffer) on a write conflict. The cursor commits at read-success inside the
// load core, exactly where a normal navigation pushes, so a rapid follow-up can't
// interleave a stale cursor over a fresher stack. A dead target (deleted or renamed
// since it was visited) toasts the generic read error and is dropped from the stack
// so navigation isn't wedged on it.
async function navGo(delta: -1 | 1): Promise<void> {
  if (state.loading) return;
  // The guards first (closeEditor can await a save flush); the cursor math after,
  // against whatever the stack is once navigation is actually allowed to proceed.
  if (!fmEditGuard()) return;
  if (!(await closeEditor())) return;
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
    // By identity, not index: the failed read resolved through an await, so the
    // stack may have shifted under us (e.g. a click-navigation truncated it).
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
// The drawer's editing surface: the raw YAML in a plain textarea with explicit
// Save/Cancel — no autosave, deliberately: half-typed YAML isn't a body sentence.
// While it's live the note pane is under the render carve-out (the body editor's
// pattern), so the buffer lives only in the DOM and the inline error is painted
// imperatively. The one rule lives behind the façade (E3): a `---` line is refused
// because it would shift bytes into the body, and anything else saves — including
// YAML B2 can't read, which comes back flagged `frontmatter_readable: false` and
// warns in the drawer, the same as an external hand-edit would.

function enterFmEdit(): void {
  const n = state.current;
  if (!n || state.editing || state.fmEditing || state.loading) return;
  state.fmEditing = true;
  state.frontmatterOpen = true; // the editor lives in the open drawer
  // One explicit pane paint WITH the editor; render() then treats the pane as
  // carved out (and nulls its memo), so nothing rebuilds under the buffer.
  el("note-pane").innerHTML = notePaneHtml(state);
  render();
  // render() treats the pane as carved out, so the highlight pass it normally schedules
  // won't fire for this hand-built one — the body below the drawer still wants colours.
  void paintCodeHighlights();
  (document.getElementById("fm-editor") as HTMLTextAreaElement | null)?.focus();
}

/** The live buffer, or null when the mini-editor isn't mounted. */
function fmBuffer(): string | null {
  const ta = document.getElementById("fm-editor");
  return ta instanceof HTMLTextAreaElement ? ta.value : null;
}

/**
 * Cancel (Esc / the button / the conflict bar's Reload): drop the buffer and adopt
 * disk. The re-read matters — reconcile defers the open note to this editor while
 * it's live, so an external edit that arrived mid-edit lands now, not never.
 */
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

/**
 * Resolve the mini-editor before an action that would repaint or repoint the note
 * pane (navigation, the view toggles, a link commit, a move/delete of the open
 * note). A pristine buffer just closes; a dirty one blocks with the way out —
 * explicit-save semantics cut both ways, so typed YAML is never silently discarded.
 */
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

// After a successful save: leave edit mode and re-read from disk — the revision to
// chain on, the verbatim block, the header metadata (type/created/tags) and the
// readability flag may all have changed — then refresh discovery, since an edited
// `b2_relations:` is a graph change.
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

// The conflict bar's "Keep mine": re-read only to chain the fresh revision, then
// re-save the buffer over it — a deliberate overwrite of the external edit, the
// body editor's conflict semantics drawer-sized. (Reload is `cancelFmEdit`.)
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

/** Typing again clears the message — it belonged to the save attempt that failed. */
function hideFmError(): void {
  const box = document.getElementById("fm-error");
  if (box) box.hidden = true;
}

// Fold a whole discovery section (Similar & unlinked / Connections). Sticky across
// notes — a viewing preference, not per-note state.
function toggleSection(section: SideSection): void {
  if (state.collapsedSections.has(section)) state.collapsedSections.delete(section);
  else state.collapsedSections.add(section);
  render();
}

// Fold a single card's body (path + snippet) down to its title row. Per-note state,
// keyed `"<section>:<path>"`; cleared on note-open (see openNote).
function toggleCard(key: string): void {
  if (state.collapsedCards.has(key)) state.collapsedCards.delete(key);
  else state.collapsedCards.add(key);
  render();
}

// --- context menus (discovery cards + the file tree) ------------------------------
//
// Right-click a Similar card → Open note / Link… (replacing the inline "Link…"
// button); right-click the file tree → New note / New folder in the folder under
// the cursor. Anchored at the cursor, but clamped so a menu never spills past the
// viewport edge (a menu that opens off-screen is unusable).
const CTX_MENU_W = 168;
const CARD_MENU_H = 76;
/** …plus *Insert link at cursor*, which the card's menu grows while a note is being edited
 *  (render.ts). Only the clamp reads these, and it must not *under*-read — a menu opened
 *  near the bottom edge would lose its last item off-screen. */
const CARD_EDIT_MENU_H = 108;
const TREE_MENU_H = 132; // the context line + three items
// + Rename / Move… / Copy vault path / Copy system path / Delete and their separator.
// Only the clamp reads these, so an approximation is fine — but it must not *under*-read,
// or a menu opened near the bottom edge loses its last item off-screen.
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

// --- tree creation: new note / new folder (left nav) ------------------------------
//
// The create affordances: the tree-head icons, ⌘N / ⇧⌘N, and the tree's right-click
// menu — all contextual, landing the entry in `state.selectedDir` (which follows
// the selection: the open document's folder, or the last folder clicked). The name
// is typed into an inline input row in the tree (Enter commits, Escape cancels,
// blur commits a non-empty name).
//
// A new *note* is real — and auto-indexed — immediately: the model-free
// `create_note` writes the file and projects it (tree, keyword search, graph), and
// its vectors fill through the normal editing pipeline (the note opens in edit
// mode; autosave's trailing embed covers whatever gets typed — an empty body has
// nothing to embed). A new *folder* is equally real — `create_dir` is a true
// `mkdir` on disk: a folder is user-authored vault structure (the fs is
// authoritative for it, empty or not), so it exists to Finder, the CLI, and any
// sync the moment the input commits, and the tree re-lists it from disk.

function startTreeCreate(kind: "note" | "folder", dir: string): void {
  if (state.vaultRoot === null) return;
  state.contextMenu = null;
  state.treeCreate = { kind, dir };
  for (const d of dirChain(dir)) state.expandedDirs.add(d); // reveal the target folder
  render(); // paintTree focuses the fresh input
}

function cancelTreeCreate(): void {
  if (!state.treeCreate) return;
  state.treeCreate = null;
  render();
}

/**
 * Commit the inline input's name. `open` distinguishes the two commit gestures:
 * Enter means "create and start writing" (the note opens in edit mode); a blur
 * commit (the user clicked into something else) creates quietly and leaves their
 * click's navigation alone.
 */
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
      for (const d of dirChain(report.dir)) state.expandedDirs.add(d);
      state.selectedDir = report.dir; // the natural next step is a note inside it
      if (refreshed) flash(`Created ${report.dir}/.`);
      else render(); // the refresh failure already toasted; still repaint the expansion
    } catch (e) {
      // Refused (e.g. the name is taken): keep the input open with the typed name
      // intact — the commitTreeCreate posture below; the toast explains.
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
    // Refused (e.g. the name already exists): keep the input open — with the typed
    // name intact, since the unchanged tree HTML skips the repaint — so the user
    // adjusts rather than retypes; the toast explains.
    state.treeCreate = create;
    flash(errText(e));
  }
}

// --- tree import: files from outside the vault ------------------------------------
//
// Two gestures, one outcome: drag files from Finder onto a folder row (the pointer
// path, wired in wireEvents) or pick them in an OS dialog from the tree's right-click
// menu (the keyboard path — K1: a drag is pointer-only, so it can't be the only way
// in). Both place the files through `Vault::import_file`/`import_path`, which copies
// the bytes verbatim and projects them — adding nothing to either: a `.md` lands as a
// note, any other file as a resource, and the tree shows it with no reindex.
//
// The two differ only in what they can hand the host. A drop yields **bytes** — WebKit
// gives the page content, never a path — so the file rides the IPC as base64 and is
// size-capped (importfiles.ts explains both). The picker yields **paths**, so the host
// reads the file itself and neither limit applies.

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
 * Read a drop's entries **synchronously** — the DataTransfer is neutered the moment
 * the handler returns, so nothing here may be deferred past the first `await`.
 * `items` is used rather than `files` for the one thing `files` can't say: whether an
 * entry is a *folder* (`webkitGetAsEntry`), which the plan refuses by name instead of
 * failing later on a read of nothing.
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
  // Belt and braces: if `items` told us nothing, fall back to the file list rather
  // than let the drop silently do nothing.
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
  if (state.reindexing) {
    flash("Indexing is running — try the import again when it finishes.");
    return false;
  }
  return true;
}

/** The drop half: send each accepted file's bytes, then report once. */
async function importDroppedFiles(dir: string, dropped: DroppedFile[]): Promise<void> {
  if (!canImportNow()) return;
  const plan = planImport(dropped);
  const refused = [...plan.refused];
  const imported: string[] = [];
  importInFlight = true;
  try {
    // Sequential on purpose: one file's refusal (a name already taken) must not
    // cancel the rest of the drop, and the reports read in the order the user
    // dropped them.
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
    // Inside the gate, like `executeMove`'s: the refresh is part of the import, and two
    // of them interleaving would re-list and toast out of order.
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

/**
 * What both gestures do once the files are placed: reveal the destination, re-list the
 * tree (the host already projected each file, so this is what makes it visible), and
 * say what happened in one toast. The trailing embed is scheduled for the same reason
 * a save schedules one — an imported note's chunks are projected but unembedded, and
 * that is the pass that fills them.
 */
async function finishImport(dir: string, imported: string[], refused: string[]): Promise<void> {
  if (imported.length > 0) {
    for (const d of dirChain(dir)) state.expandedDirs.add(d);
    state.selectedDir = dir;
    await loadNotes();
    void refreshEmbedStatus(state.vaultRoot); // the N/M denominator grew (#26)
    scheduleTrailingEmbed();
  }
  flash(importSummary(dir, imported, refused));
}

// --- tree move / rename (context menu, Move… modal, drag-and-drop) -----------------
//
// All three gestures funnel into one executor: resolve the destination (pure logic
// in move.ts), dispatch the node's kind to its IPC command (`move_note` /
// `move_resource` / `move_dir` — the host's `Vault` ops rewrite inbound links and
// re-project the index), then re-point the open document and reload the tree.
// Renaming acts on the *file path* — a frontmatter `title:` is inert (the note's
// display title is its filename, data-model.md §1) — exactly like `b2 mv`.
//
// The re-point runs BEFORE the watcher's debounced `vault-changed` pulse arrives:
// reconcileExternalChange re-reads the open note by path, so if it still pointed at
// the old path it would flash "moved or removed" for a move we made ourselves.

/** A move/rename is in flight — further gestures are ignored (no queueing in v1). */
let moveInFlight = false;

function startTreeRename(node: TreeNodeRef): void {
  state.contextMenu = null;
  state.treeRename = node;
  for (const d of dirChain(parentDir(node.path))) state.expandedDirs.add(d);
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
  // The input stays open while the move runs: on a refusal the typed name survives
  // (the memoized tree HTML is unchanged, so the DOM input is never rebuilt — the
  // commitTreeCreate posture) and the toast explains; success clears it.
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

/**
 * The one shared move executor (rename commit, Move… modal, drop). Resolves true
 * on success. Refuses while a reindex runs — the move opens the real model, and
 * two model instances at once is a needless memory spike — and while another move
 * is still in flight.
 */
async function executeMove(node: TreeNodeRef, to: string): Promise<boolean> {
  if (moveInFlight) return false;
  if (state.reindexing) {
    flash("Indexing is running — try the move again when it finishes.");
    return false;
  }
  // If the open document is affected, flush and close the editor first so the save
  // chain never targets the old path (a conflict keeps the editor and aborts the move).
  const curPath = state.current?.path ?? state.currentResource?.path ?? null;
  const affected =
    curPath !== null &&
    (node.nodeKind === "folder" ? remapPath(curPath, node.path, to) !== null : curPath === node.path);
  if (affected && !fmEditGuard()) return false;
  if (affected && state.editing && !(await closeEditor())) return false;

  moveInFlight = true;
  if (node.nodeKind === "folder") flash(`Moving ${node.path}/…`);
  try {
    let from: string;
    let rewritten: number;
    if (node.nodeKind === "note") {
      const r = await api.moveNote(node.path, to);
      from = r.from;
      to = r.to; // the host normalizes (e.g. appends .md)
      rewritten = r.links_rewritten;
    } else if (node.nodeKind === "resource") {
      const r = await api.moveResource(node.path, to);
      from = r.from;
      to = r.to;
      rewritten = r.links_rewritten;
    } else {
      const r = await api.moveDir(node.path, to);
      from = r.from;
      to = r.to;
      rewritten = r.links_rewritten;
    }

    // Re-point open/tree state through the move before the watcher pulse re-reads it.
    state.expandedDirs = new Set(
      [...state.expandedDirs].map((d) => remapPath(d, from, to) ?? d),
    );
    state.selectedDir = remapPath(state.selectedDir, from, to) ?? state.selectedDir;
    for (const d of dirChain(parentDir(to))) state.expandedDirs.add(d);
    const openNotePath = state.current ? remapPath(state.current.path, from, to) : null;
    const openResourcePath = state.currentResource
      ? remapPath(state.currentResource.path, from, to)
      : null;
    if (openNotePath !== null) {
      state.current = await api.readNote(openNotePath);
    }
    if (openResourcePath !== null) {
      const moved = await api.explainResource(openResourcePath);
      state.currentResource = moved;
      state.resourceImage = await loadResourceImage(moved);
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
// Deletes remove the file(s) from B2 *and* the disk in one gesture. Files (notes,
// resources) delete immediately — the gesture is the intent, no dialog; folders
// confirm first (a whole subtree, unindexed files included, is a bigger loss).
// Inbound links at the deleted target dangle (they surface as unresolved links) —
// they are never rewritten, exactly what an external delete would leave.

/** A delete is in flight — further gestures are ignored (the move posture). */
let deleteInFlight = false;

/**
 * Route a delete gesture to its flow: files execute immediately; folders always
 * open the confirm modal — even one the index lists nothing under may hold
 * unindexed files on disk, and `delete_dir` removes everything.
 */
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

/** Forget tree state pointing into a deleted folder subtree. */
function dropDirState(dir: string): void {
  const gone = (d: string) => d === dir || d.startsWith(`${dir}/`);
  state.expandedDirs = new Set([...state.expandedDirs].filter((d) => !gone(d)));
  if (gone(state.selectedDir)) state.selectedDir = parentDir(dir);
}

/**
 * The one shared delete executor (context menu, ⌘⌫, the folder confirm). Refuses
 * mid-reindex like a move — not for the model (deletes are model-free) but so two
 * writers never race the same index.
 */
async function executeDelete(node: TreeNodeRef): Promise<void> {
  if (deleteInFlight) return;
  if (state.reindexing) {
    flash("Indexing is running — try the delete again when it finishes.");
    return;
  }
  // If the open document dies with the delete, close the editor first so no save
  // chain targets a file that's about to be removed (a conflict aborts the delete,
  // keeping the buffer alive — the executeMove posture).
  const curPath = state.current?.path ?? state.currentResource?.path ?? null;
  const affected =
    curPath !== null &&
    (node.nodeKind === "folder"
      ? curPath === node.path || curPath.startsWith(`${node.path}/`)
      : curPath === node.path);
  if (affected && !fmEditGuard()) return;
  if (affected && state.editing && !(await closeEditor())) return;

  // Where the keyboard lands once this row is gone (K1): the next visible row, else the
  // previous one — the platform reflex after a delete, and the difference between arrow
  // navigation that survives a delete and one that dumps focus back at the top. Computed
  // *before* the delete, while the row is still in the list. Harmless for mouse users:
  // it only moves the tree's roving tabstop.
  const nextFocus = neighborPath(treeRows(), node.path);

  deleteInFlight = true;
  try {
    let what: string;
    let dangled: number;
    if (node.nodeKind === "note") {
      const r = await api.deleteNote(node.path);
      what = r.path;
      dangled = r.dangled.length;
    } else if (node.nodeKind === "resource") {
      const r = await api.deleteResource(node.path);
      what = r.path;
      dangled = r.dangled.length;
    } else {
      const r = await api.deleteDir(node.path);
      what = `${r.dir}/`;
      dangled = r.dangled.length;
    }

    // Clear state that pointed into the deleted subtree — before the watcher's
    // debounced pulse re-reads it and flashes "moved or removed" for our own delete.
    if (node.nodeKind === "folder") dropDirState(node.path);
    state.treeFocus = nextFocus;
    if (affected) {
      state.current = null;
      state.currentResource = null;
      state.resourceImage = null;
      state.similar = [];
      state.connections = [];
      state.resourceLinks = [];
      state.unresolved = [];
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

/** Flip the pane between reading and the graph — a pure state flip (the scene
 *  renders from discovery state the note-open already fetched, so no IPC happens
 *  here). Sticky across notes, like sourceOpen. */
function toggleGraph(): void {
  if (!state.current) return; // the graph anchors on an open note
  if (!fmEditGuard()) return; // the graph takes the pane the mini-editor holds
  state.graphOpen = !state.graphOpen;
  render();
}

// The `</>` toggle serves two surfaces off the one sticky `sourceOpen` (spec §3
// "Escape hatch"). In the reading view it flips rendered ↔ raw via a full re-render.
// While editing, the carve-out forbids rebuilding the pane, so it reconfigures the
// live-preview compartment in place — decorations off = raw + syntax colors, monospace
// (today's editor) — with cursor and undo intact, then repaints just the bar button.
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
  // Two independent reads with independent repaints: `explain` (Connections) is a
  // near-instant graph read, `similar` is the slower whole-vault discovery scan. A
  // Promise.all would gate the fast one on the slow one — so each settles and paints on
  // its own. Both guard against the user having navigated away before they resolved
  // (don't clobber the new note's pane) and clear only their own section's loading flag.
  const stale = () => state.current?.path !== n.path;
  const connections = api
    .explain(n.path)
    .then((explain) => {
      if (!stale()) {
        state.connections = explain.connections;
        state.resourceLinks = explain.resources;
        state.unresolved = explain.unresolved;
      }
    })
    .catch((e) => {
      if (!stale()) {
        state.connections = [];
        state.resourceLinks = [];
        state.unresolved = [];
        flash(errText(e));
      }
    })
    .finally(() => {
      if (stale()) return;
      state.discoveringConnections = false;
      render();
    });
  const similar = api
    .similar(n.path, 10)
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

// Monotonic search-request counter: bumped by `doSearch` alone, so it answers exactly
// one question — has a *newer search* taken over since this one started? (A reset is a
// different question and is asked of `state.searchQuery`; see the guards below.)
let searchSeq = 0;

// The search wiring, and the one place D2's verdict turns into what the pane shows
// (invariants.md D2, GH #202). Three states, three behaviors:
//
//   • `false` — the vault holds neither a lexical anchor nor semantic proximity
//     clearing this model's bar. The rows are **dropped here**, at the boundary, so
//     the pane serves none of them: strict, no expander, no "N more" (GH #202,
//     decision 1). Dropping them in state rather than branching in the paint is what
//     keeps `render.ts` and `sidenav.ts` agreeing by construction — the same reason
//     the row order lives in one place, since a pane you can arrow through in an
//     order you can't see is worse than no arrows at all.
//   • `true` — serve them, as always.
//   • `null` — *no verdict*: no calibrated bar for the active model (the fake
//     embedder, or any model until the harness measures one — M2). Serve them, as
//     always. Reading `null` as "no matches" would blank every dev vault.
async function doSearch(raw: string): Promise<void> {
  const query = raw.trim();
  if (!query) {
    resetSearch();
    render();
    return;
  }
  state.loading = true;
  state.searchQuery = query;
  // Search and chat both own the right column, and a search is an explicit act — so it
  // wins, and the conversation waits in state until ⌘J brings it back (chat.ts's header).
  state.chatOpen = false;
  render();
  // `refreshDiscovery`'s staleness guard, in the two parts this pane needs. A slower
  // search for A must not land on top of a newer B, nor on a pane the user cleared —
  // and it is load-bearing *because* of the verdict rather than merely tidy: a stale
  // `false` would empty the pane while the header names the newer query, so the empty
  // state would claim the vault holds no evidence for a query nothing has judged yet,
  // the exact false claim D2 exists to stop.
  //
  // Two tests, not one, because two different things are owned. **Results** belong to
  // this query, so a reset (`resetSearch` blanks the query) discards them as surely as
  // a newer search does. **`state.loading` is global** — it drives the body class and
  // disables switch-vault, unlike discovery's own per-section flags — so this call must
  // always release it *unless* a newer search has taken it over, or a clear-mid-flight
  // would strand the whole window in its loading state.
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

// Back to discovery: no query, no rows, and no verdict. All three move together —
// a verdict outliving the query it was read for is a claim about nothing.
function resetSearch(): void {
  state.searchQuery = "";
  state.searchResults = [];
  state.searchVouched = null;
}

function clearSearch(): void {
  resetSearch();
  const input = document.getElementById("search-input") as HTMLInputElement | null;
  if (input) input.value = "";
  render();
}

// --- chat (flow ④, GH #151/#153/#155) -----------------------------------------------
//
// The wiring; the paint is render.ts's `chatPaneHtml` and the pure logic is chat.ts.
// What lives here is what only the running app can own: the streaming turn, its
// cancellation, and the focus/repaint discipline a token-by-token surface demands.
//
// **Streaming does not go through `render()`.** A full render on every token would swap
// the side pane's `innerHTML` a hundred times an answer — destroying the composer's caret,
// resetting the pane's scroll, and (worst) ejecting a keyboard user to `<body>` mid-answer.
// So tokens land in `state.chatStreaming` and are painted into one element
// (`paintChatStream`), the same targeted-repaint shape `paintReindex` uses for streamed
// index progress. One full render at the start of a turn, one at the end.

/** Show or hide the chat pane. Opening it probes the model server (so the setup card is
 *  right the moment it appears) and puts the keyboard in the composer, which is the only
 *  thing anyone opens this pane to do. */
function toggleChat(): void {
  if (state.chatOpen) {
    closeChat();
    return;
  }
  state.chatOpen = true;
  // Chat and search both own the whole column, one at a time (chat.ts's header).
  clearSearch();
  // Explicitly, rather than leaning on `clearSearch`'s own repaint: `focusChatInput`
  // needs the composer to exist, and a paint that happens only as somebody else's side
  // effect is one refactor away from not happening. The panes are memoized, so a second
  // render over identical HTML costs nothing.
  render();
  focusChatInput();
  void refreshChatSetup();
}

/** Close the pane. A streaming answer is stopped first — a pane you can't see must not
 *  keep a model working, and the partial text is kept either way, since the turn resolves
 *  normally with `cancelled` set. The conversation survives a close: reopening continues
 *  it (it dies with the window, S4, not with the toggle). */
function closeChat(): void {
  // A failed cancel is nothing the user can act on and nothing to interrupt a close with:
  // the turn resolves on its own either way, and the pane is going away regardless.
  if (state.chatStreaming !== null) void api.cancelAsk().catch(() => {});
  state.chatOpen = false;
  render();
}

function focusChatInput(): void {
  (document.getElementById("chat-input") as HTMLTextAreaElement | null)?.focus();
}

/** Ask the host what the chat provider can do right now — the setup card's whole input.
 *  Never throws in practice (the probe is a status), but a rejected IPC must not take the
 *  pane down with it: an unknown setup reads as the "loading" state, which is honest. */
async function refreshChatSetup(): Promise<void> {
  try {
    state.chatSetup = await api.chatSetup();
    state.chatCloud = state.chatSetup.cloud;
  } catch (e) {
    flash(errText(e));
    return;
  }
  render();
}

/**
 * One turn: the question goes up, tokens come back, the resolved answer replaces the
 * stream. The transcript keeps a failed turn too — the question is still on screen to
 * retry, and chat.ts's `chatHistory` leaves it out of the next ask because there is no
 * answer to carry forward.
 */
async function sendChat(question: string): Promise<void> {
  const q = question.trim();
  if (!q || state.chatStreaming !== null) return;
  // The history the *next* ask carries is derived from the transcript before this
  // question joins it — the question itself is the `ask` argument, not history.
  const history = chatHistory(state.chatMessages);
  // The vault this turn is grounded in. A switch mid-answer clears the transcript (the
  // old vault's paths mean nothing in the new one), so an answer that lands afterwards
  // must be dropped rather than pushed into a conversation it doesn't belong to — the
  // `stale()` guard discovery uses, keyed on the vault instead of the note.
  const askedIn = state.vaultRoot;
  state.chatMessages.push(userMessage(q));
  state.chatStreaming = "";
  render();
  const input = document.getElementById("chat-input") as HTMLTextAreaElement | null;
  if (input) input.value = "";
  scrollChatToEnd();
  try {
    const view = await api.ask(q, history, (token) => {
      // Guard against a token arriving after the turn ended (a cancel racing the last
      // frame): appending to a null stream would resurrect the live row.
      if (state.chatStreaming === null) return;
      state.chatStreaming += token;
      paintChatStream();
    });
    if (state.vaultRoot === askedIn) state.chatMessages.push(answerMessage(view));
  } catch (e) {
    if (state.vaultRoot === askedIn) state.chatMessages.push(errorMessage(errText(e)));
  } finally {
    // Where the keyboard was *before* the closing repaint — read here because `render()`
    // swaps the pane on the next line and the answer would already be `<body>`.
    const active = document.activeElement;
    const composerHeld =
      active === document.body || (active instanceof HTMLElement && active.id === "chat-input");
    state.chatStreaming = null;
    render();
    scrollChatToEnd();
    // Back to the composer — a conversation is a sequence of questions, and hunting for
    // the field after every answer is the fastest way to make a keyboard user reach for
    // the mouse (K1). Only when the composer is where the keyboard actually was, though:
    // an answer landing while the reader is in a note or on a citation must **give** focus
    // back, never take it (crates/b2-desktop/CLAUDE.md, "Two things that bite").
    if (composerHeld) focusChatInput();
  }
}

/** Paint the streaming answer — the one targeted repaint on this surface. `textContent`,
 *  never `innerHTML`: a half-arrived answer is not a document, and model output is
 *  untrusted content either way (E5 — the finished answer goes through the sanitizing
 *  `renderMarkdown` seam on the next full render). */
function paintChatStream(): void {
  const live = document.getElementById("chat-stream");
  if (!live || state.chatStreaming === null) return;
  live.textContent = state.chatStreaming;
  scrollChatToEnd();
}

/** Keep the newest text in view. Only when the reader is already at the bottom would be
 *  the polished rule; the simple one is right here because the pane is a conversation the
 *  user just spoke into — they are at the bottom. */
function scrollChatToEnd(): void {
  const log = document.getElementById("chat-log");
  if (log) log.scrollTop = log.scrollHeight;
}

/** Esc while an answer streams: stop it. Returns whether there was one to stop, so the
 *  `dismiss` chord can fall through to closing the pane when there wasn't. */
function stopChatAnswer(): boolean {
  if (state.chatStreaming === null) return false;
  void api.cancelAsk().catch((e) => flash(errText(e)));
  return true;
}

/** Start over. The transcript is session state and nothing else — dropping it writes
 *  nothing, deletes nothing, and costs no reindex. */
function newChat(): void {
  state.chatMessages = [];
  state.sideFocus = null;
  render();
  focusChatInput();
}

/** Switch the Settings section between the two named configurations. **Local** seeds the
 *  endpoint back to the local default; **Cloud models** clears it, because there is no
 *  default cloud provider — picking one is the explicit act M5 is about, and pre-filling a
 *  company's URL would be B2 making that choice. Neither saves: Save and test does. */
function setChatMode(cloud: boolean): void {
  state.chatCloud = cloud;
  render();
  const url = document.getElementById("settings-chat-url") as HTMLInputElement | null;
  if (url) {
    url.value = cloud ? "" : LOCAL_CHAT_ENDPOINT;
    url.focus();
  }
}

/**
 * Swap the Settings → Chat **Model** field between the picker and the text box, and put
 * the keyboard on whichever one just appeared.
 *
 * The focus move is the point, not a flourish: the button that swaps them is *replaced*
 * by the repaint (each shape offers the other's), so `captureModalFocus` has no id to
 * restore and the keyboard would land on `<body>` — the ejection the settings panel's
 * "every control carries a stable id" rule exists to prevent. Focusing the field the
 * press was *about* is both the fix and the right destination.
 */
function setChatModelTyped(typed: boolean): void {
  state.chatModelTyped = typed;
  render();
  document.getElementById("settings-chat-model")?.focus();
}

/** Ollama's OpenAI-compatible endpoint — the **Local** configuration's starting point, and
 *  the only place the frontend spells it. The host's `b2_llm::DEFAULT_BASE_URL` is the
 *  authority (it is what an unset endpoint resolves to); this is the field's seed when the
 *  user presses *Local* after typing a cloud URL. */
const LOCAL_CHAT_ENDPOINT = "http://localhost:11434/v1";

/** Settings → Chat: save the endpoint/model/key and re-probe, so "Save and test" is one
 *  act. The key is sent only when the user typed one — an untouched field must not clear
 *  a key that is already in force (the host applies the same rule). */
async function saveChatConfig(): Promise<void> {
  const value = (id: string): string | null => {
    const el = document.getElementById(id) as HTMLInputElement | null;
    const v = el?.value.trim() ?? "";
    return v === "" ? null : v;
  };
  const url = value("settings-chat-url");
  const model = value("settings-chat-model");
  // An empty key field is `null` — *keep* — not `""`: the field paints empty even when a
  // key is set, so "I didn't retype my key" must never read as "sign me out". Removing a
  // key is `clearChatKey`'s explicit button.
  const key = value("settings-chat-key");
  try {
    state.chatSetup = await api.setChatConfig(url, model, key);
    state.chatCloud = state.chatSetup.cloud;
    flash(
      state.chatSetup.state === "ready"
        ? `Chat model saved — connected to ${state.chatSetup.model}.`
        : (state.chatSetup.message ?? "Chat settings saved."),
    );
  } catch (e) {
    flash(errText(e));
  }
  render();
}

/** The setup card's installed-model list: pick one and it becomes the configured model.
 *  A one-click fix for the commonest local mistake — the daemon is up, the model name is
 *  just not one it has. */
async function useChatModel(model: string): Promise<void> {
  // The endpoint rides along explicitly. `null` means *unset* to the host — it is how
  // an emptied field returns to the environment's value — so sending it here would
  // quietly reset a configured endpoint back to the default as a side effect of picking
  // a model off the card. The key is `null` in the other sense: untouched, so kept.
  await applyChatConfig(state.chatSetup?.base_url ?? null, model, null, `Chat model set to ${model}.`);
}

/**
 * Forget B2's API key — the only way back to a keyless configuration, since the field
 * paints empty whether or not one is set (a password field that echoed its secret back
 * would be a worse idea than not having this button).
 *
 * Sends `""`, which is the host's *clear* signal, as distinct from `null`'s *keep*. What
 * it clears is the key B2 is holding, in memory and in the Keychain both — a removal that
 * left the stored copy behind would simply hand it back at the next launch. A
 * `B2_LLM_API_KEY` in the environment is the user's own configuration and outlives it —
 * the copy beside the button says so.
 */
async function clearChatKey(): Promise<void> {
  try {
    state.chatSetup = await api.setChatConfig(
      state.chatSetup?.base_url ?? null,
      state.chatSetup?.model ?? null,
      "",
    );
    state.chatCloud = state.chatSetup.cloud;
    // Removal is all-or-nothing host-side, so the returned source *is* the
    // outcome — no separate success flag to keep in step. A key still reported
    // as stored/session means the Keychain refused to let go, and saying
    // "removed" there would be the one lie this button must never tell: the key
    // would be back at the next launch. (`environment` is neither outcome — B2
    // never had standing over that key, and the panel's copy says so.)
    const source = state.chatSetup.api_key_source;
    flash(
      source === "stored" || source === "session"
        ? "Couldn’t remove the key — your Keychain refused. It is still saved."
        : "API key removed.",
    );
  } catch (e) {
    flash(errText(e));
  }
  render();
}

/** Save a chat configuration, re-probe, and say what happened — the shared tail of every
 *  path that changes it (Save, the card's model picker, Remove key). */
async function applyChatConfig(
  baseUrl: string | null,
  model: string | null,
  apiKey: string | null,
  ok: string,
): Promise<void> {
  try {
    state.chatSetup = await api.setChatConfig(baseUrl, model, apiKey);
    state.chatCloud = state.chatSetup.cloud;
    flash(ok);
  } catch (e) {
    flash(errText(e));
  }
  render();
}

function openLinkModal(path: string, title: string): void {
  // A committed link rewrites the open note's frontmatter — the exact bytes the
  // mini-editor is holding — so resolve that edit before offering to link.
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
    // A link rewrites the open note's frontmatter on disk. Mid-edit: flush the buffer
    // first (so the link isn't racing an autosave), then chain the post-link revision —
    // otherwise the next autosave would false-conflict with our own link write.
    if (state.editing) await saveNow();
    const report = await api.link(src.path, target.path, relation, explanation);
    if (state.editing && !state.editConflict && state.current?.path === src.path) {
      // Skipped while the conflict bar is up: adopting a fresh revision there would
      // let a later save silently clobber the external edit the bar is guarding.
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
// A tabbed dialog (settingstabs.ts owns the rail) over the app's preferences: General
// (appearance), Embedding (the model picker — selecting one persists to the shared config
// the CLI also reads, and a real switch is completed by the user with b2 init + Reindex,
// which the flashed guidance names), and Keyboard (K1's discoverable half — the table
// lives in shortcuts.ts). This is the wiring; the paint is render.ts.

/** Open Settings, optionally jumping straight to a section — `?` lands on Keyboard, the
 *  "semantic search is off" banner lands on Embedding where its Download button is.
 *  Without one, the dialog comes back where it was left (`state.settingsTab`). */
async function openSettings(tab?: SettingsTabId): Promise<void> {
  // Read before `dismissOverlays` — it clears this flag along with everyone else's, and
  // "was the dialog already up" is what decides whether this is an open or a jump.
  const wasOpen = state.settingsOpen;
  dismissOverlays();
  if (tab) state.settingsTab = tab;
  state.settingsOpen = true;
  render(); // show the dialog shell immediately; the model list fills when it resolves
  if (wasOpen) {
    // Already up, so this was a *jump* between sections (`?` from inside the dialog).
    // Move the keyboard with the selection exactly as the rail's own arrows do —
    // `paintModal` restores focus to the tab that had it, which is no longer the
    // selected one, and a roving tabstop that disagrees with the highlight is worse
    // than no tabstop. The reads below are skipped too: nothing about the host changed.
    if (tab) document.getElementById(`settings-tab-${tab}`)?.focus();
    return;
  }
  try {
    // Models, their embedding-time history, where model files live, and the active compute
    // device (Metal/CPU) — parallel reads.
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
  // The Chat section's status, deliberately *not* in the `Promise.all` above: it is a
  // network probe, and an unreachable cloud endpoint takes seconds to say so — long
  // enough to hold the whole dialog empty for a user who came here to change the theme.
  // It fills in behind the paint, exactly as the pane's own card does.
  void refreshChatSetup();
  // No explicit focus call: the open edge put the keyboard on the selected tab, and
  // `paintModal` hands it back across this repaint by the tab's id.
  render();
}

function closeSettings(): void {
  state.settingsOpen = false;
  // The recorder lives inside this dialog, so it cannot outlive it — a listener still
  // swallowing every keystroke behind a closed Settings is the one failure a recorder
  // must never have.
  stopRecording();
}

/**
 * Show a section. `focusTab` is the keyboard's half of the ARIA tabs pattern — an arrow
 * or ⌃Tab moves focus *with* the selection, so the rail keeps the keyboard; a click does
 * not, because WebKit never focuses a button on click and forcing it would light a ring
 * the mouse user didn't ask for.
 *
 * The explicit focus is also the one case `paintModal` can't cover: it restores by id,
 * and the id that had focus (the *previous* tab) still exists after the repaint, so
 * without this the keyboard would stay behind on the tab you just moved off.
 */
function selectSettingsTab(tab: SettingsTabId, focusTab: boolean): void {
  if (state.settingsTab === tab && !focusTab) return;
  state.settingsTab = tab;
  render();
  if (focusTab) document.getElementById(`settings-tab-${tab}`)?.focus();
}

/**
 * Hand a path over to the clipboard and say so.
 *
 * The file tree's two copy items share it.
 *
 * The failure branch is not decoration: WebKit can refuse a programmatic clipboard
 * write, and a silent no-op would leave a copy action looking like it worked. Falling
 * back to the status line at least puts the path somewhere it can be read off.
 */
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
// A pure front-end preference: "system" (the default) defers to the OS via the
// stylesheet's `prefers-color-scheme` rules; "light"/"dark" pin a theme by stamping a
// `data-theme` attribute on <html> that those rules' overrides key on. Persisted in
// localStorage — a viewing choice, never vault state, so it doesn't touch the host.

const THEME_KEY = "b2:theme";

function isThemePref(v: string | null): v is ThemePref {
  return v === "system" || v === "light" || v === "dark";
}

/** Reflect `state.theme` onto <html>: absent attribute ⇒ follow the OS. */
function applyTheme(): void {
  const root = document.documentElement;
  if (state.theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", state.theme);
}

/** Read the saved preference into state and apply it (once, first thing on boot). */
function loadTheme(): void {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(THEME_KEY);
  } catch {
    // localStorage can be unavailable (e.g. private mode) — fall back to System.
  }
  state.theme = isThemePref(saved) ? saved : "system";
  applyTheme();
}

/** Persist + apply an appearance choice from the Settings control. */
function setTheme(theme: ThemePref): void {
  if (state.theme === theme) return;
  state.theme = theme;
  try {
    localStorage.setItem(THEME_KEY, theme);
  } catch {
    // Non-fatal: the choice still applies for this session if it can't persist.
  }
  applyTheme();
  render();
}

// --- text size (View ▸ Zoom In / Zoom Out / Actual Size) ---------------------------
//
// The appearance preference's sibling, and stored the same way for the same reason: a
// reading size is a viewing choice, never vault state. Two things differ.
//
// Where it lands: a theme is a `data-theme` attribute the stylesheet reads, but no
// stylesheet can scale px-sized chrome, so this one leaves the webview and comes back as
// WebKit page zoom (zoom.ts's header is the argument in full).
//
// And where the *keystroke* lands. ⌘= / ⌘- / ⌘0 are not in the registry: they are the
// View menu's, declared in `crates/b2-desktop/src/menu.rs`, because macOS expects Zoom
// In / Zoom Out / Actual Size to live there with their chords printed beside them — and
// an accelerator the menu owns is dispatched before the key window's responder chain, so
// a keydown for one never arrives here to be dispatched. The host emits the chosen item's
// id instead, which is what `initMenuCommands` below listens for. The upshot is that these
// three work from anywhere the window has focus, including mid-edit and behind a dialog,
// with no guard of their own — a size control you have to leave a text field to reach
// would be a size control that fails exactly when you need it.
//
// Module-local rather than in `state`, and the reason is panes.ts's: nothing renders
// from it, so putting it in the model would only invite a repaint the zoom already did.

let zoom = DEFAULT_ZOOM;

/** Hand a size to the host and remember it — the one place the IPC call lives, so both
 *  callers below agree on what "the size is now applied" means.
 *
 *  Resolves when the window has actually been scaled, and **never rejects**: a refusal
 *  can only come from a window that is going away or from running outside Tauri at all,
 *  and neither is worth a toast about a size the user can plainly see didn't change —
 *  still less worth failing a boot over. The two callers both wait on it, and a promise
 *  that can reject would make one of them a hang. */
function pushZoom(next: number): Promise<void> {
  zoom = next;
  saveZoom(next);
  return api.setZoom(next).catch(() => {
    // Non-fatal: the app is fully usable at whatever size it is currently drawn.
  });
}

/** A size the user just asked for: apply it, then say so if it cost a column.
 *
 *  Page zoom narrows the *layout* viewport, so style.css's breakpoints treat a ⌘= exactly
 *  as they treat dragging the window narrower — and at some size discovery goes, then the
 *  file tree. That is the responsive layout doing its job, and B2 doesn't refuse the step
 *  over it (zoom.ts's `hiddenNotice` argues why); it just stops being a surprise.
 *
 *  The notice is **measured, not predicted**, because the breakpoints are the
 *  stylesheet's and this file must not hold a second copy of them — which fixes when it
 *  can be taken. Not before the host has scaled the window (there would be nothing to
 *  see), and not on the frame it does (WebKit lays out on one frame and
 *  `getComputedStyle` can answer on the next). So: the round trip, then two frames. */
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

/** One rung, in `dir`. Silent at the ends — the ladder's walls are walls, not errors. */
function nudgeZoom(dir: Direction): void {
  const next = stepZoom(zoom, dir);
  if (next !== zoom) applyZoom(next);
}

/** Read the saved size and hand it to the host — and **wait for it**, which is the whole
 *  point of this being separate from `applyZoom`.
 *
 *  Page zoom changes the CSS viewport, so everything after this in `boot` depends on it
 *  having landed: `buildShell` + the first `render` would otherwise paint one frame at
 *  100% and jump, the appearance preference's flash-of-the-wrong-thing in a second form,
 *  and `initPanes` settles the columns against `clientWidth` — a width that is about to
 *  change under it. (The resize a zoom fires would eventually correct the columns; it
 *  can't un-paint the frame.) One IPC round trip of blank window is the cheaper half of
 *  that trade.
 *
 *  No column notice here, and not by accident: at boot there is no shell yet, so "which
 *  columns were showing before" has no answer, and the honest thing to report about a
 *  size the user chose in a previous session is nothing at all. */
async function loadZoomPref(): Promise<void> {
  const saved = loadZoom();
  zoom = saved;
  if (saved !== DEFAULT_ZOOM) await pushZoom(saved);
}

/** Listen for the menu lines that are B2's own. One `switch`, and an id it doesn't know
 *  falls through silently: the host declares the menu, so a line from a newer build is
 *  something to ignore, not something to fail on. */
function initMenuCommands(): void {
  void api.onMenuCommand((id) => {
    if (id === "view.zoom-in") nudgeZoom(1);
    else if (id === "view.zoom-out") nudgeZoom(-1);
    else if (id === "view.zoom-reset" && zoom !== DEFAULT_ZOOM) applyZoom(DEFAULT_ZOOM);
  });
}

// --- the customizable keyboard (GH #121) ------------------------------------------
//
// The same localStorage idiom as the appearance preference above, for the same reason: a
// keyboard layout is a viewing choice, never vault state, so it never touches the host,
// the index, or a byte of Markdown. keymap.ts owns the algebra and the judgement; this is
// the wiring — install a table, run a recorder, persist the result.

/** Lay the stored rebindings over the shipped table and make that the live registry.
 *  Every `isBound` in the handler below, the sheet in Settings, and the conflict checkers
 *  all read `activeBindings()`, so this one call moves the whole keyboard at once —
 *  except for the one keyboard that is not B2's to read from a table. CodeMirror holds
 *  its **own** copy of the chords it was mounted with, so the registry moving under it
 *  changes nothing until it is told; the compartment is how it's told. Null before the
 *  first edit, which is the common case at boot and needs no handling of its own. */
function installKeymap(): void {
  setActiveBindings(applyOverrides(DEFAULT_BINDINGS, state.keyOverrides));
  editorView?.dispatch({ effects: keysCompartment.reconfigure(editorKeymap()) });
}

/** Read the saved keyboard and install it. Runs before `buildShell`, which projects a
 *  chord into the find bar's tooltip, and long before anything can dispatch one.
 *
 *  Returns what it had to drop rather than reporting it: a dropped entry is worth saying
 *  out loud — it is a preference the user thought they had — but `flash` repaints, and at
 *  this point in boot there is no shell to repaint. keymap.ts's `adoptOverrides` is what
 *  guarantees whatever survives still leaves the keyboard conflict-free; this can only
 *  return a non-empty list for a hand-edited store, or one written against an older table. */
function loadKeymap(): string[] {
  const { overrides, dropped } = loadOverrides();
  state.keyOverrides = overrides;
  installKeymap();
  return dropped;
}

/** Adopt a set of rebindings: persist, install, repaint. The one write path, so the
 *  stored keyboard and the live one can't come apart. */
function setOverrides(next: Overrides): void {
  state.keyOverrides = next;
  saveOverrides(next);
  installKeymap();
  render();
}

// The recorder. `state.recorder` is the whole of its state; these five actions are the
// whole of its behavior.

/** Milliseconds the current recorder has been open with nothing having arrived — the
 *  probe's input (recorder.ts). Module-local because nothing renders from it directly;
 *  the tick writes its *reading* into `state.recorder.hint` and repaints from there. */
let recorderOpenedAt = 0;
let recorderTimer: number | null = null;

function startRecording(id: BindingId): void {
  const b = findBinding(activeBindings(), id);
  if (!b || b.fixed !== undefined) return; // a chip for a fixed chord isn't a button at all
  state.recorder = { id, candidate: null, problems: [], hint: null, blurred: false };
  recorderOpenedAt = Date.now();
  cancelProbe();
  // The probe: silence is the observation, so something has to come back and read it.
  recorderTimer = window.setTimeout(() => {
    recorderTimer = null;
    if (!state.recorder || state.recorder.candidate !== null) return;
    const { blurred } = state.recorder; // what the session has observed, not what this tick assumes
    state.recorder.hint = silenceHint({ elapsedMs: Date.now() - recorderOpenedAt, blurred });
    render();
  }, PROBE_AFTER_MS);
  render();
  // Take the keyboard off whatever had it — a tree row, or CodeMirror, which would
  // otherwise type the chord into the buffer behind the dialog. The strip carries an id,
  // so `paintModal` hands focus back to it across every repaint the recorder causes.
  document.getElementById("keys-recorder")?.focus();
}

/** Stop the pending read of silence.
 *
 *  Called wherever something has already answered the question the timer was going to ask
 *  — the recorder closing, a chord arriving, or the window blurring. That last one is the
 *  subtle case (GH #125): a blur sets the *strong* hint, and a timer left running would
 *  fire moments later, still see no candidate, and overwrite it with the weaker "nothing
 *  has reached B2 yet" — downgrading the one positive observation the recorder ever makes
 *  into a guess. */
function cancelProbe(): void {
  if (recorderTimer !== null) {
    clearTimeout(recorderTimer);
    recorderTimer = null;
  }
}

/** Tear the recorder down without painting — for the callers that are mid-teardown of
 *  something larger and will paint themselves (`dismissOverlays`). */
function clearRecorder(): void {
  cancelProbe();
  state.recorder = null;
}

function stopRecording(): void {
  clearRecorder();
  render();
}

/** A keydown while the recorder is open. Returns true when it consumed the event.
 *
 *  Esc cancels and ⏎ accepts — the dialog reflex, and the reason neither can be recorded
 *  here. Both are `fixed` in the registry anyway (`Binding.fixed`), so nothing is lost:
 *  what a text field and a dialog do with ⏎ and Esc was never B2's to hand out. Every
 *  other keystroke is a candidate, replacing whatever was captured before it, so trying
 *  three chords is three presses rather than three round trips through the buttons. */
function recorderKeydown(e: KeyboardEvent): boolean {
  const rec = state.recorder;
  if (!rec) return false;
  // Swallow the browser's own meaning for the keystroke — the point of the surface is that
  // nothing *happens* while a chord is being pressed at it. This is not what keeps the
  // chord out of the note buffer, though: CodeMirror's handler sits on a descendant of
  // `document` and so runs first. `startRecording` takes the keyboard off the editor for
  // that, and this branch is why it has to.
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

/** Save the captured chord. A refusal never reaches here — the button is disabled and
 *  ⏎ declines — so this is the commit, not the check. */
function commitRecording(): void {
  const rec = state.recorder;
  if (!rec || rec.candidate === null || refused(rec.problems)) return;
  const next = withOverride(state.keyOverrides, rec.id, [rec.candidate]);
  clearRecorder(); // the strip goes; `setOverrides` does the one repaint
  setOverrides(next);
}

/** Put one command back on its shipped chord — a delete from the override set, since
 *  there is no stored copy of a default to restore from. */
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
// Gating is the pure `shouldPromptEmbedInstall` (embedreminder.ts); these own the
// dismissal side. The banner nags once per launch on a fresh, model-less vault — a plain
// ✕ hides it for the session (it returns next launch), while "Don't remind me again"
// persists the opt-out so a keyword-only user isn't pestered. Same localStorage idiom as
// the appearance preference above (a viewing choice, never vault state).

const EMBED_REMINDER_KEY = "b2:embed-reminder-off";

/** Read the persisted "don't remind me" opt-out into state (once, on boot). */
function loadEmbedReminderPref(): void {
  try {
    state.embedReminderDismissed = localStorage.getItem(EMBED_REMINDER_KEY) === "1";
  } catch {
    // localStorage unavailable (e.g. private mode): default to showing the reminder.
  }
}

/** Turn the banner off. `persist` writes the opt-out so it survives relaunch (the
 *  checkbox); the bare ✕ passes false and only hides it for this session. */
function dismissEmbedReminder(persist: boolean): void {
  state.embedReminderDismissed = true;
  if (persist) {
    try {
      localStorage.setItem(EMBED_REMINDER_KEY, "1");
    } catch {
      // Non-fatal: the opt-out still holds for this session if it can't persist.
    }
  }
  render();
}

// Download + verify the selected model in-app (the `b2 init` button). Single-flight via
// `state.provisioning` (the webview is single-threaded, so the sync guard + button-disable
// fully prevent a concurrent download — no host guard needed). On success the model's
// `installed` flag flips and the Download button disappears.
async function provisionModel(): Promise<void> {
  if (state.provisioning) return;
  state.provisioning = true;
  render();
  try {
    state.models = await api.provisionModel();
    const now = state.models.find((m) => m.current);
    // The model is installed now, so `vault_info` reports `semantic: true` — re-read it so
    // the install banner and the search caveat both clear immediately.
    await refreshEmbedStatus(state.vaultRoot);
    flash(`Downloaded ${now?.label ?? "model"}. Embedding your vault now…`);
    // Close the loop (#25 auto-index): actually embed the vault so semantic search turns
    // on, instead of leaving it behind a manual Reindex the user is unlikely to find.
    // Background run with the usual progress + Cancel; no-ops if already complete.
    trackIndexing(autoIndexOnOpen(state.vaultRoot));
  } catch (e) {
    flash(errText(e));
  } finally {
    state.provisioning = false;
    render();
  }
}

// Persist a model choice. A no-op if it's already current; otherwise record it and tell
// the user what still has to happen for the swap to take effect (download, then Reindex).
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
    // The write was refused; re-sync the picker to the unchanged config and surface why.
    flash(errText(e));
    try {
      state.models = await api.listModels();
    } catch {
      /* leave the stale list; the toast already explains */
    }
  }
  render();
}

// Switch the active vault via the host's native folder picker. On a fresh choice the
// open note, discovery, search, and tree-expansion all reset (they belong to the old
// vault); a cancel is a no-op. The picker runs host-side, so all this action does is
// re-seed state from the new `VaultInfo` and reload the tree.
async function switchVault(): Promise<void> {
  // Flush + leave edit mode before the picker (same hook as openNote); then drop any
  // pending trailing embed — it belongs to the vault we may be about to leave, and
  // its DB-derived pending set heals on that vault's next embed/reindex anyway.
  if (!fmEditGuard()) return;
  if (!(await closeEditor())) return;
  if (embedTimer !== undefined) {
    clearTimeout(embedTimer);
    embedTimer = undefined;
  }
  try {
    const info = await api.chooseVault();
    if (!info) return; // cancelled — leave the current vault untouched
    // `choose_vault` already cancelled any in-flight index for the vault we're leaving
    // (host-side); capture its frontend run so the new vault's auto-index can be chained
    // *after* it settles — otherwise the new run could see a not-yet-cleared `reindexing`
    // flag and bail. Not awaited here: the UI reset below must not block on a wind-down.
    const departing = indexingRun;
    state.vaultRoot = info.root; // set now so the departing run's guards bail promptly
    state.semantic = info.semantic;
    state.notesEmbedded = info.notes_embedded;
    state.notesTotal = info.notes_total;
    state.current = null;
    state.currentResource = null;
    state.resourceImage = null;
    state.similar = [];
    state.connections = [];
    state.resourceLinks = [];
    state.unresolved = [];
    resetSearch();
    // The conversation is grounded in the vault we just left — every citation in it
    // names a path that means nothing here (a note's identity is its path, L1). Dropping
    // it writes nothing and loses nothing durable: the transcript was session state. The
    // answer in flight is stopped for the reason a departing reindex is: nothing should
    // keep working on the vault the app has left. `sendChat` drops what it was holding.
    if (state.chatStreaming !== null) void api.cancelAsk().catch(() => {});
    state.chatMessages = [];
    state.chatStreaming = null;
    state.expandedDirs = new Set<string>();
    state.selectedDir = ""; // the create context belongs to the vault we left
    state.dirs = []; // loadNotes below re-lists the new vault's structure
    state.treeCreate = null;
    navClear(); // history is per-vault: the old stack's paths mean nothing here
    const input = document.getElementById("search-input") as HTMLInputElement | null;
    if (input) input.value = "";
    state.loading = true;
    render();
    await loadNotes(); // catches its own errors → toast; empty tree on an unindexed vault
    state.loading = false;
    flash(`Switched to ${info.root}.`);
    // Auto-index the new vault (#25): if it's unindexed or only partly embedded, bring it
    // up to date now — the tree we just painted fills in as projection completes. Chained
    // after the departing run so it starts only once that has fully wound down.
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

// Re-read the embedding-coverage fraction (#26) from the host so the search caveat
// reflects reality after a project/embed phase. Best-effort and guarded on the vault we
// started on: a mid-run switch owns the UI, so a stale count must never clobber its fresh
// one, and a failed status read just leaves the prior fraction rather than blocking.
async function refreshEmbedStatus(forRoot: string | null): Promise<void> {
  try {
    const info = await api.vaultInfo();
    if (state.vaultRoot !== forRoot) return;
    state.semantic = info.semantic;
    state.notesEmbedded = info.notes_embedded;
    state.notesTotal = info.notes_total;
  } catch {
    // ignore — coverage is a hint, never worth surfacing an error over
  }
}

// The in-flight background index — a manual Reindex (`doReindex`), an auto-index on
// open (`autoIndexOnOpen`), or a trailing embed after a save (`runTrailingEmbed`) — or
// null when idle. A vault switch cancels the run host-side (choose_vault →
// cancel_and_wait_for_reindex) and then chains the new vault's auto-index *after* this
// handle, so the fresh run never starts on the departing run's not-yet-cleared
// `state.reindexing` flag. Only one index runs at a time (each entry point guards on
// `reindexing`), so a single slot suffices.
let indexingRun: Promise<void> | null = null;

/** Register a background-index run so a vault switch can chain after its wind-down. The
 *  tracked promise settles *after* the run's `finally` has cleared `state.reindexing`. */
function trackIndexing(run: Promise<void>): void {
  const done = run.finally(() => {
    if (indexingRun === done) indexingRun = null;
  });
  indexingRun = done;
}

// Reindex as project → embed, sequenced here (Shape A, docs/index-engine.md):
// the fast, model-free `project` completes the keyword + graph index, the tree
// paints immediately, and only then does the slow, cancellable `embed` stream behind
// it. Deliberately does NOT set `state.loading` — the app stays fully usable
// (read/search/navigate) while it runs; only the Reindex button is disabled and a
// progress + Cancel affordance shows. Progress streams in via the channel callback,
// which repaints only the affordance.
async function doReindex(): Promise<void> {
  if (state.reindexing) return; // single-in-flight (the host also guards embed)
  const startedRoot = state.vaultRoot; // guard against a vault switch mid-run
  state.reindexing = true;
  state.reindexProgress = null;
  state.reindexCancelling = false;
  render();
  try {
    // Phase 1 — projection (fast, no model): notes, keyword index, and graph are
    // complete when this resolves.
    const p = await api.project();
    // If a switch already committed (vaultRoot changed), it owns the UI — bail. (A
    // late-finishing project is harmless host-side: it wrote the old vault's own
    // .b2/, idempotently — spec §6.)
    if (state.vaultRoot !== startedRoot) return;
    // A projection can skip files it can't read (non-UTF-8, permission-denied) rather
    // than abort — appended to every reindex flash below so the user knows some files
    // were left out, and why, instead of silently missing them.
    const skipped = p.skipped.length
      ? ` — skipped ${p.skipped.length} unreadable file(s): ${p.skipped
          .map((s) => `${s.path} (${s.reason})`)
          .join(", ")}`
      : "";
    // The tree paints HERE — a projection can add, remove, or rename notes, and the
    // vault is browsable + keyword-searchable while embedding runs.
    await loadNotes();
    // The search caveat now reads "keyword-only for now (0/M embedded)" honestly while
    // the embed phase below fills the vectors (#26).
    await refreshEmbedStatus(startedRoot);
    render();
    if (state.reindexCancelling) {
      // Cancel landed during the short projection window: don't start embedding (the
      // host would clear the flag and run to completion). The projected index is
      // complete and consistent; vectors fill on the next run.
      flash(
        `Indexed ${p.indexed} note(s) — cancelled before embedding. Re-run to embed.${skipped}`,
      );
      return;
    }
    // Phase 2 — embedding (real model), metered + cancellable via the host's slot.
    const r = await embedWithProgress(startedRoot);
    // If the switch already committed (vaultRoot changed), it owns the UI — bail.
    if (state.vaultRoot !== startedRoot) return;
    // The common ordering is subtler: the host frees the embed slot *before* the
    // vault-switch command returns, so this Promise usually resolves while `vaultRoot`
    // is still `startedRoot` — the check above misses it. But a cancel we didn't
    // initiate (`reindexCancelling` is false) can only come from a vault switch
    // cancelling us host-side (main.rs `cancel_and_wait_for_reindex` is the sole other
    // cancel source). In that case the switch will reload the new vault — so we must
    // NOT toast or touch the vault we're leaving. A user-initiated cancel
    // (`reindexCancelling` true) *does* fall through: the projected index is complete
    // and a prefix embedded, worth reporting.
    if (r.cancelled && !state.reindexCancelling) return;
    // Coverage is now total/total after a full embed, or the partial count after a cancel
    // — the search caveat updates to match (#26).
    await refreshEmbedStatus(startedRoot);
    flash(
      r.cancelled
        ? `Embedded ${r.embedded}/${p.indexed} note(s) — cancelled. Re-run to finish the rest.${skipped}`
        : `Indexed ${p.indexed} note(s) — ${r.embedded} embedded.${skipped}`,
    );
    if (state.current) {
      // Projection may have stamped the open note on disk; re-read it, and refresh
      // discovery now that vectors exist for `similar` to rank with. Not mid-edit:
      // the editor's revision chain owns the note then (an indexed note is already
      // stamped), and adopting a re-read racing an in-flight save could regress the
      // chain into a false conflict.
      if (!state.editing && !state.fmEditing) {
        state.current = await api.readNote(state.current.path);
      }
      await refreshDiscovery();
    }
  } catch (e) {
    if (state.vaultRoot === startedRoot) flash(errText(e));
  } finally {
    state.reindexing = false;
    state.reindexProgress = null;
    state.reindexCancelling = false;
    render();
  }
}

// Auto-index on open (#25): the moment a vault is opened — app launch or vault switch —
// bring its index up to date with no manual Reindex click and no confirm dialog. The
// detector is the model-free embedding-coverage read already in `VaultInfo` (#26):
//   • notesTotal === 0        → never projected: run the fast `project` first (its tree +
//                               keyword search go live in seconds), then embed.
//   • notesEmbedded < total   → projected but embedding didn't finish (a prior cancel or
//                               crash): only the trailing vectors need filling — the pass
//                               is self-healing off the DB-derived pending set (split §7.2).
//   • embedded === total (>0) → index complete: left untouched, so reopening is never busywork.
// The embed phase runs only when the real model is installed (`state.semantic`); without
// it a fresh vault still gets its keyword + graph index and nothing errors — the search
// caveat already reads "keyword-only for now". Silent like the trailing embed after a
// save: the progress meter (and Cancel) are the only chrome, no toast. Reuses doReindex's
// exact vault-switch guards (spec §6) so a switch mid-run never touches the departed vault.
async function autoIndexOnOpen(startedRoot: string | null): Promise<void> {
  if (state.reindexing || state.vaultRoot === null) return; // a run is live, or no vault
  const projected = state.notesTotal > 0;
  if (projected && state.notesEmbedded >= state.notesTotal) return; // index already complete
  const needsProject = !projected;
  // An already-projected vault with no model has nothing left we can do; a never-projected
  // one still gets its keyword + graph index below (project is model-free).
  if (!needsProject && !state.semantic) return;

  state.reindexing = true;
  state.reindexProgress = null;
  state.reindexCancelling = false;
  render();
  try {
    if (needsProject) {
      await api.project();
      if (state.vaultRoot !== startedRoot) return; // a switch took over — it owns the UI
      await loadNotes(); // the tree paints HERE; keyword search is live
      await refreshEmbedStatus(startedRoot); // caveat reads "keyword-only for now (0/M)"
      render();
    }
    // Embed only with a real model, not if a Cancel landed during the project window, and
    // not if a vault switch has taken over meanwhile (don't embed the vault we're leaving).
    if (state.vaultRoot !== startedRoot || !state.semantic || state.reindexCancelling) return;
    const r = await embedWithProgress(startedRoot);
    if (state.vaultRoot !== startedRoot) return;
    // A cancel we didn't initiate came from a vault switch stopping us host-side; that
    // switch reloads the new vault, so leave the one we're departing untouched (spec §6).
    if (r.cancelled && !state.reindexCancelling) return;
    await refreshEmbedStatus(startedRoot);
    // If the user opened a note while embedding ran, its vectors exist now — re-read it
    // (projection may have stamped it) and refresh discovery so `similar` can rank. Not
    // under a live editor (body or frontmatter): adopting a fresh revision beneath an
    // open buffer would let its save silently clobber what changed on disk — the same
    // carve-out as reconcile and doReindex.
    if (state.current && !state.editing && !state.fmEditing) {
      state.current = await api.readNote(state.current.path);
      await refreshDiscovery();
    }
  } catch {
    // Silent by design (§7.2): the user didn't ask for this run, so a missing model or a
    // lost race just leaves the vault keyword-first; the pending set heals on the next run.
  } finally {
    state.reindexing = false;
    state.reindexProgress = null;
    state.reindexCancelling = false;
    render();
  }
}

// Ask the host to stop the in-flight embed at its next batch boundary. Cooperative:
// the embed Promise in `doReindex` resolves shortly after with `cancelled: true`, and
// its `finally` clears the affordance. (During the short projection window there is
// nothing host-side to stop; `doReindex` sees `reindexCancelling` and skips embed.)
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
// Edit mode hands the note pane to a CodeMirror 6 editor and autosaves on idle
// through the guarded, model-free `write_note`. Everything here that never drives a
// render is a module-local, not AppState: the EditorView, the debounce timers, and
// the single-flight save flags.

let editorView: EditorView | null = null;
let autosaveTimer: number | undefined;
let embedTimer: number | undefined;
// Live-preview lives in a Compartment (spec §5) so `</>` can swap it for raw source
// mode with no remount. Two configs off the sticky `sourceOpen`: decorated (the
// document feel) or raw + today's syntax colors.
//
// Both configs paint with `b2Highlighter` — one palette across all three surfaces
// (highlight.ts). What differs is *reach*, and the CSS scope is what expresses it: source
// mode colors the whole document (`.src-body`), live preview only the fence lines
// (`.lp-fence`), because there the Markdown's own markup is already spoken for by the
// `.lp-*` decorations. CodeMirror's stock `defaultHighlightStyle` is deliberately gone:
// its colors are hard-coded for a light background, so source mode read as ink-on-ink in
// dark mode — the `--syn-*` palette is theme-aware.
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

// Enter edit mode: one render (which now skips the note pane — the carve-out), then
// the pane is ours: chrome built once here, owned imperatively until exit.
function enterEdit(): void {
  const n = state.current;
  if (!n || state.editing || state.loading) return;
  if (!fmEditGuard()) return; // one editor at a time — resolve the drawer first
  state.editing = true;
  state.editConflict = false;
  render();
  mountEditor(n.body);
}

// Wikilink completion (the Obsidian gesture): typing `[[` opens a picker over the
// vault's notes + files. The logic — trigger detection, ranking, bracket-closing —
// is the pure wikicomplete.ts; this is only the CodeMirror adapter. `filter: false`
// because the ranking is ours (title-prefix > title > path), and no `validFor` so
// each keystroke re-queries it — the lists live in `state` and are already loaded,
// so a query is just an in-memory scan.
function wikiSource(ctx: CompletionContext): CompletionResult | null {
  const line = ctx.state.doc.lineAt(ctx.pos);
  const found = wikiQueryAt(line.text.slice(0, ctx.pos - line.from));
  if (!found) return null;
  const options = wikiCandidates(state.notes, state.resources, found.query).map((c) => ({
    label: c.label,
    detail: c.detail,
    apply: (view: EditorView, _completion: unknown, from: number, to: number) => {
      const { insert, cursor } = wikiInsertion(c.target, view.state.sliceDoc(to, to + 2));
      view.dispatch({
        changes: { from, to, insert },
        selection: { anchor: from + cursor },
      });
    },
  }));
  return { from: line.from + found.from, options, filter: false };
}

// Formatting chords (⌘B/⌘I, …) — the CodeMirror adapter over the pure format.ts
// engine. The keymap derives from the `FORMATS` table paired with each row's chord from
// the keyboard registry (`format.<id>`, bindings.ts), so adding a format is one new row
// in each and no wiring here. `changeByRange` runs the toggle per selection range and
// maps the coordinates, so multi-cursor edits come free.
function runFormat(view: EditorView, fmt: InlineFormat): boolean {
  const doc = view.state.doc.toString();
  view.dispatch(
    view.state.changeByRange((range) => {
      const r = toggleInline(doc, range.from, range.to, fmt);
      return { changes: r.changes, range: EditorSelection.range(r.selFrom, r.selTo) };
    }),
  );
  return true;
}

/**
 * Tab / ⇧Tab — nest or lift out the list item(s) the selection covers. The engine is
 * list.ts; this is the CodeMirror half, the `runFormat` pattern one construct up.
 *
 * Declining matters twice over. A `null` from the engine usually means the caret is not
 * in a list, and returning false there is what leaves Tab walking the focus ring through
 * the rest of the app — the reason this isn't `indentWithTab`, which claims the key
 * outright and takes the keyboard's way out of the buffer with it. The one exception is
 * a list the engine can't see — `inListItem` below gives the syntax tree the last word,
 * so the no-ejection contract holds there too. And a caret inside code declines before
 * the engine is asked at all: a `- item` line in a fence is text, not structure, and
 * `inCodeContext` is the same read the rich paste makes to keep its hands off code.
 *
 * One range rather than `changeByRange`: an indent moves every offset after it, so a
 * second cursor's edit would be computed against a document the first has already
 * shifted. Multi-cursor nesting is a gesture nobody makes; ⌘B's is one they do.
 */
function runListShift(
  view: EditorView,
  shift: (doc: string, from: number, to: number) => ListEdit | null,
): boolean {
  if (inCodeContext(view.state)) return false;
  const { from, to } = view.state.selection.main;
  const r = shift(view.state.doc.toString(), from, to);
  if (!r) return inListItem(view.state);
  // Claimed but inert — the first item of a list has nothing to nest under. Swallowing
  // the key is the point (list.ts's header): a gesture that sometimes ejects you from
  // the buffer is worse than one that sometimes does nothing.
  if (r.changes.length > 0) {
    view.dispatch({
      changes: r.changes,
      selection: EditorSelection.range(r.selFrom, r.selTo),
      scrollIntoView: true,
    });
  }
  return true;
}

/** Is the cursor inside a list item the *scanner* can't see? list.ts reads only lists at
 *  the top level of the note — `> - a` is a bullet behind a container prefix it doesn't
 *  parse — so on its null the tree gets the last word before the key is handed back to
 *  the focus ring. Claimed-but-inert there: the caret is visibly on a list item, and
 *  ejecting from it would break the gesture's contract even where the edit itself isn't
 *  built yet (list.ts's header). */
function inListItem(state: EditorState): boolean {
  const at = syntaxTree(state).resolveInner(state.selection.main.from, -1);
  for (let n: typeof at | null = at; n; n = n.parent) {
    if (n.name === "ListItem") return true;
  }
  return false;
}

/**
 * B2's own chords inside the editor, read from the **live** registry each time.
 *
 * A function, not a const, and that distinction is the whole of GH #125's first review
 * note: built once at module load, this array froze the shipped chords *before* boot had
 * even read the user's rebindings, so a rebound ⌘B would move in the sheet and in every
 * conflict check while CodeMirror went on answering to ⌘B forever. The compartment below
 * is what carries a change into a *mounted* editor; this is what makes a fresh mount
 * correct in the first place.
 */
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

// The editor's keymap lives in a Compartment for `lpCompartment`'s reason — a change has
// to reach a *mounted* editor without remounting it. Settings is reachable while editing
// (⌘, is an unguarded toggle), so rebinding ⌘B is something you can do with the buffer
// open behind the dialog, and without this the new chord would only take effect the next
// time you entered edit mode.
//
// B2's chords and the stock ones stay in **one** `keymap.of` array, B2's first: that
// ordering is what makes ⌘I italic rather than `selectParentSyntax`, and
// editorkeys.test.ts pins the overlap set on that reasoning. Splitting them into two
// facet inputs would leave the same outcome resting on extension order instead, which is
// a quieter thing to depend on.
const keysCompartment = new Compartment();
const editorKeymap = (): Extension => keymap.of([...b2EditorKeymap(), ...STOCK_EDITOR_KEYMAP]);

// ⌘T — drop a fresh 3-column table (header + two rows) at the cursor, caret in the
// first cell. Block insert, not an inline toggle, so it's its own binding over the pure
// `insertTable` (format.ts).
function runInsertTable(view: EditorView): boolean {
  const { from, to } = view.state.selection.main;
  const r = insertTable(view.state.doc.toString(), from, to);
  view.dispatch({
    changes: r.changes,
    selection: EditorSelection.range(r.selFrom, r.selTo),
    scrollIntoView: true,
  });
  return true;
}

// Rich paste — the CodeMirror adapter over the pure paste.ts. A copy from a web page
// carries a `text/html` flavor next to `text/plain`; CodeMirror's own paste takes the
// plain one, which is why every heading, bold and list used to vanish on the way in.
// This converts the HTML to Markdown instead, and *declines* in two cases, leaving
// CodeMirror's paste to run untouched:
//   - the cursor sits in code, where pasted text must stay literal;
//   - the HTML carried no formatting the plain flavor didn't already have (paste.ts's
//     `markdownForPaste` returns null) — pasting escaped Markdown there would be a loss.
// The third way out is the ⌘⇧V chord below, which does its own plain paste.

/** Is the cursor inside code — a fenced block, an indented block, or an inline span?
 *  The read itself is droplink.ts's (by position, since a *drop* names a place the cursor
 *  isn't); this is the cursor's spelling of the same question. */
function inCodeContext(state: EditorState): boolean {
  return inCodeAt(state, state.selection.main.from);
}

function handlePaste(event: ClipboardEvent, view: EditorView): boolean {
  const data = event.clipboardData;
  if (!data || inCodeContext(view.state)) return false;
  const md = markdownForPaste(data.getData("text/html"), data.getData("text/plain"));
  if (md === null) return false;
  event.preventDefault();
  view.dispatch({
    ...view.state.replaceSelection(md),
    scrollIntoView: true,
    userEvent: "input.paste",
  });
  return true;
}

const richPaste = EditorView.domEventHandlers({ paste: handlePaste });

/**
 * ⌘⇧V — paste as plain text, the escape hatch from the conversion above.
 *
 * It performs the paste itself rather than deferring to the webview, because the
 * webview does nothing: WebKit's *Paste and Match Style* is a menu command, so a raw
 * ⌘⇧V reaches the page and no paste event ever fires (verified in the app). Reading the
 * clipboard is therefore ours to do, and it goes through the **host** — WebKit gates a
 * programmatic `navigator.clipboard` read behind a native confirmation, and the webview
 * holds no clipboard permission by design (crates/b2-desktop/CLAUDE.md).
 *
 * The binding claims the chord (returns true) so a platform whose webview *does* paste
 * on ⌘⇧V can't also insert its own copy.
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
          <button id="edit-done" class="btn small primary" title="Save and return to reading — ⌘E (⌘S flushes anytime)">Done</button>
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
      // GFM base + the wikilink node: the reading view's `gfm: true` twin, and without
      // `markdownLanguage` there's no `Strikethrough` node (the default base is
      // CommonMark-only). Always on — the parser feeds both live preview and source mode.
      // `codeLanguages` lets a ```lang fence's body be parsed by that language's own
      // grammar (loaded lazily, reparsed when it lands) — the editor half of highlight.ts,
      // sharing its resolver so a fence resolves identically here and in the reading view.
      // Source mode picks the colours up too, in its own style.
      markdown({ base: markdownLanguage, extensions: [wikilink], codeLanguages: resolveLang }),
      history(),
      // Formatting chords first so a future format key can shadow a default binding.
      // Every `key` here comes out of the registry, which is why its syntax is
      // CodeMirror's: one spelling of a chord serves the editor and the sheet alike.
      // In a compartment so a rebinding reaches this editor while it is still mounted
      // (`editorKeymap`); the stock bindings ride along because they share the array whose
      // order decides who wins.
      keysCompartment.of(editorKeymap()),
      EditorView.lineWrapping,
      // Web-page formatting survives the clipboard (see `richPaste` above); an
      // unformatted paste still takes CodeMirror's own path.
      richPaste,
      // A discovery card dragged in from the right column — the drop preview and the
      // insertion (droplink.ts). Ordinary text drags inside the buffer are untouched: the
      // handlers decline anything that isn't the card drag.
      wikilinkDrop,
      // `[[` completion — always on, in both live-preview and source mode. Its
      // keymap (arrows/Enter/Escape while the menu is open) binds at higher
      // precedence than defaultKeymap, so Enter accepts rather than newlines.
      autocompletion({ override: [wikiSource], icons: false }),
      // The note pane is an overflow scroll container; render tooltips fixed on
      // <body> so a menu near the pane's bottom edge isn't clipped by it.
      tooltips({ position: "fixed", parent: document.body }),
      lpCompartment.of(livePreviewConf()),
      // Find-in-note (⌘F) match decorations — inert (null) until the bar sets a query.
      findField,
      EditorView.updateListener.of((u) => {
        if (u.docChanged) {
          scheduleAutosave();
          // An edit reshapes the match set (the field already recomputed) — keep the
          // bar's count pill in step.
          if (findOpen) syncEditorFind(u.view);
        }
      }),
    ],
    parent: el("editor-host"),
  });
  editorView.focus();
  paintEditor();
  // An open find bar carries across the mount (Edit clicked, or a conflict reload):
  // same query, editor engine.
  if (findOpen) setFindQuery(findInput().value);
}

/** The editor chip's tooltip. Its chord comes out of the live registry rather than being
 *  spelled here, for `graphToggleHtml`'s reason (render.ts): ⇧⌘E is rebindable (#121), so
 *  a tooltip naming the shipped default would be wrong for the user who moved it. Off
 *  "live preview" rather than the reading bar's "rendered Markdown" — one sticky flag,
 *  two surfaces, and each names what *it* shows when the flag is off. */
function editorSourceTitle(): string {
  const what = state.sourceOpen ? "Show live preview" : "Show Markdown source";
  return `${what} — ${displayKeys(["source.toggle"])}`;
}

// Repaint just the editor's conflict bar and the `</>` source-toggle button — never a
// pane rebuild (the same targeted-repaint pattern as paintReindex).
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

/**
 * The save chain's entry — an immediate flush (skips the debounce). Single-flight:
 * one save in flight, at most one trailing marked; the returned promise resolves
 * when the whole chain settles, so flush points can await it.
 */
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
      // The chain: the next save bases on the revision this one returned, so our own
      // saves never self-conflict (spec §3 "last save wins"). Mirroring the buffer
      // into `body` means exiting edit mode renders the saved text with no re-read.
      cur.revision = report.revision;
      cur.body = buffer;
      scheduleTrailingEmbed();
      void refreshConnections(); // a body edit can add/remove [[wikilink]] edges
    } catch (e) {
      if (isWriteConflict(e)) {
        // Pause the chain and put the decision to the user — never re-fire into a
        // conflict, never silently clobber.
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

// Post-save connection refresh (spec §6 "what refreshes"). Quiet on failure —
// autosave is a background hum, and the pane corrects on the next open/discovery.
async function refreshConnections(): Promise<void> {
  const cur = state.current;
  if (!cur) return;
  try {
    const explain = await api.explain(cur.path);
    if (state.current?.path !== cur.path) return; // navigated away meanwhile
    state.connections = explain.connections;
    state.resourceLinks = explain.resources;
    state.unresolved = explain.unresolved;
    render();
  } catch {
    // deliberately silent
  }
}

// After the save chain settles (~2s with no saves), fill the vectors the saves
// invalidated. Keyword search and the graph are current from the save itself;
// `similar`/semantic lag by these seconds (spec §6).
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
  // A full run is already live (its embed covers our note), or no vault: skip — the
  // missing-vector set is DB-derived, so any later embed/reindex heals it (split §7.2).
  if (state.reindexing || state.vaultRoot === null) return;
  const startedRoot = state.vaultRoot;
  state.reindexing = true;
  state.reindexProgress = null;
  state.reindexCancelling = false;
  paintReindex();
  try {
    await embedWithProgress(startedRoot);
    // Vectors are fresh — let `similar` rank with them.
    if (state.vaultRoot === startedRoot) await refreshDiscovery();
  } catch {
    // Refused (ReindexInFlight race) or failed (e.g. no model provisioned): skip
    // silently — the user didn't ask for this run, and the pending set heals.
  } finally {
    state.reindexing = false;
    state.reindexProgress = null;
    state.reindexCancelling = false;
    render();
  }
}

// The one embed invocation shape, shared by doReindex's phase 2 and the trailing
// embed: stream per-batch progress into the persistent affordance, ignoring stray
// events from a vault we've switched away from.
function embedWithProgress(startedRoot: string | null) {
  return api.embed((prog) => {
    if (state.vaultRoot !== startedRoot) return;
    state.reindexProgress = prog;
    paintReindex();
  });
}

// Conflict bar: Reload — discard the buffer; read fresh, remount on the new
// body/revision, resume autosave.
async function conflictReload(): Promise<void> {
  const cur = state.current;
  if (!cur) return;
  try {
    const fresh = await api.readNote(cur.path);
    state.current = fresh;
    editorView?.destroy();
    editorView = null;
    trailingDirty = false;
    autosavePaused = false;
    state.editConflict = false;
    mountEditor(fresh.body);
    void refreshConnections(); // the external edit may have changed edges too
  } catch (e) {
    flash(errText(e));
  }
}

// Conflict bar: Keep mine — read fresh for the *current* revision, then write the
// buffer against it: an explicit, informed overwrite through the same guarded op (no
// force flag exists; a further external edit in this window still conflicts).
async function conflictKeepMine(): Promise<void> {
  const cur = state.current;
  if (!cur || !editorView) return;
  try {
    const fresh = await api.readNote(cur.path);
    // Adopt the disk state (revision to chain on; frontmatter/metadata the external
    // writer may have changed — the splice preserves *disk* frontmatter, so mirror it).
    state.current = fresh;
    autosavePaused = false;
    state.editConflict = false;
    paintEditor();
    await saveNow();
  } catch (e) {
    flash(errText(e));
  }
}

/**
 * Flush and leave edit mode. Returns false when the buffer could not be saved — a
 * conflict (the bar is up) or a failed save — so the caller must abandon whatever
 * navigation triggered the close rather than drop the user's edits.
 */
async function closeEditor(): Promise<boolean> {
  if (!state.editing) return true;
  await saveNow();
  if (state.editConflict) return false;
  if (editorView && state.current && editorView.state.doc.toString() !== state.current.body)
    return false; // the flush failed (its error already toasted) — keep the buffer alive
  if (autosaveTimer !== undefined) {
    clearTimeout(autosaveTimer);
    autosaveTimer = undefined;
  }
  editorView?.destroy();
  editorView = null;
  trailingDirty = false;
  autosavePaused = false;
  state.editing = false;
  state.editConflict = false;
  return true;
}

async function exitEdit(): Promise<void> {
  if (await closeEditor()) render(); // shows the saved text — no re-read needed
}

// --- discovery card → wikilink (droplink.ts) ---------------------------------------
//
// Drag a "Similar & unlinked" card onto a line of the note you are editing and a
// `[[wikilink]]` lands at the end of that line. The rules about *where* it lands, and the
// drop preview, are droplink.ts's; what lives here is what only the running app can own:
// the drag's payload, the save that makes the link real, and the pane refresh that is the
// whole point — a card you have linked is no longer *un*linked, and the column must say so.
//
// The drag is withheld outside edit mode (render.ts sets `draggable` on that condition), so
// this state is only ever set while there is a buffer to drop into. Its keyboard half is
// the card menu's *Insert link at cursor* (K1) — a different gesture aimed at the caret,
// exactly as *Import files…* is the Finder drop's picker-shaped twin.

/** The candidate being dragged out of discovery, or null. A module-local for `treeDrag`'s
 *  reason (same-window DnD needs no dataTransfer round-trip) — but the *authority* on
 *  whether a drag is ours is the payload's MIME type, which survives a mid-drag repaint
 *  destroying the card element and with it the `dragend` that would have cleared this. */
let cardDrag: DraggedCard | null = null;

/** Is this drag the discovery card's? Asked of the *payload* rather than of `cardDrag`,
 *  which a mid-drag repaint can strand: destroying the dragged element takes the `dragend`
 *  that would have cleared it, and a stale flag must never turn an ordinary text drag
 *  inside the editor into a wikilink insertion. */
function carriesCard(e: DragEvent): boolean {
  return e.dataTransfer?.types.includes(CARD_DRAG_MIME) ?? false;
}

/** The editor extension, built once: it reads the drag through this closure rather than
 *  being rebuilt per mount, so `mountEditor` stays a list of extensions. */
const wikilinkDrop = cardDrop({
  dragged: (e) => (carriesCard(e) ? cardDrag : null),
  onDrop: (card) => void commitDroppedLink(card),
});

/** Clear the editor's drop preview — called as the pointer leaves the buffer, so the ghost
 *  never lingers on a line the drag has wandered away from. */
function clearDropPreview(): void {
  editorView?.dispatch({ effects: setDropTarget.of(null) });
}

/** The discovery column as the drag's **cancel** target: let go here and nothing happens.
 *  A dashed wash says the column will take the card back, and `dropEffect = "none"` (the
 *  global dragover, below) is what makes AppKit refuse the drop rather than B2 having to. */
function markSideCancel(on: boolean): void {
  el("side-pane").classList.toggle("is-drag-cancel", on);
}

/**
 * After the link is in the buffer: flush it to disk, then take the card out of Similar.
 *
 * The flush is what makes the gesture a *commit* rather than a keystroke — `commitLink`
 * (the typed, frontmatter kind) has the same shape, and the right column can only tell the
 * truth about a link the index has seen. The save chain does the rest of the work already:
 * `refreshConnections` re-reads the graph, so the new edge appears under Connections within
 * the round trip.
 *
 * Dropping the card from `state.similar` here is an **optimism with a receipt**: the write
 * succeeded, so the note genuinely links it now, and discovery excludes 1-hop neighbours by
 * construction (`discover::candidates`) — the eventual `refreshDiscovery` the trailing embed
 * fires will reach the same list. Waiting for it instead would leave a linked note sitting
 * in "unlinked" for the seconds the embed takes, or (worse) empty the whole section while
 * the note's own vectors are being refilled.
 */
async function commitDroppedLink(card: DraggedCard): Promise<void> {
  const src = state.current;
  if (!src) return;
  await saveNow();
  // Not saved: a conflict (the bar is up, and it owns the decision) or a failed write
  // (already toasted). The link stays in the buffer either way — it is the user's edit —
  // but nothing may claim it landed, and the card stays where it is.
  if (state.editConflict) return;
  if (editorView && state.current && editorView.state.doc.toString() !== state.current.body)
    return;
  if (state.current?.path !== src.path) return; // navigated away while the save ran
  // By the card, not by either of its strings: `withoutCard` is keyed on the path, and
  // taking the pair is what stops the target being handed to a path comparison (the review
  // note on PR #185 — it matched nothing, so a dropped card stayed in the list).
  state.similar = withoutCard(state.similar, card);
  render();
  flash(`Linked [[${card.target}]].`);
}

/** The keyboard's half (K1): insert the same link at the caret's line, from the card menu.
 *  One insertion path with the drop — both plan with `planDrop` and apply with
 *  `insertDrop`, so the two gestures cannot drift into two behaviours. */
function insertCardLink(path: string): void {
  const view = editorView;
  if (!state.editing || !view || !path) return;
  // The same pair the drag carries, built here from the menu's note path — so both halves
  // hand the commit one shape rather than two spellings (`DraggedCard`).
  const card: DraggedCard = { path, target: noteTarget(path) };
  const plan = planDrop(view.state, view.state.selection.main.head, card.target);
  if (plan === null) {
    // The same refusal the drop makes silently (no ghost, no-drop cursor) — said out loud,
    // because a menu item that appeared to do nothing teaches nothing.
    flash("The cursor is inside code — a wikilink there would stay literal. Move it out first.");
    return;
  }
  insertDrop(view, plan);
  view.focus();
  void commitDroppedLink(card);
}

// --- find in note (⌘F) ------------------------------------------------------------
//
// One bar, two engines, one pure core (findbar.ts). The bar is static shell chrome —
// a floating overlay in the note pane's grid area, built once in buildShell and never
// innerHTML-swapped — and its state is module-local like the editor's: transient view
// state, not AppState, so typing in it never triggers a render. Over the reading view
// it paints matches with the CSS Custom Highlight API (Ranges over the rendered text
// nodes — no DOM mutation, so the render memo, scroll position, and click delegation
// are untouched, and a match spanning inline markup still highlights whole). Over the
// editor it drives the CodeMirror StateField below. ⇧⌘F is different in kind: it just
// focuses the global vault-search box in the top bar.

let findOpen = false;
let findQuery = "";
/** The current match set + active index, whichever engine produced it. */
let findMatchesList: Match[] = [];
let findActive = -1;
/** Reading-mode DOM anchors, parallel to findMatchesList; stale after any pane swap. */
let findRanges: globalThis.Range[] = [];
/** The doc the bar is bound to — navigating anywhere else closes it (syncFind). */
let findDocKey: string | null = null;

// The editor engine: match state lives in a StateField so the decorations re-derive on
// every doc change — typing with the bar open keeps the highlights honest, with no
// listener→dispatch round-trip.
type EditorFind = { query: string; matches: Match[]; active: number };
const setFindEffect = StateEffect.define<{ query: string; active: number } | null>();
const findMark = Decoration.mark({ class: "find-match" });
const findMarkActive = Decoration.mark({ class: "find-match is-active" });
const findField = StateField.define<EditorFind | null>({
  create: () => null,
  update(value, tr) {
    let next = value;
    for (const ef of tr.effects) {
      if (ef.is(setFindEffect))
        next = ef.value && { query: ef.value.query, matches: [], active: ef.value.active };
    }
    if (!next) return null;
    if (next === value && !tr.docChanged) return value;
    const matches = findMatches(tr.newDoc.toString(), next.query);
    const active =
      next !== value || value === null
        ? matches.length === 0
          ? -1
          : Math.max(0, Math.min(next.active, matches.length - 1))
        : // A doc edit: re-anchor on where the old active match ended up.
          activeAfter(
            matches,
            value.active >= 0 && value.matches[value.active]
              ? tr.changes.mapPos(value.matches[value.active].from)
              : 0,
          );
    return { query: next.query, matches, active };
  },
  provide: (f) =>
    EditorView.decorations.from(f, (v): DecorationSet => {
      if (!v) return Decoration.none;
      return Decoration.set(
        v.matches.map((m, i) => (i === v.active ? findMarkActive : findMark).range(m.from, m.to)),
      );
    }),
});

/** What the note pane is showing, as an identity — null means "nothing findable"
 *  (empty pane, or the graph, which has no text to find in). */
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
  // Seed from the live selection — the "search for this" gesture; otherwise the bar
  // keeps its last query, preselected so typing replaces it.
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

/** Recompute matches + Ranges over the rendered note and repaint the highlights.
 *  Runs on open, on each query keystroke, and after any note-pane swap (which
 *  invalidates the previous pass's Ranges). */
function applyReadingFind(): void {
  const anchor = findMatchesList[findActive]?.from ?? 0;
  clearReadingFind();
  // The article is the reading surface (title, meta, tags, body — or the raw source
  // in `</>` mode); the bars above it are chrome and stay out of the match set.
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
  // Leave it alone when it's already in the comfortable band (below the floating bar,
  // above the bottom edge); otherwise bring it to the pane's upper third.
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
    // Anchor on the previous active match, else the caret — typing a longer query
    // stays near where the user was instead of snapping to the top.
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
    // Selecting the match is the editor-find convention (a follow-up ⌘F re-seeds from
    // it); the dispatch doesn't move focus, so Enter in the bar keeps stepping.
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

/** render()'s hook: close when the pane now shows a different doc (or the graph/empty
 *  state); re-derive the highlights when the pane's DOM was rebuilt under an open bar
 *  (the old pass's Ranges point into detached nodes). */
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
  const input = document.getElementById("search-input") as HTMLInputElement | null;
  input?.focus();
  input?.select();
}

// --- external-edit reconciliation (crates/b2-desktop/CLAUDE.md / #14) --------------------
//
// The host watches the vault and emits a debounced `vault-changed` pulse whenever the
// Markdown changes on disk from outside the app (an external editor, a `git pull`). We
// reconcile by re-reading through the façade — never by trusting event paths — so this
// stays honest against `index = projection of (Markdown)` and reuses the exact ops the
// rest of the UI uses. Our *own* writes also pulse, but they're no-ops here: a save keeps
// `state.current.revision` in lockstep with disk, so the revision compare below sees "no
// change" and skips (the guard that stops a self-inflicted reload loop).

let reconcileInFlight = false;
let reconcilePending = false;

// Serialize reconciles: pulses can arrive faster than a reconcile completes (a big `git
// pull`), so coalesce overlaps into one trailing run rather than racing reads against state.
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
  // The tree first — re-derive, then re-list, so an external add / remove / rename shows
  // up immediately: the tree lists are index-first, and a Finder-dropped file has no index
  // row until the (model-free, idempotent) projection runs (#65 item 4; reconcile.ts has
  // the full argument), and the vectors that re-derivation cleared are healed behind it.
  // Safe in every mode: `render()` rebuilds the tree and side panes but skips the note
  // pane while editing (the carve-out), so a live editor is never touched — and
  // projection reads disk, never the live buffer.
  await reconcileIndex({
    reindexing: state.reindexing,
    project: api.project,
    list: loadNotes,
    // …and then the vectors that projection just cleared. Re-chunking a changed note
    // drops its chunk rows (its `embeddings` cascade) and its centroid, and the
    // projection pass is model-free, so an externally edited note comes back with
    // nothing for `similar` to rank from or be ranked against — an empty discovery
    // pane until someone reindexes by hand (the reported bug). The heal is the save
    // path's own trailing embed, which debounces, coalesces with that path's timer,
    // and refreshes discovery when it lands.
    //
    // The gate is the model-free N/M coverage read (#26) — cheap, and it keeps that
    // fraction honest after an external add/remove, which a pulse also left stale. It
    // still errs the safe way (it can over-fire, never miss: any chunk lacking a vector
    // drops its note out of the count), but it no longer over-fires *forever*. The count
    // used to require ≥1 chunk, so a note with an empty body — the tree's freshly-created
    // one, until you type — read as permanently pending and booked a no-op embed on every
    // single pulse. A chunkless note has nothing to embed, and now counts as embedded.
    vectorsPending: async () => {
      await refreshEmbedStatus(state.vaultRoot);
      return state.notesTotal > 0 && state.notesEmbedded < state.notesTotal;
    },
    healVectors: scheduleTrailingEmbed,
  });

  // The open note. Two cases are deliberately left alone:
  //   • editing (the body editor OR the frontmatter mini-editor) — the live buffer is
  //     the user's unsaved work; never clobber it, and never adopt a fresh revision
  //     under it (that would let its save silently overwrite the external edit). The
  //     conflict surfaces through each editor's own save guard instead
  //     (crates/b2-desktop/CLAUDE.md), the one case live reload can't own safely.
  //   • reindexing — our own project/embed run owns the open note's refresh (doReindex);
  //     reconciling here would fight it. Its own writes don't pulse anyway (sqlite under
  //     `.b2/`, filtered host-side) — a projection writes nothing to the vault at all.
  if (state.current && !state.editing && !state.fmEditing && !state.reindexing) {
    const cur = state.current;
    try {
      const fresh = await api.readNote(cur.path);
      // The read is async: apply only if this note still owns the pane and we're still in
      // reading mode (the user may have navigated or started editing meanwhile).
      if (state.current?.path === cur.path && !state.editing) {
        // Unchanged bytes (our own save's echo, or a touch that didn't alter content):
        // skip — no discovery churn, no flicker.
        if (fresh.revision !== cur.revision) {
          state.current = fresh;
          await refreshDiscovery(); // the edit may have changed similar/edges
          flash("Reloaded — this note changed on disk.");
        }
      }
    } catch {
      // The open note was moved or removed on disk. Keep the (now stale) pane rather than
      // blanking it, but say so — the freshly reloaded tree lets the user navigate away.
      if (state.current?.path === cur.path) {
        flash("This note is no longer on disk — it was moved or removed.");
      }
    }
  }

  // The open resource card, same posture: refresh in place (its metadata/backlinks
  // may have changed), and if the file vanished keep the stale card but say so.
  if (state.currentResource && !state.reindexing) {
    const cur = state.currentResource;
    try {
      const fresh = await api.explainResource(cur.path);
      // The bytes are re-read too: an external edit can rewrite the picture in place
      // without the path ever changing, and a stale `data:` URL would show the old one.
      const picture = await loadResourceImage(fresh);
      if (state.currentResource?.path === cur.path) {
        state.currentResource = fresh;
        state.resourceImage = picture;
      }
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
        <button id="nav-back" class="btn ghost icon-btn" title="Back (⌘[)" aria-label="Back" disabled>
          ${icon("chevron-left", { size: 15 })}
        </button>
        <button id="nav-forward" class="btn ghost icon-btn" title="Forward (⌘])" aria-label="Forward" disabled>
          ${icon("chevron-right", { size: 15 })}
        </button>
      </div>
      <form id="search-form" class="search" autocomplete="off">
        <input id="search-input" type="search" placeholder="Search the vault…  ⇧⌘F" aria-label="Search"
               title="Search the vault — ⇧⌘F (⌘F finds inside the open note)" />
      </form>
      <div class="topbar-right">
        <!-- The vault and its indexing state, as one group: a progress meter is *about*
             a vault, so it reads beside the name of the one being indexed rather than
             floating at the far end of the bar. Hidden between runs, so this is just the
             path almost all of the time. The Reindex button that used to stand here has
             moved into Settings → Index — indexing is automatic now, and permanent chrome
             for an exception trains the eye to skip the bar (render.ts, indexPanelHtml).
             What stays is the live meter and the Cancel that belongs with it — visible
             wherever you are in the app, except behind Settings, which covers the bar and
             so paints a second meter of its own. -->
        <div class="vault-status">
          <span id="vault-root" class="vault-root" title="Active vault"></span>
          <!-- Classes, not ids: this is one of those two meters, and paintReindex writes
               the same values into every one on screen. -->
          <div class="reindex-progress" hidden aria-live="polite">
            <div class="reindex-track"><div class="reindex-fill"></div></div>
            <span class="reindex-label"></span>
            <button class="btn ghost small" data-cancel-reindex>Cancel</button>
          </div>
        </div>
        <button id="open-chat" class="btn ghost icon-btn" title="Ask your notes (${escapeHtml(
          displayKeys(["chat.toggle"]),
        )})" aria-label="Ask your notes">
          ${icon("chat-dots", { size: 15 })}
        </button>
        <button id="switch-vault" class="btn ghost icon-btn" title="Switch vault — choose another folder" aria-label="Switch vault">
          ${icon("folder", { size: 15 })}
        </button>
        <button id="open-settings" class="btn ghost icon-btn" title="Settings (⌘,)" aria-label="Settings">
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
        <button id="find-prev" class="btn ghost icon-btn" title="Previous match (⇧Enter)" aria-label="Previous match">
          ${icon("chevron-up", { size: 15 })}
        </button>
        <button id="find-next" class="btn ghost icon-btn" title="Next match (Enter)" aria-label="Next match">
          ${icon("chevron-down", { size: 15 })}
        </button>
        <button id="find-close" class="btn ghost icon-btn" title="Close (${escapeHtml(
          displayKeys(["dismiss"]),
        )})" aria-label="Close find">
          ${icon("x-lg", { size: 13 })}
        </button>
      </div>
    </main>
    <div id="menu-root"></div>
    <div id="modal-root"></div>
    <div id="toast" class="toast" role="status" hidden></div>`;
}

function wireEvents(): void {
  // Remember where the keyboard is, continuously (K1). `syncOverlayFocus` needs the
  // element that *triggered* an overlay, and by the time it runs that element has been
  // swapped out of the DOM — see `lastFocused`. Capture-phase isn't needed (`focusin`
  // bubbles); `<body>` is skipped so a destroyed control doesn't read as a real target.
  document.addEventListener("focusin", (e) => {
    const t = e.target;
    if ((t instanceof HTMLElement || t instanceof SVGElement) && t !== document.body) {
      lastFocused = t;
    }
  });

  // Typing in the frontmatter mini-editor clears its inline error — the message
  // belonged to the save attempt that failed. Delegated (like the clicks below)
  // because the textarea renders dynamically.
  document.addEventListener("input", (e) => {
    if (state.fmEditing && e.target instanceof HTMLTextAreaElement && e.target.id === "fm-editor") {
      hideFmError();
    }
  });

  // Delegated clicks for everything that renders dynamically.
  document.addEventListener("click", (e) => {
    const target = e.target as HTMLElement;

    // An open right-click menu owns the next click: its own items act, any other
    // click merely dismisses it (a menu-dismissing click isn't also a card click).
    if (state.contextMenu) {
      const menu = state.contextMenu;
      if (menu.kind === "tree") {
        if (menu.node && target.closest("[data-ctx-rename]")) {
          startTreeRename(menu.node); // clears the menu itself
          return;
        }
        if (menu.node && target.closest("[data-ctx-move]")) {
          openMoveModal(menu.node);
          return;
        }
        // The two copy actions. Both close the menu first — the copy is instant and
        // its confirmation is the status line, so leaving the menu up over it would
        // be the only thing hiding the answer.
        if (menu.node && target.closest("[data-ctx-copy-vault-path]")) {
          const p = menu.node.path;
          closeContextMenu();
          void copyPath(p);
          return;
        }
        if (menu.node && target.closest("[data-ctx-copy-system-path]")) {
          const p = menu.node.path;
          const root = state.vaultRoot;
          closeContextMenu();
          // Both entry points refuse to open this menu without a vault, so `root` is
          // set here — but the absolute path is the root's to give, and inventing one
          // for a vault-less window is not this handler's call.
          if (root !== null) void copyPath(systemPath(root, p));
          return;
        }
        if (menu.node && target.closest("[data-ctx-delete]")) {
          requestDelete(menu.node); // clears the menu itself
          return;
        }
        if (target.closest("[data-ctx-new-note]")) {
          startTreeCreate("note", menu.dir); // clears the menu itself
          return;
        }
        if (target.closest("[data-ctx-new-folder]")) {
          startTreeCreate("folder", menu.dir);
          return;
        }
        if (target.closest("[data-ctx-import]")) {
          const dir = menu.dir;
          closeContextMenu(); // the picker is modal to the OS — let the menu go first
          void pickAndImport(dir);
          return;
        }
        closeContextMenu();
        return;
      }
      if (target.closest("[data-ctx-open]")) {
        const p = menu.path;
        closeContextMenu();
        void openNote(p);
        return;
      }
      if (target.closest("[data-ctx-insert]")) {
        const p = menu.path;
        closeContextMenu();
        insertCardLink(p); // the drag's keyboard half — aimed at the caret's line
        return;
      }
      if (target.closest("[data-ctx-link]")) {
        const { path, title } = menu;
        closeContextMenu();
        openLinkModal(path, title ?? "");
        return;
      }
      closeContextMenu();
      return;
    }

    // A web link in a note belongs to the **system**, not to this window. The webview
    // *is* the application, so letting a `https://…` navigate replaces B2 with a web page
    // in a window with no address bar and no way back — the app is gone until it's
    // relaunched. So the click is cancelled and the URL handed to the host, which opens
    // it in the user's browser (`open_external`, the sibling of the resource card's
    // *Open in system default*). links.ts owns which hrefs qualify.
    //
    // High in the delegation because an anchor means the same thing wherever it is
    // painted — a note's reading view, live preview's rendered table widget, a backlink
    // snippet — and no branch below handles one. Below the context menu, though: while a
    // menu is up the next click only dismisses it. Wikilinks never reach here (they are
    // `href="#"`, which links.ts declines) and are followed further down.
    //
    // Keyboard-complete for free (K1): ⏎ on a focused anchor dispatches a click, so this
    // is the one activation path for both, and the sheet's "Follow the focused link"
    // row already covers it.
    //
    // The `else` is the other half of the same promise, and it is not "do nothing":
    // `renderMarkdown`'s allow-list is *wider* than `externalUrl`'s, so a note can put an
    // `ftp:`/`tel:`/`xmpp:` href — or an ordinary relative path — into the document, and
    // any of those left alone is a webview navigation, which is the failure this whole
    // branch exists to prevent. So a link B2 won't follow is **cancelled and said so**,
    // not silently ignored. It falls *through* rather than returning, because the one
    // href that must keep its click is B2's own: a wikilink is `href="#"`, an in-page
    // anchor, and the follow handler below is what acts on it. Leaving fragments alone
    // also keeps a note's own `[to the top](#heading)` scrolling, which is the single
    // navigation that doesn't unload the app.
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
    // The chat pane's own chrome. `data-chat-stop` is Esc's equal for the mouse (K1 cuts
    // both ways: no action reachable only by keyboard either).
    if (target.closest("[data-chat-stop]")) {
      stopChatAnswer();
      return;
    }
    if (target.closest("[data-chat-new]")) {
      newChat();
      return;
    }
    // The setup card's retry — the card is looked at *while* the user goes and starts the
    // daemon or pulls a model, so it has to be able to notice that they did.
    if (target.closest("[data-chat-recheck]")) {
      void refreshChatSetup();
      return;
    }
    // The install banner (the "semantic search is off" strip): its primary action opens
    // Settings → Embedding, where the Download button it is pointing at lives; the ✕
    // dismisses for this session. ("Don't remind me again" is a checkbox — handled in the
    // `change` delegation below.)
    if (target.closest("[data-install-open-settings]")) {
      void openSettings("embedding");
      return;
    }
    if (target.closest("[data-install-dismiss]")) {
      dismissEmbedReminder(false);
      return;
    }
    // Settings: a rail tab, the Download button (in-app `b2 init`), else the Done button
    // closes it. Checked before the link-modal backdrop branch so settings wins when it's
    // up. There is no click-outside to close on any more — the surface is the whole window
    // (render.ts) — so the ways out are Done and Escape.
    if (state.settingsOpen) {
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
      // Settings → Chat. The Local/Cloud segments are a *view* of the endpoint (render.ts
      // says why), so pressing one rewrites the URL field to that configuration's starting
      // point and shows or hides the key + its privacy copy — the consent moment is the
      // configuration moment (M5).
      const chatMode = target.closest<HTMLElement>("[data-chat-mode]");
      if (chatMode) {
        setChatMode(chatMode.dataset.chatMode === "cloud");
        return;
      }
      if (target.closest("#settings-chat-save")) {
        void saveChatConfig();
        return;
      }
      // The Model field's two shapes (render.ts's `chatModelFieldHtml`). Neither saves:
      // this only decides whether the field is a list of what the daemon has or a box for
      // a name it doesn't have yet.
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
      // Settings → Index: the manual Reindex, which used to be a top-bar button. Handled
      // in here because this branch returns unconditionally — a click inside the dialog
      // never reaches the shell's handlers below. The dialog deliberately stays open: the
      // run's meter and its Cancel are in the top bar, one Esc away, and closing a dialog
      // out from under the button you just pressed hides the result of pressing it.
      if (target.closest("#reindex")) {
        trackIndexing(doReindex());
        return;
      }
      // …and the Cancel beside it while a run is live. It is the top bar's Cancel in a
      // second place, not a second behaviour — the bar itself is behind this surface now.
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
      // Settings → Keyboard: a chord chip opens the recorder on that command; the strip's
      // own buttons commit, back out, or restore a default. Checked before Done/backdrop
      // so a click inside the strip is never read as "close the dialog".
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
      const node = state.deleteTarget;
      state.deleteTarget = null;
      void executeDelete(node);
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

    // A click on a discovery row moves the keyboard's idea of "where I am" with it, exactly
    // as the tree's rows do below — and for the same reason: WebKit doesn't focus a button
    // on click, so the `focusin` path alone would never see a mouse user's choice. Ahead of
    // the fold/open handlers, which return.
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
    // A ghost is a question — clicking it opens the link palette (the typing moment;
    // committing re-runs discovery, so the ghost solidifies into a typed edge in place).
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

    // A click on a tree row moves the keyboard's idea of "where I am" with it, so
    // switching from mouse to keyboard resumes at the row just clicked rather than
    // teleporting to the roving tabstop's fallback. (WebKit doesn't focus a button on
    // click, so the `focusin` path alone wouldn't see this.)
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
    // (Reindex itself is Settings → Index's button — wired in the `state.settingsOpen`
    // branch above, which is the only place it can be clicked. This is the top bar's
    // Cancel; the one in Settings' own meter is handled in that branch, since a click
    // inside the surface never reaches here.)
    if (target.closest("[data-cancel-reindex]")) {
      void cancelReindex();
      return;
    }
  });

  // Right-click surfaces. The file tree's default menu is taken over wholesale:
  // New note / New folder, contextual on the row under the cursor — a folder row
  // targets itself, a file row its parent folder, the pane's empty space the vault
  // root — and, like a click, the right-click also moves the selection context.
  // Similar cards — and ghost nodes in the graph (same latent candidate) — keep
  // their menu (Open note / Link…). Everywhere else the webview's stays untouched.
  document.addEventListener("contextmenu", (e) => {
    const target = e.target as HTMLElement;
    if (target.closest("#tree-pane") && state.vaultRoot !== null) {
      e.preventDefault();
      const dirRow = target.closest<HTMLElement>("[data-dir]");
      const fileRow = target.closest<HTMLElement>("[data-open], [data-open-resource]");
      const dir = dirRow
        ? (dirRow.dataset.dir ?? "")
        : fileRow
          ? parentDir(fileRow.dataset.open ?? fileRow.dataset.openResource ?? "")
          : "";
      // Over a concrete row, the menu also targets that node (Rename / Move…).
      const node: TreeNodeRef | null = dirRow
        ? { path: dirRow.dataset.dir ?? "", nodeKind: "folder", label: baseName(dirRow.dataset.dir ?? "") }
        : fileRow?.dataset.open
          ? { path: fileRow.dataset.open, nodeKind: "note", label: baseName(fileRow.dataset.open) }
          : fileRow?.dataset.openResource
            ? {
                path: fileRow.dataset.openResource,
                nodeKind: "resource",
                label: baseName(fileRow.dataset.openResource),
              }
            : null;
      state.selectedDir = dir;
      openTreeMenu(e.clientX, e.clientY, dir, node && node.path ? node : null);
      return;
    }
    const card = target.closest<HTMLElement>(".card.candidate, .gnode.is-ghost");
    if (!card) return;
    e.preventDefault();
    openCardMenu(e.clientX, e.clientY, card.dataset.cardPath ?? "", card.dataset.cardTitle ?? "");
  });

  // The file tree's own keyboard, the ARIA `tree` pattern (K1, GH #78). Bound to the
  // pane rather than the document so it answers *before* the global chords below, and
  // only while the keyboard is actually on a row. The moves themselves are pure and
  // tested (treenav.ts `arrowMove`) — this half is just the DOM and the folding.
  //
  // ⏎ and Space are deliberately absent: a row IS a <button>, so the platform already
  // turns both into the click the delegation above answers. Re-binding them here would
  // be a second activation path to keep in sync with the first.
  el("tree-pane").addEventListener("keydown", (e) => {
    // The inline create/rename inputs live in this pane but are text entry — they own
    // their keys (Enter/Escape), handled with the other text surfaces below.
    if (e.target instanceof HTMLInputElement) return;
    const row = (e.target as HTMLElement).closest<HTMLElement>(".tree-row[data-tree-row]");
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
        // Expand/collapse keeps the focus where it is; the fold is the whole gesture.
        if (move.kind === "expand") state.expandedDirs.add(move.path);
        else state.expandedDirs.delete(move.path);
        state.selectedDir = move.path; // folding a folder makes it the create context, as a click does
        state.treeFocus = move.path;
        render();
        treeRowEl(move.path)?.focus();
      }
      return;
    }

    // First-letter typeahead — the reflex every file browser answers to. Guarded to
    // bare printable keys so ⌘N, ⌘F and friends still reach the global handler.
    if (e.key.length === 1 && e.key !== " " && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const hit = typeaheadTarget(rows, rowIndex(rows, path), e.key);
      if (hit !== null) {
        e.preventDefault();
        focusTreeRow(hit);
      }
    }
  });

  // Discovery's own keyboard — the *same* ARIA `tree` pattern as the file tree (K1, GH #78),
  // bound to the pane so it answers before the global chords. The moves are pure and tested
  // (sidenav.ts `sideArrowMove`); this half is the DOM and the folding, plus one wrinkle the
  // tree doesn't have: a card row is a `<div>` because it *contains* the open button (nested
  // buttons are illegal), so ⏎/Space dispatch that button's own click rather than inventing a
  // second activation path — the graph nodes' rule (crates/b2-desktop/CLAUDE.md). Section
  // heads and search results *are* buttons, so there the platform's activation is left alone.
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
      // Folding keeps the focus where it is; the fold is the whole gesture. Note the
      // inverted sets: state tracks what's *collapsed*, so expanding is a delete.
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
      // Only when the *row* holds focus: a row that is itself a button (a section head, a
      // search result) activates through the platform, and an inner control would already
      // be sending its own click — dispatching a second one would open the note twice.
      if (e.target !== row || row instanceof HTMLButtonElement) return;
      // Cancelled before the lookup below, not after: an unresolved row has no open
      // button, and a *focusable div* that lets Space through gets the browser default —
      // scrolling the pane — where the row's contract says nothing happens (PR #90 review).
      e.preventDefault();
      const open = row.querySelector<HTMLElement>(".card-open");
      if (!open) return; // an unresolved link points at nothing — ⏎/Space do nothing
      open.click();
    }
  });

  // The floating menu is positioned at fixed viewport coords, so any scroll or resize
  // strands it — dismiss rather than let it hover over the wrong card. Capture-phase so
  // a scroll inside the side pane (which doesn't bubble) is still caught.
  document.addEventListener("scroll", closeContextMenu, true);
  window.addEventListener("resize", closeContextMenu);

  // The find bar is static shell chrome — direct listeners, not delegation. mousedown
  // preventDefault keeps focus in the find input across button clicks, so Enter keeps
  // stepping without a re-click.
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

  // Search on submit (Enter).
  document.addEventListener("submit", (e) => {
    if ((e.target as HTMLElement).id === "search-form") {
      e.preventDefault();
      const input = document.getElementById("search-input") as HTMLInputElement | null;
      void doSearch(input?.value ?? "");
    }
    // The chat composer is a form so its Ask button is a submit button — the platform's
    // own "this field's default action", which is what makes ⏎ work in it without B2
    // claiming the key twice (the registry marks `chat.send` fixed for that reason).
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
    // The install banner's "Don't remind me again" — checking it persists the opt-out and
    // dismisses the banner (the strip is gone the moment it's checked, which is the intent).
    if (t instanceof HTMLInputElement && t.matches("[data-install-remind-off]")) {
      dismissEmbedReminder(true);
    }
  });

  // The inline create input commits on blur (a non-empty name — clicking away is a
  // "yes, make it", VS Code-style; empty backs out). `isConnected` distinguishes a
  // real blur from the input being torn down by a tree repaint or its own commit —
  // a removed node must never re-commit.
  document.addEventListener("focusout", (e) => {
    const t = e.target as HTMLElement;
    if (t.id === "tree-create-input" && t.isConnected && state.treeCreate) {
      void commitTreeCreate((t as HTMLInputElement).value, false);
    }
    // The rename input commits on blur too, same VS Code posture (a changed name is
    // a "yes, rename it"; an unchanged or empty one backs out via renameDestination).
    if (t.id === "tree-rename-input" && t.isConnected && state.treeRename) {
      void commitTreeRename((t as HTMLInputElement).value);
    }
  });

  // The app's chords. Every `isBound(e, …)` below asks the keyboard registry
  // (bindings.ts) whether this keystroke is that command — the registry owns *what* the
  // chord is, and the sheet in Settings → Keyboard is projected from the same table, so
  // the two can't drift. What stays here is everything the table deliberately doesn't
  // model: the order the surfaces get their turn (innermost first), and the guard beside
  // each branch saying when its command applies at all — an overlay owns the keyboard,
  // the tree has no focused row, ⌘⌫ must not hijack delete-to-line-start while editing.
  document.addEventListener("keydown", (e) => {
    // The chord recorder is above everything, including the registry itself: while it is
    // open the user is *pressing chords at it*, not issuing them, so a keystroke that
    // reached a command would be a keystroke the recorder failed to record. It is the
    // only branch here that consumes an event without asking bindings.ts anything.
    if (recorderKeydown(e)) return;
    // The tree's inline create input owns its keys first: Enter commits, Escape
    // cancels, and nothing else typed there leaks into the global chords below.
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
    // The chat composer's ⏎. Deliberately **not** a `return` like the two branches above:
    // a name field is a modal little world, but a composer is somewhere you sit and think,
    // and ⌘J, ⌘, and Esc have to keep working while the caret is in it. ⇧⏎ is a newline —
    // the textarea's own behavior, which B2 keeps by not claiming it (bindings.ts).
    if ((e.target as HTMLElement).id === "chat-input" && isBound(e, "chat.send")) {
      e.preventDefault();
      void sendChat((e.target as HTMLTextAreaElement).value);
      return;
    }
    // Settings' rail (K1, the ARIA tabs pattern — settingstabs.ts owns the moves). Above
    // the Tab trap because ⌃Tab is a Tab: the trap swallows the key unconditionally, so a
    // section chord placed after it would never run.
    //
    // The two moves differ in reach on purpose. ⌃Tab / ⇧⌃Tab cycle sections from
    // *anywhere* in the dialog — you should never have to walk back to the rail to switch
    // — while ↑↓ and Home/End apply only with the keyboard actually **on** a tab, since
    // those keys belong to the panel's own controls (and to the panel itself, which
    // scrolls) the moment focus leaves the rail.
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
    // An open overlay owns Tab (K1): without a trap, Tab walks the page *behind* the
    // modal — focus vanishes under the backdrop and the overlay becomes dismissible
    // only with the mouse. Wrapping keeps every control in it reachable, forever.
    if (isBound(e, "overlay.focus.step") && currentOverlay() !== null) {
      // The binding is `Any-Tab`: this branch's contract is that *no* Tab reaches the
      // page, so a modifier the user is still holding must not defeat it. Swallowed
      // unconditionally for the same reason — an overlay that somehow renders with no
      // focusable control must still not let Tab walk the page behind it, which is the
      // exact failure this block exists to prevent.
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
    // ↑/↓ walk an open context menu — the menu-pattern sibling of the tree's arrows.
    // (⏎ needs no binding: the items are buttons.)
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
    // The find bar's input: Enter steps (⇧Enter back), Escape closes. Everything else
    // falls through so the global chords (⌘F itself, ⇧⌘F) still work from the bar.
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
    // ⌘G — flip the pane between reading and the connection graph, the keyboard sibling
    // of the graph chip in the note bar. Below the find branch on purpose: while the bar
    // is open ⌘G is Find Next, the macOS reflex, and the chord only becomes the graph's
    // once there are no matches to step (bindings.test.ts pins that shadow). Editing is
    // out for the same reason the chip isn't drawn there — the pane belongs to the live
    // editor, so `render` won't paint the graph over it and the flip would be invisible.
    // Chat takes the right column (⌘J). Unlike the graph it is *not* refused while
    // editing: asking your notes a question is a thing you do mid-sentence, and the pane
    // it opens is not the one the editor owns.
    if (isBound(e, "chat.toggle")) {
      if (currentOverlay() !== null) return;
      e.preventDefault();
      toggleChat();
      return;
    }
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
    // ⇧F10 / the Menu key — the keyboard's right-click, and the entry point that makes
    // Rename / Move… / Link… reachable without a mouse at all (they live only in the
    // context menu). Opens the *same* menu the mouse does, anchored under whatever the
    // keyboard is on: a tree row, or a discovery card / graph ghost.
    if (isBound(e, "menu.open")) {
      if (currentOverlay() !== null || state.vaultRoot === null) return;
      const row = focusedTreeRow();
      if (row) {
        e.preventDefault();
        const node = treeRowRef(row);
        const dir = node.nodeKind === "folder" ? node.path : parentDir(node.path);
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
    // F2 — rename the focused tree row. The platform rename chord, and the direct path
    // the context menu advertises next to the item.
    if (isBound(e, "tree.rename")) {
      const row = focusedTreeRow();
      if (!row) return;
      e.preventDefault();
      startTreeRename(treeRowRef(row));
      return;
    }
    // ? — the keyboard reference, which is Settings' Keyboard section (settingstabs.ts).
    // Bare `?` (⇧/ on a US layout), so it can't conflict with typing: any text surface is
    // excluded, editing included. Any *other* overlay owns the keyboard first (Escape,
    // then ask again) — the sheet used to render over them, and folding it into Settings
    // is what makes this chord an ordinary one. A toggle, like ⌘,: pressing it while
    // already reading the section closes the dialog.
    if (isBound(e, "help.keyboard")) {
      if (state.editing || inTextEntry()) return;
      const overlay = currentOverlay();
      if (overlay !== null && overlay !== "settings") return;
      e.preventDefault();
      if (state.settingsOpen && state.settingsTab === "keyboard") closeSettings();
      else void openSettings("keyboard");
      return;
    }
    // ⌘1 / ⌘2 / ⌘3 — put the keyboard in the files, the note, or discovery. Without
    // them, reaching the tree means Tab-ing through the whole top bar first, which is
    // "operable" only in the letter of K1, not its spirit.
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
    // Enter commits the link modal from anywhere inside it — the keyboard sibling of
    // "Commit link" (the explanation field is a plain input, so ⏎ would do nothing).
    if (state.linkTarget && isBound(e, "link.commit")) {
      e.preventDefault();
      void commitLink();
      return;
    }
    // Enter commits the folder-delete confirm (its keyboard sibling of the button).
    if (state.deleteTarget && isBound(e, "delete.confirm")) {
      e.preventDefault();
      const node = state.deleteTarget;
      state.deleteTarget = null;
      void executeDelete(node);
      return;
    }
    // ⏎ / Space on a focused graph node. SVG has no native button activation, so the
    // key becomes the click the delegation above already answers — one activation path
    // for both hands, not two implementations to keep in step.
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
    // ⌘⌫ — delete. The focused tree row wins (a folder among them: it opens the
    // confirm, so the one gesture now covers folders too); with the keyboard elsewhere
    // it falls back to the open document, the reader's expectation. Reading view only:
    // while editing — or in any text field — ⌘⌫ is the platform delete-to-line-start
    // and must not be hijacked.
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
    // ⌘E toggles edit mode — the keyboard sibling of the Edit / Done buttons. A modal
    // owns the keyboard first; a resource or empty pane has nothing to edit. Works while
    // editing (CodeMirror leaves Mod-e unbound, so the event bubbles here) to flip back.
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
    // ⇧⌘E flips the note between rendered and raw Markdown — the `</>` chip's chord, and
    // the keyboard's route to the escape hatch. Live while editing for the same reason ⌘E
    // is: CodeMirror leaves the chord unbound (editorkeys.test.ts is what keeps that
    // true), so the event reaches this handler, and `toggleSource` reconfigures the live
    // preview in place rather than rebuilding the pane. Refused with the graph up, the
    // mirror of ⌘G being refused while editing — the pane belongs to the scene, so the
    // flip would land somewhere nobody can see it. A resource card has no source to show.
    if (isBound(e, "source.toggle")) {
      if (currentOverlay() !== null || state.graphOpen) return;
      if (!state.current || state.currentResource) return;
      e.preventDefault();
      toggleSource();
      return;
    }
    if (isBound(e, "dismiss")) {
      // Innermost first, always — and with the `?` sheet folded into Settings, the
      // overlay layer is flat, so this is simply the one overlay that is up.
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
      // The frontmatter mini-editor: Esc is its documented discard (the hint says so).
      if (state.fmEditing) {
        void cancelFmEdit();
        return;
      }
      // An open find bar dismisses next (Escape from anywhere, not just its input).
      if (findOpen) {
        closeFind();
        return;
      }
      // Chat, innermost part first: Esc **stops a streaming answer** (the spec's own
      // cancellation gesture — the partial text stands and is marked stopped), and only a
      // second Esc closes the pane. Stopping and closing on one keystroke would make the
      // stop invisible, which is the opposite of rendering a cancelled turn honestly.
      //
      // Both halves are gated on the pane being *open*, because `closeChat` cancels but
      // `chatStreaming` stays set until the turn resolves a tick later: in that window an
      // Esc aimed at the graph would otherwise be swallowed by a surface that is no longer
      // on screen, with nothing visible to show for it.
      if (state.chatOpen) {
        if (stopChatAnswer()) return;
        closeChat();
        return;
      }
      // With nothing else to dismiss, Escape backs out of the graph into reading.
      if (state.graphOpen && state.current && !state.editing) toggleGraph();
      return;
    }
    if (state.editing && isBound(e, "editor.save")) {
      e.preventDefault();
      void saveNow();
      return;
    }
    // ⌘⏎ / ⌘S save the frontmatter mini-editor — its explicit-save chords (the
    // buttons' keyboard siblings, K1). Plain Enter stays a newline in the textarea.
    if (state.fmEditing && isBound(e, "fm.save")) {
      e.preventDefault();
      void saveFmEdit();
      return;
    }
    // ⌘[ / ⌘] (and the ⌘←/⌘→ aliases) walk the pane's history (#52) — but never over
    // text entry or a modal. While editing, both chords belong to CodeMirror (Mod-[/]
    // are indent bindings, Mod-arrows caret movement); in an input, only the arrows
    // mean caret-to-edge, so the brackets still navigate (e.g. straight from the
    // search field). The buttons and mouse back/forward stay live everywhere — they
    // flush through navGo's edit-mode guard.
    const back = isBound(e, "nav.back");
    if ((back || isBound(e, "nav.forward")) && !state.editing) {
      if (currentOverlay() !== null) return;
      if (canonicalKey(e.key).startsWith("Arrow") && inTextEntry()) return;
      e.preventDefault();
      void navGo(back ? -1 : 1);
    }
  });

  // Mouse back/forward buttons (W3C numbering: 3 back, 4 forward) walk the history
  // too. `auxclick` fires only for non-primary buttons, so this never doubles the
  // click delegation above.
  document.addEventListener("auxclick", (e) => {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    void navGo(e.button === 3 ? -1 : 1);
  });

  // Losing window focus is a flush point: the buffer lands on disk before the user
  // looks at (or edits in) anything else.
  //
  // It is also the recorder's one *positive* signal (recorder.ts): Spotlight, the app
  // switcher and Hide all take the key window, so a window that lost focus while a chord
  // was being pressed at us is evidence that something outside B2 answered — not an
  // inference drawn from nothing happening.
  window.addEventListener("blur", () => {
    if (state.editing) void saveNow();
    if (state.recorder && state.recorder.candidate === null) {
      cancelProbe(); // the blur has answered; there is no silence left to read
      state.recorder.blurred = true;
      state.recorder.hint = silenceHint({ elapsedMs: Date.now() - recorderOpenedAt, blurred: true });
      render();
    }
  });

  // --- tree drag-and-drop ---------------------------------------------------------
  //
  // Two drags land here, and they are told apart by `treeDrag` being set: a **tree
  // row** being moved within the vault, and a **file from outside** (Finder) being
  // imported. One set of listeners serves both because both aim at the same targets.
  //
  // Both depend on `dragDropEnabled: false` in tauri.conf.json's window config: with
  // Tauri's native drag-drop interception on (the default), wry consumes drag
  // events for its own file-drop channel and the DOM never sees dragover/drop on
  // macOS — dragstart fires, but no drop zone ever activates. Turning it on to get
  // the OS drop's *paths* would therefore cost the in-app move, so the import takes
  // the bytes route instead (importDroppedFiles).
  //
  // The flip side of that setting is why the external drag must be handled at all:
  // with wry not intercepting, an unhandled file drop is WebKit's to act on, and
  // WebKit's default is to **navigate to the dropped file** — which replaces the whole
  // app with a rendering of that file, with no address bar and no way back (the same
  // reason a note's web links are handed to the OS, links.ts). So every file drag is
  // preventDefaulted, whether or not it lands somewhere B2 can use, and the OS cursor
  // carries the difference: copy over a tree target, no-drop everywhere else.
  //
  // Any tree row drags; folder rows and the pane background (= vault root) accept
  // drops — a drop on a *file* row lands in that file's folder, mirroring the
  // right-click context rule. The payload is a module-local (same-window DnD needs
  // no dataTransfer round-trip); validity is `canMoveInto` (pure, move.ts), and only
  // a valid target preventDefaults dragover, so the OS cursor says no everywhere
  // else. The highlight is applied imperatively — dragover fires continuously, and
  // a render() per event would fight the drag.
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
    const dirRow = target.closest<HTMLElement>("[data-dir]");
    if (dirRow) return { el: dirRow, dir: dirRow.dataset.dir ?? "" };
    const fileRow = target.closest<HTMLElement>("[data-open], [data-open-resource]");
    if (fileRow)
      return { el: fileRow, dir: parentDir(fileRow.dataset.open ?? fileRow.dataset.openResource ?? "") };
    return { el: pane, dir: "" };
  };

  document.addEventListener("dragstart", (e) => {
    const target = e.target as HTMLElement;
    // A discovery candidate on its way into the note (droplink.ts). It carries a private
    // MIME rather than `text/plain`: CodeMirror drops plain text where it lands, so a
    // plain flavor would give a missed interception a second, silent behaviour.
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
    const dirRow = target.closest<HTMLElement>("[data-dir]");
    const noteRow = target.closest<HTMLElement>("[data-open]");
    const resRow = target.closest<HTMLElement>("[data-open-resource]");
    treeDrag = dirRow?.dataset.dir
      ? { path: dirRow.dataset.dir, nodeKind: "folder", label: baseName(dirRow.dataset.dir) }
      : noteRow?.dataset.open
        ? { path: noteRow.dataset.open, nodeKind: "note", label: baseName(noteRow.dataset.open) }
        : resRow?.dataset.openResource
          ? {
              path: resRow.dataset.openResource,
              nodeKind: "resource",
              label: baseName(resRow.dataset.openResource),
            }
          : null;
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

  /**
   * Would WebKit *navigate* if this drag were dropped unhandled? Files, and also a
   * dragged **link** — from a browser, a mail client, or a note's own anchor. Both
   * end the same way (the window holds the whole app, so a navigation is the app
   * gone), so both are cancelled; only files go anywhere afterwards.
   */
  const navigatesWebview = (e: DragEvent) =>
    carriesFiles(e) || (e.dataTransfer?.types.includes("text/uri-list") ?? false);

  /** Where an external file drag may land: a tree target, or nowhere. */
  const importTargetOf = (e: DragEvent) =>
    !carriesFiles(e) || state.vaultRoot === null
      ? null
      : dropTargetOf(e.target as HTMLElement);

  // dragenter as well as dragover: some engines want the *first* event over an
  // element cancelled before they will treat it as a drop zone at all.
  document.addEventListener("dragenter", (e) => {
    if (!treeDrag && navigatesWebview(e)) e.preventDefault();
  });

  document.addEventListener("dragover", (e) => {
    // The card drag (droplink.ts). Over the buffer, the editor's own handler has already
    // painted the preview and claimed the event — this is only about everywhere *else*:
    // clear the ghost, and mark the discovery column as the cancel target it is. Nothing
    // outside the buffer is preventDefaulted, so the OS refuses those drops for us and a
    // release there is the "put it back" the gesture promises.
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
      // Unconditional: cancelling dragover is what stops WebKit navigating the whole
      // app to the file, and that has to hold over the editor and every other pane —
      // not just over the tree.
      e.preventDefault();
      const drop = importTargetOf(e);
      if (drop?.el !== dropHighlight) clearDropHighlight();
      // "none" everywhere but the tree, so the OS cursor says where this can land —
      // and so a drop outside it is refused by AppKit before anything sees it.
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
    // A card that landed in the buffer was handled by the editor before this bubbled here
    // (the link is already in, and its save is running); a card released anywhere else was
    // refused by the OS and is a cancel. Both end the same way — put the state back.
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

  // An external drag that leaves the window fires no dragend (the drag isn't ours),
  // so the highlight would stick until the next drag. dragleave for the document's
  // edge — `relatedTarget` is null exactly when the pointer left the window.
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
 * The app menu bar's chords, from the host that declares them (#119) — the last group of
 * the keyboard reference, and the one set of chords the webview never sees a keydown for.
 *
 * It doubles as the mirror's only check. `menukeys.ts` carries an offline copy — the
 * suite's conflict gate reads it, and the sheet paints from it until this resolves — and
 * a copy free to fall behind the menu is precisely what #119 set out to end. A difference
 * goes to the console, not to the user: it means someone edited `menu.rs` without editing
 * the mirror, which is a developer's bug, and the sheet has already switched to the host's
 * own list by the time anyone can open Settings to read it. Nothing here blocks the paint,
 * and a failure costs only the switch from mirror to host.
 */
async function loadMenuChords(): Promise<void> {
  try {
    const chords = await api.menuChords();
    state.menuChords = chords;
    // The sheet reads this, so a panel that is already up has to be told. `boot` fires
    // this fetch without awaiting it and `wireEvents` has already bound ⌘, by then, so
    // "Settings is open before the host answers" is reachable, and without a repaint the
    // reader would sit looking at the mirror — the one thing the host list exists to
    // replace. Guarded rather than unconditional because during boot the answer normally
    // lands *before* the first `render()`, and painting there would flash the empty shell
    // ahead of the vault read. In the healthy case the two lists agree, so the HTML is
    // identical and `paintModal`'s memo skips the swap; the case where the DOM really
    // changes is drift, which is the case worth showing.
    if (state.settingsOpen) render();
    const drift = menuDrift(chords);
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
  await loadZoomPref(); // and the saved size — awaited, because it changes what "the viewport" means below
  initMenuCommands(); // View ▸ Zoom In / Zoom Out / Actual Size arrive from the host
  const lostChords = loadKeymap(); // the user's chords, before anything paints or dispatches one
  loadEmbedReminderPref(); // honor a persisted "don't remind me" before the banner can paint
  buildShell();
  initPanes(el("layout")); // restore the saved column widths, likewise before the paint
  wireEvents();
  // Auto-reload on external edits (#14): subscribe once for the window's lifetime. The
  // host only pulses when the *watched* vault's Markdown changes, and re-points the watch
  // on a vault switch, so this single subscription always tracks the active vault.
  void api.onVaultChanged(() => void onVaultChanged());
  void loadMenuChords(); // the keyboard reference's last group — never blocks the paint
  try {
    const info = await api.vaultInfo();
    state.vaultRoot = info.root;
    state.semantic = info.semantic;
    state.notesEmbedded = info.notes_embedded;
    state.notesTotal = info.notes_total;
    // Populate the file tree so the vault is navigable before anything is opened.
    await loadNotes();
  } catch (e) {
    // No vault (or another startup failure): the note pane shows the actionable state.
    state.vaultRoot = null;
    flash(errText(e));
  }
  if (lostChords.length > 0) {
    // Said here rather than at the read, which happens before there is a shell to paint
    // into — and after the vault load, so a startup failure's own notice isn't clobbered.
    flash(`${lostChords.length} saved shortcut(s) couldn't be applied — those are back to their defaults.`);
  }
  render();
  // Auto-index on launch (#25): if the startup vault is unindexed or only partly embedded,
  // bring it up to date now instead of waiting behind a manual Reindex click. No-ops when
  // no vault resolved (vaultRoot === null) or the index is already complete.
  trackIndexing(autoIndexOnOpen(state.vaultRoot));
}

void boot();
