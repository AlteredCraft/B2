// The file tree's pure logic: fold the flat listings into one folder tree (`buildTree`)
// and flatten its visible rows into the order the keyboard walks (`visibleRows`). The paint
// and the arrow keys share the sort here so their order can't drift (K1, GH #78).
//
// bindings.ts owns key → command (#121); this module owns command → move.

import { type BindingId, type KeyEventLike, boundOf } from "./bindings.ts";
import { type IconName, NOTE_ICON, resourceIcon } from "./icons.ts";
import type { NodeKind } from "./move";
import type { NoteSummary, ResourceSummary } from "./types";

/** One tree leaf — a note or a resource, normalized for display. */
export interface TreeFile {
  kind: "note" | "resource";
  path: string;
  label: string;
  /** The row's icon name; render.ts resolves it to markup, keeping this module DOM-free. */
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

/** Fold the flat note and resource lists into one folder tree. `dirs` (a live fs walk)
 *  makes empty folders render too. */
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

/** The paint order for one folder's files: by display label, after the sub-folders. */
export function sortedFiles(dir: TreeDir): TreeFile[] {
  return [...dir.files].sort((a, b) => a.label.localeCompare(b.label));
}

/** One navigable row, identified by `path` (a folder and a file can't share one). */
export interface TreeRow {
  path: string;
  nodeKind: NodeKind;
  /** The text the row shows, which typeahead matches. */
  label: string;
  /** 0 at the vault root; ARIA's `aria-level` is this + 1. */
  depth: number;
  /** Folders: expanded right now. Always false for a file. */
  expanded: boolean;
  /** Folders: has something under it to step into. Always false for a file. */
  hasChildren: boolean;
}

/**
 * Every row the tree paints, in paint order. The inline create/rename inputs are absent:
 * they are text entry, not navigable rows.
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
 * The roving tabstop that makes the tree a single Tab stop: the row last focused, else the
 * open document's row, else the first row.
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
 * Where focus lands once `path` is deleted: the next visible row outside it (a folder takes
 * its subtree), else the row before it. Null when `path` isn't in the list.
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

/** The tree's navigation commands, in the order the dispatcher tries them. */
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
 * The ARIA tree-pattern move for one command, or null when there is nowhere to go (so the
 * caller leaves the event alone). `from` is -1 when nothing is focused yet.
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
 * First-letter typeahead: the next row after `from` whose label starts with `ch`, wrapping.
 * Null when nothing matches, so the keystroke falls through.
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
