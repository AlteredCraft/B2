// The paint's half of the focus contract (K1, GH #91): main.ts swaps panes in wholesale, so
// focus is restored by an identity carried in the markup (a graph node's scene id, else an
// `id`). These cases pin that the paint emits it, plus other markup contracts.

import { JSDOM } from "jsdom";
import { buildScene } from "./graph.ts";
import { contextMenuHtml, modalHtml, notePaneHtml, sidePaneHtml } from "./render.ts";
import { chordProblems } from "./keymap.ts";
import { STRENGTH_MIN_CANDIDATES } from "./strength.ts";
import { PROBE_AFTER_MS, silenceHint } from "./recorder.ts";
import { state, type AppState } from "./state.ts";
import type {
  ChatSetup,
  NeighborView,
  NoteView,
  OllamaModel,
  OllamaSetup,
  ResourceLink,
  EvidencedResult,
  SimilarExplainView,
  SimilarView,
  UnresolvedLink,
} from "./types.ts";

// The render seam sanitizes with a DOM parser (E5, sanitize.ts), so the pane needs a DOM.
(globalThis as unknown as { window: unknown }).window = new JSDOM("").window;

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: string, expected: string, msg: string): void {
  assert(actual === expected, `${msg} — expected ${expected}, got ${actual}`);
}
function assertEq(actual: unknown, expected: unknown, msg: string): void {
  const [a, b] = [JSON.stringify(actual), JSON.stringify(expected)];
  if (a !== b) throw new Error(`assertion failed: ${msg}
  actual:   ${a}
  expected: ${b}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- fixtures -----------------------------------------------------------------------

function note(over: Partial<NoteView> = {}): NoteView {
  return {
    path: "notes/anchor.md",
    title: "anchor",
    type: null,
    created: null,
    updated: null,
    tags: [],
    body: "",
    frontmatter: null,
    frontmatter_readable: true,
    revision: "r0",
    ...over,
  };
}

function neighbor(over: Partial<NeighborView> = {}): NeighborView {
  return {
    path: "notes/edge.md",
    title: "edge",
    relation: "supports",
    direction: "outbound",
    label: "supports",
    explanation: null,
    origin: "inline",
    created: null,
    ...over,
  };
}

function ghost(over: Partial<SimilarView> = {}): SimilarView {
  return { path: "notes/ghost.md", title: "ghost", score: 0.42, evidence: "", ...over };
}

function resourceLink(over: Partial<ResourceLink> = {}): ResourceLink {
  return {
    // A quote is legal in a path, so in a node id; it rides through the markup escaped.
    path: 'assets/a "quoted".png',
    class: "image",
    relation: "references",
    origin: "inline",
    caption: null,
    embed: false,
    explanation: null,
    ...over,
  };
}

const dangling = (target: string): UnresolvedLink => ({
  target,
  relation: "references",
  origin: "inline",
  explanation: null,
});

const hit = (path: string): EvidencedResult => ({
  path,
  title: path,
  score: 1,
  snippet: "",
  bm25_rank: 0,
  vector_rank: 0,
  cos: 0.7,
});

/** A ready local chat provider. */
function chatSetup(over: Partial<ChatSetup> = {}): ChatSetup {
  return {
    base_url: "http://localhost:11434/v1",
    model: "llama3.2",
    cloud: false,
    api_key_source: "none",
    state: "ready",
    message: null,
    available: [],
    ollama: null,
    tool_calls: { in_force: 64, default: 64, ceiling: 4096 },
    ...over,
  };
}

/** A fresh `AppState` over the defaults, with its own collections so cases can't leak. */
function app(over: Partial<AppState> = {}): AppState {
  return {
    ...state,
    expandedDirs: new Set(),
    collapsedSections: new Set(),
    collapsedCards: new Set(),
    ...over,
  };
}

/** The opening tag carrying `marker`, to check a control has an id without pinning it. */
function tagWith(html: string, marker: string): string {
  const at = html.indexOf(marker);
  assert(at !== -1, `the markup contains ${marker}`);
  const start = html.lastIndexOf("<", at);
  const end = html.indexOf(">", at);
  assert(start !== -1 && end !== -1, `${marker} sits inside a tag`);
  return html.slice(start, end + 1);
}

const hasId = (tag: string): boolean => /\sid="[^"]+"/.test(tag);

/** Decodes an escaped attribute value, as the browser's `dataset` would. */
function decode(s: string): string {
  return s
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&");
}

const sorted = (xs: string[]): string => [...xs].sort().join("|");

// --- the graph's nodes ----------------------------------------------------------------

check("every activatable graph node carries its scene id", () => {
  // `paintNote` re-finds a node by its scene id, a pure function of this same state.
  const s = app({
    current: note(),
    graphOpen: true,
    connections: [neighbor()],
    resourceLinks: [resourceLink()],
    unresolved: [dangling("Hermes")],
    similar: [ghost()],
  });
  const html = notePaneHtml(s);
  const painted = [...html.matchAll(/data-gnode="([^"]*)"/g)].map((m) => decode(m[1]));
  const scene = buildScene({
    anchor: { path: s.current!.path, title: s.current!.title },
    connections: s.connections,
    resources: s.resourceLinks,
    unresolved: s.unresolved,
    ghosts: s.similar,
  });
  const activatable = scene.nodes.filter((n) => n.kind !== "dangling").map((n) => n.id);
  equal(sorted(painted), sorted(activatable), "the painted ids are the scene's activatable ids");
  assert(
    activatable.length === 4,
    "the fixture covers all four activatable kinds (anchor, note, resource, ghost)",
  );
  assert(
    !painted.some((id) => id.startsWith("dangling:")),
    "a dangling node opens nothing, so it is neither a tab stop nor a focus target",
  );
  assert(
    painted.includes('res:assets/a "quoted".png'),
    "a node id round-trips through the markup escaped — a path may contain anything",
  );
});

// --- the note pane's chrome -------------------------------------------------------------

check("the note pane's chrome carries an id, in reading and in graph mode", () => {
  // Toggling the drawer, source or graph repaints the pane, destroying the pressed chip.
  const reading = notePaneHtml(app({ current: note(), frontmatterOpen: true }));
  for (const marker of [
    "data-toggle-frontmatter",
    "data-toggle-source",
    "data-toggle-graph",
    "data-toggle-edit",
    "data-fm-edit",
  ]) {
    assert(hasId(tagWith(reading, marker)), `the reading bar's ${marker} control carries an id`);
  }
  const graph = notePaneHtml(app({ current: note(), graphOpen: true }));
  for (const marker of ["data-toggle-graph", "data-toggle-edit"]) {
    assert(hasId(tagWith(graph, marker)), `the graph bar's ${marker} control carries an id`);
  }
});

// --- the side pane's chrome -------------------------------------------------------------

// D2 (GH #202): the empty state says why it is empty.
check("an unvouched query names the query back and serves no rows", () => {
  const html = sidePaneHtml(
    app({ searchQuery: "Fasdfadsf", searchResults: [], searchVouched: false }),
  );
  assert(html.includes("No matches."), `says no matches: ${html}`);
  assert(html.includes("Fasdfadsf"), `names the query back: ${html}`);
  assert(!html.includes("data-side-row"), `and paints no row at all: ${html}`);
});

check("a null verdict is no verdict: the plain empty state, never “no matches here”", () => {
  // The fake embedder and uncalibrated models land here (M2): nothing judged the query.
  const html = sidePaneHtml(app({ searchQuery: "kivo", searchResults: [], searchVouched: null }));
  assert(html.includes("No matches."), `still an empty state: ${html}`);
  assert(!html.includes("Nothing in this vault"), `but claims nothing about it: ${html}`);
});

check("search mode's clear button carries an id — the one focusable that isn't a row", () => {
  const html = sidePaneHtml(app({ searchQuery: "kivo", searchResults: [hit("notes/kivo.md")] }));
  assert(hasId(tagWith(html, "data-clear-search")), "clear carries an id");
  assert(
    /data-side-row="[^"]+"/.test(tagWith(html, "notes/kivo.md")),
    "a result row still carries its row key (what paintSide restores rows by)",
  );
});

// The menu bar's accelerators left the Keyboard panel (#119): macOS prints them itself.
check("the Keyboard panel is B2's own chords, not the menu bar's", () => {
  const html = modalHtml(app({ settingsOpen: true, settingsTab: "keyboard" }));
  assert(html.includes("Getting around"), "B2's groups are there");
  assert(!html.includes("The menu bar"), "and the menu bar's group is not");
  assert(!html.includes("Quit B2"), "nor any of its rows");
});

// Chords are edited here (#121): a `<button>` chip says "B2 can move this", a `<kbd>` can't.
check("a movable chord paints as a button, and everything else as plain text", () => {
  const html = modalHtml(app({ settingsOpen: true, settingsTab: "keyboard" }));
  assert(html.includes('data-rebind="find.open"'), "⌘F is B2's, and B2 can move it");
  const ids = [...html.matchAll(/id="(keys-chip-[^"]+)"/g)].map((m) => m[1]);
  assert(ids.length > 20, "the sheet is mostly chips");
  // Ids must be unique though `menu.open` (⇧F10) appears in two rows.
  assertEq(new Set(ids).size, ids.length, "no two chips share an id");
  assertEq(ids.filter((id) => id.endsWith("-menu.open")).length, 2, "and ⇧F10 really is in two rows");
  // `graph.activate` is fixed (a node stands in for a button) but still listed, as text.
  assert(!html.includes('data-rebind="graph.activate"'), "⏎ on a graph node is not B2's to hand out");
  // The platform's own keys have no command behind them at all.
  assert(html.includes("Jump to the next row starting with that letter"), "typeahead has a row");
  assert(!html.includes('data-rebind="A–Z"'), "and nothing to rebind");
});

check("a rebound chord is marked, and offers to be put back", () => {
  const html = modalHtml(
    app({ settingsOpen: true, settingsTab: "keyboard", keyOverrides: { "find.open": ["Mod-Alt-f"] } }),
  );
  assert(html.includes("kbd-changed"), "the chip says it has moved");
  assert(html.includes("Reset all (1)"), "and the count is the number of moved commands");
  assert(!modalHtml(app({ settingsOpen: true, settingsTab: "keyboard" })).includes("Reset all"), "no reset with nothing to reset");
});

check("the recorder shows what it is rebinding, and refuses to save a refused chord", () => {
  // A refusal disables Save; an advisory is said and Save stays live (keymap.ts).
  const open = (candidate: string) =>
    modalHtml(
      app({
        settingsOpen: true,
        settingsTab: "keyboard",
        recorder: {
          id: "edit.toggle",
          candidate,
          problems: chordProblems("edit.toggle", candidate),
          hint: null,
          blurred: false,
        },
      }),
    );
  const clash = open("Mod-w");
  assert(clash.includes("Enter or leave edit mode"), "the recorder names its command");
  assert(clash.includes("Close Window"), "and names what took the chord");
  assert(clash.includes('id="keys-save" disabled'), "a refused chord cannot be saved");
  const fine = open("Mod-Alt-Shift-e");
  assert(!fine.includes('id="keys-save" disabled'), "a free chord can");
  assert(fine.includes("kbd-recording"), "and the chip being edited is marked in the table");
});

check("the recorder says out loud when nothing has reached B2", () => {
  // Without this line, macOS taking the chord would look like a broken recorder.
  const html = modalHtml(
    app({
      settingsOpen: true,
      settingsTab: "keyboard",
      recorder: {
        id: "edit.toggle",
        candidate: null,
        problems: [],
        hint: silenceHint({ elapsedMs: PROBE_AFTER_MS, blurred: false }),
        blurred: false,
      },
    }),
  );
  assert(html.includes("Press a chord"), "still waiting");
  assert(html.includes("claimed it first"), "and saying why that might be");
});

// main.ts's `overlayFocusables` scopes the trap by `[role="dialog"]`, and
// `focusIntoOverlay` opens Settings on the first focusable, meant to be the selected tab.
check("the Settings surface is a dialog whose first focusable is the selected tab", () => {
  const html = modalHtml(app({ settingsOpen: true, settingsTab: "chat" }));
  assert(tagWith(html, "settings-screen").includes('role="dialog"'), "the trap has a scope");
  assert(tagWith(html, "settings-screen").includes('aria-modal="true"'), "and it is modal");
  assert(!html.includes("modal-backdrop"), "no backdrop: there is no outside to click");
  // Order matters: nothing focusable may precede the rail's selected tab.
  const rail = html.indexOf('id="settings-tab-chat"');
  assert(rail !== -1 && rail < html.indexOf('id="settings-panel"'), "rail before panel");
  assert(html.indexOf('id="settings-panel"') < html.indexOf('id="settings-done"'), "Done last");
});

// main.ts's click delegation and `captureModalFocus` both find Reindex by its `id`.
check("the Index panel's Reindex button carries the id both halves restore by", () => {
  const html = modalHtml(app({ settingsOpen: true, settingsTab: "index", vaultRoot: "/v" }));
  assert(tagWith(html, ">Reindex<").includes('id="reindex"'), "the button is identified");
});

// Both states come from the predicates main.ts repaints with, keeping the two paints in step.
check("Reindex is disabled and renamed while a run is live", () => {
  const idle = modalHtml(app({ settingsOpen: true, settingsTab: "index", vaultRoot: "/v" }));
  assert(!tagWith(idle, ">Reindex<").includes("disabled"), "pressable with no run going");
  const busy = modalHtml(
    app({ settingsOpen: true, settingsTab: "index", vaultRoot: "/v", reindexing: true }),
  );
  assert(tagWith(busy, ">Indexing…<").includes("disabled"), "refused mid-run, and says so");
  const novault = modalHtml(app({ settingsOpen: true, settingsTab: "index", vaultRoot: null }));
  assert(tagWith(novault, ">Reindex<").includes("disabled"), "nothing to reindex with no vault");
});

// Settings covers the top bar, so the panel carries its own meter (the classes
// `paintReindex` writes into) and Cancel.
check("the Index panel carries the live meter itself, since the top bar is behind it", () => {
  const busy = modalHtml(
    app({ settingsOpen: true, settingsTab: "index", vaultRoot: "/v", reindexing: true }),
  );
  assert(busy.includes('class="reindex-progress"'), "the meter is in the panel");
  assert(busy.includes('class="reindex-label"'), "with the element paintReindex writes into");
  const cancel = tagWith(busy, "data-cancel-reindex");
  assert(cancel.includes('id="settings-cancel-reindex"'), "Cancel is identified for the repaint");
  assert(!busy.includes("top bar"), "and nothing points at chrome this surface covers");
  const idle = modalHtml(app({ settingsOpen: true, settingsTab: "index", vaultRoot: "/v" }));
  assert(!idle.includes("reindex-progress"), "no run, no meter");
});

// #26: a projected but unembedded vault must never read as done.
check("the Index panel counts indexed and embedded separately", () => {
  const partial = modalHtml(
    app({
      settingsOpen: true,
      settingsTab: "index",
      vaultRoot: "/v",
      notesTotal: 10,
      notesEmbedded: 4,
    }),
  );
  assert(partial.includes("4/10 embedded"), "a partial embed reports the gap");
  const done = modalHtml(
    app({
      settingsOpen: true,
      settingsTab: "index",
      vaultRoot: "/v",
      notesTotal: 10,
      notesEmbedded: 10,
    }),
  );
  assert(done.includes("all embedded"), "a finished index says so plainly");
  assert(!done.includes("10/10"), "and doesn't make you read a fraction to find out");
  const keyword = modalHtml(
    app({
      settingsOpen: true,
      settingsTab: "index",
      vaultRoot: "/v",
      semantic: false,
      notesTotal: 10,
      notesEmbedded: 0,
    }),
  );
  assert(keyword.includes("isn’t installed"), "no model names the reason none are embedded");
  const fresh = modalHtml(
    app({ settingsOpen: true, settingsTab: "index", vaultRoot: "/v", notesTotal: 0 }),
  );
  assert(fresh.includes("Nothing indexed yet"), "an unindexed vault is not a complete one");
});

// --- Settings → Chat: where the cloud API key lives ----------------------------------
//
// The key has four sources (GH #176), and which one is in force exists only as copy. These
// pin that each reaches the screen as a different sentence.

const chatTab = (over: Partial<ChatSetup>): string =>
  modalHtml(
    app({
      settingsOpen: true,
      settingsTab: "chat",
      chatCloud: true,
      chatSetup: chatSetup({ base_url: "https://api.example.com/v1", cloud: true, ...over }),
    }),
  );

check("the cloud key field says which key is in force, and never shows one", () => {
  const stored = chatTab({ api_key_source: "stored" });
  assert(stored.includes("Keychain"), "a remembered key says where it is remembered");
  assert(stored.includes("data-chat-clear-key"), "and can be removed");

  // The environment wins, so a key typed here would be stored and not used.
  const env = chatTab({ api_key_source: "environment" });
  assert(env.includes("B2_LLM_API_KEY"), "the overriding variable is named");
  assert(env.includes("overrides"), `the override is stated, not implied: ${env}`);

  // The Keychain refused: the key is gone at quit, so the general Keychain claim must not
  // appear.
  const session = chatTab({ api_key_source: "session" });
  assert(session.includes("this session only"), "a session-only key says so");
  assert(session.includes("couldn’t save it"), `and says why: ${session}`);
  assert(
    !session.includes("B2 saves the key in your macOS Keychain"),
    `and never also claims it was saved: ${session}`,
  );
  assert(session.includes("not</strong> saved"), `it says the opposite, plainly: ${session}`);
  assert(
    stored.includes("B2 saves the key in your macOS Keychain"),
    "a stored key still gets the Keychain sentence",
  );

  const none = chatTab({ api_key_source: "none" });
  assert(!none.includes("data-chat-clear-key"), "no key, no Remove button");
  assert(!none.includes("Keychain — encrypted"), "and no claim one is saved");
});

check("the cloud section says where an endpoint can come from, since B2 names none", () => {
  // B2 ships no default cloud provider (M5), so the panel answers "where do I get one?"
  // with a link.
  const html = chatTab({ api_key_source: "none" });
  assert(html.includes("https://docs.ollama.com/cloud"), `the cloud docs are linked: ${html}`);
  assert(
    html.includes("Any OpenAI-compatible provider works"),
    "and the link is an example, not a requirement",
  );
  // Not beside the Local copy's "nothing leaves this machine".
  const local = modalHtml(
    app({ settingsOpen: true, settingsTab: "chat", chatCloud: false, chatSetup: chatSetup() }),
  );
  assert(!local.includes("docs.ollama.com/cloud"), "the local section stays local");
});

// --- Settings → Chat: the Model field (a picker over what is actually installed) --------
//
// A picker over the daemon's inventory when it has answered, a text box otherwise; these
// pin that each shape can express what the other can.

/** Settings → Chat against a local Ollama endpoint; the daemon answers unless overridden. */
const localChatTab = (
  over: Partial<ChatSetup>,
  ollama: Partial<OllamaSetup> = {},
  stateOver: Partial<AppState> = {},
): string =>
  modalHtml(
    app({
      settingsOpen: true,
      settingsTab: "chat",
      chatCloud: false,
      chatSetup: chatSetup({
        ollama: {
          root: "http://localhost:11434",
          running: true,
          installed: [],
          ram_gb: 16,
          tiers: [{ min_ram_gb: 16, ram: "16 GB", size: "7–8B", model: "llama3.1:8b" }],
          suggested: { min_ram_gb: 16, ram: "16 GB", size: "7–8B", model: "llama3.1:8b" },
          ...ollama,
        },
        ...over,
      }),
      ...stateOver,
    }),
  );

const model = (name: string, size = 2_019_393_189): OllamaModel => ({
  name,
  size,
  parameters: "3.2B",
});

check("an answering daemon turns the Model field into a picker over what it has", () => {
  const html = localChatTab(
    { model: "llama3.2:latest" },
    { installed: [model("llama3.2:latest"), model("gemma3:12b")] },
  );
  const tag = tagWith(html, 'id="settings-chat-model"');
  assert(tag.startsWith("<select"), `the field is a picker, not ${tag}`);
  // saveChatConfig reads `.value` by this id and captureModalFocus restores by it.
  assert(html.includes('<option value="gemma3:12b"'), "every installed model is offered");
  assert(
    html.includes('<option value="llama3.2:latest" selected>'),
    `and the configured one is what's selected: ${html}`,
  );
  // A way out: the list can't contain a model still being pulled.
  assert(html.includes("data-chat-model-custom"), "a typed name stays reachable");
});

check("a configured model the daemon doesn't have still leads the picker", () => {
  // Otherwise the field shows the first model, and merely opening Settings re-points it.
  const html = localChatTab(
    { model: "gemma4:latest", state: "model_missing" },
    { installed: [model("llama3.2:latest")] },
  );
  assert(
    html.includes('<option value="gemma4:latest" selected>gemma4:latest — not installed'),
    `the configured model is present and marked: ${html}`,
  );
  // One control for one value: the card doesn't repeat the inventory.
  assert(!html.includes("data-chat-use-model"), `no duplicate picker in Settings: ${html}`);
});

check("Cloud models never inherits the local daemon's picker", () => {
  // After pressing *Cloud models* nothing re-probes, so `setup` is still the local one; the
  // picker must not key off it.
  const html = localChatTab(
    { model: "llama3.2:latest" },
    { installed: [model("llama3.2:latest")] },
    { chatCloud: true },
  );
  assert(tagWith(html, 'id="settings-chat-model"').startsWith("<input"), "a box, not a picker");
  assert(!html.includes("data-chat-model-pick"), "and no offer to go back to a local list");
});

check("the typed shape is a text box, and offers the way back", () => {
  const html = localChatTab(
    { model: "llama3.2:latest" },
    { installed: [model("llama3.2:latest")] },
    { chatModelTyped: true },
  );
  const tag = tagWith(html, 'id="settings-chat-model"');
  assert(tag.startsWith("<input"), `the field is a text box, not ${tag}`);
  assert(tag.includes('value="llama3.2:latest"'), "carrying the configured model");
  assert(html.includes("data-chat-model-pick"), "with the picker one press away");
});

check("no inventory means the text box, with nothing to go back to", () => {
  // No daemon: a text box, and no button back to an empty picker.
  const html = localChatTab({ state: "unreachable" }, { running: false });
  assert(tagWith(html, 'id="settings-chat-model"').startsWith("<input"), "a box, not a picker");
  assert(!html.includes("data-chat-model-pick"), "and no way to a list that doesn't exist");
});

check("the local section names the pull command and links the quickstart", () => {
  const html = localChatTab(
    { model: "llama3.2:latest" },
    { installed: [model("llama3.2:latest")] },
  );
  assert(html.includes("ollama pull &lt;model-name&gt;"), `the command's shape, escaped: ${html}`);
  assert(html.includes("ollama pull llama3.1:8b"), "made concrete by this machine's rung");
  assert(html.includes("https://docs.ollama.com/quickstart"), "and the page that has both");
});

check("nothing listening is a card with a fix, not an error", () => {
  // No server at all is a status, not a failure (b2-llm's setup.rs), with two ways out.
  const html = localChatTab(
    {
      state: "unreachable",
      message:
        "Can't reach the model server at http://localhost:11434/v1 — is Ollama running? " +
        "(`ollama serve`, or install: https://docs.ollama.com/quickstart)",
    },
    { running: false },
  );
  assert(html.includes("is Ollama running?"), "the actionable sentence is on screen");
  assert(html.includes("<code>ollama serve</code>"), "with the command that fixes the common case");
  assert(
    html.includes(`<a href="https://docs.ollama.com/quickstart"`),
    `and an openable link for the other one: ${html}`,
  );
  // The fields stay, so the endpoint can be pointed somewhere that is running.
  assert(html.includes('id="settings-chat-save"'), "and Save and test is still there");
  // Exactly once: in Settings the card yields the suggested pull to the Local section.
  assertEq(html.split("ollama pull llama3.1:8b").length - 1, 1, "the pull command is said once");
});

check("a running daemon behind a wrong path is never told to start itself", () => {
  // An endpoint typo (`…/v1X`) 404s with the daemon up, so the card must not say "start
  // it"; it keys that advice off `running` (b2-llm's `refusal_message`).
  const html = localChatTab(
    {
      state: "unreachable",
      base_url: "http://localhost:11434/v1X",
      message:
        "Something is running at http://localhost:11434/v1X, but it isn't an " +
        "OpenAI-compatible API (HTTP 404). Check the endpoint path — it usually ends in " +
        "`/v1`. The Ollama daemon at http://localhost:11434 is running — try " +
        "http://localhost:11434/v1.",
    },
    { installed: [model("llama3.2:latest")] },
  );
  assert(html.includes("isn’t an OpenAI-compatible API") || html.includes("HTTP 404"), "the fix");
  assert(!html.includes("<code>ollama serve</code>"), `nothing to start: ${html}`);
  // Nor "if it isn't installed yet" (the Local section's `ollama pull` note is different).
  assert(!html.includes("isn’t installed yet"), `and nothing to install: ${html}`);
  // The inventory still answered, so the picker stays.
  assert(tagWith(html, 'id="settings-chat-model"').startsWith("<select"), "the picker stands");
});

// --- the tree's context menu ---------------------------------------------------------
//
// The menu is the only home of Rename / Move… / the copy paths (K1). main.ts finds items by
// `data-ctx-*` and traps focus over `.context-item`, so each item needs both.

check("a tree row's menu offers both of the row's paths, as reachable items", () => {
  const html = contextMenuHtml(
    app({
      contextMenu: {
        kind: "tree",
        x: 10,
        y: 20,
        dir: "projects",
        node: { path: "projects/idea.md", nodeKind: "note", label: "idea.md" },
      },
    }),
  );
  for (const attr of ["data-ctx-copy-vault-path", "data-ctx-copy-system-path"]) {
    const tag = tagWith(html, attr);
    assert(tag.includes("context-item"), `${attr} is a menu item the trap collects`);
    assert(tag.includes('role="menuitem"'), `${attr} announces itself as one`);
  }
  assert(html.includes("Copy vault path"), "the vault-relative path is named as such");
  assert(html.includes("Copy system path"), "and the absolute one as such");
  // Delete stays last in the node group.
  assert(
    html.indexOf("data-ctx-copy-system-path") < html.indexOf("data-ctx-delete"),
    "the destructive item is still the group's last",
  );
});

check("the folder-context menu offers no path to copy", () => {
  // No row under the cursor: no node, so no copy items.
  const html = contextMenuHtml(
    app({ contextMenu: { kind: "tree", x: 10, y: 20, dir: "projects", node: null } }),
  );
  assert(!html.includes("data-ctx-copy"), "no copy item without a node");
  assert(html.includes("data-ctx-new-note"), "but the create pair is still there");
});

// --- discovery strength (GH #150) -------------------------------------------------------

check("a graded card carries its σ figure, not only a hover tooltip", () => {
  // The number is in the markup, not only `title=`, so focus can reveal it (K1).
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost({ z: 2.13 })] }),
  );
  assert(html.includes("●●○"), "the band still grades at a glance");
  assert(html.includes("2.1σ"), "and the figure is on the card, awaiting selection");
});

check("the accessible name carries the figure too, not the band alone", () => {
  // The label carries the number; the figure's own span is aria-hidden so it is announced
  // once (PR #184).
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost({ z: 2.13 })] }),
  );
  assert(
    html.includes('aria-label="clear match, 2.1σ"'),
    "the band and its figure reach AT together",
  );
  assert(
    html.includes('class="card-sigma" aria-hidden="true"'),
    "and the visible figure is not announced a second time",
  );
});

check("an ungraded list says so rather than implying a judgement B2 didn't make", () => {
  // A vault too small for statistics returns no z; bare cards would read as all low.
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost(), ghost({ path: "b.md" })] }),
  );
  assert(html.includes("Ungraded"), "the pane admits it didn't grade these");
  assert(!html.includes("●"), "and claims no band, since no statistic was computed");
  // The bar is named as a number.
  assert(
    html.includes(`${STRENGTH_MIN_CANDIDATES} or more notes`),
    "and names what 'enough' is",
  );
  // A notice box, not body prose and not a failure (PR #184).
  assert(html.includes('class="side-note"'), "painted as a notice");
});

check("a graded list carries no ungraded caveat", () => {
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost({ z: 3.1 })] }),
  );
  assert(!html.includes("Ungraded"), "nothing to admit — the floor judged this list");
});

check("an empty pane never claims nothing relates — only that there was nothing to compare", () => {
  // The ranked list is always served (GH #197), so empty means no candidates, never a
  // judgement that nothing relates (GH #196).
  const html = sidePaneHtml(app({ current: note(), semantic: true, similar: [] }));
  assert(
    html.includes("Nothing unlinked has stored vectors to compare"),
    "the empty state states a fact about the candidate set",
  );
  assert(!html.includes("stands out"), "and claims no knowledge of what relates");
  assert(!html.includes("Show nearest anyway"), "no escape hatch — there is no gate to escape");
});

// --- chat (flow ④, GH #155) ------------------------------------------------------------

check("a citation navigates in-app — a button with a path, never a link with an href", () => {
  // E5: the webview is the app, so a citation is a `data-open` button, never a followable
  // link.
  const html = sidePaneHtml(
    app({
      chatOpen: true,
      vaultRoot: "/vault",
      chatSetup: chatSetup(),
      chatMessages: [
        { role: "user", text: "what about memory?", citations: [], cancelled: false },
        {
          role: "assistant",
          text: "Grounded in [1].",
          citations: [{ marker: 1, path: "concepts/memory.md", excerpt: "spacing works" }],
          cancelled: false,
        },
      ],
    }),
  );
  const tag = tagWith(html, 'data-open="concepts/memory.md"');
  assert(tag.startsWith("<button"), `a citation is a button, not ${tag}`);
  assert(!html.includes("href="), "nothing in the transcript is a followable link");
  // A row of the pane's tree, with the identity `paintSide` restores focus by.
  assert(tag.includes('role="treeitem"'), "a citation is a navigable row");
  assert(tag.includes("data-side-row="), "carrying the key focus is restored by");
});

check("a streaming answer paints as escaped text under the id tokens land in", () => {
  // main.ts writes tokens into `#chat-stream` as `textContent`; the first paint escapes the
  // partial text likewise.
  const html = sidePaneHtml(
    app({
      chatOpen: true,
      vaultRoot: "/vault",
      chatSetup: chatSetup(),
      chatStreaming: "<script>alert(1)</script> partial",
    }),
  );
  assert(html.includes('id="chat-stream"'), "the stream has the id main.ts writes into");
  assert(!html.includes("<script>"), "and the partial text is escaped, never parsed");
  // Stop replaces Ask. The composer stays enabled: disabling a focused control drops focus.
  assert(html.includes("data-chat-stop"), "Stop is on screen while streaming");
});

check("no server: the pane shows the setup card instead of a composer that can't work", () => {
  const html = sidePaneHtml(
    app({
      chatOpen: true,
      vaultRoot: "/vault",
      chatSetup: chatSetup({
        state: "unreachable",
        message: "Can't reach the model server at http://localhost:11434/v1 — is Ollama running?",
        ollama: {
          root: "http://localhost:11434",
          running: false,
          installed: [],
          ram_gb: 16,
          tiers: [{ min_ram_gb: 16, ram: "16 GB", size: "7–8B", model: "llama3.1:8b" }],
          suggested: { min_ram_gb: 16, ram: "16 GB", size: "7–8B", model: "llama3.1:8b" },
        },
      }),
    }),
  );
  assert(html.includes("is Ollama running?"), "the actionable sentence is the card's lead");
  assert(html.includes("ollama pull llama3.1:8b"), "with a pull sized to this machine");
  assert(
    html.includes("search, discovery, editing"),
    "and the promise that everything else still works (E4)",
  );
  const input = html.includes('id="chat-input"');
  assert(!input, "no composer until there is something to answer with");
});

// --- dragging a candidate into the note (droplink.ts) ---------------------------------
//
// The draggable card and the menu's *Insert link at cursor* (K1) both need an open buffer,
// so both appear only in edit mode.

check("a candidate is draggable only while the note is being edited", () => {
  const reading = sidePaneHtml(app({ current: note(), semantic: true, similar: [ghost()] }));
  assert(!reading.includes("draggable"), "nothing to drop into from the reading view");
  const editing = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost()], editing: true }),
  );
  const card = tagWith(editing, "card foldable candidate");
  assert(card.includes('draggable="true"'), `the card itself is the drag handle: ${card}`);
  assert(card.includes("title="), "and says what the drag does — a drag has no visible label");
});

check("only the candidates are draggable — a connection is already linked", () => {
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, connections: [neighbor()], editing: true }),
  );
  assert(!html.includes("draggable"), "there is no link left for the gesture to make");
});

check("the card menu carries the drag's keyboard half while editing, and not otherwise", () => {
  const menu = (editing: boolean) =>
    contextMenuHtml(
      app({
        editing,
        contextMenu: { kind: "card", x: 10, y: 20, path: "notes/ghost.md", title: "ghost" },
      }),
    );
  const tag = tagWith(menu(true), "data-ctx-insert");
  assert(tag.includes("context-item"), "a menu item the focus trap collects");
  assert(tag.includes('role="menuitem"'), "announcing itself as one");
  assert(menu(true).includes("Insert link at cursor"), "named for where it puts the link");
  assert(!menu(false).includes("data-ctx-insert"), "no buffer, no insertion offered");
  // Link… writes frontmatter, so it stays either way.
  assert(menu(false).includes("data-ctx-link"), "Link… is unaffected");
});

// --- asking chat why a candidate was suggested ----------------------------------------
//
// The *Why?* button and the card menu item (⇧F10, K1) name only the candidate; the anchor
// is the open note, read at click time.

check("a candidate card offers Why? as a button that names the candidate", () => {
  const html = sidePaneHtml(app({ current: note(), semantic: true, similar: [ghost()] }));
  const why = tagWith(html, "data-why=");
  assert(why.startsWith("<button"), `a real button, not a clickable div: ${why}`);
  assert(why.includes(`data-why="${ghost().path}"`), `it names the candidate: ${why}`);
  assert(why.includes('tabindex="-1"'), "inside the row's roving tabstop, not a stop of its own");
  assert(why.includes("title="), "and says what it does");
  const folded = sidePaneHtml(
    app({
      current: note(),
      semantic: true,
      similar: [ghost()],
      collapsedCards: new Set([`similar:${ghost().path}`]),
    }),
  );
  assert(!folded.includes("data-why"), "a folded card shows its title row and nothing else");
  const connections = sidePaneHtml(
    app({ current: note(), semantic: true, connections: [neighbor()] }),
  );
  assert(!connections.includes("data-why"), "a linked note was not *suggested* — nothing to explain");
});

check("the card menu carries Why was this suggested? for the keyboard", () => {
  const html = contextMenuHtml(
    app({ contextMenu: { kind: "card", x: 10, y: 20, path: "notes/ghost.md", title: "ghost" } }),
  );
  const tag = tagWith(html, "data-ctx-why");
  assert(tag.includes("context-item"), "a menu item the focus trap collects");
  assert(tag.includes('role="menuitem"'), "announcing itself as one");
  assert(html.includes("Why was this suggested?"), "named for the question it asks");
  const tree = contextMenuHtml(
    app({ contextMenu: { kind: "tree", x: 10, y: 20, dir: "projects", node: null } }),
  );
  assert(!tree.includes("data-ctx-why"), "the tree's menu has no candidate to explain");
});

check("Settings → Chat offers the tool-call cap, painted from the host's own numbers", () => {
  const html = chatTab({ tool_calls: { in_force: 128, default: 64, ceiling: 4096 } });
  const field = tagWith(html, 'id="settings-chat-tool-cap"');
  assert(field.startsWith("<input"), `a field: ${field}`);
  assert(field.includes('value="128"'), `it shows the cap in force: ${field}`);
  // Not `type="number"`: the modal restores the caret after a repaint, which throws there.
  assert(field.includes('type="text"') && field.includes('inputmode="numeric"'), field);
  assert(html.includes("Default 64") && html.includes("4096"), "the copy quotes the host's range");
  assert(html.includes("B2_LLM_MAX_TOOL_CALLS"), "and names the variable Settings overrides");
});

check("an answer built with tools says so, escaped like everything a model names", () => {
  const html = sidePaneHtml(
    app({
      chatOpen: true,
      vaultRoot: "/vault",
      chatSetup: chatSetup(),
      chatMessages: [
        {
          role: "assistant",
          text: "Both cover brewing.",
          citations: [],
          cancelled: false,
          tools: [
            { name: "b2_passage_pairs", arguments: "{}", seeded: false },
            { name: "<img src=x>", arguments: "{}", seeded: false },
          ],
        },
      ],
    }),
  );
  assert(html.includes("Looked up with B2 tools: passage pairs"), "the tools line is painted");
  assert(!html.includes("<img src=x>"), "a tool name is model output — never markup");
  const plain = sidePaneHtml(
    app({
      chatOpen: true,
      vaultRoot: "/vault",
      chatSetup: chatSetup(),
      chatMessages: [
        { role: "assistant", text: "An answer.", citations: [], cancelled: false, tools: [] },
      ],
    }),
  );
  assert(!plain.includes("chat-tools"), "a plain ask has no tools line");
});

// --- Explain (GH #236) -------------------------------------------------------------------

function explainView(over: Partial<SimilarExplainView> = {}): SimilarExplainView {
  return {
    anchor: { path: "notes/anchor.md", title: "anchor" },
    candidate: { path: "notes/ghost.md", title: "ghost" },
    limit: 10,
    standing: { kind: "ranked", rank: 1, of: 20, served: true },
    z: 2.8,
    centroid_rank: 7,
    population: [2.8, 1.0, 0.5, 0.1, -0.2, -0.4, -0.6, -0.8, -1, -1.2, -1.4, -1.6],
    pairs: [
      {
        anchor: { heading_path: "Intro", text: "<img src=x onerror=alert(1)> anchor side" },
        candidate: { heading_path: null, text: "**not markdown** candidate side" },
        score: -0.4,
        z: 2.8,
        identical: false,
      },
    ],
    shared_neighbors: [{ path: "notes/hub.md", title: "hub" }],
    ...over,
  };
}

const explaining = (over: Partial<AppState> = {}) =>
  app({
    current: note(),
    similar: [ghost()],
    explainCard: {
      anchor: "notes/anchor.md",
      candidate: "notes/ghost.md",
      view: explainView(),
      error: null,
      allPairs: false,
      help: false,
    },
    ...over,
  });

check("a Similar card offers Explain beside Why?", () => {
  const html = sidePaneHtml(app({ current: note(), similar: [ghost()] }));
  assert(html.includes('data-explain="notes/ghost.md"'), "the card's Explain names its note");
  assert(html.includes('data-why="notes/ghost.md"'), "Why? stays");
  const menu = contextMenuHtml(
    app({ contextMenu: { kind: "card", x: 0, y: 0, path: "notes/ghost.md", title: "ghost" } }),
  );
  assert(menu.includes("data-ctx-explain"), "the card menu carries Explain: its keyboard half (K1)");
});

check("Explain takes the note pane, and passages are text, never markup (E5)", () => {
  const html = notePaneHtml(explaining());
  assert(html.includes("explain-view"), "the Compare view is painted");
  assert(!html.includes("<img src=x"), "a passage is note content: escaped");
  assert(html.includes("&lt;img src=x"), "and still shown");
  assert(html.includes("**not markdown**"), "shown as text, not rendered as Markdown");
  assert(html.includes("Card #1 of the 10 shown"), "the standing is in words");
  assert(html.includes("ranks #7"), "the whole-note rank is pointed out when it differs");
  assert(html.includes('data-open="notes/hub.md"'), "a shared neighbor opens");
});

check("Explain's controls carry ids, so a repaint gives focus back", () => {
  const html = notePaneHtml(explaining());
  for (const marker of ["data-explain-close", 'data-open="notes/ghost.md"', "data-why"]) {
    assert(hasId(tagWith(html, marker)), `the Explain view's ${marker} control carries an id`);
  }
  const loading = notePaneHtml(
    explaining({
      explainCard: {
        anchor: "notes/anchor.md",
        candidate: "notes/ghost.md",
        view: null,
        error: null,
        allPairs: false,
        help: false,
      },
    }),
  );
  assert(hasId(tagWith(loading, "data-explain-close")), "Back is there while the read runs");
});

check("Explain belongs to the note it was opened on, and editing takes the pane back", () => {
  const other = notePaneHtml(explaining({ current: note({ path: "notes/other.md" }) }));
  assert(!other.includes("explain-view"), "another note never shows a stale explanation");
  const editing = notePaneHtml(explaining({ editing: true }));
  assert(!editing.includes("explain-view"), "the editor owns the pane while editing");
});

check("the strip is labelled in σ, and its longer account waits behind the ?", () => {
  const closed = notePaneHtml(explaining());
  assert(closed.includes(">0σ<"), "the average is labelled on the axis");
  assert(hasId(tagWith(closed, "data-explain-help")), "the ? carries an id for focus");
  assert(tagWith(closed, "data-explain-help").includes('aria-expanded="false"'), "closed by default");
  assert(!closed.includes('id="explain-help-text"'), "the account is not painted until asked for");
  const ec = explaining().explainCard!;
  const open = notePaneHtml(explaining({ explainCard: { ...ec, help: true } }));
  assert(tagWith(open, "data-explain-help").includes('aria-expanded="true"'), "says it is open");
  assert(open.includes('id="explain-help-text"'), "the account is painted");
  assert(open.includes("standard deviations"), "and says what the axis is");
  assert(closed.includes("strip-key"), "the strip carries a key for its three marks");
  assert(closed.includes("where ●●○ and ●●● start"), "the dashed lines are named as cut-offs");
  assert(closed.includes("The 12 notes closest to anchor"), "the caption says whose strip it is");
});

console.log(`render: ${passed} checks passed`);
