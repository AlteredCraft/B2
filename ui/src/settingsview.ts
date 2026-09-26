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
 * **Tool calls per reply** — the cap on what one model reply may ask B2 to run
 * (`LlmConfig::max_tool_calls`). A safety bound, not a tuning knob, and the copy says so:
 * a reply past it is stopped with an error, and the only reason to raise it is a model
 * that really does ask for that many. Painted from the host's own numbers (`tool_calls`),
 * saved with the rest of the panel by *Save and test*, validated by chat.ts's
 * `toolCapInput`.
 *
 * `type="text"` with a numeric keypad rather than `type="number"`: `captureModalFocus`
 * re-selects the focused field's caret across a repaint, and a number input throws on
 * `setSelectionRange`.
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
  // While a run is live the panel carries the meter itself. It used to point at the top
  // bar's ("Progress and Cancel are in the top bar"), which was true while Settings was a
  // box floating over the bar and became a lie the moment it took the window. Same markup
  // and the same painter as the shell's (`paintReindex` walks every `.reindex-progress` on
  // screen), so the two can't disagree about a run — only one of them is ever visible.
  // The Cancel carries an id for the reason every control in here does: the surface
  // repaints per progress batch, and focus is put back by id.
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
export function settingsScreenHtml(state: AppState): string {
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
        <span class="modal-hint">${escapeHtml(
          `${displayKeys(["settings.tab.prev", "settings.tab.next"], "/")} picks a section · ${displayKeys([
            "settings.section.next",
          ])} cycles · ${displayKeys(["dismiss"])} closes`,
        )}</span>
        <button class="btn primary" id="settings-done" data-settings-close>Done</button>
      </div>
    </div>`;
}
