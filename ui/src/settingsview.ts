// Settings: the full-window tabbed surface and its panels (General, Index, Embedding,
// Chat, Keyboard). The rail's moves are settingstabs.ts's; the keyboard panel is
// keysview.ts's, the chat setup card chatview.ts's.

import { escapeHtml } from "./escape.ts";
import type { AppState } from "./state.ts";
import { coverage } from "./coverage.ts";
import { displayKeys } from "./bindings.ts";
import { SETTINGS_TABS, tabDomId } from "./settingstabs.ts";
import {
  LOCAL_CHAT_ENDPOINT,
  OLLAMA_CLOUD_URL,
  OLLAMA_QUICKSTART_URL,
  PULL_PLACEHOLDER,
  modelDetail,
  pullCommand,
} from "./chat.ts";
import type { ChatSetup } from "./types.ts";
import { reindexMeterHtml, segmentedHtml } from "./widgets.ts";
import { chatSetupCardHtml } from "./chatview.ts";
import { keyboardPanelHtml } from "./keysview.ts";

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

// The per-model embedding-time ledger (b2-desktop stats.rs), so a model swap can be judged
// on real speed. One row per model with history.
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
// Every control in here needs a stable `id`: `#modal-root` is swapped wholesale on a
// repaint, and `captureModalFocus` re-finds the focused control by id afterwards
// (crates/b2-desktop/CLAUDE.md, "Two things that bite"). Without one, focus drops to <body>.

/** The panel for one section. */
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

// Chat — which model answers, and where it runs (GH #151). Local vs Cloud is a view of the
// URL, not a second piece of state. The privacy copy sits beside the Cloud fields because
// the consent moment is the configuration moment (M5). No vault or index state, so
// changing it costs no reindex (contrast M2).
function chatPanelHtml(state: AppState): string {
  const setup = state.chatSetup;
  const cloud = state.chatCloud;
  const segments = segmentedHtml(
    "Chat model location",
    "settings-chat-",
    "data-chat-mode",
    [
      { id: "local", label: "Local" },
      { id: "cloud", label: "Cloud models" },
    ],
    cloud ? "cloud" : "local",
  );
  // The same sentence the chat pane shows, from the same probe.
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
        ${segments}
      </div>
      <label class="field">Endpoint
        <input id="settings-chat-url" type="text" autocomplete="off" spellcheck="false"
          value="${escapeHtml(setup?.base_url ?? "")}" placeholder="${LOCAL_CHAT_ENDPOINT}" />
      </label>
      ${chatModelFieldHtml(state, setup)}
      ${key}
      ${toolCapFieldHtml(setup)}
      <div class="settings-action">
        <button class="btn small primary" id="settings-chat-save">Save and test</button>
      </div>
      ${status}`;
}

/**
 * Tool calls per reply (`LlmConfig::max_tool_calls`): a safety bound, validated by chat.ts's
 * `toolCapInput`. `type="text"`, not `number`: `captureModalFocus` restores the caret across
 * a repaint, and a number input throws on `setSelectionRange`.
 */
function toolCapFieldHtml(setup: ChatSetup | null): string {
  if (!setup) return "";
  const cap = setup.tool_calls;
  return `<label class="field">Tool calls per reply
        <input id="settings-chat-tool-cap" type="text" inputmode="numeric" autocomplete="off"
          spellcheck="false" value="${cap.in_force}" placeholder="${cap.default}" />
      </label>
      <p class="settings-detail muted">The most B2 tools the chat model may call in one
        reply when it explains a suggestion. A reply that asks for more is stopped with an
        error instead of being run. Default ${cap.default}, up to ${cap.ceiling}; clear the
        field to go back to the default. Overrides <code>B2_LLM_MAX_TOOL_CALLS</code>.</p>`;
}

/**
 * The Model field: a picker over what the daemon has installed, or a text box when there is
 * no inventory or the user is naming a model still being pulled (`chatModelTyped`). Both
 * shapes share one id: `saveChatConfig` reads `.value` off it, and focus is restored by it.
 */
function chatModelFieldHtml(state: AppState, setup: ChatSetup | null): string {
  const ollama = setup?.ollama;
  // Local only. `chatCloud`, not `setup.cloud`: the view flag flips the instant Cloud is
  // pressed, while `setup` is the last probe's stale answer (nothing re-probes on that press).
  const installed = !state.chatCloud && ollama?.running ? ollama.installed : [];
  const current = setup?.model ?? "";
  if (state.chatModelTyped || installed.length === 0) {
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
  // Keep the configured model even if not installed, or looking at the field would silently
  // re-point the configuration at the first option.
  const missing =
    current !== "" && !installed.some((m) => m.name === current)
      ? `<option value="${escapeHtml(current)}" selected>${escapeHtml(
          current,
        )} — not installed</option>`
      : "";
  const options = installed
    .map((m) => {
      const detail = modelDetail(m);
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

// The Local configuration's note: nothing leaves, so no key field and no privacy warning.
// Plus the pull command, with the suggested model when memory could be read.
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

// The Cloud models key field, and where that key lives (`ApiKeySource`, GH #176). Under
// `environment` a key typed here is stored but not used; under `session` it is gone at quit.
// The field always paints empty, so an empty save means "keep" and Remove is the only way
// back to no key (else repointing the endpoint would send the old token to the new one).
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
  // Must agree with `where`: under `session`, this is the key B2 could not save.
  const storage =
    source === "session"
      ? `This key was <strong>not</strong> saved — B2 normally keeps it in your macOS Keychain,
         never in a plain file. Set <code>B2_LLM_API_KEY</code> in your environment to have one
         that persists regardless.`
      : `B2 saves the key in your macOS Keychain, never in a plain file — set
         <code>B2_LLM_API_KEY</code> in your environment to override it.`;
  // Under `environment` it clears only the stored key, so the copy says which key it means.
  const remove =
    source === "none"
      ? ""
      : `<div class="settings-action"><button class="btn small" id="settings-chat-clear-key" data-chat-clear-key
           title="Forget the key B2 has saved">Remove key</button>
         <span class="muted">Removes the key B2 saved. A key set in <code>B2_LLM_API_KEY</code>
         is your environment's, and stays.</span></div>`;
  // B2 ships no default cloud provider: picking one is the explicit act M5 is about (see
  // `setChatMode`). So a link, not a pre-filled URL.
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

// General — app-wide preferences that belong to no subsystem.
function generalPanelHtml(state: AppState): string {
  const themes = segmentedHtml(
    "Appearance",
    "settings-theme-",
    "data-theme-choice",
    [
      { id: "system", label: "System" },
      { id: "light", label: "Light" },
      { id: "dark", label: "Dark" },
    ],
    state.theme,
  );
  return `<div class="settings-subhead">Appearance</div>
      <p class="settings-detail muted">System follows macOS; Light and Dark pin B2 regardless.</p>
      <div class="field">
        <span class="field-label">Theme</span>
        ${themes}
      </div>`;
}

// Index — the vault's projection (index-engine.md §1), and the manual Reindex. Indexing is
// automatic (#25), so the button lives here, beside the coverage that says whether you
// need it, rather than in the top bar.
function indexPanelHtml(state: AppState): string {
  const disabled = reindexDisabled(state);
  // "Indexed" and "embedded" differ: an unembedded vault must never read as finished (#26).
  const summary = ((): string => {
    if (state.vaultRoot === null) return "No vault is open.";
    const c = coverage(state);
    if (c.embedded === "empty") return "Nothing indexed yet — B2 indexes a vault when you open it.";
    const notes = `${c.m} note${c.m === 1 ? "" : "s"}`;
    if (!c.model)
      return `${notes} indexed for keyword search. The embedding model isn’t installed, so none are embedded.`;
    return c.embedded === "all"
      ? `${notes} indexed, all embedded.`
      : `${notes} indexed · ${c.n}/${c.m} embedded.`;
  })();
  // Settings covers the top bar's meter, so it paints its own; `paintReindex` writes every
  // `.reindex-progress` on screen, so the two can't disagree.
  const running = state.reindexing
    ? reindexMeterHtml({ hidden: false, indeterminate: true, cancelId: "settings-cancel-reindex" })
    : `<span class="muted">Rarely needed — B2 indexes on open and as you save.</span>`;
  return `<div class="settings-subhead">Vault index</div>
      <p class="settings-detail muted">The index is a disposable projection of your Markdown — delete it and a reindex rebuilds it identically.</p>
      <p class="settings-coverage">${escapeHtml(summary)}</p>
      <div class="settings-action">
        <button class="btn small" id="reindex"${disabled ? " disabled" : ""}
          title="Re-project the vault into the index">${escapeHtml(reindexLabel(state))}</button>
        ${running}
      </div>
      <p class="settings-note">Reindex re-projects every note (notes, keyword index, and the
        typed graph), then embeds whatever is missing vectors. Reach for it after changing the
        embedding model, or after editing the vault with B2 closed.</p>`;
}

/** Whether the Reindex button is refused. Shared with main.ts's `paintReindex` so the two
 *  can't drift. */
export function reindexDisabled(state: AppState): boolean {
  return state.loading || state.reindexing || state.vaultRoot === null;
}

export function reindexLabel(state: AppState): string {
  return state.reindexing ? "Indexing…" : "Reindex";
}

// Embedding — which model, its device, download state, files and time ledger.
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
  // Which compute device the build embeds on (GH #40). Hidden until the async read resolves.
  const device = state.embedDevice;
  const deviceRow = device
    ? `<p class="settings-device">Embedding on <span class="settings-badge${
        device === "Metal" ? " settings-badge-metal" : ""
      }">${device === "Metal" ? "⚡ " : ""}${escapeHtml(device)}</span></p>`
    : "";
  // In-app `b2 init`, when the selected model isn't installed.
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

/**
 * Settings: a rail of sections beside the active panel, covering the whole window. Still
 * modal (dialog role, Tab trap, Escape), but with no backdrop: the ways out are Done and Esc.
 *
 * DOM order matters: rail, panel, Done. `focusIntoOverlay` opens on the first focusable,
 * which must be the selected tab. The rail is the ARIA `tabs` pattern with a roving
 * `tabindex` (settingstabs.ts). The panel has `tabindex="0"` because it is the scroll
 * container, and the keyboard can't scroll a region it can't focus.
 */
export function settingsScreenHtml(state: AppState): string {
  const active = state.settingsTab;
  const tabs = SETTINGS_TABS.map((t) => {
    const on = t.id === active;
    return `<button class="stab${on ? " stab-on" : ""}" id="${tabDomId(t.id)}" role="tab"
              aria-selected="${on}" aria-controls="settings-panel" tabindex="${on ? "0" : "-1"}"
              data-settings-tab="${t.id}" title="${escapeHtml(t.hint)}">${escapeHtml(t.label)}</button>`;
  }).join("");
  // Caps prose line length in a window-wide panel; the Keyboard table takes a wider measure.
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
        <span class="modal-hint">${escapeHtml(
          `${displayKeys(["settings.tab.prev", "settings.tab.next"], "/")} picks a section · ${displayKeys([
            "settings.section.next",
          ])} cycles · ${displayKeys(["dismiss"])} closes`,
        )}</span>
        <button class="btn primary" id="settings-done" data-settings-close>Done</button>
      </div>
    </div>`;
}
