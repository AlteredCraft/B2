// Pure path logic for the tree's create affordances (new note / new folder) — no
// DOM, no IPC — so node runs its test straight off the source (`npm test`), like
// panes.ts/graph.ts. The host re-validates every path (`create_note` refuses
// absolute/escaping/occupied destinations); these helpers just resolve the
// creation *context* and keep honest input from round-tripping through a generic
// error.

/** The folder containing `path` ("" for a root-level entry). */
export function parentDir(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

/**
 * Normalize a typed entry name into a clean vault-relative fragment, or null when
 * nothing valid was typed (a null is a *cancel*, not an error — an empty input is
 * how you back out). Forgiving on shape — trims, treats `\` as `/`, drops empty
 * segments (so `a//b`, `/a`, `a/` all resolve) and allows nesting
 * (`projects/2026`) — but refuses traversal (`.`/`..` segments).
 */
export function normalizeName(input: string): string | null {
  const segs = input
    .replace(/\\/g, "/")
    .split("/")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  if (segs.length === 0) return null;
  if (segs.some((s) => s === "." || s === "..")) return null;
  return segs.join("/");
}

/** Join a context folder and a normalized name into a vault-relative path. */
export function joinPath(dir: string, name: string): string {
  return dir ? `${dir}/${name}` : name;
}

/**
 * Every folder prefix of `path`, shallowest first: `a/b/c` → `["a","a/b","a/b/c"]`.
 * Empty for "" — the root needs no expansion or staging. Feeds both the staged
 * `pendingDirs` set (each level renders as a folder) and `expandedDirs` (reveal
 * the whole chain down to a new entry).
 */
export function dirChain(path: string): string[] {
  if (!path) return [];
  const out: string[] = [];
  let acc = "";
  for (const seg of path.split("/")) {
    acc = acc ? `${acc}/${seg}` : seg;
    out.push(acc);
  }
  return out;
}
