// Pure path logic for the tree menu's two copy actions — no DOM, no IPC — so node
// runs its test straight off the source (`npm test`), like newentry.ts/move.ts.
//
// A tree row names one file by two paths, and which one you want depends on where
// you are pasting. The **vault path** (`projects/idea.md`) is what B2's own surfaces
// speak — a wikilink, a `b2` argument, a frontmatter relation — and it is the path
// the index is keyed by. The **system path** (`/Users/me/vault/projects/idea.md`) is
// what everything outside B2 needs: Finder's Go-to-Folder, a terminal, another
// editor. Neither is derivable from the other without the vault root, which is why
// the menu offers both rather than picking one and being wrong half the time.

/**
 * The absolute on-disk path of a vault-relative `path`, under `vaultRoot`.
 *
 * String joining, deliberately: the root arrives from the host already resolved
 * (`VaultInfo.root`, a `Path::display`), and the vault path is the index's own key —
 * so there is nothing here to normalize, only a separator to get right. A trailing
 * slash on the root is tolerated (a hand-typed `B2_VAULT_PATH=~/notes/` reaches the
 * UI verbatim) so the join can't produce a doubled `//`. An empty `path` — the
 * folder-context menu, which offers no copy action today — is the root itself.
 *
 * `/` is the separator, hard-coded, and `\` is left alone as the ordinary filename
 * character it is here: B2 ships on macOS only (ci.yml's header), where the sole
 * bytes a path component may not contain are `/` and NUL. Sniffing the root for a
 * backslash to guess a Windows separator would therefore mangle a legitimate vault
 * — see copypath.test.ts, which pins that case.
 */
export function systemPath(vaultRoot: string, path: string): string {
  const root = vaultRoot.replace(/\/+$/, "");
  if (path === "") return root === "" ? "/" : root;
  return `${root}/${path}`;
}
