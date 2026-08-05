// The tree menu's two copy actions, pure half (copypath.ts). Dependency-free — node
// runs it straight off the source: `npm test`.
//
// One function, and the cases are all about the seam it sits on: the vault root comes
// from the *host* (`VaultInfo.root`, a `Path::display` of whatever `B2_VAULT_PATH`, the
// picker, or `~/.config` handed over) while the path comes from the *index*. Neither
// side promises the other anything about separators, so the join is the only place a
// doubled `//` or a missing one can appear — and a wrong system path is worse than no
// copy action, because it pastes into Finder and silently finds nothing.

import { systemPath } from "./copypath.ts";

let passed = 0;

function assertEq(actual: unknown, expected: unknown, msg: string): void {
  const [a, b] = [JSON.stringify(actual), JSON.stringify(expected)];
  if (a !== b) throw new Error(`assertion failed: ${msg}\n  actual:   ${a}\n  expected: ${b}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

check("a vault path becomes the absolute path under the root", () => {
  assertEq(
    systemPath("/Users/me/vault", "projects/idea.md"),
    "/Users/me/vault/projects/idea.md",
    "the ordinary case — the one that gets pasted into a terminal",
  );
  assertEq(systemPath("/Users/me/vault", "idea.md"), "/Users/me/vault/idea.md", "a root-level note");
  assertEq(
    systemPath("/Users/me/vault", "papers/2026/scan.pdf"),
    "/Users/me/vault/papers/2026/scan.pdf",
    "a resource, nested — the copy items don't care which kind of row they're on",
  );
  assertEq(
    systemPath("/Users/me/vault", "projects"),
    "/Users/me/vault/projects",
    "a folder row: no extension, and no trailing slash added",
  );
});

check("a trailing slash on the root never doubles the separator", () => {
  // `B2_VAULT_PATH=~/notes/` reaches the UI verbatim — the host resolves the root but
  // doesn't strip it, and `/Users/me/notes//a.md` is a path Finder's Go-to-Folder
  // tolerates and a shell script may not.
  assertEq(systemPath("/Users/me/vault/", "a.md"), "/Users/me/vault/a.md", "one slash");
  assertEq(systemPath("/Users/me/vault//", "a.md"), "/Users/me/vault/a.md", "and several");
});

check("a vault at the filesystem root still joins to one slash", () => {
  assertEq(systemPath("/", "a.md"), "/a.md", "the root's own trailing slash is the separator");
});

check("spaces and non-ASCII survive verbatim", () => {
  // Nothing here escapes or encodes: the destination is a clipboard, not a URL, and a
  // percent-encoded path pastes into Finder as a file that doesn't exist.
  assertEq(
    systemPath("/Users/me/My Vault", "réunions/notes d’hier.md"),
    "/Users/me/My Vault/réunions/notes d’hier.md",
    "copied exactly as the vault spells it",
  );
});

check("an empty path is the vault root itself", () => {
  // Not reachable from the menu today (the folder-context surface offers no copy item),
  // but a total function is what keeps that a UI choice rather than a latent `/`-prefixed
  // wrong answer if it ever is.
  assertEq(systemPath("/Users/me/vault", ""), "/Users/me/vault", "the root, unadorned");
  assertEq(systemPath("/", ""), "/", "and it stays a path when the root is /");
});

console.log(`copypath: ${passed} checks passed`);
