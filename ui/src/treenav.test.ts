// The file tree's navigation rules (treenav.ts), pinned. Pure logic — no DOM — so node
// runs it straight off the source via its native type-stripping: `npm test`.
// Dependency-free like newentry.test.ts / panes.test.ts (hand-rolled assert).
//
// What's under test is invariant K1's tree half (GH #78): the *visible* row order the
// arrows walk, and the ARIA tree-pattern moves over it. The order matters twice over —
// render.ts paints from the same `sortedSubdirs`/`sortedFiles` this flattens with, so
// these cases are also the guard against the paint and the arrows drifting apart.
import type { KeyEventLike } from "./bindings.ts";
import {
  arrowMove,
  buildTree,
  neighborPath,
  rovingPath,
  rowIndex,
  treeNavFor,
  type TreeNav,
  typeaheadTarget,
  visibleRows,
  type TreeRow,
} from "./treenav.ts";
import type { NoteSummary, ResourceSummary } from "./types.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: unknown, expected: unknown, msg: string): void {
  assert(
    actual === expected,
    `${msg} — expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
  );
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- fixtures ------------------------------------------------------------------------

const note = (path: string, title: string | null = null): NoteSummary =>
  ({ path, title }) as NoteSummary;
const resource = (path: string, cls = "pdf"): ResourceSummary =>
  ({ path, class: cls }) as ResourceSummary;

/**
 * A small vault with every shape that matters: nested folders, an empty folder (the fs
 * is authoritative for structure, so it's a real row), a resource beside a note, and a
 * title that sorts differently from its filename.
 *
 *   archive/            (folder, empty)
 *   concepts/           (folder)
 *     deep/             (folder)
 *       nested.md
 *     memory.md
 *     spec.pdf
 *   readme.md           ("Alpha" by title)
 */
const NOTES = [note("concepts/memory.md"), note("concepts/deep/nested.md"), note("readme.md", "Alpha")];
const RESOURCES = [resource("concepts/spec.pdf")];
const DIRS = ["archive", "concepts", "concepts/deep"];

const tree = () => buildTree(NOTES, RESOURCES, DIRS);
const rowsWith = (...expanded: string[]) => visibleRows(tree(), new Set(expanded));
const paths = (rows: readonly TreeRow[]) => rows.map((r) => r.path).join("|");

// --- visibleRows: the one order the paint and the arrows share ------------------------

check("collapsed: only root-level rows, folders before files", () => {
  equal(paths(rowsWith()), "archive|concepts|readme.md", "root rows");
});

check("an expanded folder's children follow it inline, sub-folders first", () => {
  equal(
    paths(rowsWith("concepts")),
    "archive|concepts|concepts/deep|concepts/memory.md|concepts/spec.pdf|readme.md",
    "one level open",
  );
});

check("expansion nests, and a collapsed child hides its own subtree", () => {
  equal(
    paths(rowsWith("concepts", "concepts/deep")),
    "archive|concepts|concepts/deep|concepts/deep/nested.md|concepts/memory.md|concepts/spec.pdf|readme.md",
    "two levels open",
  );
  // `concepts/deep` expanded but its parent closed shows nothing: visibility is the
  // whole ancestor chain, not a single flag.
  equal(paths(rowsWith("concepts/deep")), "archive|concepts|readme.md", "an orphaned expansion is inert");
});

check("files sort by their display label, not their filename", () => {
  // readme.md is titled "Alpha", so it sorts under A — exactly as the tree paints it.
  const rows = visibleRows(buildTree([note("zulu.md", "Alpha"), note("alpha.md", "Zulu")], [], []), new Set());
  equal(paths(rows), "zulu.md|alpha.md", "label order wins");
});

check("a row carries the depth, kind, and fold state the paint needs", () => {
  const rows = rowsWith("concepts");
  const concepts = rows[rowIndex(rows, "concepts")];
  equal(concepts.nodeKind, "folder", "kind");
  equal(concepts.depth, 0, "root-level depth");
  equal(concepts.expanded, true, "expanded");
  equal(concepts.hasChildren, true, "has children");
  const archive = rows[rowIndex(rows, "archive")];
  equal(archive.hasChildren, false, "an empty folder has nothing to step into");
  const pdf = rows[rowIndex(rows, "concepts/spec.pdf")];
  equal(pdf.nodeKind, "resource", "a resource is its own kind");
  equal(pdf.depth, 1, "nested depth");
});

// --- arrowMove: the ARIA tree pattern ------------------------------------------------

function move(rows: readonly TreeRow[], from: string | null, nav: TreeNav) {
  return arrowMove(rows, rowIndex(rows, from), nav);
}

/** A keydown, as the registry's matcher sees it. */
function press(key: string): KeyEventLike {
  return { key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false };
}

check("up/down step between visible rows and stop at the ends", () => {
  const rows = rowsWith("concepts");
  equal(move(rows, "archive", "tree.row.next")?.path, "concepts", "down");
  equal(move(rows, "concepts", "tree.row.prev")?.path, "archive", "up");
  equal(move(rows, "archive", "tree.row.prev"), null, "no wrap off the top");
  equal(move(rows, "readme.md", "tree.row.next"), null, "no wrap off the bottom");
});

check("down never descends into a collapsed folder", () => {
  const rows = rowsWith();
  equal(move(rows, "concepts", "tree.row.next")?.path, "readme.md", "skips the hidden subtree");
});

check("Home/End jump to the first and last visible rows", () => {
  const rows = rowsWith("concepts");
  equal(move(rows, "concepts", "tree.row.first")?.path, "archive", "Home");
  equal(move(rows, "concepts", "tree.row.last")?.path, "readme.md", "End");
});

check("the first arrow press lands even with nothing focused", () => {
  const rows = rowsWith();
  equal(move(rows, null, "tree.row.next")?.path, "archive", "down → first");
  equal(move(rows, null, "tree.row.prev")?.path, "readme.md", "up → last");
  equal(move(rows, null, "tree.row.in")?.path, "archive", "right → first");
});

check("right expands a collapsed folder, then steps into it", () => {
  const closed = rowsWith();
  const expand = move(closed, "concepts", "tree.row.in");
  equal(expand?.kind, "expand", "collapsed → expand");
  equal(expand?.path, "concepts", "the folder itself");
  const open = rowsWith("concepts");
  const into = move(open, "concepts", "tree.row.in");
  equal(into?.kind, "focus", "expanded → step in");
  equal(into?.path, "concepts/deep", "its first child");
});

check("right does nothing on a file, or on a folder with nothing in it", () => {
  const rows = rowsWith("concepts");
  equal(move(rows, "readme.md", "tree.row.in"), null, "a file has no children");
  equal(move(rows, "archive", "tree.row.in"), null, "an empty folder stays put");
});

check("left collapses an expanded folder, else steps out to the parent", () => {
  const open = rowsWith("concepts");
  const collapse = move(open, "concepts", "tree.row.out");
  equal(collapse?.kind, "collapse", "expanded → collapse");
  equal(collapse?.path, "concepts", "the folder itself");
  const out = move(open, "concepts/memory.md", "tree.row.out");
  equal(out?.kind, "focus", "a file steps out");
  equal(out?.path, "concepts", "to its folder");
  equal(move(open, "concepts/deep", "tree.row.out")?.path, "concepts", "a collapsed folder steps out too");
  equal(move(open, "readme.md", "tree.row.out"), null, "at the root there is nowhere out to");
});

check("the shipped keys mean what the ARIA tree pattern says", () => {
  // Since #121 this module no longer decides which key is which move — the registry does
  // (bindings.ts's header says why), and `treeNavFor` is the lookup. So the mapping is
  // pinned *here*, where the pattern it implements is documented, rather than being
  // re-derived from a table in the test the way it used to be read off a switch.
  equal(treeNavFor(press("ArrowDown")), "tree.row.next", "↓ is the next row");
  equal(treeNavFor(press("ArrowUp")), "tree.row.prev", "↑ is the previous one");
  equal(treeNavFor(press("Home")), "tree.row.first", "Home");
  equal(treeNavFor(press("End")), "tree.row.last", "End");
  equal(treeNavFor(press("ArrowRight")), "tree.row.in", "→ steps in");
  equal(treeNavFor(press("ArrowLeft")), "tree.row.out", "← steps out");
});

check("a key the tree doesn't own is left alone", () => {
  // The other half of what the old `default:` case did, and the half that matters: a pane
  // handler that swallowed keys it has no move for would eat ⏎ off the row's button and
  // every letter typeahead needs.
  equal(treeNavFor(press("Enter")), null, "Enter belongs to the button");
  equal(treeNavFor(press("n")), null, "letters fall through to typeahead");
  equal(arrowMove([], -1, "tree.row.next"), null, "and an empty tree has no moves at all");
});

// --- typeahead ------------------------------------------------------------------------

check("typeahead finds the next match after the focused row, wrapping", () => {
  const rows = rowsWith("concepts");
  equal(typeaheadTarget(rows, rowIndex(rows, "archive"), "c"), "concepts", "forward");
  // From `concepts` itself, "c" wraps past the end and back to `concepts`.
  equal(typeaheadTarget(rows, rowIndex(rows, "concepts"), "c"), "concepts", "wraps around to itself");
  equal(typeaheadTarget(rows, -1, "a"), "archive", "from nowhere, the first match");
});

check("typeahead is case-insensitive and matches the *label*", () => {
  const rows = rowsWith();
  // readme.md's label is its title, "Alpha" — so `r` misses it and `a` finds it.
  equal(typeaheadTarget(rows, -1, "A"), "archive", "upper-case needle");
  equal(typeaheadTarget(rows, rowIndex(rows, "archive"), "a"), "readme.md", "the title, not the filename");
  equal(typeaheadTarget(rows, -1, "z"), null, "no match is a miss, not a jump");
});

// --- the roving tabstop ----------------------------------------------------------------

check("the tabstop prefers the keyboard's row, then the open note, then the first", () => {
  const rows = rowsWith("concepts");
  equal(rovingPath(rows, "concepts/memory.md", "readme.md"), "concepts/memory.md", "keyboard wins");
  equal(rovingPath(rows, null, "readme.md"), "readme.md", "else the open document");
  equal(rovingPath(rows, null, null), "archive", "else the first row");
  equal(rovingPath([], "gone.md", "gone.md"), null, "an empty tree has no tabstop");
});

check("a tabstop that scrolled out of existence falls back rather than vanishing", () => {
  // `concepts` collapsed: the focused child is no longer a row, so the tree must not be
  // left with zero tabbable rows (Tab would skip it entirely).
  const rows = rowsWith();
  equal(rovingPath(rows, "concepts/memory.md", null), "archive", "collapsed away → first row");
  equal(rovingPath(rows, "deleted.md", "readme.md"), "readme.md", "deleted → the open document");
});

// --- neighbour after a delete ----------------------------------------------------------

check("a deleted row hands focus to the next row, else the previous one", () => {
  const rows = rowsWith("concepts");
  equal(neighborPath(rows, "archive"), "concepts", "next");
  equal(neighborPath(rows, "readme.md"), "concepts/spec.pdf", "the last row falls back to the previous");
  equal(neighborPath(rows, "nope.md"), null, "a row that isn't there has no neighbour");
});

check("a deleted folder's whole subtree goes with it", () => {
  const rows = rowsWith("concepts", "concepts/deep");
  // Not `concepts/deep` (inside it) and not its children — the next row at or above it.
  equal(neighborPath(rows, "concepts"), "readme.md", "skips the subtree it takes along");
  equal(neighborPath(rows, "concepts/deep"), "concepts/memory.md", "a nested folder, same rule");
});

console.log(`treenav: ${passed} checks passed`);
