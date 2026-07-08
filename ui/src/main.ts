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
import { state } from "./state";
import { livePreview, wikilink } from "./livepreview";
import { escapeHtml, modalHtml, notePaneHtml, sidePaneHtml, treePaneHtml } from "./render";

// --- render ---------------------------------------------------------------------

function el(id: string): HTMLElement {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node;
}

function render(): void {
  el("tree-pane").innerHTML = treePaneHtml(state);
  // The carve-out (desktop-editing.md §6): while editing, the note pane belongs to
  // the live EditorView — rebuilding it here (e.g. from a toast timer) would destroy
  // the editor mid-keystroke. Everything else keeps rendering.
  if (!state.editing) el("note-pane").innerHTML = notePaneHtml(state);
  el("side-pane").innerHTML = sidePaneHtml(state);
  el("modal-root").innerHTML = modalHtml(state);
  el("vault-root").textContent = state.vaultRoot ?? "no vault";
  document.body.classList.toggle("is-loading", state.loading);
  paintReindex();
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
  } catch (e) {
    state.notes = [];
    flash(errText(e));
  }
}

async function openNote(ref: string): Promise<void> {
  // Mid-edit navigation (tree, side pane, search) flushes the buffer and leaves edit
  // mode first; a conflict keeps the editor — and the user's buffer — alive instead.
  if (!(await closeEditor())) return;
  state.loading = true;
  render();
  try {
    state.current = await api.readNote(ref);
    expandAncestors(state.current.path);
    state.searchQuery = "";
    state.searchResults = [];
    await refreshDiscovery();
  } catch (e) {
    flash(errText(e));
  } finally {
    state.loading = false;
    render();
  }
}

function toggleDir(path: string): void {
  if (state.expandedDirs.has(path)) state.expandedDirs.delete(path);
  else state.expandedDirs.add(path);
  render();
}

function toggleFrontmatter(): void {
  state.frontmatterOpen = !state.frontmatterOpen;
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
  try {
    const [similar, explain] = await Promise.all([
      api.similar(n.path),
      api.explain(n.path),
    ]);
    state.similar = similar;
    state.connections = explain.connections;
  } catch (e) {
    // Discovery failing (e.g. an unembedded vault) is non-fatal — show the note,
    // empty the panes, and surface the reason.
    state.similar = [];
    state.connections = [];
    flash(errText(e));
  }
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
    state.vaultRoot = info.root;
    state.semantic = info.semantic;
    state.current = null;
    state.similar = [];
    state.connections = [];
    state.searchQuery = "";
    state.searchResults = [];
    state.expandedDirs = new Set<string>();
    const input = document.getElementById("search-input") as HTMLInputElement | null;
    if (input) input.value = "";
    state.loading = true;
    render();
    await loadNotes(); // catches its own errors → toast; empty tree on an unindexed vault
    state.loading = false;
    flash(`Switched to ${info.root}.`);
  } catch (e) {
    state.loading = false;
    flash(errText(e));
  }
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
    // The tree paints HERE — a projection can add, remove, or rename notes, and the
    // vault is browsable + keyword-searchable while embedding runs.
    await loadNotes();
    render();
    if (state.reindexCancelling) {
      // Cancel landed during the short projection window: don't start embedding (the
      // host would clear the flag and run to completion). The projected index is
      // complete and consistent; vectors fill on the next run.
      flash(`Indexed ${p.indexed} note(s) — cancelled before embedding. Re-run to embed.`);
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
    flash(
      r.cancelled
        ? `Embedded ${r.embedded}/${p.indexed} note(s) — cancelled. Re-run to finish the rest.`
        : `Indexed ${p.indexed} note(s) — ${r.embedded} embedded, ${p.stamped} stamped.`,
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
    void runTrailingEmbed();
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

// --- shell + events -------------------------------------------------------------

function buildShell(): void {
  el("app").innerHTML = `
    <header class="topbar">
      <div class="brand">B2</div>
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
      </div>
    </header>
    <main class="layout">
      <nav id="tree-pane" class="tree-pane"></nav>
      <section id="note-pane" class="note-pane"></section>
      <aside id="side-pane" class="side-pane"></aside>
    </main>
    <div id="modal-root"></div>
    <div id="toast" class="toast" role="status" hidden></div>`;
}

function wireEvents(): void {
  // Delegated clicks for everything that renders dynamically.
  document.addEventListener("click", (e) => {
    const target = e.target as HTMLElement;

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

    const linkBtn = target.closest<HTMLElement>("[data-link-path]");
    if (linkBtn) {
      openLinkModal(linkBtn.dataset.linkPath ?? "", linkBtn.dataset.linkTitle ?? "");
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
    if (target.closest("#switch-vault")) {
      void switchVault();
      return;
    }
    if (target.closest("#reindex")) {
      void doReindex();
      return;
    }
    if (target.closest("#cancel-reindex")) {
      void cancelReindex();
      return;
    }
  });

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
  });

  // Escape closes the modal; Cmd/Ctrl+S forces an immediate flush while editing
  // (autosave means it's never *required* — this is for the reflex).
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state.linkTarget) closeModal();
    if (state.editing && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void saveNow();
    }
  });

  // Losing window focus is a flush point: the buffer lands on disk before the user
  // looks at (or edits in) anything else.
  window.addEventListener("blur", () => {
    if (state.editing) void saveNow();
  });
}

// --- boot -----------------------------------------------------------------------

async function boot(): Promise<void> {
  buildShell();
  wireEvents();
  try {
    const info = await api.vaultInfo();
    state.vaultRoot = info.root;
    state.semantic = info.semantic;
    // Populate the file tree so the vault is navigable before anything is opened.
    await loadNotes();
  } catch (e) {
    // No vault (or another startup failure): the note pane shows the actionable state.
    state.vaultRoot = null;
    flash(errText(e));
  }
  render();
}

void boot();
