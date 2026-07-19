// The controller: build the shell once, wire events (delegated), run the actions
// that mutate `state` and re-render. No framework — the app is small enough that a
// full-pane innerHTML swap on each change is instant and keeps the model honest.
// All backend access goes through `api` (the one IPC seam); this file holds the UI
// flow, never engine logic.

import "../style.css";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Compartment, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { api, errText, isWriteConflict } from "./api";
import { state, type SideSection, type ThemePref } from "./state";
import { dirChain, joinPath, normalizeName, parentDir } from "./newentry";
import { livePreview, wikilink } from "./livepreview";
import { BOUNDS, initPanes } from "./panes";
import {
  contextMenuHtml,
  escapeHtml,
  modalHtml,
  notePaneHtml,
  sidePaneHtml,
  treePaneHtml,
} from "./render";

// --- render ---------------------------------------------------------------------

function el(id: string): HTMLElement {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node;
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

/** Repaint the tree pane (memoized), carrying an open create input across the swap. */
function paintTree(): void {
  const html = treePaneHtml(state);
  if (html === lastTreePaneHtml) return;
  const prev = document.getElementById("tree-create-input") as HTMLInputElement | null;
  const saved =
    prev && state.treeCreate
      ? { value: prev.value, start: prev.selectionStart, end: prev.selectionEnd }
      : null;
  el("tree-pane").innerHTML = html;
  lastTreePaneHtml = html;
  const input = document.getElementById("tree-create-input") as HTMLInputElement | null;
  if (input) {
    if (saved) {
      input.value = saved.value;
      input.setSelectionRange(saved.start ?? saved.value.length, saved.end ?? saved.value.length);
    }
    input.focus();
  }
}

function render(): void {
  paintTree();
  // The carve-out (desktop-editing.md §6): while editing, the note pane belongs to
  // the live EditorView — rebuilding it here (e.g. from a toast timer) would destroy
  // the editor mid-keystroke. Everything else keeps rendering.
  if (!state.editing) {
    // Memoized: an unrelated render (a toast timer, streamed progress) with identical
    // pane HTML skips the innerHTML swap, so reading scroll position survives and the
    // graph view's entrance animation plays on real changes only — never on a toast.
    const noteHtml = notePaneHtml(state);
    if (noteHtml !== lastNotePaneHtml) {
      el("note-pane").innerHTML = noteHtml;
      lastNotePaneHtml = noteHtml;
    }
  } else {
    lastNotePaneHtml = null;
  }
  // Graph mode owns the pane's box: padding off, scrolling off, column flex on
  // (the stage flexes to fill; the SVG viewBox scales the scene into it).
  el("note-pane").classList.toggle(
    "is-graph",
    state.graphOpen && !state.editing && state.current !== null && state.currentResource === null,
  );
  el("side-pane").innerHTML = sidePaneHtml(state);
  el("menu-root").innerHTML = contextMenuHtml(state);
  el("modal-root").innerHTML = modalHtml(state);
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
  // Focus the explanation field the moment the modal appears.
  document.getElementById("link-explanation")?.focus();
}

// Paint just the reindex affordance — the Reindex button's disabled state and the
// progress bar/label/Cancel. Called on every full render AND on each streamed progress
// batch, so progress updates never rebuild the panes (which would fight scrolling and
// churn on a large vault). The progress element lives in the persistent shell.
function paintReindex(): void {
  (el("reindex") as HTMLButtonElement).disabled =
    state.loading || state.reindexing || state.vaultRoot === null;

  const wrap = document.getElementById("reindex-progress");
  if (!wrap) return;
  wrap.hidden = !state.reindexing;
  if (!state.reindexing) return;

  const fill = document.getElementById("reindex-fill");
  const label = document.getElementById("reindex-label");
  const cancelBtn = document.getElementById("cancel-reindex") as HTMLButtonElement | null;
  if (cancelBtn) {
    cancelBtn.disabled = state.reindexCancelling;
    cancelBtn.textContent = state.reindexCancelling ? "Cancelling…" : "Cancel";
  }

  const p = state.reindexProgress;
  if (p && p.notes_to_embed > 0) {
    // Determinate once embedding starts: fraction of the notes that (re)embed this run.
    const pct = Math.min(100, Math.round((p.notes_embedded / p.notes_to_embed) * 100));
    if (fill) {
      fill.classList.remove("is-indeterminate");
      (fill as HTMLElement).style.width = `${pct}%`;
    }
    if (label) {
      const name = p.note_path.replace(/\.md$/, "");
      label.textContent = state.reindexCancelling
        ? "Cancelling…"
        : `Embedding ${p.notes_embedded}/${p.notes_to_embed} · ${name}`;
    }
  } else {
    // Before the first batch (fast projection phase): indeterminate.
    if (fill) {
      fill.classList.add("is-indeterminate");
      (fill as HTMLElement).style.width = "";
    }
    if (label) label.textContent = state.reindexCancelling ? "Cancelling…" : "Indexing…";
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

// Load the vault listing for the file tree. Non-fatal on failure (e.g. an unindexed
// vault): the tree shows its empty state and the reason surfaces as a toast.
async function loadNotes(): Promise<void> {
  try {
    state.notes = await api.listNotes();
    state.resources = await api.listResources();
  } catch (e) {
    state.notes = [];
    state.resources = [];
    flash(errText(e));
  }
}

/**
 * Load a note into the center pane — the shared core of `openNote` and back/forward
 * (#52): everything after the edit-mode guard. `commit` runs the history-stack
 * mutation the moment the read succeeds — before the slower discovery tail, so rapid
 * navigations can't interleave stack updates out of order — and receives the
 * canonical vault-relative path (the ref may be a b2id or a wikilink target).
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
    commit(note.path);
    expandAncestors(note.path);
    state.selectedDir = parentDir(note.path); // the create context follows the selection
    state.searchQuery = "";
    state.searchResults = [];
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
  if (!(await closeEditor())) return;
  await loadNote(ref, (path) => navPush({ kind: "note", path }));
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
    state.current = null;
    commit(resource.path);
    expandAncestors(resource.path);
    state.selectedDir = parentDir(resource.path); // the create context follows the selection
    state.searchQuery = "";
    state.searchResults = [];
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
  // The guard first (it can await a save flush); the cursor math after, against
  // whatever the stack is once navigation is actually allowed to proceed.
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
  state.frontmatterOpen = !state.frontmatterOpen;
  render();
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
const TREE_MENU_H = 100; // the context line + two items

function clampMenu(clientX: number, clientY: number, height: number): { x: number; y: number } {
  const x = Math.min(clientX, window.innerWidth - CTX_MENU_W - 8);
  const y = Math.min(clientY, window.innerHeight - height - 8);
  return { x: Math.max(8, x), y: Math.max(8, y) };
}

function openCardMenu(clientX: number, clientY: number, path: string, title: string): void {
  const { x, y } = clampMenu(clientX, clientY, CARD_MENU_H);
  state.contextMenu = { kind: "card", x, y, path, title: title || null };
  render();
}

function openTreeMenu(clientX: number, clientY: number, dir: string): void {
  const { x, y } = clampMenu(clientX, clientY, TREE_MENU_H);
  state.contextMenu = { kind: "tree", x, y, dir };
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
// nothing to embed). A new *folder* is staged UI state (`pendingDirs`): the
// index-derived tree can't list an empty dir and nothing durable lives outside the
// Markdown, so B2 writes no empty folder — it materializes on disk when its first
// note is created inside it (`create_note` creates missing parent dirs).

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
    for (const d of dirChain(path)) {
      state.pendingDirs.add(d);
      state.expandedDirs.add(d);
    }
    state.selectedDir = path; // the natural next step is a note inside it
    render();
    return;
  }
  try {
    const report = await api.createNote(path);
    await loadNotes(); // the tree lists it now — create_note already projected it
    void refreshEmbedStatus(state.vaultRoot); // the N/M denominator grew (#26)
    if (open) {
      await openNote(report.path); // sets selectedDir to the new note's folder
      enterEdit(); // a fresh, empty note wants a cursor, not a reading view
    } else {
      flash(`Created ${report.path}.`);
    }
  } catch (e) {
    // Refused (e.g. the name already exists): keep the input open — with the typed
    // name intact, since the unchanged tree HTML skips the repaint — so the user
    // adjusts rather than retypes; the toast explains.
    state.treeCreate = create;
    flash(errText(e));
  }
}

// --- the anchored ghost graph (GH #22) --------------------------------------------
//
// The center pane's third mode. Both toggles below are pure state flips — the scene
// renders from discovery state the note-open already fetched, so no IPC happens here.

/** Flip the pane between reading and the graph. Sticky across notes, like sourceOpen. */
function toggleGraph(): void {
  if (!state.current) return; // the graph anchors on an open note
  state.graphOpen = !state.graphOpen;
  render();
}

/** Switch the graph's typed lens (All / Lineage / Argument). */
function setGraphLens(lens: string): void {
  if (lens !== "all" && lens !== "lineage" && lens !== "argument") return;
  if (state.graphLens === lens) return;
  state.graphLens = lens;
  render();
}

// The `</>` toggle serves two surfaces off the one sticky `sourceOpen` (spec §3
// "Escape hatch"). In the reading view it flips rendered ↔ raw via a full re-render.
// While editing, the carve-out forbids rebuilding the pane, so it reconfigures the
// live-preview compartment in place — decorations off = raw + syntax colors, monospace
// (today's editor) — with cursor and undo intact, then repaints just the bar button.
function toggleSource(): void {
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
    .similar(n.path)
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

async function doSearch(raw: string): Promise<void> {
  const query = raw.trim();
  if (!query) {
    state.searchQuery = "";
    state.searchResults = [];
    render();
    return;
  }
  state.loading = true;
  state.searchQuery = query;
  render();
  try {
    state.searchResults = await api.search(query);
  } catch (e) {
    state.searchResults = [];
    flash(errText(e));
  } finally {
    state.loading = false;
    render();
  }
}

function clearSearch(): void {
  state.searchQuery = "";
  state.searchResults = [];
  const input = document.getElementById("search-input") as HTMLInputElement | null;
  if (input) input.value = "";
  render();
}

function openLinkModal(path: string, title: string): void {
  state.linkTarget = { path, title: title || null };
  state.linkRelation = "references";
  render();
}

function closeModal(): void {
  state.linkTarget = null;
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
// A small modal over the global embedder config. Its one setting today is the model
// picker; selecting a model persists to the shared config the CLI also reads, and a real
// switch is completed by the user (b2 init + Reindex), which the flashed guidance names.

async function openSettings(): Promise<void> {
  state.contextMenu = null; // a card menu could be up via the ⌘, shortcut path
  state.settingsOpen = true;
  render(); // show the modal shell immediately; the list fills when it resolves
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
  render();
  document.getElementById("settings-model")?.focus();
}

function closeSettings(): void {
  state.settingsOpen = false;
  render();
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
    flash(`Downloaded ${now?.label ?? "model"}. Reindex to embed your vault with it.`);
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
    state.similar = [];
    state.connections = [];
    state.resourceLinks = [];
    state.unresolved = [];
    state.searchQuery = "";
    state.searchResults = [];
    state.expandedDirs = new Set<string>();
    state.selectedDir = ""; // the create context belongs to the vault we left…
    state.pendingDirs = new Set<string>(); // …as do any staged, still-empty folders
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

// Reindex as project → embed, sequenced here (Shape A, projection-embedding-split.md
// §6): the fast, model-free `project` completes the keyword + graph index, the tree
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
        : `Indexed ${p.indexed} note(s) — ${r.embedded} embedded, ${p.stamped} stamped.${skipped}`,
    );
    if (state.current) {
      // Projection may have stamped the open note on disk; re-read it, and refresh
      // discovery now that vectors exist for `similar` to rank with. Not mid-edit:
      // the editor's revision chain owns the note then (an indexed note is already
      // stamped), and adopting a re-read racing an in-flight save could regress the
      // chain into a false conflict.
      if (!state.editing) state.current = await api.readNote(state.current.path);
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
    // (projection may have stamped it) and refresh discovery so `similar` can rank.
    if (state.current && !state.editing) {
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

// --- editing (desktop-editing.md §6/§8) -------------------------------------------
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
const lpCompartment = new Compartment();
function livePreviewConf(): Extension {
  return state.sourceOpen
    ? syntaxHighlighting(defaultHighlightStyle, { fallback: true })
    : livePreview((target) => void openNote(target));
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
  state.editing = true;
  state.editConflict = false;
  render();
  mountEditor(n.body);
}

function mountEditor(body: string): void {
  const n = state.current;
  if (!n) return;
  el("note-pane").innerHTML = `
    <div class="editor-bar">
      <span class="editor-title">Editing · ${escapeHtml(n.path)}</span>
      <div class="note-bar-actions">
        <button id="edit-source" class="source-toggle${
          state.sourceOpen ? " is-active" : ""
        }" data-toggle-source aria-pressed="${state.sourceOpen}" title="${
          state.sourceOpen ? "Show live preview" : "Show Markdown source"
        }">&lt;/&gt;</button>
        <button id="edit-done" class="btn small primary" title="Save and return to reading (⌘S flushes anytime)">Done</button>
      </div>
    </div>
    <div id="edit-conflict" class="conflict-bar" hidden>
      <span>This note changed on disk.</span>
      <span class="conflict-actions">
        <button id="conflict-reload" class="btn small" title="Discard my edits and load the note from disk">Reload</button>
        <button id="conflict-keep" class="btn small" title="Overwrite the note on disk with my edits">Keep mine</button>
      </span>
    </div>
    <div id="editor-host" class="editor-host"></div>`;
  editorView = new EditorView({
    doc: body,
    extensions: [
      // GFM base + the wikilink node: the reading view's `gfm: true` twin, and without
      // `markdownLanguage` there's no `Strikethrough` node (the default base is
      // CommonMark-only). Always on — the parser feeds both live preview and source mode.
      markdown({ base: markdownLanguage, extensions: [wikilink] }),
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      EditorView.lineWrapping,
      lpCompartment.of(livePreviewConf()),
      EditorView.updateListener.of((u) => {
        if (u.docChanged) scheduleAutosave();
      }),
    ],
    parent: el("editor-host"),
  });
  editorView.focus();
  paintEditor();
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
    src.title = state.sourceOpen ? "Show live preview" : "Show Markdown source";
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

// --- external-edit reconciliation (desktop-ui-mvp.md §5 / #14) --------------------
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
  // The tree first — an external add / remove / rename shows up immediately. Safe in every
  // mode: `render()` rebuilds the tree and side panes but skips the note pane while editing
  // (the carve-out), so a live editor is never touched.
  await loadNotes();

  // The open note. Two cases are deliberately left alone:
  //   • editing — the live buffer is the user's unsaved work; never clobber it. An external
  //     edit to the note being typed in surfaces through the save chain's conflict bar
  //     instead (desktop-editing.md §5), the one case live reload can't own safely.
  //   • reindexing — our own project/embed run owns the open note's refresh (doReindex);
  //     reconciling here would fight it. Its own writes don't pulse anyway (sqlite under
  //     `.b2/`, filtered host-side), but a projection can rewrite `.md` (b2id stamp).
  if (state.current && !state.editing && !state.reindexing) {
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
      if (state.currentResource?.path === cur.path) state.currentResource = fresh;
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
          <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M10 3.5 5.5 8l4.5 4.5"/>
          </svg>
        </button>
        <button id="nav-forward" class="btn ghost icon-btn" title="Forward (⌘])" aria-label="Forward" disabled>
          <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M6 3.5 10.5 8 6 12.5"/>
          </svg>
        </button>
      </div>
      <form id="search-form" class="search" autocomplete="off">
        <input id="search-input" type="search" placeholder="Search the vault…" aria-label="Search" />
      </form>
      <div class="topbar-right">
        <div id="reindex-progress" class="reindex-progress" hidden aria-live="polite">
          <div class="reindex-track"><div id="reindex-fill" class="reindex-fill"></div></div>
          <span id="reindex-label" class="reindex-label"></span>
          <button id="cancel-reindex" class="btn ghost small">Cancel</button>
        </div>
        <span id="vault-root" class="vault-root" title="Active vault"></span>
        <button id="switch-vault" class="btn ghost icon-btn" title="Switch vault — choose another folder" aria-label="Switch vault">
          <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M1.75 4c0-.55.45-1 1-1h2.9c.32 0 .62.15.8.4L7.7 4.6h5.55c.55 0 1 .45 1 1v6.65c0 .55-.45 1-1 1H2.75c-.55 0-1-.45-1-1V4Z"/>
          </svg>
        </button>
        <button id="reindex" class="btn ghost" title="Re-project the vault into the index">Reindex</button>
        <button id="open-settings" class="btn ghost icon-btn" title="Settings (⌘,)" aria-label="Settings">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.324.196.72.257 1.076.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.24-.438.613-.431.992a6.759 6.759 0 0 1 0 .255c-.007.378.138.75.43.99l1.005.828c.424.35.534.954.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.992a6.932 6.932 0 0 1 0-.255c.007-.378-.138-.75-.43-.99l-1.004-.828a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28Z"/>
            <path d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z"/>
          </svg>
        </button>
      </div>
    </header>
    <main id="layout" class="layout">
      <nav id="tree-pane" class="tree-pane"></nav>
      <div id="gutter-tree" class="gutter" role="separator" aria-orientation="vertical"
           aria-label="Resize the file tree" aria-controls="tree-pane" tabindex="0"
           aria-valuemin="${BOUNDS.tree.min}" aria-valuemax="${BOUNDS.tree.max}"></div>
      <section id="note-pane" class="note-pane"></section>
      <div id="gutter-side" class="gutter" role="separator" aria-orientation="vertical"
           aria-label="Resize the discovery pane" aria-controls="side-pane" tabindex="0"
           aria-valuemin="${BOUNDS.side.min}" aria-valuemax="${BOUNDS.side.max}"></div>
      <aside id="side-pane" class="side-pane"></aside>
    </main>
    <div id="menu-root"></div>
    <div id="modal-root"></div>
    <div id="toast" class="toast" role="status" hidden></div>`;
}

function wireEvents(): void {
  // Delegated clicks for everything that renders dynamically.
  document.addEventListener("click", (e) => {
    const target = e.target as HTMLElement;

    // An open right-click menu owns the next click: its own items act, any other
    // click merely dismisses it (a menu-dismissing click isn't also a card click).
    if (state.contextMenu) {
      const menu = state.contextMenu;
      if (menu.kind === "tree") {
        if (target.closest("[data-ctx-new-note]")) {
          startTreeCreate("note", menu.dir); // clears the menu itself
          return;
        }
        if (target.closest("[data-ctx-new-folder]")) {
          startTreeCreate("folder", menu.dir);
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
      if (target.closest("[data-ctx-link]")) {
        const { path, title } = menu;
        closeContextMenu();
        openLinkModal(path, title ?? "");
        return;
      }
      closeContextMenu();
      return;
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
    // Settings modal: the Download button (in-app `b2 init`), else the Done button or a
    // click on the backdrop itself closes it. Checked before the link-modal backdrop
    // branch so settings wins when it's up.
    if (state.settingsOpen) {
      if (target.closest("#settings-provision")) {
        void provisionModel();
        return;
      }
      const themeBtn = target.closest<HTMLElement>("[data-theme-choice]");
      if (themeBtn) {
        const choice = themeBtn.dataset.themeChoice ?? null;
        if (isThemePref(choice)) setTheme(choice);
        return;
      }
      if (
        target.closest("[data-settings-close]") ||
        target.classList.contains("modal-backdrop")
      ) {
        closeSettings();
      }
      return; // clicks inside the settings modal do nothing else
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

    const wiki = target.closest<HTMLElement>(".wikilink");
    if (wiki) {
      e.preventDefault();
      const t = wiki.dataset.target;
      if (t) void openNote(t);
      return;
    }

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
    const lens = target.closest<HTMLElement>("[data-graph-lens]");
    if (lens) {
      setGraphLens(lens.dataset.graphLens ?? "");
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
    if (target.closest("#reindex")) {
      trackIndexing(doReindex());
      return;
    }
    if (target.closest("#cancel-reindex")) {
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
      state.selectedDir = dir;
      openTreeMenu(e.clientX, e.clientY, dir);
      return;
    }
    const card = target.closest<HTMLElement>(".card.candidate, .gnode.is-ghost");
    if (!card) return;
    e.preventDefault();
    openCardMenu(e.clientX, e.clientY, card.dataset.cardPath ?? "", card.dataset.cardTitle ?? "");
  });

  // The floating menu is positioned at fixed viewport coords, so any scroll or resize
  // strands it — dismiss rather than let it hover over the wrong card. Capture-phase so
  // a scroll inside the side pane (which doesn't bubble) is still caught.
  document.addEventListener("scroll", closeContextMenu, true);
  window.addEventListener("resize", closeContextMenu);

  // Search on submit (Enter).
  document.addEventListener("submit", (e) => {
    if ((e.target as HTMLElement).id === "search-form") {
      e.preventDefault();
      const input = document.getElementById("search-input") as HTMLInputElement | null;
      void doSearch(input?.value ?? "");
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
  });

  // ⌘, toggles Settings (the macOS Preferences reflex); Escape closes whichever modal is
  // up; Cmd/Ctrl+S forces an immediate flush while editing (autosave means it's never
  // *required* — this is for the reflex); ⌘N / ⇧⌘N create a note / folder in the
  // selection's folder (the tree-head icons' shortcuts).
  document.addEventListener("keydown", (e) => {
    // The tree's inline create input owns its keys first: Enter commits, Escape
    // cancels, and nothing else typed there leaks into the global chords below.
    if (state.treeCreate && (e.target as HTMLElement).id === "tree-create-input") {
      if (e.key === "Enter") {
        e.preventDefault();
        void commitTreeCreate((e.target as HTMLInputElement).value, true);
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancelTreeCreate();
      }
      return;
    }
    if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "n") {
      if (state.settingsOpen || state.linkTarget) return; // a modal owns the keyboard
      e.preventDefault();
      startTreeCreate(e.shiftKey ? "folder" : "note", state.selectedDir);
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === ",") {
      e.preventDefault();
      if (state.settingsOpen) closeSettings();
      else void openSettings();
      return;
    }
    if (e.key === "Escape") {
      if (state.contextMenu) {
        closeContextMenu();
        return;
      }
      if (state.settingsOpen) {
        closeSettings();
        return;
      }
      if (state.linkTarget) {
        closeModal();
        return;
      }
      // With nothing else to dismiss, Escape backs out of the graph into reading.
      if (state.graphOpen && state.current && !state.editing) toggleGraph();
      return;
    }
    if (state.editing && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void saveNow();
      return;
    }
    // ⌘[ / ⌘] (and the ⌘←/⌘→ aliases) walk the pane's history (#52) — but never over
    // text entry or a modal. While editing, both chords belong to CodeMirror (Mod-[/]
    // are indent bindings, Mod-arrows caret movement); in an input, only the arrows
    // mean caret-to-edge, so the brackets still navigate (e.g. straight from the
    // search field). The buttons and mouse back/forward stay live everywhere — they
    // flush through navGo's edit-mode guard.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && !state.editing) {
      if (state.settingsOpen || state.linkTarget) return;
      const back = e.key === "[" || e.key === "ArrowLeft";
      const forward = e.key === "]" || e.key === "ArrowRight";
      if (!back && !forward) return;
      if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && inTextEntry()) return;
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
  window.addEventListener("blur", () => {
    if (state.editing) void saveNow();
  });
}

// --- boot -----------------------------------------------------------------------

async function boot(): Promise<void> {
  loadTheme(); // stamp the saved appearance onto <html> before the first paint
  buildShell();
  initPanes(el("layout")); // restore the saved column widths, likewise before the paint
  wireEvents();
  // Auto-reload on external edits (#14): subscribe once for the window's lifetime. The
  // host only pulses when the *watched* vault's Markdown changes, and re-points the watch
  // on a vault switch, so this single subscription always tracks the active vault.
  void api.onVaultChanged(() => void onVaultChanged());
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
  render();
  // Auto-index on launch (#25): if the startup vault is unindexed or only partly embedded,
  // bring it up to date now instead of waiting behind a manual Reindex click. No-ops when
  // no vault resolved (vaultRoot === null) or the index is already complete.
  trackIndexing(autoIndexOnOpen(state.vaultRoot));
}

void boot();
