// The chat pane's paint (flow ④) and the setup card it shares with Settings → Chat. The
// pure logic is chat.ts; the streaming, cancellation and focus discipline are main.ts's.

import { escapeHtml } from "./escape.ts";
import type { AppState } from "./state.ts";
import { displayKeys } from "./bindings.ts";
import {
  type ChatMessage,
  OLLAMA_QUICKSTART_URL,
  STREAMING_ROW_KEY,
  chatReady,
  chatStateOf,
  citationRowKey,
  modelDetail,
  pullCommand,
  retrievalNote,
  toolsLine,
  turnRowKey,
} from "./chat.ts";
import type { ChatSetup } from "./types.ts";
import { renderMarkdown } from "./markdown.ts";
import { sideTab } from "./widgets.ts";

// --- chat (flow ④, GH #151/#153/#155) -----------------------------------------------
//
// The right column's third mode: ask a question, watch the answer stream, click a
// citation to open the note behind it — *without* the conversation leaving the screen,
// which is why chat lives here rather than in the centre pane (chat.ts's header).
//
// Three rules this builder holds, all of them invariants rather than styling:
//
//   • **E5 — model output is untrusted content.** An answer is a string from a model that
//     was itself fed note content (which anyone can author), so it renders through the one
//     sanitizing `renderMarkdown` seam like every other document. The *streaming* half is
//     stronger still: it is written as `textContent` by `paintChatStream` (main.ts), so a
//     half-arrived answer is never parsed as markup at all.
//   • **A citation navigates in-app, never the webview.** Each one is a `data-open` button
//     — the same delegation search results and discovery cards use — so the mouse and ⏎
//     share one activation path (K1), and no `href` ever exists to be followed.
//   • **K1 — the pane is keyboard-complete.** Rows are `role="treeitem"` over chat.ts's
//     row order (the same order the arrows walk), with a roving tabstop; every control has
//     a stable `id` so `paintSide` can hand focus back across the repaint each token causes.
export function chatPaneHtml(state: AppState, roving: string | null): string {
  const setup = state.chatSetup;
  const streaming = state.chatStreaming !== null;
  const model = setup
    ? `<span class="chat-model" title="${escapeHtml(
        `${setup.model} · ${setup.base_url}`,
      )}">${escapeHtml(setup.model)}</span>`
    : "";
  const head = `<div class="side-head chat-head">
      <h2>Chat</h2>
      ${model}
      <button id="chat-new" class="linklike"${
        state.chatMessages.length === 0 || streaming ? " disabled" : ""
      } data-chat-new title="Start a new conversation — nothing here is saved">new</button>
    </div>`;
  // No composer until there is something to answer with: a disabled field under a card
  // that says why is chrome with nothing behind it, and one more stop for a keyboard user
  // to walk past on the way to the fix the card is pointing at.
  return head + chatStageHtml(state, roving) + (chatReady(state) ? chatComposerHtml(state) : "");
}

/** The conversation, or the state that stands in for one (chat.ts's `chatEmptyState`
 *  makes that choice once, so the paint doesn't re-derive it branch by branch). */
function chatStageHtml(state: AppState, roving: string | null): string {
  switch (chatStateOf(state)) {
    case "no-vault":
      return `<p class="side-empty">Open a vault to chat with your notes.</p>`;
    case "loading":
      return `<p class="side-empty">Looking for a model…</p>`;
    case "no-server":
    case "no-model":
      return chatSetupCardHtml(state.chatSetup, false);
    case "ready":
      return chatLogHtml(state, roving);
  }
}

/** The transcript. Empty until the first question — and then it is what the pane is. */
function chatLogHtml(state: AppState, roving: string | null): string {
  if (state.chatMessages.length === 0 && state.chatStreaming === null) {
    const fake = state.chatSetup?.state === "fake";
    return `<div class="chat-log" id="chat-log">
        <div class="chat-intro">
          <p><strong>Ask your notes a question.</strong></p>
          <p class="muted">Answers come only from passages B2 retrieves from this vault, cited by
            [n]. Nothing here is written to your notes, and the conversation isn’t saved.</p>
          ${
            fake
              ? `<p class="muted">The fake chat provider is in use (<code>B2_LLM=fake</code>) — answers are deterministic test scaffolding, not a model.</p>`
              : ""
          }
        </div>
      </div>`;
  }
  const turns = state.chatMessages
    .map((m, i) => chatTurnHtml(m, i, roving))
    .join("");
  // The in-flight answer is a row of its own so the keyboard can sit on it while it
  // fills. `#chat-stream` is the element `paintChatStream` writes tokens into — as text,
  // never markup — which is what keeps a streaming answer off the full-render path.
  const live =
    state.chatStreaming === null
      ? ""
      : `<div class="chat-turn chat-answer chat-live" role="treeitem" aria-level="1"${sideTab(
          STREAMING_ROW_KEY,
          roving,
        )} data-side-row="${escapeHtml(STREAMING_ROW_KEY)}" aria-live="polite">
          <div class="chat-role">B2</div>
          <div class="chat-text" id="chat-stream" data-waiting="${escapeHtml(
            state.chatWaiting,
          )}">${escapeHtml(state.chatStreaming)}</div>
        </div>`;
  return `<div class="chat-log" id="chat-log" role="tree" aria-label="Conversation">${turns}${live}</div>`;
}

/** One turn: the question as typed, or the answer — rendered through the sanitizing
 *  Markdown seam (E5) — with its citations as rows beneath it. */
function chatTurnHtml(m: ChatMessage, index: number, roving: string | null): string {
  const key = turnRowKey(index);
  const row = (cls: string, role: string, body: string): string =>
    `<div class="chat-turn ${cls}" role="treeitem" aria-level="1"${sideTab(
      key,
      roving,
    )} data-side-row="${escapeHtml(key)}">
      <div class="chat-role">${role}</div>
      ${body}
    </div>`;
  if (m.role === "user") {
    return row("chat-question", "You", `<div class="chat-text">${escapeHtml(m.text)}</div>`);
  }
  if (m.error !== undefined) {
    return row(
      "chat-answer chat-failed",
      "B2",
      `<div class="chat-error">${escapeHtml(m.error)}</div>`,
    );
  }
  const stopped = m.cancelled
    ? `<p class="chat-stopped">Stopped — this answer is partial.</p>`
    : "";
  // Which B2 tools the answer was built from (a *Why?* turn). Tool names are the
  // model's own words, so they are escaped like every other value in the chrome (E5).
  const line = toolsLine(m.tools);
  const used = line ? `<p class="chat-tools">${escapeHtml(line)}</p>` : "";
  const cites = m.citations
    .map(
      (c) => `<button class="chat-cite" role="treeitem" aria-level="2"${sideTab(
        citationRowKey(index, c.marker, c.path),
        roving,
      )} data-side-row="${escapeHtml(
        citationRowKey(index, c.marker, c.path),
      )}" data-open="${escapeHtml(c.path)}" title="Open ${escapeHtml(c.path)}">
        <span class="chat-cite-marker">[${c.marker}]</span>
        <span class="chat-cite-path">${escapeHtml(c.path)}</span>
        ${c.excerpt ? `<span class="chat-cite-excerpt">${escapeHtml(c.excerpt)}</span>` : ""}
      </button>`,
    )
    .join("");
  return (
    row(
      "chat-answer",
      "B2",
      `<div class="chat-text">${renderMarkdown(m.text)}</div>${stopped}${used}`,
    ) + cites
  );
}

/**
 * The composer. A `<textarea>` rather than an input because a question can be a
 * paragraph: ⏎ asks, ⇧⏎ is a newline (the platform's own reflex in a multi-line field,
 * which is why the registry marks the chord `fixed`).
 *
 * While an answer streams, Ask becomes **Stop** — the mouse's equal of Esc (K1: no action
 * reachable only by keyboard either). The field itself stays **enabled** throughout: you
 * can line up the next question while this answer arrives, and — the reason it matters —
 * disabling a focused control drops the keyboard to `<body>`, so the one gesture that
 * always precedes a stream would be the one that ejects you from the pane. `sendChat`
 * refuses a second turn instead, where it costs nobody their focus.
 */
function chatComposerHtml(state: AppState): string {
  const streaming = state.chatStreaming !== null;
  const note = retrievalNote(state);
  return `<form class="chat-composer" id="chat-composer">
      <textarea id="chat-input" class="chat-input" rows="2" placeholder="Ask your notes…"
        aria-label="Ask your notes"></textarea>
      <div class="chat-actions">
        ${
          streaming
            ? `<button type="button" class="btn small" id="chat-stop" data-chat-stop title="Stop this answer (${escapeHtml(
                displayKeys(["dismiss"]),
              )})">Stop</button>`
            : `<button type="submit" class="btn small primary" id="chat-send">Ask</button>`
        }
        ${note ? `<span class="chat-note muted">${escapeHtml(note)}</span>` : ""}
      </div>
    </form>`;
}

/**
 * The setup card — deliberately **Ollama-native** (GH #151: guided setup is a per-runtime
 * feature, and Ollama is the runtime B2 guides), shown both as the chat pane's empty state
 * and inside Settings → Chat.
 *
 * Three cards, one builder, because they are the same facts in a different order: no
 * server (start the daemon), no model (pull one — sized to this machine, illustrative and
 * non-binding), and the Settings copy of both. A non-Ollama endpoint gets the message and
 * nothing else: there is no honest instruction to give about pulling a model into LM
 * Studio or a cloud provider.
 */
export function chatSetupCardHtml(setup: ChatSetup | null, inSettings: boolean): string {
  if (!setup) return "";
  const ollama = setup.ollama;
  const message = setup.message
    ? `<p class="chat-setup-message">${escapeHtml(setup.message)}</p>`
    : "";
  // What's installed, when the daemon answered — so "no model" can offer what *is* there
  // instead of only naming what isn't.
  //
  // The pane's card only. In Settings the same inventory *is* the Model field (a picker,
  // `chatModelFieldHtml`), and two controls setting one value is two things to keep in
  // step — the second of which would apply on click while the first waits for Save.
  const installed =
    !inSettings && ollama && ollama.running && ollama.installed.length > 0
      ? `<div class="chat-setup-block">
          <div class="settings-subhead">Installed models</div>
          <ul class="chat-models">${ollama.installed
            .map(
              (m) =>
                `<li><button type="button" class="linklike" data-chat-use-model="${escapeHtml(
                  m.name,
                )}">${escapeHtml(m.name)}</button> <span class="muted">${escapeHtml(
                  modelDetail(m),
                )}</span></li>`,
            )
            .join("")}</ul>
        </div>`
      : "";
  // Pane-only for the same reason as the inventory: in Settings the **Local** section's
  // own copy already carries `ollama pull` sized to this machine (`localNoteHtml`), and
  // one command printed twice on one screen reads as two different instructions.
  const suggestion =
    !inSettings && ollama && ollama.suggested
      ? `<div class="chat-setup-block">
          <div class="settings-subhead">Suggested for this machine</div>
          <p class="settings-detail muted">${
            ollama.ram_gb ? `${ollama.ram_gb} GB of memory` : "This machine"
          } — a ${escapeHtml(ollama.suggested.size)} model. Illustrative, not a requirement.</p>
          <p class="chat-command"><code>${escapeHtml(
            pullCommand(ollama.suggested.model),
          )}</code></p>
        </div>`
      : "";
  const tiers =
    ollama && ollama.tiers.length > 0
      ? `<details class="chat-tiers"><summary>Sizes by memory</summary>
          <ul>${ollama.tiers
            .map(
              (t) =>
                `<li>${escapeHtml(t.ram)} → ${escapeHtml(t.size)} <code>${escapeHtml(
                  t.model,
                )}</code></li>`,
            )
            .join("")}</ul>
        </details>`
      : "";
  const install =
    ollama && !ollama.running
      ? `<p class="settings-note">B2 talks to any OpenAI-compatible model server; Ollama is the
          one it can walk you through. Start it with <code>ollama serve</code> — or, if it
          isn’t installed yet, the
          <a href="${OLLAMA_QUICKSTART_URL}">Ollama quickstart</a> is the install and the
          first pull, in that order.</p>`
      : "";
  // A retry, in the pane only: the card's whole job is to be looked at while the user
  // goes and fixes something (starts the daemon, pulls a model), so it must be able to
  // notice that they did — without making them close and reopen the pane. Settings has
  // its own re-probe, spelled *Save and test*.
  const recheck = inSettings
    ? ""
    : `<div class="settings-action"><button class="btn small" id="chat-recheck" data-chat-recheck>Check again</button></div>`;
  return `<div class="chat-setup${inSettings ? " chat-setup-inline" : ""}">
      ${inSettings ? "" : `<div class="chat-setup-head">Chat isn’t ready yet</div>`}
      ${message}
      ${recheck}
      ${install}
      ${suggestion}
      ${installed}
      ${tiers}
      ${
        inSettings
          ? ""
          : `<p class="settings-note">Everything else in B2 — search, discovery, editing —
              works exactly as it does with chat off.</p>`
      }
    </div>`;
}
