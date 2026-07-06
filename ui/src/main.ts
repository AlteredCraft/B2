// The controller: build the shell once, wire events (delegated), run the actions
// that mutate `state` and re-render. No framework — the app is small enough that a
// full-pane innerHTML swap on each change is instant and keeps the model honest.
// All backend access goes through `api` (the one IPC seam); this file holds the UI
// flow, never engine logic.

import "../style.css";
import { api } from "./api";
import { state } from "./state";
import { modalHtml, notePaneHtml, sidePaneHtml, treePaneHtml } from "./render";

// --- render ---------------------------------------------------------------------

function el(id: string): HTMLElement {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node;
}

function render(): void {
  el("tree-pane").innerHTML = treePaneHtml(state);
  el("note-pane").innerHTML = notePaneHtml(state);
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

// A rejected `invoke` resolves to the host's user-facing string (CmdError serializes
// to `user_message`), so surface it directly — it's already generic and actionable.
function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
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

function toggleSource(): void {
  state.sourceOpen = !state.sourceOpen;
  render();
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
    const report = await api.link(src.path, target.path, relation, explanation);
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

// Reindex as a cancellable background action (async-indexing.md §4). Deliberately does
// NOT set `state.loading` — the app stays fully usable (read/search/navigate) while it
// runs; only the Reindex button is disabled and a progress + Cancel affordance shows.
// Progress streams in via the channel callback, which repaints only the affordance.
async function doReindex(): Promise<void> {
  if (state.reindexing) return; // single-in-flight (the host also guards this)
  const startedRoot = state.vaultRoot; // guard against a vault switch mid-run
  state.reindexing = true;
  state.reindexProgress = null;
  state.reindexCancelling = false;
  render();
  try {
    const r = await api.reindex((p) => {
      if (state.vaultRoot !== startedRoot) return; // stray event from a vault we've left
      state.reindexProgress = p;
      paintReindex();
    });
    // If the switch already committed (vaultRoot changed), it owns the UI — bail.
    if (state.vaultRoot !== startedRoot) return;
    // The common ordering is subtler: the host frees the reindex slot *before* the
    // vault-switch command returns, so this Promise usually resolves while `vaultRoot`
    // is still `startedRoot` — the check above misses it. But a cancel we didn't
    // initiate (`reindexCancelling` is false) can only come from a vault switch
    // cancelling us host-side (main.rs `cancel_and_wait_for_reindex` is the sole other
    // cancel source). In that case the switch will reload the new vault — so we must
    // NOT toast or reload the vault we're leaving. A user-initiated cancel
    // (`reindexCancelling` true) *does* fall through: phase 1/2 ran, so notes/edges may
    // have changed and the tree should refresh.
    if (r.cancelled && !state.reindexCancelling) return;
    flash(
      r.cancelled
        ? `Indexed ${r.embedded}/${r.indexed} note(s) — cancelled. Re-run to finish the rest.`
        : `Indexed ${r.indexed} note(s) — ${r.embedded} embedded, ${r.stamped} stamped.`,
    );
    // A reindex can add, remove, or rename notes — refresh the tree to match.
    await loadNotes();
    if (state.current) {
      // The open note may have changed on disk; re-read it and refresh discovery.
      state.current = await api.readNote(state.current.path);
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

// Ask the host to stop the in-flight reindex at its next batch boundary. Cooperative:
// the reindex Promise in `doReindex` resolves shortly after with `cancelled: true`, and
// its `finally` clears the affordance.
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

  // Escape closes the modal.
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state.linkTarget) closeModal();
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
