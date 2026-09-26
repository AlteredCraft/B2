// The coverage classifier (coverage.ts), and the five surfaces that speak from it, pinned.
//
// The surfaces are pinned by what they *say* at each tier rather than by how they ask,
// because they deliberately ask in different orders (coverage.ts's header): chat is
// silent about an empty vault even with no model, search names the missing model first.

import { strict as assert } from "node:assert";
import test from "node:test";
import { JSDOM } from "jsdom";
import { coverage } from "./coverage.ts";
import { retrievalNote } from "./chat.ts";
import { modalHtml, notePaneHtml, sidePaneHtml } from "./render.ts";
import { state, type AppState } from "./state.ts";
import type { NoteView } from "./types.ts";

// The note pane renders a body through the sanitizer, which needs a DOM (render.test.ts).
(globalThis as unknown as { window: unknown }).window = new JSDOM("").window;

const cov = (semantic: boolean, notesEmbedded: number, notesTotal: number) => ({
  semantic,
  notesEmbedded,
  notesTotal,
});

test("the tiers fall where the numbers say", () => {
  assert.equal(coverage(cov(true, 0, 0)).embedded, "empty");
  assert.equal(coverage(cov(true, 0, 5)).embedded, "none");
  assert.equal(coverage(cov(true, 3, 5)).embedded, "partial");
  assert.equal(coverage(cov(true, 5, 5)).embedded, "all");
  assert.equal(coverage(cov(true, 6, 5)).embedded, "all", "more embedded than listed reads as all");
  assert.equal(coverage(cov(true, 2, 0)).embedded, "empty", "no notes is empty, whatever the count says");
});

test("the model is its own fact, never folded into a tier", () => {
  const c = coverage(cov(false, 3, 5));
  assert.equal(c.model, false);
  assert.equal(c.embedded, "partial");
  assert.deepEqual([c.n, c.m], [3, 5]);
});

// --- the surfaces --------------------------------------------------------------------

const note = {
  path: "notes/a.md",
  title: "a",
  type: null,
  created: null,
  updated: null,
  tags: [],
  body: "hello",
  frontmatter: null,
  frontmatter_readable: true,
  revision: "r0",
} as NoteView;

const app = (semantic: boolean, n: number, m: number, over: Partial<AppState> = {}): AppState =>
  ({ ...state, vaultRoot: "/v", semantic, notesEmbedded: n, notesTotal: m, ...over }) as AppState;

/** The search pane's caveat line, as painted. */
const caveat = (semantic: boolean, n: number, m: number): string => {
  const html = sidePaneHtml(app(semantic, n, m, { searchQuery: "q", searchResults: [] }));
  return /<p class="side-sub">for “q”(.*?)<\/p>/s.exec(html)?.[1] ?? "(no sub line)";
};

test("search: the missing model first, then the fraction; silent when semantic is live", () => {
  assert.equal(caveat(false, 0, 0), " · keyword only (run <code>b2 init</code> for semantic)");
  assert.equal(caveat(true, 0, 0), "");
  assert.equal(caveat(true, 0, 5), " · keyword-only for now (0/5 embedded — Reindex)");
  assert.equal(caveat(true, 3, 5), " · keyword-first (3/5 embedded)");
  assert.equal(caveat(true, 5, 5), "");
});

test("chat: silent about an empty vault, even with no model", () => {
  assert.equal(retrievalNote(cov(false, 0, 0)), "");
  assert.equal(
    retrievalNote(cov(false, 0, 5)),
    "Answers are grounded by keyword search only — the embedding model isn’t installed.",
  );
  assert.equal(
    retrievalNote(cov(true, 0, 5)),
    "Answers are grounded by keyword search for now — this vault isn’t embedded yet.",
  );
  assert.equal(
    retrievalNote(cov(true, 3, 5)),
    "Keyword-first grounding — 3/5 notes embedded. Reindex to fill the rest.",
  );
  assert.equal(retrievalNote(cov(true, 5, 5)), "");
});

test("the graph: a ghost hint for no model or an unfinished vault, else nothing", () => {
  const hint = (semantic: boolean, n: number, m: number): string => {
    const html = notePaneHtml(app(semantic, n, m, { current: note, graphOpen: true, similar: [] }));
    return /<div class="graph-hint">(.*?)<\/div>/s.exec(html)?.[1] ?? "";
  };
  assert.ok(hint(false, 5, 5).startsWith("ghost connections need the semantic model"));
  assert.equal(hint(true, 0, 5), "ghosts appear once the vault is embedded — Reindex");
  assert.equal(hint(true, 3, 5), "ghosts appear once the vault is embedded — Reindex");
  assert.equal(hint(true, 5, 5), "");
  assert.equal(hint(true, 0, 0), "");
});

test("the Similar section: its empty state names only the missing model", () => {
  const empty = (semantic: boolean) =>
    sidePaneHtml(app(semantic, 0, 5, { current: note, similar: [], discoveringSimilar: false }));
  assert.ok(empty(false).includes("Semantic similarity is off"));
  assert.ok(!empty(true).includes("Semantic similarity is off"));
  assert.ok(empty(true).includes("Nothing unlinked has stored vectors to compare"));
});

test("Settings → Index: indexed and embedded are two different states", () => {
  const line = (semantic: boolean, n: number, m: number, vaultRoot: string | null = "/v") => {
    const html = modalHtml(app(semantic, n, m, { vaultRoot, settingsOpen: true, settingsTab: "index" }));
    return /<p class="settings-coverage">(.*?)<\/p>/s.exec(html)?.[1] ?? "(none)";
  };
  assert.equal(line(true, 0, 0, null), "No vault is open.");
  assert.equal(line(false, 0, 0), "Nothing indexed yet — B2 indexes a vault when you open it.");
  assert.equal(
    line(false, 0, 1),
    "1 note indexed for keyword search. The embedding model isn’t installed, so none are embedded.",
  );
  assert.equal(line(true, 5, 5), "5 notes indexed, all embedded.");
  assert.equal(line(true, 3, 5), "5 notes indexed · 3/5 embedded.");
  assert.equal(line(true, 0, 5), "5 notes indexed · 0/5 embedded.");
});
