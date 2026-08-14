// The chat pane's pure logic — no DOM, no IPC — so node runs its test straight off the
// source (`npm test`), like sidenav.ts / treenav.ts / settingstabs.ts.
//
// Chat is flow ④ (GH #151/#153) reaching the GUI (GH #155): ask a question, watch the
// answer stream in, click a citation to open the note it came from. Three things about it
// are decisions rather than details, and they are why this module exists at all.
//
// **Where it lives: the right column.** Chat replaces discovery there rather than taking
// the centre pane, and the reason is the citations. An answer's whole value is that you
// can go *read* the note behind it, and the centre pane is where notes are read — so chat
// beside it means a citation opens the note **without the conversation leaving the
// screen**. Chat in the centre would make every citation a choice between the answer and
// the evidence. (Search takes the column too; opening either closes the other — see
// main.ts. One column, one thing in it.)
//
// **The transcript is a row list, like discovery's.** `chatRows` emits sidenav.ts's own
// `SideRow` shape, so the pane inherits the discovery pane's whole ARIA `tree` walk —
// ↑↓ between rows, Home/End, a roving `tabindex`, and (crucially) focus restoration by row
// key across the `innerHTML` swap every streamed answer causes. Reusing the shape rather
// than the *state* is what keeps that free: `sideRows` delegates here when the pane is in
// chat mode, and everything downstream (the paint, the arrows, `paintSide`'s focus
// capture) carries on not knowing which mode it is in. A turn is a row you can land on; a
// citation is a row *under* it that ⏎ opens — the only actionable rows in the transcript,
// which is exactly K1's "a row you can see is a row you can reach".
//
// **History is session-only** (invariant S4). `chatHistory` is what the next ask carries,
// and it is derived from the transcript in memory — never persisted, not even to
// `localStorage` where the theme and the keymap live. A saved transcript would be
// B2-derived state outside the Markdown; "save this chat as a note" would be an explicit
// human-invoked export, and is not MVP (GH #151's open question 2).

import type { SideRow } from "./sidenav.ts";
import type { AnswerView, ChatSetup, ChatTurn, Citation } from "./types";

/**
 * One entry in the transcript. A `user` message is what was typed; an `assistant` message
 * is what came back — with its citations, whether it was stopped mid-answer, and, when the
 * call failed outright, the generic message the host gave instead of an answer.
 *
 * A **failed** turn is still a turn: it stays on screen (the question is still there to
 * retry) but contributes nothing to `chatHistory`, because there is no answer to carry
 * forward as context.
 */
export interface ChatMessage {
  role: "user" | "assistant";
  /** The text — the question, the answer, or "" when `error` says why there isn't one. */
  text: string;
  /** Resolved `[n]` markers; empty for a user message or a failed turn. */
  citations: Citation[];
  /** The stream was stopped: the text is an honest prefix, and the pane says so. */
  cancelled: boolean;
  /** The generic, actionable failure this turn produced instead of an answer. */
  error?: string;
}

/** The question the human typed, as a transcript entry. */
export function userMessage(text: string): ChatMessage {
  return { role: "user", text, citations: [], cancelled: false };
}

/** A finished (or stopped) answer, as a transcript entry. */
export function answerMessage(view: AnswerView): ChatMessage {
  return {
    role: "assistant",
    text: view.answer,
    citations: view.citations,
    cancelled: view.cancelled,
  };
}

/** A turn that failed — the host's generic message stands in for the answer. */
export function errorMessage(error: string): ChatMessage {
  return { role: "assistant", text: "", citations: [], cancelled: false, error };
}

/**
 * The conversation as the next ask should see it (`b2-core`'s `ChatTurn`), oldest first.
 *
 * A **cancelled** answer is included, deliberately and for the reason `b2 chat` includes
 * it: the human read that text, so a follow-up like "go on" has to be condensed against
 * what was actually said, not against a turn we pretend never happened. A **failed** turn
 * is excluded — there is nothing it said.
 */
export function chatHistory(messages: readonly ChatMessage[]): ChatTurn[] {
  return messages
    .filter((m) => m.error === undefined && m.text !== "")
    .map((m) => ({ role: m.role, content: m.text }));
}

/** A transcript row's key — the identity that survives the pane's `innerHTML` swap.
 *  Position-based because two identical questions are two different rows. */
export function turnRowKey(index: number): string {
  return `chat:turn:${index}`;
}

/** A citation row's key: its turn, its marker, and what it points at — the same
 *  belt-and-braces as `cardRowKey` (position makes it unique, target makes a stale key
 *  fail to match rather than resolve to whatever now sits there).
 *
 *  What the row *opens* is not derived from this key: a citation paints as a `data-open`
 *  button, so the mouse and ⏎ share the one activation path the rest of the app uses (K1),
 *  and the note it names lives in the markup rather than in a lookup that could go stale
 *  between the paint and the keystroke. render.test.ts pins that it is a button and never
 *  an `href` — in-app navigation, never the webview (E5). */
export function citationRowKey(turn: number, marker: number, path: string): string {
  return `chat:cite:${turn}:${marker}:${path}`;
}

/** The row key of the answer currently streaming — a row like any other, so the keyboard
 *  can sit on it while it fills, and so focus restoration has something to name. */
export const STREAMING_ROW_KEY = "chat:streaming";

/**
 * Every row the chat pane paints, in paint order: each turn, with its citations nested
 * under it, then the in-flight answer if one is streaming.
 *
 * Emits sidenav.ts's `SideRow` so the discovery pane's walk drives this one unchanged.
 * Nothing here **folds** (`fold: null`): a turn's citations are always shown with it — a
 * conversation is read, not browsed. Folding and having children are different facts,
 * though, and only the second one moves →, so an answer with citations still steps into
 * them and ← still steps back out to the turn (sidenav.ts's `side.row.in` says so).
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
 * Which state the chat pane is in before (or between) questions — the empty-state
 * selection the spec asks for, as one pure decision rather than a chain of `if`s spread
 * through the paint.
 *
 *   no-vault      nothing to ground an answer in
 *   loading       the setup probe hasn't answered yet
 *   no-server     nothing is listening (the daemon isn't running, or the URL is wrong)
 *   no-model      a server is there but doesn't serve the configured model
 *   ready         chat works — the pane shows its prompt, or the conversation
 *
 * `unembedded` is deliberately **not** one of these: a projected-but-unembedded vault
 * still answers, BM25-only (M4), so blocking on it would break a working thing. It is a
 * quiet note beside the composer instead — [`retrievalNote`].
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
    // The fake provider answers deterministically — chat "works", and the pane says
    // what is answering rather than pretending a model is (`b2 chat`'s own note).
    case "fake":
    case "ready":
      return "ready";
  }
}

/**
 * The honest note about *retrieval* under the composer, or "" when there is nothing to
 * say — the search caveat (#26) applied to chat.
 *
 * Chat retrieves through the same hybrid search everything else does, so an unembedded
 * vault answers from keyword matches alone. That is a real difference in answer quality
 * and it must be said, quietly, in place — never as a blocker (E4: chat degrading never
 * degrades anything else, and a degraded chat is still chat).
 */
export function retrievalNote(s: {
  semantic: boolean;
  notesEmbedded: number;
  notesTotal: number;
}): string {
  if (s.notesTotal === 0) return "";
  if (!s.semantic)
    return "Answers are grounded by keyword search only — the embedding model isn’t installed.";
  if (s.notesEmbedded === 0)
    return "Answers are grounded by keyword search for now — this vault isn’t embedded yet.";
  if (s.notesEmbedded < s.notesTotal)
    return `Keyword-first grounding while this vault embeds (${s.notesEmbedded}/${s.notesTotal}).`;
  return "";
}

/** The one command the setup card tells a human to run, spelled here so the label, the
 *  copy button's payload and any message agree (`b2-llm`'s `pull_command` mirror). */
export function pullCommand(model: string): string {
  return `ollama pull ${model}`;
}

/** A model's on-disk size, for the installed list. Whole GB past a gigabyte, one decimal
 *  below — an inventory line, not a measurement. */
export function formatModelSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  const gb = bytes / 1_073_741_824;
  if (gb >= 10) return `${Math.round(gb)} GB`;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.max(1, Math.round(bytes / 1_048_576))} MB`;
}
