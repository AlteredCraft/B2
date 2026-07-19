// The tree-creation path rules (newentry.ts), pinned. Pure string logic — no DOM —
// so node runs it straight off the source via its native type-stripping: `npm test`.
// Dependency-free like panes.test.ts (hand-rolled assert; no @types/node).
import { dirChain, joinPath, normalizeName, parentDir } from "./newentry.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: string | null, expected: string | null, msg: string): void {
  assert(actual === expected, `${msg} — expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- parentDir: the selection's folder context --------------------------------------

check("a nested path's parent is its folder", () => {
  equal(parentDir("concepts/memory.md"), "concepts", "one level");
  equal(parentDir("a/b/c.md"), "a/b", "two levels");
});

check("a root-level path's parent is the root", () => {
  equal(parentDir("solo.md"), "", "root file");
  equal(parentDir(""), "", "empty stays root");
});

// --- normalizeName: forgiving shape, refused traversal ------------------------------

check("a plain name passes through trimmed", () => {
  equal(normalizeName("  my idea  "), "my idea", "trimmed");
  equal(normalizeName("note.md"), "note.md", "an explicit .md is kept");
});

check("nesting is allowed and separators are normalized", () => {
  equal(normalizeName("projects/2026"), "projects/2026", "nested");
  equal(normalizeName("a\\b"), "a/b", "backslash counts as a slash");
  equal(normalizeName("/a//b/"), "a/b", "stray + doubled slashes drop");
  equal(normalizeName(" a / b "), "a/b", "per-segment trim");
});

check("an empty input is a cancel (null), never an error", () => {
  equal(normalizeName(""), null, "empty");
  equal(normalizeName("   "), null, "whitespace");
  equal(normalizeName("//"), null, "only slashes");
});

check("traversal segments are refused", () => {
  equal(normalizeName(".."), null, "plain ..");
  equal(normalizeName("a/../b"), null, "embedded ..");
  equal(normalizeName("./a"), null, ". segment");
});

// --- joinPath: context + name --------------------------------------------------------

check("joins against a folder context, or stands alone at the root", () => {
  equal(joinPath("projects", "idea"), "projects/idea", "in a folder");
  equal(joinPath("", "idea"), "idea", "at the root");
});

// --- dirChain: staging + reveal ------------------------------------------------------

check("every prefix of a nested folder, shallowest first", () => {
  const chain = dirChain("a/b/c");
  equal(chain.join("|"), "a|a/b|a/b/c", "the full chain");
});

check("the root yields no chain", () => {
  equal(dirChain("").length === 0 ? "empty" : "not", "empty", "no prefixes");
});

console.log(`newentry: ${passed} checks passed`);
