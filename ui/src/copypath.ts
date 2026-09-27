// Pure path logic for the tree menu's two copy actions: the vault path (what B2 speaks,
// and the index key) and the system path (what Finder or a terminal needs).

/**
 * The absolute on-disk path of a vault-relative `path`, under `vaultRoot`. A plain join,
 * tolerating a trailing slash on the root; an empty `path` is the root itself. `/` is
 * hard-coded and `\` left alone: B2 ships on macOS only, where `\` is a filename character.
 */
export function systemPath(vaultRoot: string, path: string): string {
  const root = vaultRoot.replace(/\/+$/, "");
  if (path === "") return root === "" ? "/" : root;
  return `${root}/${path}`;
}
