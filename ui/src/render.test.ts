// The paint's half of the **focus contract** (K1, GH #91). `render.ts` builds a pane as a
// string and main.ts swaps it in wholesale, which destroys whatever held focus — so the
// keyboard can only be put back by an identity *carried in the markup* and re-found after
// the swap (crates/b2-desktop/CLAUDE.md, "Two things that bite"). The restoration is
// generic: a graph node by its scene id, every other control by its `id`. Both halves are
// worthless if the paint stops emitting them, and only this side is testable off the DOM —
// so these cases pin the identity exactly where the restoration goes looking for it.
//
// Pure strings in, assertions out, hand-rolled asserts like graph.test.ts: node runs it
// straight off the source (`npm test`).

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
  SearchResult,
  SimilarView,
  UnresolvedLink,
} from "./types.ts";

// `notePaneHtml` renders a note body, and the render seam sanitizes with the host's own
// HTML parser (E5, sanitize.ts) — so the pane needs a DOM even here, where the subject is
// focus identity. Two lines rather than a shared shim module: the alternative is a
// non-test module in `src/` that only tests import. sanitize.test.ts is where the seam
// itself is exercised.
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
    // A quote in a path is legal on disk, so it is legal in a node id — and a node id is
    // what the focus lookup compares. It rides through the markup escaped.
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

const hit = (path: string): SearchResult => ({
  path,
  title: path,
  score: 1,
  snippet: "",
});

/** A ready local chat provider — the setup the pane paints a conversation under. */
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
    ...over,
  };
}

/** A fresh `AppState` over the app's own defaults — with its own collections, so a case
 *  that mutates one can't leak into the next. */
function app(over: Partial<AppState> = {}): AppState {
  return {
    ...state,
    expandedDirs: new Set(),
    collapsedSections: new Set(),
    collapsedCards: new Set(),
    ...over,
  };
}

/** The opening tag carrying `marker` — how a case asks "does *this* control have an id?"
 *  without pinning the id's spelling (main.ts restores by whatever id it finds). */
function tagWith(html: string, marker: string): string {
  const at = html.indexOf(marker);
  assert(at !== -1, `the markup contains ${marker}`);
  const start = html.lastIndexOf("<", at);
  const end = html.indexOf(">", at);
  assert(start !== -1 && end !== -1, `${marker} sits inside a tag`);
  return html.slice(start, end + 1);
}

const hasId = (tag: string): boolean => /\sid="[^"]+"/.test(tag);

/** Attribute values are escaped on the way out and decoded by the browser on the way back
 *  in (`dataset`), so a case comparing against a scene id decodes them here. */
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
  // The identity `paintNote` re-finds a node by: the element is gone after the swap, so
  // the id must come from the scene, which is a pure function of this same state.
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
  // Not the graph's problem alone: toggling the drawer, source, or graph *is* a note-pane
  // repaint, so the chip you pressed is destroyed by the swap it triggered.
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

check("search mode's clear button carries an id — the one focusable that isn't a row", () => {
  const html = sidePaneHtml(app({ searchQuery: "kivo", searchResults: [hit("notes/kivo.md")] }));
  assert(hasId(tagWith(html, "data-clear-search")), "clear carries an id");
  assert(
    /data-side-row="[^"]+"/.test(tagWith(html, "notes/kivo.md")),
    "a result row still carries its row key (what paintSide restores rows by)",
  );
});

// The Keyboard panel is the K1 promise's *findable* half, and #119 extended it to chords
// B2 doesn't own — the app menu's, which the host declares and hands over at boot. This is
// the one line of wiring that carries them into the paint (`shortcuts(state.menuChords)`),
// and dropping it is invisible: the panel would keep rendering menukeys.ts's mirror and
// look right until the two came apart.
check("the Keyboard panel lists the menu bar the host declared, not the mirror", () => {
  const html = modalHtml(
    app({
      settingsOpen: true,
      settingsTab: "keyboard",
      menuChords: [{ id: "app.panic", label: "Panic", keys: "Mod-Shift-p" }],
    }),
  );
  assert(html.includes("The menu bar"), "the group is in the sheet");
  assert(html.includes("<kbd>⇧⌘P</kbd>"), "with the host's chord");
  assert(!html.includes("Quit B2"), "and none of the mirror's rows");
});

// The Keyboard panel is also where chords are *edited* since #121, and the affordance is
// the contract: a chip painted as a `<button>` says "B2 can move this", a `<kbd>` says it
// can't. Getting that split wrong is a panel that offers to rebind ⏎-in-a-text-field, or
// one that quietly makes a movable chord unreachable from the only surface that moves it.
check("a movable chord paints as a button, and everything else as plain text", () => {
  const html = modalHtml(app({ settingsOpen: true, settingsTab: "keyboard" }));
  assert(html.includes('data-rebind="find.open"'), "⌘F is B2's, and B2 can move it");
  const ids = [...html.matchAll(/id="(keys-chip-[^"]+)"/g)].map((m) => m[1]);
  assert(ids.length > 20, "the sheet is mostly chips");
  // Every chip carries an id so focus survives the repaint its own click causes — and the
  // ids are *unique*, which is not free: ⇧F10 opens a menu on a tree row and on a
  // discovery card, so `menu.open` appears in two rows. Two controls answering to one id
  // is a repaint that hands the keyboard to the wrong one.
  assertEq(new Set(ids).size, ids.length, "no two chips share an id");
  assertEq(ids.filter((id) => id.endsWith("-menu.open")).length, 2, "and ⇧F10 really is in two rows");
  // `graph.activate` is fixed: a graph node stands in for a button, and ⏎/Space are what a
  // button answers to. It still appears — K1 is about being findable — as text.
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
  // The two tiers reach the paint differently on purpose: a refusal disables the button,
  // an advisory is said and the button stays live — the human is the gate (keymap.ts).
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
  // The probe's whole output is this one line, and a panel that dropped it would turn a
  // real observation — macOS took that chord — into a recorder that looks broken.
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

// Settings is the overlay layer's odd member: same modal semantics, no box and no
// backdrop, because it takes the whole window. Two of those facts are contracts main.ts
// depends on and neither is visible from that side. `overlayFocusables` finds the trap's
// scope by `[role="dialog"]` — so the surface must carry it, or ⇥ escapes into the app
// behind it — and `focusIntoOverlay` opens Settings on the *first* focusable, documented
// as "the selected tab", which holds only while nothing focusable precedes the rail.
check("the Settings surface is a dialog whose first focusable is the selected tab", () => {
  const html = modalHtml(app({ settingsOpen: true, settingsTab: "chat" }));
  assert(tagWith(html, "settings-screen").includes('role="dialog"'), "the trap has a scope");
  assert(tagWith(html, "settings-screen").includes('aria-modal="true"'), "and it is modal");
  assert(!html.includes("modal-backdrop"), "no backdrop: there is no outside to click");
  // Order, not just presence: the rail's selected tab is the only tab in the Tab cycle
  // (the others are the roving tabstop's -1), so it is `overlayFocusables()[0]` exactly
  // while Done and everything else in the panel comes after it in the markup.
  const rail = html.indexOf('id="settings-tab-chat"');
  assert(rail !== -1 && rail < html.indexOf('id="settings-panel"'), "rail before panel");
  assert(html.indexOf('id="settings-panel"') < html.indexOf('id="settings-done"'), "Done last");
});

// Settings → Index is where the manual Reindex went when it left the top bar. Its `id` is
// what main.ts's click delegation *and* `captureModalFocus` both go looking for, so a
// panel that painted a nameless button would be a button that does nothing and drops the
// keyboard on `<body>` — this file's subject exactly.
check("the Index panel's Reindex button carries the id both halves restore by", () => {
  const html = modalHtml(app({ settingsOpen: true, settingsTab: "index", vaultRoot: "/v" }));
  assert(tagWith(html, ">Reindex<").includes('id="reindex"'), "the button is identified");
});

// The button is refused while a run is live (`doReindex` single-in-flights anyway), and
// says so rather than sitting there looking pressable. Both halves come from the exported
// predicates main.ts repaints with, which is what keeps the two paints of this button in
// step.
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

// The panel used to point at the top bar for the live meter and its Cancel. Settings takes
// the whole window now, so the top bar is *behind* it: the pointer became a lie, and the
// only way to stop a run became Escape-then-hunt. The panel carries its own meter instead
// — the same classes `paintReindex` writes into, and a Cancel main.ts's delegation can see.
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

// The #26 honesty, in the one place a human comes to ask "is my index finished?": indexed
// and embedded are two different states, and a projected-but-unembedded vault must never
// read as done — that is the whole reason the button is still reachable at all.
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
// Not focus identity either, but the same "only this side is testable" argument: since
// GH #176 the key has four resolutions and the app's honesty about which one is in force
// exists *only* as copy. A user who exports `B2_LLM_API_KEY` and then types a key into
// Settings has done something that does not do what it looks like, and a user whose
// Keychain refused has a key that disappears at quit — both are things to be told before
// they are discovered. The host has its own tests that the source is computed right; these
// pin that each one reaches the screen as a different sentence.

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

  // The environment wins, so a key typed here would be stored and not used. Saying so is
  // the whole difference between a documented precedence and a field that does nothing.
  const env = chatTab({ api_key_source: "environment" });
  assert(env.includes("B2_LLM_API_KEY"), "the overriding variable is named");
  assert(env.includes("overrides"), `the override is stated, not implied: ${env}`);

  // The Keychain refused: chat works, and the key is gone at quit. The panel must not
  // then close with the general "B2 saves the key in your Keychain" — this key is the
  // one it could not save, and a paragraph asserting both leaves the reader unable to
  // tell which sentence is about them.
  const session = chatTab({ api_key_source: "session" });
  assert(session.includes("this session only"), "a session-only key says so");
  assert(session.includes("couldn’t save it"), `and says why: ${session}`);
  assert(
    !session.includes("B2 saves the key in your macOS Keychain"),
    `and never also claims it was saved: ${session}`,
  );
  assert(session.includes("not</strong> saved"), `it says the opposite, plainly: ${session}`);
  // Where the claim *is* true, it stays.
  assert(
    stored.includes("B2 saves the key in your macOS Keychain"),
    "a stored key still gets the Keychain sentence",
  );

  // Nothing configured: nothing to remove, and no claim about a key that isn't there.
  const none = chatTab({ api_key_source: "none" });
  assert(!none.includes("data-chat-clear-key"), "no key, no Remove button");
  assert(!none.includes("Keychain — encrypted"), "and no claim one is saved");
});

check("the cloud section says where an endpoint can come from, since B2 names none", () => {
  // B2 ships no default cloud provider and never will — picking one is the explicit act
  // M5 is about, so the empty URL field is deliberate. That makes "and where do I get
  // one?" a question the panel has to answer in the only way it may: a link.
  const html = chatTab({ api_key_source: "none" });
  assert(html.includes("https://docs.ollama.com/cloud"), `the cloud docs are linked: ${html}`);
  assert(
    html.includes("Any OpenAI-compatible provider works"),
    "and the link is an example, not a requirement",
  );
  // The **Local** copy must not carry it: a cloud pointer beside "nothing leaves this
  // machine" is the one place that sentence would read as an invitation.
  const local = modalHtml(
    app({ settingsOpen: true, settingsTab: "chat", chatCloud: false, chatSetup: chatSetup() }),
  );
  assert(!local.includes("docs.ollama.com/cloud"), "the local section stays local");
});

// --- Settings → Chat: the Model field (a picker over what is actually installed) --------
//
// The commonest local-setup mistake is a model name the daemon doesn't have, and until the
// daemon answers `/api/tags` there is nothing to do about that but let a human type and
// find out. When it *has* answered, its inventory is what the field should offer — so the
// field has two shapes, and these pin that each one can still express what the other can.

/** Settings → Chat against the local (Ollama-shaped) endpoint. `ollama` is the daemon's
 *  half — an answering daemon with an inventory unless a case says otherwise. */
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
  // The id is the contract — saveChatConfig reads `.value` off it and captureModalFocus
  // restores the keyboard by it, and both must keep working across the swap.
  assert(html.includes('<option value="gemma3:12b"'), "every installed model is offered");
  assert(
    html.includes('<option value="llama3.2:latest" selected>'),
    `and the configured one is what's selected: ${html}`,
  );
  // A way out, because a list of *installed* models cannot contain the one being pulled
  // right now — which is exactly when you want to name it.
  assert(html.includes("data-chat-model-custom"), "a typed name stays reachable");
});

check("a configured model the daemon doesn't have still leads the picker", () => {
  // The failure this prevents is silent: drop the unknown model and the field shows
  // whatever happens to be first, so merely *looking* at Settings re-points the
  // configuration — and the status line below it would still name the old one.
  const html = localChatTab(
    { model: "gemma4:latest", state: "model_missing" },
    { installed: [model("llama3.2:latest")] },
  );
  assert(
    html.includes('<option value="gemma4:latest" selected>gemma4:latest — not installed'),
    `the configured model is present and marked: ${html}`,
  );
  // And the card below doesn't repeat the same inventory as a second, click-to-apply
  // control — two controls for one value is two things to keep in step.
  assert(!html.includes("data-chat-use-model"), `no duplicate picker in Settings: ${html}`);
});

check("Cloud models never inherits the local daemon's picker", () => {
  // Pressing *Cloud models* repaints immediately and nothing re-probes (the URL field is
  // cleared — there is no default provider to probe), so `setup` is still the local one,
  // inventory and all. Keying the picker off that would put this machine's models under
  // an empty cloud endpoint, with a button press standing between the user and the field
  // they opened the section to fill in.
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
  // The daemon isn't running (or the endpoint isn't Ollama's): there is no list to offer,
  // so the field is a box and the *return* button must not be painted — a control that
  // swaps to an empty picker is a control that strands the keyboard.
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
  // The question this answers: what does Settings do when there is no server at all? It
  // stays a status — the probe cannot fail (b2-llm's setup.rs) — and the panel prints the
  // sentence plus the two ways out: start the daemon, or install one.
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
  // Still a settings panel, not a dead end: the fields and Save and test stay put, so the
  // user can point the endpoint somewhere that *is* running.
  assert(html.includes('id="settings-chat-save"'), "and Save and test is still there");
  // Exactly once. The card and the Local section can each reach for the same suggested
  // pull, and one command printed twice on one screen reads as two instructions — so the
  // card yields it to the section in Settings (it keeps it in the pane, which has none).
  assertEq(html.split("ollama pull llama3.1:8b").length - 1, 1, "the pull command is said once");
});

check("a running daemon behind a wrong path is never told to start itself", () => {
  // The other half of the endpoint-typo fix (b2-llm's `refusal_message`): `…/v1X`
  // answers 404, so chat isn't ready — but the daemon is up, and "Start it with
  // `ollama serve`" is advice about a program that is already running. The card keys
  // that paragraph off `running`, which is the evidence, so the host's sentence and
  // the card's advice can't contradict each other.
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
  // Nor the card's "if it isn't installed yet" paragraph. (The Local section's standing
  // "add a model with `ollama pull …`" note keeps its quickstart link — that one is about
  // adding a model, which is still true here, so it is not the same sentence.)
  assert(!html.includes("isn’t installed yet"), `and nothing to install: ${html}`);
  // The inventory still answered, so the picker is on screen — the endpoint is what's
  // wrong, and everything known about the daemon stays usable.
  assert(tagWith(html, 'id="settings-chat-model"').startsWith("<select"), "the picker stands");
});

// --- the tree's context menu ---------------------------------------------------------
//
// Not focus identity like the rest of this file, but the same argument one step out: the
// context menu is the *only* home of Rename / Move… / the two copy paths, and ⇧F10 is how
// a keyboard reaches them (K1). main.ts's click delegation finds each item by its
// `data-ctx-*` attribute and its focus trap collects them by `.context-item`, so an item
// that painted without either is an action reachable by neither mouse nor keyboard.

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
  // Delete stays last in the node group: the copy items went above it, not below.
  assert(
    html.indexOf("data-ctx-copy-system-path") < html.indexOf("data-ctx-delete"),
    "the destructive item is still the group's last",
  );
});

check("the folder-context menu offers no path to copy", () => {
  // With no row under the cursor the menu targets a *folder* for the create pair — there
  // is no node whose path a copy item could mean, and the vault root is already in the
  // top bar.
  const html = contextMenuHtml(
    app({ contextMenu: { kind: "tree", x: 10, y: 20, dir: "projects", node: null } }),
  );
  assert(!html.includes("data-ctx-copy"), "no copy item without a node");
  assert(html.includes("data-ctx-new-note"), "but the create pair is still there");
});

// --- discovery strength (GH #150) -------------------------------------------------------

check("a graded card carries its σ figure, not only a hover tooltip", () => {
  // The band's dots grade; the number behind them used to live only in `title=`, which
  // a keyboard can't reach (K1). It ships in the markup now and the selected row
  // reveals it, so focus — not just a pointer — gets the number.
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost({ z: 2.53 })] }),
  );
  assert(html.includes("●●○"), "the band still grades at a glance");
  assert(html.includes("2.5σ"), "and the figure is on the card, awaiting selection");
});

check("the accessible name carries the figure too, not the band alone", () => {
  // The dots and the σ are one reading for the eye; naming only the band would hand a
  // screen reader "clear match" with no route to the number behind it, while the figure's
  // own span stays aria-hidden so it is announced once rather than twice (PR #184 review).
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost({ z: 2.53 })] }),
  );
  assert(
    html.includes('aria-label="clear match, 2.5σ"'),
    "the band and its figure reach AT together",
  );
  assert(
    html.includes('class="card-sigma" aria-hidden="true"'),
    "and the visible figure is not announced a second time",
  );
});

check("an ungraded list says so rather than implying a judgement B2 didn't make", () => {
  // A vault too small for the floor's statistics returns candidates with no z at all
  // (the starter-vault posture: gate inert, everything served). Without a word, those
  // bare cards read exactly like graded ones that all scored low.
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost(), ghost({ path: "b.md" })] }),
  );
  assert(html.includes("Ungraded"), "the pane admits it didn't grade these");
  assert(!html.includes("●"), "and claims no band, since no statistic was computed");
  // "Not enough" leaves a reader with nothing to do; the bar is a number, so say it.
  assert(
    html.includes(`${STRENGTH_MIN_CANDIDATES} or more candidates`),
    "and names what 'enough' is",
  );
  // A notice, not body prose and not a failure: the caveat gets the accent-toned box the
  // install banner established, so it is seen rather than skimmed (PR #184 review).
  assert(html.includes('class="side-note"'), "painted as a notice");
});

check("a graded list carries no ungraded caveat", () => {
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, similar: [ghost({ z: 3.1 })] }),
  );
  assert(!html.includes("Ungraded"), "nothing to admit — the floor judged this list");
});

check("raw mode says raw, not ungraded — one caveat, not two", () => {
  // `--no-floor`'s GUI sibling turns the floor off, so *of course* nothing carries a z.
  // The honest caveat there is already "the quality floor is off"; adding "ungraded" on
  // top would explain the same fact twice, in words that sound like a different problem.
  const html = sidePaneHtml(
    app({ current: note(), semantic: true, rawDiscovery: true, similar: [ghost()] }),
  );
  assert(html.includes("quality floor is off"), "the raw banner is the caveat here");
  assert(!html.includes("Ungraded"), "and it is the only one");
});

// --- chat (flow ④, GH #155) ------------------------------------------------------------

check("a citation navigates in-app — a button with a path, never a link with an href", () => {
  // The E5 rule this pane owes on top of sanitizing: the webview *is* the app, so a
  // citation must not be something the browser can follow. It is the same `data-open`
  // button search results and discovery cards are, which also gives the mouse and ⏎ one
  // activation path (K1) rather than two to keep in step.
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
  // And it is a row of the pane's tree, with the identity `paintSide` restores focus by.
  assert(tag.includes('role="treeitem"'), "a citation is a navigable row");
  assert(tag.includes("data-side-row="), "carrying the key focus is restored by");
});

check("a streaming answer paints as escaped text under the id tokens land in", () => {
  // Tokens are written into `#chat-stream` as `textContent` (main.ts) rather than
  // re-rendered, so this element is the contract between the two. The first paint escapes
  // the partial text for the same reason: a half-arrived answer is not a document.
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
  // Stop replaces Ask while an answer is arriving — Esc's equal for the mouse. The
  // composer itself stays *enabled* on purpose (render.ts says why: disabling a focused
  // control drops the keyboard to `<body>`), so this asserts only what changes.
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

console.log(`render: ${passed} checks passed`);
