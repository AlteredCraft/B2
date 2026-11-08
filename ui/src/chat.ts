// The chat pane's pure logic (no DOM, no IPC), flow ④ in the GUI (GH #155).
//
// Chat lives in the right column so a citation opens its note in the centre pane without
// the conversation leaving the screen. The transcript emits sidenav.ts's `SideRow`, so it
// inherits the discovery pane's keyboard walk and focus restoration. History is
// session-only (S4): never persisted, not even to `localStorage`.

import type { SideRow } from "./sidenav.ts";
import { coverage } from "./coverage.ts";
import type {
  AnswerView,
  ChatSetup,
  ChatTurn,
  Citation,
  CapSetting,
  OllamaModel,
  ToolUse,
} from "./types";

/**
 * One transcript entry. A failed turn stays on screen but contributes nothing to
 * `chatHistory`.
 */
export interface ChatMessage {
  role: "user" | "assistant";
  /** The question, the answer, or "" when `error` is set. */
  text: string;
  /** Resolved `[n]` markers; empty for a user message or a failed turn. */
  citations: Citation[];
  /** The stream was stopped; the text is a prefix. */
  cancelled: boolean;
  /** The B2 tools the answer was built from. */
  tools?: ToolUse[];
  /** The host's failure message, in place of an answer. */
  error?: string;
}

/** The question the human typed, as a transcript entry. */
export function userMessage(text: string): ChatMessage {
  return { role: "user", text, citations: [], cancelled: false, tools: [] };
}

/** A finished (or stopped) answer, as a transcript entry. */
export function answerMessage(view: AnswerView): ChatMessage {
  return {
    role: "assistant",
    text: view.answer,
    citations: view.citations,
    cancelled: view.cancelled,
    tools: view.tools ?? [],
  };
}

/**
 * The question a Similar card's **Why?** puts in the transcript. Display text only: the
 * prompt is assembled host-side by `Vault::why_similar`.
 */
export function whyQuestion(
  candidate: { path: string; title: string | null },
  anchor: { path: string; title: string | null },
): string {
  const name = (n: { path: string; title: string | null }) => n.title || n.path;
  return `Why is “${name(candidate)}” suggested as similar to “${name(anchor)}”?`;
}

/**
 * The line naming the B2 tools an answer used, or "". Not escaped here: a tool name is
 * untrusted model output, and the paint escapes it.
 */
export function toolsLine(tools: readonly ToolUse[] = []): string {
  const names = [...new Set(tools.map((t) => t.name.replace(/^b2_/, "").replaceAll("_", " ")))];
  return names.length === 0 ? "" : `Looked up with B2 tools: ${names.join(", ")}`;
}

/** What a cap field sends with a save, or why it can't. */
export type CapVerdict = { send: string | null } | { error: string };

/**
 * What a cap field sends with a save, per the host's three-state rule (`apply_cap`):
 * `null` when untouched (storing the shown value would pin today's default over a later
 * env var), `""` when cleared, the number when set, or an `error` naming the field.
 */
function capInput(raw: string, cap: CapSetting, label: string): CapVerdict {
  const typed = raw.trim();
  if (typed === "") return { send: "" };
  if (typed === String(cap.in_force)) return { send: null };
  // Digits only: `Number()` would wave through "1e3", " 12 " and "0x10".
  const n = /^\d+$/.test(typed) ? Number(typed) : Number.NaN;
  if (!Number.isSafeInteger(n) || n < 1 || n > cap.ceiling)
    return { error: `${label} must be a whole number from 1 to ${cap.ceiling}.` };
  return { send: String(n) };
}

/** The **Tool calls per reply** field (`apply_tool_cap`). */
export function toolCapInput(raw: string, cap: CapSetting): CapVerdict {
  return capInput(raw, cap, "Tool calls per reply");
}

/** The **Focus passages** field (`apply_focus_cap`, GH #253). */
export function focusCapInput(raw: string, cap: CapSetting): CapVerdict {
  return capInput(raw, cap, "Focus passages");
}

/** The note a question is about, by path (its identity) and title. */
export interface ChatFocus {
  path: string;
  title: string | null;
}

/**
 * The note chat is about (GH #253): the open note, sent as `ask`'s `focus` so "this doc"
 * has a referent. A resource is never one: the engine has no passages of it to ground on.
 */
export function chatFocus(s: {
  current: ChatFocus | null;
  currentResource: unknown;
}): ChatFocus | null {
  if (s.currentResource !== null || s.current === null) return null;
  return { path: s.current.path, title: s.current.title };
}

/** The composer chip's text, so what the model is told is visible. Not escaped here: a
 *  title is note content, and the paint escapes it (E5). */
export function focusChipLabel(f: ChatFocus): string {
  return `about: ${f.title || f.path}`;
}

/** A turn that failed. */
export function errorMessage(error: string): ChatMessage {
  return { role: "assistant", text: "", citations: [], cancelled: false, tools: [], error };
}

/**
 * The conversation as the next ask sees it, oldest first. A cancelled answer is included
 * (the human read it, so "go on" must condense against it); a failed turn is not.
 */
export function chatHistory(messages: readonly ChatMessage[]): ChatTurn[] {
  return messages
    .filter((m) => m.error === undefined && m.text !== "")
    .map((m) => ({ role: m.role, content: m.text }));
}

/** A transcript row's key. Position-based: two identical questions are two rows. */
export function turnRowKey(index: number): string {
  return `chat:turn:${index}`;
}

/** A citation row's key. Like `cardRowKey`, position makes it unique and the target makes
 *  a stale key fail to match. What it opens comes from the markup's `data-open` (E5). */
export function citationRowKey(turn: number, marker: number, path: string): string {
  return `chat:cite:${turn}:${marker}:${path}`;
}

/** The streaming answer's row key, so focus can sit on it while it fills. */
export const STREAMING_ROW_KEY = "chat:streaming";

/**
 * Every row the chat pane paints, in order: each turn with its citations under it, then
 * the streaming answer. Nothing folds, but → still steps into a turn's citations.
 */
export function chatRows(messages: readonly ChatMessage[], streaming: boolean): SideRow[] {
  const rows: SideRow[] = [];
  messages.forEach((m, i) => {
    const cites = m.citations.map((c) => ({
      key: citationRowKey(i, c.marker, c.path),
      depth: 1,
      fold: null,
      expanded: false,
      hasChildRows: false,
    }));
    rows.push({
      key: turnRowKey(i),
      depth: 0,
      fold: null,
      expanded: true,
      hasChildRows: cites.length > 0,
    });
    rows.push(...cites);
  });
  if (streaming) {
    rows.push({
      key: STREAMING_ROW_KEY,
      depth: 0,
      fold: null,
      expanded: true,
      hasChildRows: false,
    });
  }
  return rows;
}

/**
 * The chat pane's empty state. An unembedded vault is not one: it still answers,
 * keyword-only (M4), so it gets a note instead ([`retrievalNote`]).
 */
export type ChatEmptyState = "no-vault" | "loading" | "no-server" | "no-model" | "ready";

export function chatEmptyState(s: {
  hasVault: boolean;
  setup: ChatSetup | null;
}): ChatEmptyState {
  if (!s.hasVault) return "no-vault";
  if (s.setup === null) return "loading";
  switch (s.setup.state) {
    case "unreachable":
      return "no-server";
    case "model_missing":
      return "no-model";
    case "fake":
    case "ready":
      return "ready";
  }
}

/** `chatEmptyState` read off the app state. */
export function chatStateOf(s: { vaultRoot: string | null; chatSetup: ChatSetup | null }): ChatEmptyState {
  return chatEmptyState({ hasVault: s.vaultRoot !== null, setup: s.chatSetup });
}

/** Can a question be asked right now? */
export function chatReady(s: { vaultRoot: string | null; chatSetup: ChatSetup | null }): boolean {
  return chatStateOf(s) === "ready";
}

/** Ollama's OpenAI-compatible endpoint, the Local configuration's seed. Mirrors
 *  `b2_llm::DEFAULT_BASE_URL`; change them together. */
export const LOCAL_CHAT_ENDPOINT = "http://localhost:11434/v1";

/**
 * The retrieval note under the composer, or "": the search caveat (#26) applied to chat,
 * never a blocker (E4). It reports a state, not an activity: a partial fraction may be
 * the residue of a cancelled reindex, so it names the gesture that closes the gap.
 */
export function retrievalNote(s: {
  semantic: boolean;
  notesEmbedded: number;
  notesTotal: number;
}): string {
  const c = coverage(s);
  if (c.embedded === "empty") return "";
  if (!c.model)
    return "Answers are grounded by keyword search only — the embedding model isn’t installed.";
  switch (c.embedded) {
    case "none":
      return "Answers are grounded by keyword search for now — this vault isn’t embedded yet.";
    case "partial":
      return `Keyword-first grounding — ${c.n}/${c.m} notes embedded. Reindex to fill the rest.`;
    case "all":
      return "";
  }
}

/** The setup card's command, spelled once (mirrors `b2-llm`'s `pull_command`). */
export function pullCommand(model: string): string {
  return `ollama pull ${model}`;
}

/** The `ollama pull` placeholder where no model is named; angle brackets so it can't be
 *  mistaken for a real model. */
export const PULL_PLACEHOLDER = "<model-name>";

/**
 * Where B2 sends someone who has to install or pull in Ollama. Mirrors `b2-llm`'s
 * `OLLAMA_INSTALL_URL`; change them together.
 */
export const OLLAMA_QUICKSTART_URL = "https://docs.ollama.com/quickstart";

/** Ollama's hosted models, offered as a link, never a pre-filled URL (M5). */
export const OLLAMA_CLOUD_URL = "https://docs.ollama.com/cloud";

/** An installed model's detail, e.g. "3.2B · 2.0 GB". */
export function modelDetail(m: OllamaModel): string {
  return [m.parameters ?? "", formatModelSize(m.size)].filter(Boolean).join(" · ");
}

/** A model's on-disk size, for the installed list. */
export function formatModelSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  const gb = bytes / 1_073_741_824;
  if (gb >= 10) return `${Math.round(gb)} GB`;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.max(1, Math.round(bytes / 1_048_576))} MB`;
}
