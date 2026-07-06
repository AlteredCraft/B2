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
  (el("reindex") as HTMLButtonElement).disabled = state.loading || state.vaultRoot === null;

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

async function doReindex(): Promise<void> {
  state.loading = true;
  render();
  try {
    const r = await api.reindex();
    flash(`Indexed ${r.indexed} note(s) — ${r.embedded} embedded, ${r.stamped} stamped.`);
    // A reindex can add, remove, or rename notes — refresh the tree to match.
    await loadNotes();
    if (state.current) {
      // The open note may have changed on disk; re-read it and refresh discovery.
      state.current = await api.readNote(state.current.path);
      await refreshDiscovery();
    }
  } catch (e) {
    flash(errText(e));
  } finally {
    state.loading = false;
    render();
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
        <span id="vault-root" class="vault-root" title="Active vault"></span>
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
    if (target.closest("#reindex")) {
      void doReindex();
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
