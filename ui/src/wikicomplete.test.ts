// Tests for the pure wikilink-completion logic (wikicomplete.ts) — the `[[` trigger
// detection, the candidate ranking, and the closing-bracket insertion. Run directly:
//   node --experimental-strip-types src/wikicomplete.test.ts
// No framework — the suite is a list of (name, fn) pairs and hand-rolled asserts,
// the same idiom as panes.test.ts / move.test.ts.

import { wikiCandidates, wikiInsertion, wikiQueryAt } from "./wikicomplete.ts";
import type { NoteSummary, ResourceSummary } from "./types.ts";

let checks = 0;

function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
  checks++;
}

// --- wikiQueryAt: find the open `[[` the cursor is typing into ---------------------

assertEq(wikiQueryAt("plain text, no link"), null, "no brackets → no trigger");
assertEq(wikiQueryAt("["), null, "a single [ is not a trigger");
assertEq(wikiQueryAt("[["), { from: 2, query: "" }, "bare [[ triggers with empty query");
assertEq(wikiQueryAt("see [[con"), { from: 6, query: "con" }, "query is the text after [[");
assertEq(
  wikiQueryAt("[[a]] then [[b"),
  { from: 13, query: "b" },
  "the last open [[ wins; a closed link before it is ignored",
);
assertEq(wikiQueryAt("[[a]]"), null, "a closed wikilink is not a trigger");
assertEq(wikiQueryAt("[[a|lab"), null, "past the | the target is fixed — no trigger");
assertEq(wikiQueryAt("![[pi"), { from: 3, query: "pi" }, "the embed form ![[ triggers too");
assertEq(
  wikiQueryAt("[[docs/al pha"),
  { from: 2, query: "docs/al pha" },
  "slashes and spaces are legal in a query",
);

// --- wikiCandidates: rank notes + resources against the query ----------------------

const notes: NoteSummary[] = [
  { b2id: "1", path: "docs/alpha.md", title: "Alpha" },
  { b2id: "2", path: "docs/beta.md", title: null },
  { b2id: "3", path: "notes/alpha-two.md", title: "Alpha Two" },
];
const resources: ResourceSummary[] = [
  { path: "docs/pic.png", class: "image", size: 1, mtime: null },
];

assertEq(
  wikiCandidates(notes, resources, "").map((c) => c.target),
  ["docs/alpha", "notes/alpha-two", "docs/beta", "docs/pic.png"],
  "empty query lists everything, label-sorted; note targets drop .md, resources keep the extension",
);
assertEq(
  wikiCandidates(notes, resources, "al").map((c) => c.label),
  ["Alpha", "Alpha Two"],
  "title-prefix matches rank and label from the note title",
);
assertEq(
  wikiCandidates(notes, resources, "two").map((c) => c.label),
  ["Alpha Two"],
  "title-substring matches rank after prefixes",
);
assertEq(
  wikiCandidates(notes, resources, "beta").map((c) => c.label),
  ["beta"],
  "a title-less note labels from its filename (minus .md)",
);
assertEq(
  wikiCandidates(notes, resources, "docs/").map((c) => c.target),
  ["docs/alpha", "docs/beta", "docs/pic.png"],
  "a path fragment matches by path",
);
assertEq(
  wikiCandidates(notes, resources, "ALPHA").map((c) => c.label),
  ["Alpha", "Alpha Two"],
  "matching is case-insensitive",
);
assertEq(wikiCandidates(notes, resources, "zzz"), [], "no match → empty");
assertEq(
  wikiCandidates(notes, resources, "", 2).map((c) => c.label),
  ["Alpha", "Alpha Two"],
  "limit caps the list",
);
assertEq(
  wikiCandidates(notes, resources, "pic")[0],
  { target: "docs/pic.png", label: "pic.png", detail: "docs/" },
  "a resource candidate: full path target, filename label, folder detail",
);
assertEq(
  wikiCandidates([{ b2id: "4", path: "root.md", title: null }], [], "root")[0].detail,
  "",
  "a root-level entry has an empty folder detail",
);

// A title-prefix match outranks an alphabetically-earlier path-only match.
assertEq(
  wikiCandidates(
    [
      { b2id: "5", path: "a-dir/note.md", title: "Zebra" },
      { b2id: "6", path: "zebra-notes/other.md", title: "Aardvark" },
    ],
    [],
    "zeb",
  ).map((c) => c.label),
  ["Zebra", "Aardvark"],
  "title matches outrank path-only matches regardless of alphabetical order",
);

// --- wikiInsertion: complete the target and close the brackets ---------------------

assertEq(
  wikiInsertion("docs/alpha", " rest of line"),
  { insert: "docs/alpha]]", cursor: 12 },
  "no closing ahead → append ]] and land after it",
);
assertEq(
  wikiInsertion("docs/alpha", "]] rest"),
  { insert: "docs/alpha", cursor: 12 },
  "an existing ]] is reused — cursor hops over it",
);
assertEq(
  wikiInsertion("docs/alpha", "] rest"),
  { insert: "docs/alpha]", cursor: 12 },
  "a single ] ahead gets one more — never a stray third bracket",
);
assertEq(
  wikiInsertion("docs/alpha", ""),
  { insert: "docs/alpha]]", cursor: 12 },
  "end of document behaves like no closing ahead",
);

console.log(`wikicomplete.test: ${checks} checks passed`);
