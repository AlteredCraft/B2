// The right column's row order and keyboard walk (pure; treenav.ts's sibling). The paint
// (render.ts) and the arrow keys both derive from here so their order can't drift (K1,
// GH #78).
//
// Not treenav.ts's `arrowMove`: a tree row expands to reveal child rows, a discovery card
// to reveal its own body, so "foldable" and "has child rows" come apart here. Keys come
// from the registry's own `side` scope (#121), rebindable apart from the tree's.

import { type BindingId, type KeyEventLike, boundOf } from "./bindings.ts";
// chat.ts imports only the `SideRow` type back, so there is no runtime cycle.
import { type ChatMessage, chatRows } from "./chat.ts";
import type { SideSection } from "./state";
import type { NeighborView, NoteView, SearchResult, SimilarView, UnresolvedLink } from "./types";

/** What →← folds on a row: the section's sticky viewing preference, or a card's body. */
export type SideFold =
  | { kind: "section"; section: SideSection }
  | { kind: "card"; key: string };

/**
 * One navigable row of the side pane. `key` is its identity across a repaint, and carries
 * the list position because two edges to one target are legal (data-model.md §2).
 */
export interface SideRow {
  key: string;
  /** 0 for a section head (and a flat search result), 1 for a card under a head. */
  depth: number;
  /** Null when there is nothing to fold. */
  fold: SideFold | null;
  /** Folded rows still navigate; only their body/cards leave the row list. */
  expanded: boolean;
  /** Rows nested under this one. Never true for a card. */
  hasChildRows: boolean;
}

/** The slice of `AppState` the row list derives from; mirrors the paint's inputs. */
export interface SideNavState {
  /** The column shows chat (GH #155), which wins over search and discovery. */
  chatOpen: boolean;
  /** The conversation, session-only (S4). */
  chatMessages: readonly ChatMessage[];
  /** The streaming answer, or null; only its presence matters here. */
  chatStreaming: string | null;
  searchQuery: string;
  searchResults: readonly SearchResult[];
  /** A search in flight: the previous results aren't on screen, so aren't navigable. */
  loading: boolean;
  /** Discovery is an empty-state hint until a note is open. */
  current: NoteView | null;
  similar: readonly SimilarView[];
  connections: readonly NeighborView[];
  unresolved: readonly UnresolvedLink[];
  collapsedSections: ReadonlySet<SideSection>;
  collapsedCards: ReadonlySet<string>;
}

/** A card's fold key: path-keyed, so folding follows the note wherever it sits. */
export function cardKey(section: SideSection, path: string): string {
  return `${section}:${path}`;
}

/** A section head's row key. */
export function sectionRowKey(section: SideSection): string {
  return `section:${section}`;
}

/** A card's row key. The position makes it unique; the target makes a stale key fail to
 *  match rather than resolve to whatever card now sits there. */
export function cardRowKey(group: string, index: number, id: string): string {
  return `${group}:${index}:${id}`;
}

/**
 * Every row the side pane paints, in paint order: the chat transcript, the search
 * results, or each discovery section's head and cards. Mirrors render.ts's branches
 * exactly, including those that paint no rows.
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
    // Nothing to fold or open, but a visible row must be reachable (K1).
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
 * The roving tabstop: the row last focused, else the first, so ⇥ never skips a populated
 * pane.
 */
export function rovingSideKey(rows: readonly SideRow[], focus: string | null): string | null {
  if (sideRowIndex(rows, focus) !== -1) return focus;
  return rows.length > 0 ? rows[0].key : null;
}

/** What one navigation key does: move focus, or fold the focused row (which keeps focus). */
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

/** The pane's navigation commands, in the order the dispatcher tries them. */
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
 * The ARIA tree-pattern move for one command, or null when there is nowhere to go (so the
 * caller leaves the event alone). `from` is -1 when nothing is focused yet.
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
      // Open, or unfoldable: step to the first child row if any. Only "no children"
      // stops →; a chat turn doesn't fold but has citation rows.
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
