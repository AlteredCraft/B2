// Pure path logic for the tree's move/rename affordances (rename input, Move…
// modal, drag-and-drop) — no DOM, no IPC — so node runs its test straight off the
// source (`npm test`), like newentry.ts. The host re-validates every destination
// (`move_note`/`move_resource`/`move_dir` refuse absolute/escaping/occupied
// paths); these helpers resolve the *destination* from a gesture and keep honest
// input from round-tripping through a generic error.

// Full-filename import so node's type-stripping can run move.test.ts off the
// source (allowImportingTsExtensions; the panes.test.ts precedent).
import { joinPath, normalizeName, parentDir } from "./newentry.ts";

/** The three kinds of tree node a move/rename gesture can target. */
export type NodeKind = "note" | "resource" | "folder";

/** The last path segment — a file's name (with extension) or a folder's name. */
export function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? path : path.slice(i + 1);
}

/**
 * Classify a document reference by shape alone — the UI twin of the core's
 * `doc_kind` (b2-core `resource.rs`), the *one* rule shared with the adapters'
 * argument dispatch: **an extension other than `md` means a resource; `.md` or
 * no extension means a note** (which also covers the extensionless wikilink
 * habit `[[concepts/memory]]`). A trailing `#fragment` is
 * dropped first, matching the core's link resolver. Used to route a followed
 * wikilink to the note pane vs the resource pane so a `[[file.pdf]]` opens the
 * resource card instead of failing a note read; the host re-validates either way.
 */
export function refKind(ref: string): "note" | "resource" {
  const name = baseName(ref.split("#")[0]).trim();
  const dot = name.lastIndexOf(".");
  // dot <= 0 covers no extension (-1) and a leading-dot dotfile (empty stem).
  if (dot <= 0) return "note";
  const ext = name.slice(dot + 1);
  return ext !== "" && ext.toLowerCase() !== "md" ? "resource" : "note";
}

/**
 * What the inline rename input starts out holding. Notes drop the `.md` (the
 * tree labels notes without it, and the host re-appends it); resources keep
 * their extension (it *is* the kind identity); folders are just the name.
 */
export function renamePrefill(path: string, kind: NodeKind): string {
  const base = baseName(path);
  return kind === "note" ? base.replace(/\.md$/i, "") : base;
}

/**
 * Resolve a typed rename into the full destination path, or null to back out —
 * nothing valid typed (empty / traversal, per `normalizeName`), or a name that
 * resolves to the node's current path (a no-op rename is a cancel, not an
 * error). Nesting is allowed (`archive/idea` renames *and* moves, like the
 * create input); a note's `.md` is re-appended so no-op detection compares like
 * with like.
 */
export function renameDestination(path: string, kind: NodeKind, raw: string): string | null {
  const name = normalizeName(raw);
  if (name === null) return null;
  const withExt = kind === "note" && !/\.md$/i.test(name) ? `${name}.md` : name;
  const dest = joinPath(parentDir(path), withExt);
  return dest === path ? null : dest;
}

/**
 * Is `path` the folder `dir` itself or anything inside it? Segment-aware, so a
 * prefix-sharing sibling (`notes2/x` against `notes`) is not inside. `dir` names a
 * folder, never the vault root (`""`): nothing is "within" the root in this sense.
 */
export function isWithin(path: string, dir: string): boolean {
  return path === dir || path.startsWith(`${dir}/`);
}

/** The folder a tree node stands for when something is created, dropped or imported
 *  "here": a folder is its own context, a file its parent's. */
export function folderContext(path: string, kind: NodeKind): string {
  return kind === "folder" ? path : parentDir(path);
}

/** The destination path for "move `srcPath` into `destDir`" — same name, new folder. */
export function moveDestination(srcPath: string, destDir: string): string {
  return joinPath(destDir, baseName(srcPath));
}

/** Why a destination can't take a node: it is already there, or it is the folder
 *  being moved (or inside it). */
export type MoveRefusal = "current-folder" | "inside-itself";

/**
 * Why dropping / moving `srcPath` (of `kind`) into `destDir` is not a real move, or
 * null when it is: its current folder is a no-op, and a folder can't go into itself or
 * any of its own descendants (the host refuses those too — this keeps the gesture
 * honest before the IPC round-trip, and lets the Move… modal say which).
 */
export function moveRefusal(srcPath: string, kind: NodeKind, destDir: string): MoveRefusal | null {
  if (destDir === parentDir(srcPath)) return "current-folder";
  if (kind === "folder" && isWithin(destDir, srcPath)) return "inside-itself";
  return null;
}

/** Whether moving `srcPath` into `destDir` is a real move (`moveRefusal` has no reason). */
export function canMoveInto(srcPath: string, kind: NodeKind, destDir: string): boolean {
  return moveRefusal(srcPath, kind, destDir) === null;
}

/**
 * Every folder the Move… modal offers: the vault root (`""`) first, then every
 * folder on disk (`list_dirs` — empty folders included, since the fs is
 * authoritative for structure), deduped and sorted.
 */
export function allDirs(dirs: string[]): string[] {
  return ["", ...[...new Set(dirs)].sort()];
}

/**
 * Where `path` lands after `from` moved to `to`: `to` itself for an exact match,
 * the prefix-remapped path for anything inside a moved folder, and null when the
 * move doesn't touch it. One function serves file moves (exact) and folder moves
 * (prefix) — a prefix-sharing sibling (`notes2/x` under a move of `notes`) is
 * never remapped.
 */
export function remapPath(path: string, from: string, to: string): string | null {
  if (path === from) return to;
  if (path.startsWith(`${from}/`)) return `${to}/${path.slice(from.length + 1)}`;
  return null;
}
