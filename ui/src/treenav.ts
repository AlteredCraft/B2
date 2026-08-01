// The file tree's pure logic — no DOM, no IPC — so node runs its test straight off
// the source (`npm test`), like newentry.ts / move.ts.
//
// Two jobs, deliberately in one module because they must agree: fold the flat
// `list_notes` / `list_resources` / `list_dirs` listings into one nested folder tree
// (`buildTree`), and flatten the *currently visible* rows of that tree into the single
// ordered list keyboard navigation walks (`visibleRows`). The paint (render.ts's
// `treeChildrenHtml`) and the arrow keys must agree on row order down to the last
// tie-break, so the sort lives here once and both sides call it — a tree you can
// arrow through in a different order than you can see is worse than no arrows at all.
//
// Invariant K1 (docs/design/invariants.md, GH #78): the tree is fully navigable from
// the keyboard, following the ARIA `tree` pattern (up/down between visible rows,
// right/left to expand/collapse or step in/out, Home/End, first-letter typeahead).
//
// **Which key means which move is not this module's business** (#121). `arrowMove` used
// to switch on `e.key`, which made this file the owner of "ArrowDown means next row" —
// fine for a fixed keyboard, and wrong once a user can rebind, since it put the arrows
// beyond the reach of both the recorder and the conflict checker. It switches on a
// registry command id now (`tree.row.next`), so bindings.ts owns key → command and this
// module owns command → move. The rule the registry states is unchanged; only the seam
// between the two halves moved.

import { type BindingId, type KeyEventLike, boundOf } from "./bindings.ts";
import { type IconName, NOTE_ICON, resourceIcon } from "./icons.ts";
import type { NodeKind } from "./move";
import type { NoteSummary, ResourceSummary } from "./types";

/** One tree leaf — a note or a resource, normalized for display. */
export interface TreeFile {
  kind: "note" | "resource";
  path: string;
  label: string;
  /** The row's icon, as a registry *name* — resolved to markup by render.ts, so this
   *  module stays DOM-free and node can run its test off the source. */
  icon: IconName;
}

export interface TreeDir {
  name: string;
  /** Vault-relative folder path, no trailing slash ("" for the root). */
  path: string;
  dirs: Map<string, TreeDir>;
  files: TreeFile[];
}

/** A note's display label: its title, else the filename without the `.md`. */
export function fileLabel(note: NoteSummary): string {
  if (note.title) return note.title;
  const base = note.path.split("/").pop() ?? note.path;
  return base.replace(/\.md$/i, "");
}

/** Fold the flat, path-ordered note + resource lists into one nested folder tree.
 *  `dirs` is the vault's full folder list (`list_dirs`, a live fs walk) — the
 *  structure half of the tree, so a folder renders even when it holds no file
 *  (the fs supports empty folders, so the tree does); a folder that also appears
 *  as a file's path prefix merges harmlessly. */
export function buildTree(
  notes: NoteSummary[],
  resources: ResourceSummary[],
  dirs: Iterable<string>,
): TreeDir {
  const root: TreeDir = { name: "", path: "", dirs: new Map(), files: [] };
  const descend = (dirPath: string): TreeDir => {
    let dir = root;
    if (!dirPath) return dir;
    for (const seg of dirPath.split("/")) {
      const full = dir.path ? `${dir.path}/${seg}` : seg;
      let child = dir.dirs.get(seg);
      if (!child) {
        child = { name: seg, path: full, dirs: new Map(), files: [] };
        dir.dirs.set(seg, child);
      }
      dir = child;
    }
    return dir;
  };
  const insert = (file: TreeFile) => {
    const parts = file.path.split("/");
    descend(parts.slice(0, -1).join("/")).files.push(file);
  };
  for (const note of notes) {
    insert({ kind: "note", path: note.path, label: fileLabel(note), icon: NOTE_ICON });
  }
  for (const r of resources) {
    insert({
      kind: "resource",
      path: r.path,
      label: r.path.split("/").pop() ?? r.path,
      icon: resourceIcon(r.class),
    });
  }
  for (const dir of dirs) {
    descend(dir);
  }
  return root;
}

/** The paint order for one folder's sub-folders: by name. */
export function sortedSubdirs(dir: TreeDir): TreeDir[] {
  return [...dir.dirs.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/** The paint order for one folder's files: by display label (sub-folders first,
 *  then files — the caller's concatenation order, mirrored by `visibleRows`). */
export function sortedFiles(dir: TreeDir): TreeFile[] {
  return [...dir.files].sort((a, b) => a.label.localeCompare(b.label));
}

/** One navigable row, in paint order. Rows are identified by `path` alone: the
 *  filesystem guarantees a folder and a file can never share one. */
export interface TreeRow {
  path: string;
  nodeKind: NodeKind;
  /** The text the row shows — what first-letter typeahead matches against. */
  label: string;
  /** 0 at the vault root; ARIA's `aria-level` is this + 1. */
  depth: number;
  /** Folders: expanded right now. Always false for a file. */
  expanded: boolean;
  /** Folders: has something under it to step into. Always false for a file. */
  hasChildren: boolean;
}

/**
 * Every row the tree currently paints, in paint order: for each folder — its own
 * row, then (only when expanded) its children — and then the folder's files. The
 * inline create/rename inputs are deliberately absent: they are text entry, not
 * navigable rows, and they own the keyboard while they're open.
 */
export function visibleRows(root: TreeDir, expanded: ReadonlySet<string>): TreeRow[] {
  const rows: TreeRow[] = [];
  const walk = (dir: TreeDir, depth: number): void => {
    for (const sub of sortedSubdirs(dir)) {
      const open = expanded.has(sub.path);
      rows.push({
        path: sub.path,
        nodeKind: "folder",
        label: sub.name,
        depth,
        expanded: open,
        hasChildren: sub.dirs.size > 0 || sub.files.length > 0,
      });
      if (open) walk(sub, depth + 1);
    }
    for (const file of sortedFiles(dir)) {
      rows.push({
        path: file.path,
        nodeKind: file.kind,
        label: file.label,
        depth,
        expanded: false,
        hasChildren: false,
      });
    }
  };
  walk(root, 0);
  return rows;
}

/** Where `path` sits in the visible list, or -1 (gone: collapsed away, deleted, moved). */
export function rowIndex(rows: readonly TreeRow[], path: string | null): number {
  if (path === null) return -1;
  return rows.findIndex((r) => r.path === path);
}

/**
 * The one row that carries `tabindex="0"` — the roving tabstop that makes the whole
 * tree a *single* stop in the Tab order instead of one stop per file (a 1500-note
 * vault is not a tab sequence). Preference order: the row the keyboard last focused,
 * else the open document's row, else the first row — so Tab always lands somewhere
 * meaningful, and lands where you left off.
 */
export function rovingPath(
  rows: readonly TreeRow[],
  focus: string | null,
  activePath: string | null,
): string | null {
  if (rowIndex(rows, focus) !== -1) return focus;
  if (rowIndex(rows, activePath) !== -1) return activePath;
  return rows.length > 0 ? rows[0].path : null;
}

/**
 * Where keyboard focus should land once `path` leaves the tree (a delete): the next
 * visible row *outside* it — a folder takes its whole expanded subtree with it, so the
 * next row at or above its depth — else the row before it, else nothing. Null when
 * `path` isn't in the list at all.
 */
export function neighborPath(rows: readonly TreeRow[], path: string): string | null {
  const i = rowIndex(rows, path);
  if (i === -1) return null;
  const depth = rows[i].depth;
  for (let j = i + 1; j < rows.length; j++) {
    if (rows[j].depth <= depth) return rows[j].path;
  }
  return i > 0 ? rows[i - 1].path : null;
}

/** What one navigation key does: move the focus, or fold the focused folder. */
export type TreeMove =
  | { kind: "focus"; path: string }
  | { kind: "expand"; path: string }
  | { kind: "collapse"; path: string };

/** The enclosing folder's row path — what ArrowLeft steps out to. */
export function parentRowPath(rows: readonly TreeRow[], index: number): string | null {
  const depth = rows[index].depth;
  for (let i = index - 1; i >= 0; i--) {
    if (rows[i].depth < depth) return rows[i].path;
  }
  return null;
}

/** The navigation commands the tree answers to — registry ids, in the order the
 *  dispatcher tries them. `satisfies` ties them to the table: deleting or misspelling a
 *  row in bindings.ts is a compile error here rather than an arrow that stops working. */
export const TREE_NAV = [
  "tree.row.prev",
  "tree.row.next",
  "tree.row.first",
  "tree.row.last",
  "tree.row.in",
  "tree.row.out",
] as const satisfies readonly BindingId[];

export type TreeNav = (typeof TREE_NAV)[number];

/** Which tree move — if any — this keystroke is, per the live registry. */
export function treeNavFor(e: KeyEventLike): TreeNav | null {
  return boundOf(e, TREE_NAV);
}

/**
 * The ARIA tree-pattern move for one navigation command, or null when there is nowhere
 * to go (so the caller leaves the event alone rather than swallowing it):
 *
 *   next / prev    next / previous **visible** row (never into a collapsed folder)
 *   first / last   first / last visible row
 *   in             a collapsed folder expands; an expanded one steps to its first child
 *   out            an expanded folder collapses; anything else steps out to its parent
 *
 * `from` is -1 when nothing is focused yet, which next/first resolve to the first row
 * and prev/last to the last — so the first arrow press always lands.
 */
export function arrowMove(rows: readonly TreeRow[], from: number, nav: TreeNav): TreeMove | null {
  if (rows.length === 0) return null;
  const last = rows.length - 1;
  const at = (i: number): TreeMove => ({ kind: "focus", path: rows[i].path });
  switch (nav) {
    case "tree.row.first":
      return at(0);
    case "tree.row.last":
      return at(last);
    case "tree.row.next":
      if (from < 0) return at(0);
      return from < last ? at(from + 1) : null;
    case "tree.row.prev":
      if (from < 0) return at(last);
      return from > 0 ? at(from - 1) : null;
    case "tree.row.in": {
      if (from < 0) return at(0);
      const row = rows[from];
      if (row.nodeKind !== "folder") return null;
      if (!row.expanded) return row.hasChildren ? { kind: "expand", path: row.path } : null;
      // Expanded: the next row *is* the first child (visibleRows emits it inline).
      return from < last && rows[from + 1].depth > row.depth ? at(from + 1) : null;
    }
    case "tree.row.out": {
      if (from < 0) return at(0);
      const row = rows[from];
      if (row.nodeKind === "folder" && row.expanded) return { kind: "collapse", path: row.path };
      const parent = parentRowPath(rows, from);
      return parent === null ? null : { kind: "focus", path: parent };
    }
  }
}

/**
 * First-letter typeahead: the next row after `from` whose label starts with `ch`,
 * wrapping past the end and back around to `from` itself. Null when nothing matches
 * — the keystroke then falls through to the app's chords untouched.
 */
export function typeaheadTarget(
  rows: readonly TreeRow[],
  from: number,
  ch: string,
): string | null {
  const needle = ch.toLowerCase();
  const n = rows.length;
  if (n === 0 || needle === "") return null;
  const start = from < 0 ? -1 : from;
  for (let k = 1; k <= n; k++) {
    const row = rows[(start + k + n) % n];
    if (row.label.toLowerCase().startsWith(needle)) return row.path;
  }
  return null;
}
