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

import { buildScene } from "./graph.ts";
import { modalHtml, notePaneHtml, sidePaneHtml } from "./render.ts";
import { state, type AppState } from "./state.ts";
import type {
  NeighborView,
  NoteView,
  ResourceLink,
  SearchResult,
  SimilarView,
  UnresolvedLink,
} from "./types.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: string, expected: string, msg: string): void {
  assert(actual === expected, `${msg} — expected ${expected}, got ${actual}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- fixtures -----------------------------------------------------------------------

function note(over: Partial<NoteView> = {}): NoteView {
  return {
    b2id: "01ANCHOR",
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
    b2id: "01EDGE",
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
  return { b2id: "01GHOST", path: "notes/ghost.md", title: "ghost", score: 0.42, evidence: "", ...over };
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
  b2id: "01HIT",
  path,
  title: path,
  score: 1,
  snippet: "",
});

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

// --- the anomaly review panel (GH #88) --------------------------------------------------

// The panel's one load-bearing claim: which of an anomaly's paths B2 can actually reach.
// A shadowed copy has no index row, so it is not in the file tree and `read_note` cannot
// resolve it — offering "Open" there would be a button that does nothing, which is the
// same complaint (#88) about inert text one step worse.
check("a collision row offers Open on the keeper and Copy path on the shadowed copy", () => {
  const html = modalHtml(
    app({
      anomaliesOpen: true,
      anomalies: {
        collisions: [
          {
            b2id: "01ABC",
            kept_path: "notes/a.md",
            precedence: "incumbent",
            shadowed_paths: ["notes/a copy.md"],
          },
        ],
        restamped: [],
      },
    }),
  );
  assert(html.includes('data-anomaly-open="notes/a.md"'), "the keeper opens");
  assert(html.includes('data-anomaly-copy="notes/a copy.md"'), "the shadowed copy is copyable");
  assert(!html.includes('data-anomaly-open="notes/a copy.md"'), "an un-indexed file never opens");
  assert(html.includes("data-anomalies-close"), "the panel has a Done control");
});

check("each anomaly is its own row, and an empty report says so", () => {
  const html = modalHtml(
    app({
      anomaliesOpen: true,
      anomalies: {
        collisions: [
          { b2id: "01A", kept_path: "a.md", precedence: "incumbent", shadowed_paths: ["a2.md"] },
        ],
        restamped: [{ path: "x.md", old_b2id: "01OLD", new_b2id: "01NEW" }],
      },
    }),
  );
  assert(
    (html.match(/data-anomaly="/g) ?? []).length === 2,
    "two anomalies paint two rows — never one run-on paragraph (the #88 regression)",
  );
  const empty = modalHtml(app({ anomaliesOpen: true, anomalies: { collisions: [], restamped: [] } }));
  assert(empty.includes("anomaly-empty"), "a clean pass gets an explicit empty state");
  assert(!empty.includes("data-anomaly="), "and no rows");
});

console.log(`render: ${passed} checks passed`);
