// Pure path logic for the tree's new note / new folder. The host re-validates every path
// (`create_note`); these resolve the creation context and keep honest input from hitting
// a generic error.

/** The folder containing `path` ("" for a root-level entry). */
export function parentDir(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

/**
 * Normalize a typed name into a vault-relative fragment, or null (a cancel) when nothing
 * valid was typed. Forgiving on shape (trims, `\` as `/`, empty segments dropped,
 * nesting allowed) but refuses `.`/`..` traversal.
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
 * Feeds `expandedDirs` to reveal a new or renamed entry.
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
