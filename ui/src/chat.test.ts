import { strict as assert } from "node:assert";
import test from "node:test";
import {
  type ChatMessage,
  STREAMING_ROW_KEY,
  answerMessage,
  chatEmptyState,
  chatHistory,
  chatRows,
  citationRowKey,
  errorMessage,
  formatModelSize,
  pullCommand,
  retrievalNote,
  turnRowKey,
  userMessage,
} from "./chat.ts";
import { rovingSideKey, sideArrowMove, sideRowIndex } from "./sidenav.ts";
import type { ChatSetup } from "./types.ts";

const setup = (over: Partial<ChatSetup> = {}): ChatSetup => ({
  base_url: "http://localhost:11434/v1",
  model: "llama3.2",
  cloud: false,
  api_key_source: "none",
  state: "ready",
  message: null,
  available: [],
  ollama: null,
  ...over,
});

const answered = (text: string, paths: string[]): ChatMessage =>
  answerMessage({
    answer: text,
    citations: paths.map((path, i) => ({ marker: i + 1, path, excerpt: `from ${path}` })),
    cancelled: false,
  });

/** A two-turn conversation with citations on each answer. */
function conversation(): ChatMessage[] {
  return [
    userMessage("what did I write about memory?"),
    answered("Grounded in [1] and [2].", ["concepts/memory.md", "notes/spaced-repetition.md"]),
    userMessage("tell me more"),
    answered("More, per [1].", ["concepts/memory.md"]),
  ];
}

test("the transcript's rows are the turns, with citations nested under them", () => {
  const rows = chatRows(conversation(), false);
  assert.deepEqual(
    rows.map((r) => [r.key, r.depth]),
    [
      [turnRowKey(0), 0],
      [turnRowKey(1), 0],
      [citationRowKey(1, 1, "concepts/memory.md"), 1],
      [citationRowKey(1, 2, "notes/spaced-repetition.md"), 1],
      [turnRowKey(2), 0],
      [turnRowKey(3), 0],
      [citationRowKey(3, 1, "concepts/memory.md"), 1],
    ],
  );
  // Nothing in a conversation folds — it is read, not browsed.
  assert.ok(rows.every((r) => r.fold === null));
});

test("a streaming answer is a row too, so the keyboard can sit on it while it fills", () => {
  const rows = chatRows([userMessage("q")], true);
  assert.deepEqual(rows.map((r) => r.key), [turnRowKey(0), STREAMING_ROW_KEY]);
  assert.equal(chatRows([userMessage("q")], false).length, 1);
});

test("the pane walks with discovery's own arrows (K1) — one row order, both directions", () => {
  const rows = chatRows(conversation(), false);
  // ↓ from the top walks every row, citations included: a row you can see is a row you
  // can reach.
  let at = -1;
  const visited: string[] = [];
  for (;;) {
    const move = sideArrowMove(rows, at, "side.row.next");
    if (move === null) break;
    visited.push(move.key);
    at = sideRowIndex(rows, move.key);
  }
  assert.deepEqual(visited, rows.map((r) => r.key));

  // ← from a citation steps out to the answer it belongs to (nothing folds, so `out` is
  // purely "step to the parent").
  const cite = sideRowIndex(rows, citationRowKey(1, 2, "notes/spaced-repetition.md"));
  assert.deepEqual(sideArrowMove(rows, cite, "side.row.out"), {
    kind: "focus",
    key: turnRowKey(1),
  });
  // → on a turn row steps into its first citation; on a turn with none it is spent.
  assert.deepEqual(sideArrowMove(rows, sideRowIndex(rows, turnRowKey(1)), "side.row.in"), {
    kind: "focus",
    key: citationRowKey(1, 1, "concepts/memory.md"),
  });
  assert.equal(sideArrowMove(rows, sideRowIndex(rows, turnRowKey(0)), "side.row.in"), null);

  // The roving tabstop: the pane is one Tab stop, and it lands on the remembered row —
  // or the first one when that row is gone (a new conversation).
  assert.equal(rovingSideKey(rows, turnRowKey(2)), turnRowKey(2));
  assert.equal(rovingSideKey(rows, "chat:turn:99"), turnRowKey(0));
  assert.equal(rovingSideKey([], null), null);
});

test("history carries what was actually said — a stopped answer included, a failure not", () => {
  const stopped = answerMessage({ answer: "Half an ans", citations: [], cancelled: true });
  const messages = [
    userMessage("first"),
    stopped,
    userMessage("second"),
    errorMessage("The model server couldn't answer."),
    userMessage("third"),
  ];
  assert.deepEqual(chatHistory(messages), [
    { role: "user", content: "first" },
    // The human read this text, so "go on" must be condensed against it.
    { role: "assistant", content: "Half an ans" },
    { role: "user", content: "second" },
    // The failed turn contributes nothing — there is no answer to carry forward.
    { role: "user", content: "third" },
  ]);
});

test("the empty state is chosen once, from the vault and the probe", () => {
  assert.equal(chatEmptyState({ hasVault: false, setup: null }), "no-vault");
  assert.equal(chatEmptyState({ hasVault: false, setup: setup() }), "no-vault");
  assert.equal(chatEmptyState({ hasVault: true, setup: null }), "loading");
  assert.equal(
    chatEmptyState({ hasVault: true, setup: setup({ state: "unreachable" }) }),
    "no-server",
  );
  assert.equal(
    chatEmptyState({ hasVault: true, setup: setup({ state: "model_missing" }) }),
    "no-model",
  );
  assert.equal(chatEmptyState({ hasVault: true, setup: setup() }), "ready");
  // The fake provider answers, so chat is usable — the pane says *what* is answering
  // rather than blocking on it.
  assert.equal(chatEmptyState({ hasVault: true, setup: setup({ state: "fake" }) }), "ready");
});

test("an unembedded vault is a quiet note, never a blocker (M4)", () => {
  // Nothing indexed yet: the tree's own empty state covers it, so chat says nothing.
  assert.equal(retrievalNote({ semantic: true, notesEmbedded: 0, notesTotal: 0 }), "");
  // No model installed at all.
  assert.match(
    retrievalNote({ semantic: false, notesEmbedded: 0, notesTotal: 12 }),
    /keyword search only/,
  );
  // A model, nothing embedded yet.
  assert.match(
    retrievalNote({ semantic: true, notesEmbedded: 0, notesTotal: 12 }),
    /keyword search for now/,
  );
  // Part way through embedding — say how far.
  assert.match(
    retrievalNote({ semantic: true, notesEmbedded: 7, notesTotal: 12 }),
    /Keyword-first grounding while this vault embeds \(7\/12\)/,
  );
  // Fully embedded: nothing to caveat.
  assert.equal(retrievalNote({ semantic: true, notesEmbedded: 12, notesTotal: 12 }), "");
});

test("the pull command is spelled in one place", () => {
  assert.equal(pullCommand("llama3.1:8b"), "ollama pull llama3.1:8b");
});

test("a model's size reads as an inventory line", () => {
  assert.equal(formatModelSize(2_019_393_189), "1.9 GB");
  assert.equal(formatModelSize(21_474_836_480), "20 GB");
  assert.equal(formatModelSize(500_000_000), "477 MB");
  assert.equal(formatModelSize(0), "");
});
