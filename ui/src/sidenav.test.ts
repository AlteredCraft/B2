// The discovery pane's navigation rules (sidenav.ts), pinned. Pure logic — no DOM — so
// node runs it straight off the source via its native type-stripping: `npm test`.
// Dependency-free like treenav.test.ts (hand-rolled assert), and deliberately its mirror:
// the right column now answers the same ARIA `tree` pattern the file tree does, so these
// cases are the guard against the paint (render.ts) and the arrows drifting apart.
import type { KeyEventLike } from "./bindings.ts";
import {
  cardKey,
  rovingSideKey,
  sideArrowMove,
  sideNavFor,
  type SideNav,
  sideRowIndex,
  sideRows,
  type SideNavState,
  type SideRow,
} from "./sidenav.ts";
import type { NeighborView, NoteView, SearchResult, SimilarView, UnresolvedLink } from "./types.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: unknown, expected: unknown, msg: string): void {
  assert(
    actual === expected,
    `${msg} — expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
  );
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- fixtures ------------------------------------------------------------------------

const similar = (path: string): SimilarView => ({ path, evidence: "" }) as SimilarView;
const edge = (path: string, label = "references"): NeighborView =>
  ({ path, label, direction: "outbound", origin: "inline" }) as NeighborView;
const dangling = (target: string): UnresolvedLink =>
  ({ target, relation: "references", origin: "inline" }) as UnresolvedLink;
const hit = (path: string): SearchResult => ({ path, snippet: "" }) as SearchResult;

/** A note open, both sections populated, nothing folded — the everyday case. */
function pane(over: Partial<SideNavState> = {}): SideNavState {
  return {
    // Chat is the column's third mode and owns it outright when open (chat.test.ts
    // covers its rows); everything here is the other two.
    chatOpen: false,
    chatMessages: [],
    chatStreaming: null,
    searchQuery: "",
    searchResults: [],
    loading: false,
    current: {} as NoteView,
    similar: [similar("notes/kivo.md"), similar("notes/prep.md")],
    connections: [edge("notes/onsite.md")],
    unresolved: [],
    collapsedSections: new Set(),
    collapsedCards: new Set(),
    ...over,
  };
}

const keys = (rows: readonly SideRow[]) => rows.map((r) => r.key).join("|");

check("chat mode owns the column: its rows replace search's and discovery's", () => {
  // The delegation itself — the transcript's shape is chat.test.ts's subject, but *that
  // the pane switches wholesale* is this module's rule: one column, one list in it.
  const rows = sideRows(
    pane({
      chatOpen: true,
      chatMessages: [{ role: "user", text: "hi", citations: [], cancelled: false }],
      searchQuery: "kivo",
      searchResults: [hit("a.md")],
    }),
  );
  equal(keys(rows), "chat:turn:0", "chat wins over a live search and over discovery");
});

// --- sideRows: the one order the paint and the arrows share ---------------------------

check("no note open: discovery has no rows at all", () => {
  equal(keys(sideRows(pane({ current: null }))), "", "the empty-state hint is not a row");
});

check("both section heads, each followed by its cards in list order", () => {
  equal(
    keys(sideRows(pane())),
    "section:similar|similar:0:notes/kivo.md|similar:1:notes/prep.md|" +
      "section:connections|connections:0:notes/onsite.md",
    "rows",
  );
});

check("a row carries the depth and fold state the paint needs", () => {
  const rows = sideRows(pane());
  const head = rows[0];
  equal(head.depth, 0, "a head sits at the top level");
  equal(head.fold?.kind, "section", "a head folds its section");
  equal(head.expanded, true, "expanded by default");
  equal(head.hasChildRows, true, "with cards to step into");
  const card = rows[1];
  equal(card.depth, 1, "a card sits under its head");
  equal(card.fold?.kind, "card", "a card folds its own body");
  equal(card.expanded, true, "cards default expanded");
  equal(card.hasChildRows, false, "a card's body is not a row — nothing to step into");
});

check("a collapsed section keeps its head and drops its cards", () => {
  const rows = sideRows(pane({ collapsedSections: new Set(["similar"] as const) }));
  equal(keys(rows), "section:similar|section:connections|connections:0:notes/onsite.md", "rows");
  equal(rows[0].expanded, false, "the head reads collapsed");
});

check("an empty section is a row with nothing to step into", () => {
  const rows = sideRows(pane({ similar: [], connections: [] }));
  equal(keys(rows), "section:similar|section:connections", "both heads survive");
  equal(rows[0].hasChildRows, false, "no cards under it");
  equal(rows[0].fold?.kind, "section", "still foldable — the chevron works either way");
});

check("a folded card stays a row; only its body goes", () => {
  const rows = sideRows(
    pane({ collapsedCards: new Set([cardKey("similar", "notes/kivo.md")]) }),
  );
  equal(keys(rows).includes("similar:0:notes/kivo.md"), true, "still navigable");
  equal(rows[1].expanded, false, "and reads collapsed");
  equal(rows[2].expanded, true, "its neighbour is untouched");
});

// Two edges to one target are legal (a body link plus a typed frontmatter relation —
// data-model.md §2's "augment" case), and they share ONE fold key. Row identity must
// still be unique, or ↓ off the second lands back under the first and navigation sticks.
check("duplicate edges to one note are distinct rows sharing one fold key", () => {
  const rows = sideRows(pane({ connections: [edge("notes/a.md"), edge("notes/a.md", "supports")] }));
  const [first, second] = rows.slice(-2);
  assert(first.key !== second.key, "row keys are unique");
  equal(first.fold?.kind === "card" && first.fold.key, cardKey("connections", "notes/a.md"), "fold key");
  equal(second.fold?.kind === "card" && second.fold.key, cardKey("connections", "notes/a.md"), "the same");
  equal(sideArrowMove(rows, sideRowIndex(rows, first.key), "side.row.next")?.key, second.key, "↓ lands");
});

check("unresolved links are rows too — after the edges, and unfoldable", () => {
  const rows = sideRows(pane({ unresolved: [dangling("Hermes")] }));
  equal(keys(rows).endsWith("connections:0:notes/onsite.md|unresolved:0:Hermes"), true, "last");
  equal(rows[rows.length - 1].fold, null, "a broken link has no body to fold");
});

check("search mode is a flat list of results — no section heads", () => {
  const rows = sideRows(pane({ searchQuery: "kivo", searchResults: [hit("a.md"), hit("b.md")] }));
  equal(keys(rows), "search:0:a.md|search:1:b.md", "rows");
  equal(rows[0].depth, 0, "results sit at the top level");
  equal(rows[0].fold, null, "a result has no fold");
});

check("a search still running has no rows — the pane paints “Searching…”", () => {
  equal(
    keys(sideRows(pane({ searchQuery: "kivo", searchResults: [hit("stale.md")], loading: true }))),
    "",
    "the previous query's results are not on screen",
  );
  equal(keys(sideRows(pane({ searchQuery: "kivo", searchResults: [] }))), "", "no matches, no rows");
});

// --- sideArrowMove: the ARIA tree pattern over those rows -----------------------------

function move(rows: readonly SideRow[], from: string | null, nav: SideNav) {
  return sideArrowMove(rows, from === null ? -1 : sideRowIndex(rows, from), nav);
}

/** A keydown, as the registry's matcher sees it. */
function press(key: string): KeyEventLike {
  return { key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false };
}

check("up/down step between visible rows and stop at the ends", () => {
  const rows = sideRows(pane());
  equal(move(rows, "section:similar", "side.row.next")?.key, "similar:0:notes/kivo.md", "down");
  equal(move(rows, "similar:0:notes/kivo.md", "side.row.prev")?.key, "section:similar", "up");
  equal(move(rows, "section:similar", "side.row.prev"), null, "no wrap off the top");
  equal(move(rows, "connections:0:notes/onsite.md", "side.row.next"), null, "no wrap off the bottom");
});

check("down never descends into a collapsed section", () => {
  const rows = sideRows(pane({ collapsedSections: new Set(["similar"] as const) }));
  equal(move(rows, "section:similar", "side.row.next")?.key, "section:connections", "skips the cards");
});

check("Home/End jump to the first and last rows", () => {
  const rows = sideRows(pane());
  equal(move(rows, "section:connections", "side.row.first")?.key, "section:similar", "Home");
  equal(move(rows, "section:similar", "side.row.last")?.key, "connections:0:notes/onsite.md", "End");
});

check("the first arrow press lands even with nothing focused", () => {
  const rows = sideRows(pane());
  equal(move(rows, null, "side.row.next")?.key, "section:similar", "down → first");
  equal(move(rows, null, "side.row.prev")?.key, "connections:0:notes/onsite.md", "up → last");
  equal(move(rows, null, "side.row.in")?.key, "section:similar", "right → first");
});

check("right expands a collapsed section, then steps into it", () => {
  const closed = sideRows(pane({ collapsedSections: new Set(["similar"] as const) }));
  const expand = move(closed, "section:similar", "side.row.in");
  equal(expand?.kind, "expand", "collapsed → expand");
  equal(expand?.kind === "expand" && expand.fold.kind === "section" && expand.fold.section, "similar", "which");
  const open = sideRows(pane());
  const into = move(open, "section:similar", "side.row.in");
  equal(into?.kind, "focus", "expanded → step in");
  equal(into?.key, "similar:0:notes/kivo.md", "its first card");
});

check("right opens a card's body, and stops there — a card has no children", () => {
  const folded = sideRows(pane({ collapsedCards: new Set([cardKey("similar", "notes/kivo.md")]) }));
  const open = move(folded, "similar:0:notes/kivo.md", "side.row.in");
  equal(open?.kind, "expand", "folded body → expand");
  equal(open?.kind === "expand" && open.fold.kind === "card" && open.fold.key, "similar:notes/kivo.md", "which");
  equal(move(sideRows(pane()), "similar:0:notes/kivo.md", "side.row.in"), null, "already open → nothing");
});

check("right does nothing where there is no fold: an empty section, a search result", () => {
  const empty = sideRows(pane({ similar: [], connections: [] }));
  equal(move(empty, "section:similar", "side.row.in"), null, "expanded and empty");
  const results = sideRows(pane({ searchQuery: "k", searchResults: [hit("a.md")] }));
  equal(move(results, "search:0:a.md", "side.row.in"), null, "a result has nothing to open");
});

check("left folds what is open, else steps out to the section head", () => {
  const rows = sideRows(pane());
  const body = move(rows, "similar:0:notes/kivo.md", "side.row.out");
  equal(body?.kind, "collapse", "an open card folds its body first");
  const folded = sideRows(pane({ collapsedCards: new Set([cardKey("similar", "notes/kivo.md")]) }));
  const out = move(folded, "similar:0:notes/kivo.md", "side.row.out");
  equal(out?.kind, "focus", "a folded card steps out");
  equal(out?.key, "section:similar", "to its head");
  const head = move(rows, "section:similar", "side.row.out");
  equal(head?.kind, "collapse", "an expanded head collapses");
  const closed = sideRows(pane({ collapsedSections: new Set(["similar"] as const) }));
  equal(move(closed, "section:similar", "side.row.out"), null, "a collapsed head has nowhere out to");
});

check("the shipped keys mean what the ARIA tree pattern says", () => {
  // Pinned here rather than derived, for treenav.test.ts's reason: the registry owns the
  // keys since #121, and this pane's set is a *sibling* of the tree's — same keys, its own
  // commands, so rebinding one leaves the other where it was.
  equal(sideNavFor(press("ArrowDown")), "side.row.next", "↓ is the next row");
  equal(sideNavFor(press("ArrowUp")), "side.row.prev", "↑ is the previous one");
  equal(sideNavFor(press("Home")), "side.row.first", "Home");
  equal(sideNavFor(press("End")), "side.row.last", "End");
  equal(sideNavFor(press("ArrowRight")), "side.row.in", "→ unfolds or steps in");
  equal(sideNavFor(press("ArrowLeft")), "side.row.out", "← folds or steps out");
});

check("a key the pane doesn't own is left alone", () => {
  equal(sideNavFor(press("Enter")), null, "Enter belongs to the row's button");
  equal(sideNavFor(press("k")), null, "letters fall through to the global chords");
  equal(sideArrowMove([], -1, "side.row.next"), null, "and an empty pane has no moves at all");
});

// --- the roving tabstop ----------------------------------------------------------------

check("the tabstop prefers the keyboard's row, else the first", () => {
  const rows = sideRows(pane());
  equal(rovingSideKey(rows, "similar:1:notes/prep.md"), "similar:1:notes/prep.md", "keyboard wins");
  equal(rovingSideKey(rows, null), "section:similar", "else the first row");
  equal(rovingSideKey([], "gone"), null, "an empty pane has no tabstop");
});

check("a tabstop that folded or scrolled out of existence falls back", () => {
  // The section collapsed under the focused card, or a new note replaced discovery
  // wholesale: the pane must not be left with zero tabbable rows (⇥ would skip it).
  const rows = sideRows(pane({ collapsedSections: new Set(["similar"] as const) }));
  equal(rovingSideKey(rows, "similar:0:notes/kivo.md"), "section:similar", "folded away → first row");
  equal(sideRowIndex(rows, null), -1, "nothing focused is not row 0");
});

console.log(`sidenav: ${passed} checks passed`);
