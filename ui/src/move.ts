// Pure path logic for the tree's move/rename gestures. The host re-validates every
// destination; these resolve it from a gesture and catch honest mistakes early.

// Full-filename import for node's type-stripping.
import { joinPath, normalizeName, parentDir } from "./newentry.ts";

/** The three kinds of tree node a move/rename gesture can target. */
export type NodeKind = "note" | "resource" | "folder";

/** The last path segment — a file's name (with extension) or a folder's name. */
export function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? path : path.slice(i + 1);
}

/**
 * Classify a reference by shape, the twin of b2-core's `doc_kind`: an extension other than
 * `md` is a resource; `.md` or none is a note. A `#fragment` is dropped first. Routes a
 * followed wikilink to the right pane.
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
 * The rename input's prefill. Notes drop `.md` (the host re-appends it); resources keep
 * their extension, which is their kind.
 */
export function renamePrefill(path: string, kind: NodeKind): string {
  const base = baseName(path);
  return kind === "note" ? base.replace(/\.md$/i, "") : base;
}

/**
 * A typed rename's full destination, or null to back out (nothing valid typed, or a no-op
 * rename). Nesting is allowed: `archive/idea` renames and moves.
 */
export function renameDestination(path: string, kind: NodeKind, raw: string): string | null {
  const name = normalizeName(raw);
  if (name === null) return null;
  const withExt = kind === "note" && !/\.md$/i.test(name) ? `${name}.md` : name;
  const dest = joinPath(parentDir(path), withExt);
  return dest === path ? null : dest;
}

/**
 * Is `path` the folder `dir` or inside it? Segment-aware (`notes2/x` is not in `notes`).
 * `dir` is never the vault root.
 */
export function isWithin(path: string, dir: string): boolean {
  return path === dir || path.startsWith(`${dir}/`);
}

/** The folder "here" means for a tree node: a folder itself, a file's parent. */
export function folderContext(path: string, kind: NodeKind): string {
  return kind === "folder" ? path : parentDir(path);
}

/** The destination path for "move `srcPath` into `destDir`" — same name, new folder. */
export function moveDestination(srcPath: string, destDir: string): string {
  return joinPath(destDir, baseName(srcPath));
}

/** Why a destination can't take a node. */
export type MoveRefusal = "current-folder" | "inside-itself";

/**
 * Why moving `srcPath` into `destDir` is not a real move (already there, or a folder into
 * itself), or null when it is. The host refuses these too; this lets the modal say which.
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

/** Every folder the Move… modal offers: the root (`""`) first, then all folders, sorted. */
export function allDirs(dirs: string[]): string[] {
  return ["", ...[...new Set(dirs)].sort()];
}

/**
 * Where `path` lands after `from` moved to `to` (exact or folder-prefix match), or null
 * when the move doesn't touch it. A prefix-sharing sibling is never remapped.
 */
export function remapPath(path: string, from: string, to: string): string | null {
  if (path === from) return to;
  if (path.startsWith(`${from}/`)) return `${to}/${path.slice(from.length + 1)}`;
  return null;
}
