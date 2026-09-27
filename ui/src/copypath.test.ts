// The tree menu's copy actions (copypath.ts). The root comes from the host and the path
// from the index, so the join is the one place a doubled or missing `/` can appear.

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
  // `B2_VAULT_PATH=~/notes/` reaches the UI with its trailing slash.
  assertEq(systemPath("/Users/me/vault/", "a.md"), "/Users/me/vault/a.md", "one slash");
  assertEq(systemPath("/Users/me/vault//", "a.md"), "/Users/me/vault/a.md", "and several");
});

check("a vault at the filesystem root still joins to one slash", () => {
  assertEq(systemPath("/", "a.md"), "/a.md", "the root's own trailing slash is the separator");
});

check("a backslash in the root is a filename character, not a separator", () => {
  // On macOS (B2's only platform, per ci.yml) `back\slash` is a legal folder name, so
  // sniffing for a Windows separator would break a real vault.
  assertEq(
    systemPath("/Users/me/back\\slash", "projects/idea.md"),
    "/Users/me/back\\slash/projects/idea.md",
    "the root is copied through and the separator stays /",
  );
  assertEq(
    systemPath("/Users/me/vault", "odd\\name.md"),
    "/Users/me/vault/odd\\name.md",
    "and a backslash in the vault path is equally none of our business",
  );
});

check("spaces and non-ASCII survive verbatim", () => {
  // A clipboard, not a URL: a percent-encoded path would find nothing in Finder.
  assertEq(
    systemPath("/Users/me/My Vault", "réunions/notes d’hier.md"),
    "/Users/me/My Vault/réunions/notes d’hier.md",
    "copied exactly as the vault spells it",
  );
});

check("an empty path is the vault root itself", () => {
  // Not reachable from the menu today, but the function stays total.
  assertEq(systemPath("/Users/me/vault", ""), "/Users/me/vault", "the root, unadorned");
  assertEq(systemPath("/", ""), "/", "and it stays a path when the root is /");
});

console.log(`copypath: ${passed} checks passed`);
