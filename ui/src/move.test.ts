// The tree move/rename path rules (move.ts), pinned. Pure string logic — no DOM —
// so node runs it straight off the source via its native type-stripping: `npm test`.
// Dependency-free like newentry.test.ts (hand-rolled assert; no @types/node).
import {
  allDirs,
  baseName,
  canMoveInto,
  moveDestination,
  refKind,
  remapPath,
  renameDestination,
  renamePrefill,
} from "./move.ts";

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

// --- renamePrefill: what the inline input starts out holding ------------------------

check("a note prefills its basename without .md", () => {
  equal(renamePrefill("concepts/memory.md", "note"), "memory", "nested note");
  equal(renamePrefill("solo.md", "note"), "solo", "root note");
});

check("a resource keeps its extension; a folder is just its name", () => {
  equal(renamePrefill("img/pic.png", "resource"), "pic.png", "resource ext is identity");
  equal(renamePrefill("a/b/media", "folder"), "media", "folder name");
});

// --- renameDestination: typed name → full destination -------------------------------

check("a rename resolves inside the node's own folder", () => {
  equal(renameDestination("concepts/memory.md", "note", "recall"), "concepts/recall.md", "note");
  equal(renameDestination("img/pic.png", "resource", "photo.png"), "img/photo.png", "resource");
  equal(renameDestination("a/media", "folder", "assets"), "a/assets", "folder");
});

check("a note's .md is re-appended only when missing", () => {
  equal(renameDestination("a/x.md", "note", "y.md"), "a/y.md", "typed with .md");
  equal(renameDestination("a/x.md", "note", "y"), "a/y.md", "typed without");
});

check("empty and traversal names back out (null)", () => {
  equal(renameDestination("a/x.md", "note", "   "), null, "empty is a cancel");
  equal(renameDestination("a/x.md", "note", "../up"), null, "traversal refused");
});

check("an unchanged name is a no-op (null), not an error", () => {
  equal(renameDestination("concepts/memory.md", "note", "memory"), null, "note same name");
  equal(renameDestination("img/pic.png", "resource", "pic.png"), null, "resource same name");
  equal(renameDestination("a/media", "folder", "media"), null, "folder same name");
});

check("a nested rename input moves deeper, like the create input", () => {
  equal(renameDestination("x.md", "note", "archive/x"), "archive/x.md", "root → folder");
});

// --- moveDestination + canMoveInto: the drop / Move… gesture ------------------------

check("moveDestination keeps the name, swaps the folder", () => {
  equal(moveDestination("concepts/memory.md", "archive"), "archive/memory.md", "note");
  equal(moveDestination("a/b/pic.png", ""), "pic.png", "to root");
  equal(baseName("a/b/c.md"), "c.md", "baseName nested");
  equal(baseName("solo"), "solo", "baseName root");
});

check("dropping into the current folder is a no-op, not a move", () => {
  assert(!canMoveInto("concepts/memory.md", "note", "concepts"), "note → own parent");
  assert(!canMoveInto("root.md", "note", ""), "root note → root");
});

check("a folder can't move into itself or its own descendants", () => {
  assert(!canMoveInto("a/b", "folder", "a/b"), "into itself");
  assert(!canMoveInto("a/b", "folder", "a/b/c"), "into a descendant");
  assert(canMoveInto("a/b", "folder", "a/bc"), "a prefix-sharing sibling is fine");
  assert(canMoveInto("a/b", "folder", ""), "to the root is fine");
});

check("ordinary cross-folder moves are valid", () => {
  assert(canMoveInto("concepts/memory.md", "note", "archive"), "note across folders");
  assert(canMoveInto("a/b", "folder", "c"), "folder across folders");
});

// --- allDirs: the Move… modal's folder list -----------------------------------------

check("allDirs is root-first, deduped, sorted", () => {
  // Input is `list_dirs`' walk — already every folder on disk, empty ones included.
  const dirs = allDirs(["staged/deep", "a", "a/b", "staged", "a"]);
  equal(dirs.join("|"), "|a|a/b|staged|staged/deep", "root first, deduped, sorted");
});

// --- remapPath: re-pointing open state after a move ---------------------------------

check("an exact match remaps to the destination", () => {
  equal(remapPath("a/x.md", "a/x.md", "b/x.md"), "b/x.md", "file move");
});

check("a path inside a moved folder is prefix-remapped", () => {
  equal(remapPath("docs/a/x.md", "docs", "media"), "media/a/x.md", "nested file");
  equal(remapPath("docs", "docs", "media"), "media", "the folder itself");
});

check("prefix-sharing siblings and outsiders are untouched (null)", () => {
  equal(remapPath("docs2/x.md", "docs", "media"), null, "sibling shares the prefix only");
  equal(remapPath("other/x.md", "docs", "media"), null, "outside");
});

// --- refKind: the note/resource dispatch a followed wikilink routes on --------------

check("a non-.md extension is a resource", () => {
  equal(refKind("Cascadia Builders Club.pdf"), "resource", "vault-root pdf");
  equal(refKind("img/photo.png"), "resource", "nested resource");
  equal(refKind("archive.tar.gz"), "resource", "double extension");
  equal(refKind("notes/a.md.bak"), "resource", "trailing non-md wins");
});

check(".md and extensionless refs are notes", () => {
  equal(refKind("notes/a.md"), "note", "explicit .md");
  equal(refKind("A.MD"), "note", "uppercase .md");
  equal(refKind("concepts/memory"), "note", "the extensionless wikilink habit");
  equal(refKind("01JMEM0000000000000000000A"), "note", "a bare b2id");
  equal(refKind("LICENSE"), "note", "extensionless file (the documented limit)");
});

check("a #fragment is dropped before classifying", () => {
  equal(refKind("Cascadia Builders Club.pdf#page=2"), "resource", "resource + fragment");
  equal(refKind("concepts/memory#anchor"), "note", "note + fragment");
});

check("a leading-dot dotfile has no stem, so it is a note ref", () => {
  equal(refKind(".gitignore"), "note", "dotfile: empty stem");
});

console.log(`move.test: ${passed} checks passed`);
