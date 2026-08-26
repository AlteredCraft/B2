// The discovery pane's pure logic — no DOM, no IPC — so node runs its test straight off
// the source (`npm test`), like treenav.ts.
//
// This is treenav.ts's sibling for the right-hand column, and it exists for the same
// reason: the paint (render.ts's side-pane builders) and the arrow keys must agree on row
// order down to the last tie-break, so the row identities and the fold state live here
// once and both sides call them. A pane you can arrow through in a different order than
// you can see is worse than no arrows at all.
//
// Invariant K1 (design/invariants.md, GH #78): discovery is navigable from the
// keyboard, following the same ARIA `tree` pattern the file tree does — ↑↓ between visible
// rows, →← to fold or step in/out, Home/End, and a roving `tabindex` so the whole list is
// one Tab stop rather than three per card.
//
// **Why not just reuse treenav.ts's `arrowMove`?** Because one thing genuinely differs,
// and it's the thing the pattern turns on: a file tree row expands to reveal *child rows*,
// while a discovery card expands to reveal *its own body* (the path + snippet). So
// "foldable" and "has child rows" come apart here — a card is the first, never the second
// — and the two moves read differently enough that one function pretending to serve both
// would need a predicate per branch. Two small tested walks beat one parameterized one;
// the duplication is ~20 lines and the semantics are stated in each.
//
// As in treenav.ts, **which key means which move is the registry's** since #121:
// `sideArrowMove` switches on a binding id (`side.row.next`), not on `e.key`, so the
// pane's arrows are rebindable and visible to the conflict checker. The pane has a scope
// of its own (`side`) rather than sharing the tree's — they are siblings, so ↑ can mean
// "previous row" in both, and rebinding one leaves the other alone.

import { type BindingId, type KeyEventLike, boundOf } from "./bindings.ts";
// A *value* import, and the one dependency this module has on a sibling: chat is the
// right column's third mode (GH #155), so its rows are emitted here alongside search's
// and discovery's. chat.ts imports only the `SideRow` *type* back, which erases — so the
// two modules read as a pair without a runtime cycle.
import { type ChatMessage, chatRows } from "./chat.ts";
import type { SideSection } from "./state";
import type { NeighborView, NoteView, SearchResult, SimilarView, UnresolvedLink } from "./types";

/** What →← folds on a row: the section's sticky viewing preference, or a card's body. */
export type SideFold =
  | { kind: "section"; section: SideSection }
  | { kind: "card"; key: string };

/**
 * One navigable row of the side pane, in paint order.
 *
 * `key` is the row's identity *across a repaint* — what the roving tabstop and the focus
 * restoration in `paintSide` are keyed by, since the element holding focus does not
 * survive an `innerHTML` swap. It carries the list position because a card's *path* is
 * not unique: two edges to one target are legal (data-model.md §2's "augment" case) and
 * share a single fold key, but two rows that answer to one key would make ↓ off the
 * second land back under the first.
 */
export interface SideRow {
  key: string;
  /** 0 for a section head (and a flat search result), 1 for a card under a head. */
  depth: number;
  /** Null when there is nothing to fold: a search result, an unresolved link. */
  fold: SideFold | null;
  /** Folded rows still navigate; only their body/cards leave the row list. */
  expanded: boolean;
  /** Rows nested under this one — a section with cards. Never true for a card. */
  hasChildRows: boolean;
}

/** The slice of `AppState` the row list is derived from (so `sideRows(state)` just works,
 *  and a test can build one by hand). Mirrors the paint's inputs exactly. */
export interface SideNavState {
  /** The right column is showing the chat pane (GH #155) — its third mode, and the one
   *  that wins: chat and search both own the whole column, and opening either closes the
   *  other (main.ts), so a single flag decides which list this is. */
  chatOpen: boolean;
  /** The conversation, when chat is open — session-only (S4), held in `AppState`. */
  chatMessages: readonly ChatMessage[];
  /** The answer streaming right now, or null between turns — carried in `AppState`'s own
   *  shape (the text, not a flag) so this slice stays a literal mirror of the paint's
   *  inputs; all the row list needs of it is whether there is one. */
  chatStreaming: string | null;
  searchQuery: string;
  searchResults: readonly SearchResult[];
  /** A search in flight: the pane paints "Searching…", so the *previous* query's
   *  results are not on screen and must not be navigable either. */
  loading: boolean;
  /** Discovery is an empty-state hint until a note is open. */
  current: NoteView | null;
  similar: readonly SimilarView[];
  connections: readonly NeighborView[];
  unresolved: readonly UnresolvedLink[];
  collapsedSections: ReadonlySet<SideSection>;
  collapsedCards: ReadonlySet<string>;
}

/** A card's per-note **fold** key — unique across the two sections a path can appear in,
 *  and deliberately path-keyed: folding a card folds it for that note, wherever it sits. */
export function cardKey(section: SideSection, path: string): string {
  return `${section}:${path}`;
}

/** A section head's row key. */
export function sectionRowKey(section: SideSection): string {
  return `section:${section}`;
}

/** A card's **row** key: its list position plus what it points at. Both halves matter —
 *  the position makes it unique, the target makes a stale key fail to match (and so fall
 *  back) rather than silently resolve to whatever card now sits at that index. */
export function cardRowKey(group: string, index: number, id: string): string {
  return `${group}:${index}:${id}`;
}

/**
 * Every row the side pane currently paints, in paint order: in chat mode the transcript
 * (chat.ts, which emits these same rows); in search mode the flat list of results; in
 * discovery each section's head followed by its cards (only while the section is
 * expanded), the Connections section carrying its unresolved links last.
 *
 * Mirrors render.ts's branch order exactly, including the states that paint *no* rows —
 * no note open, a search still running, a section with nothing in it.
 */
export function sideRows(s: SideNavState): SideRow[] {
  if (s.chatOpen) return chatRows(s.chatMessages, s.chatStreaming !== null);
  if (s.searchQuery) {
    if (s.loading) return [];
    return s.searchResults.map((r, i) => ({
      key: cardRowKey("search", i, r.path),
      depth: 0,
      fold: null,
      expanded: false,
      hasChildRows: false,
    }));
  }
  if (s.current === null) return [];

  const rows: SideRow[] = [];
  const section = (id: SideSection, cards: () => SideRow[]): void => {
    const open = !s.collapsedSections.has(id);
    const children = open ? cards() : [];
    rows.push({
      key: sectionRowKey(id),
      depth: 0,
      fold: { kind: "section", section: id },
      expanded: open,
      hasChildRows: children.length > 0,
    });
    rows.push(...children);
  };
  const card = (group: SideSection, index: number, path: string): SideRow => {
    const key = cardKey(group, path);
    return {
      key: cardRowKey(group, index, path),
      depth: 1,
      fold: { kind: "card", key },
      expanded: !s.collapsedCards.has(key),
      hasChildRows: false,
    };
  };

  section("connections", () => [
    ...s.connections.map((c, i) => card("connections", i, c.path)),
    // An unresolved link points at nothing, so it has no body to fold and nothing to
    // open — but it is a row you can see, and a row you can see must be a row you can
    // reach (K1). ⏎ on it simply does nothing.
    ...s.unresolved.map((u, i) => ({
      key: cardRowKey("unresolved", i, u.target),
      depth: 1,
      fold: null,
      expanded: false,
      hasChildRows: false,
    })),
  ]);
  section("similar", () => s.similar.map((c, i) => card("similar", i, c.path)));
  return rows;
}

/** Where `key` sits in the row list, or -1 (folded away, or replaced by another note). */
export function sideRowIndex(rows: readonly SideRow[], key: string | null): number {
  if (key === null) return -1;
  return rows.findIndex((r) => r.key === key);
}

/**
 * The one row that carries `tabindex="0"` — the roving tabstop that makes the whole pane a
 * *single* Tab stop instead of three per card. The row the keyboard last focused, else the
 * first row, so ⌘3 lands where you left off and ⇥ never skips a populated pane.
 */
export function rovingSideKey(rows: readonly SideRow[], focus: string | null): string | null {
  if (sideRowIndex(rows, focus) !== -1) return focus;
  return rows.length > 0 ? rows[0].key : null;
}

/** What one navigation key does: move the focus, or fold what the focused row folds.
 *  Every variant names a row `key` (treenav.ts's `TreeMove` carries `path` the same way):
 *  the row to focus, or — folding keeps the focus put — the row that stays focused. */
export type SideMove =
  | { kind: "focus"; key: string }
  | { kind: "expand"; key: string; fold: SideFold }
  | { kind: "collapse"; key: string; fold: SideFold };

/** The enclosing section head's row key — what ← steps out to. */
export function parentSideKey(rows: readonly SideRow[], index: number): string | null {
  for (let i = index - 1; i >= 0; i--) {
    if (rows[i].depth < rows[index].depth) return rows[i].key;
  }
  return null;
}

/** The navigation commands the discovery pane answers to — registry ids, in the order the
 *  dispatcher tries them (treenav.ts's `TREE_NAV` is the sibling of this). */
export const SIDE_NAV = [
  "side.row.prev",
  "side.row.next",
  "side.row.first",
  "side.row.last",
  "side.row.in",
  "side.row.out",
] as const satisfies readonly BindingId[];

export type SideNav = (typeof SIDE_NAV)[number];

/** Which discovery move — if any — this keystroke is, per the live registry. */
export function sideNavFor(e: KeyEventLike): SideNav | null {
  return boundOf(e, SIDE_NAV);
}

/**
 * The ARIA tree-pattern move for one navigation command, or null when there is nowhere to
 * go (so the caller leaves the event alone rather than swallowing it):
 *
 *   next / prev    next / previous **visible** row (never into a collapsed section)
 *   first / last   first / last row
 *   in             folded → unfold; an open section steps to its first card; an open card
 *                  stays put (its body is content, not rows)
 *   out            open → fold; anything else steps out to its section head
 *
 * `from` is -1 when nothing is focused yet, which next/first resolve to the first row and
 * prev/last to the last — so the first arrow press always lands.
 */
export function sideArrowMove(
  rows: readonly SideRow[],
  from: number,
  nav: SideNav,
): SideMove | null {
  if (rows.length === 0) return null;
  const last = rows.length - 1;
  const at = (i: number): SideMove => ({ kind: "focus", key: rows[i].key });
  switch (nav) {
    case "side.row.first":
      return at(0);
    case "side.row.last":
      return at(last);
    case "side.row.next":
      if (from < 0) return at(0);
      return from < last ? at(from + 1) : null;
    case "side.row.prev":
      if (from < 0) return at(last);
      return from > 0 ? at(from - 1) : null;
    case "side.row.in": {
      if (from < 0) return at(0);
      const row = rows[from];
      if (row.fold !== null && !row.expanded)
        return { kind: "expand", key: row.key, fold: row.fold };
      // Open: the next row *is* the first child, when there is one (`sideRows` emits it
      // inline). A card never has one — its body is content, so → is spent there.
      //
      // Reached for an **unfoldable** row too, which is the ARIA pattern's own rule
      // rather than a special case: "nothing to fold" and "no children" are different
      // facts, and only the second one stops →. Discovery has no such row (a search
      // result and a broken link are both childless, so this still returns null for
      // them); chat's turns are exactly it — nothing folds in a conversation, but an
      // answer's citations are rows underneath it (chat.ts).
      return row.hasChildRows && from < last && rows[from + 1].depth > row.depth
        ? at(from + 1)
        : null;
    }
    case "side.row.out": {
      if (from < 0) return at(0);
      const row = rows[from];
      if (row.fold !== null && row.expanded)
        return { kind: "collapse", key: row.key, fold: row.fold };
      const parent = parentSideKey(rows, from);
      return parent === null ? null : { kind: "focus", key: parent };
    }
  }
}
