// The import rules (importfiles.ts), pinned. Pure logic — no DOM — so node runs it
// straight off the source via its native type-stripping: `npm test`. Dependency-free
// like newentry.test.ts (hand-rolled assert; no @types/node).
import {
  bytesToBase64,
  destinationLabel,
  formatSize,
  importSummary,
  planImport,
  IMPORT_SIZE_LIMIT,
} from "./importfiles.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function equal(actual: string | number, expected: string | number, msg: string): void {
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

const file = (name: string, size = 10) => ({ name, size, isDirectory: false });
const folder = (name: string) => ({ name, size: 0, isDirectory: true });

// --- planImport: what may be sent, and why not ---------------------------------------

check("ordinary files are all accepted, in drop order", () => {
  const plan = planImport([file("a.png"), file("b.md"), file("c.pdf")]);
  equal(plan.accepted.map((e) => e.name).join(","), "a.png,b.md,c.pdf", "order preserved");
  equal(plan.refused.length, 0, "nothing refused");
});

check("a dropped folder is refused with the reason, not silently ignored", () => {
  const plan = planImport([folder("Archive"), file("a.png")]);
  equal(plan.accepted.length, 1, "the file still goes");
  equal(plan.refused.length, 1, "the folder is reported");
  assert(plan.refused[0].includes("Archive"), "the reason names it");
  assert(plan.refused[0].includes("folder"), "the reason says what it is");
});

check("a file over the limit is refused before a byte is read", () => {
  const plan = planImport([file("huge.mov", IMPORT_SIZE_LIMIT + 1), file("ok.mov", IMPORT_SIZE_LIMIT)]);
  equal(plan.accepted.map((e) => e.name).join(","), "ok.mov", "exactly at the limit passes");
  assert(plan.refused[0].includes("huge.mov"), "the reason names it");
  assert(
    plan.refused[0].includes("over the 64 MB limit"),
    `the reason states the limit: ${plan.refused[0]}`,
  );
});

check("the caller's own entry fields survive the plan", () => {
  // The plan filters; it never projects. main.ts hangs the File handle off the entry
  // and reads it back from `accepted`, so a copy that dropped extra fields would
  // leave the import with nothing to send.
  const plan = planImport([{ ...file("a.png"), handle: "FILE" }]);
  equal(plan.accepted[0].handle, "FILE", "extra fields ride along");
});

// --- the toast ----------------------------------------------------------------------

check("a destination reads as a folder path, or as the root", () => {
  equal(destinationLabel("papers/2026"), "papers/2026/", "trailing slash names a folder");
  equal(destinationLabel(""), "the vault root", "the root has no path to print");
});

check("one imported file is named; several are counted with their destination", () => {
  equal(importSummary("papers", ["papers/a.pdf"], []), "Imported papers/a.pdf.", "single");
  equal(
    importSummary("papers", ["papers/a.pdf", "papers/b.pdf"], []),
    "Imported 2 files into papers/.",
    "several",
  );
  equal(importSummary("", ["a.pdf", "b.pdf"], []), "Imported 2 files into the vault root.", "root");
});

check("refusals are spelled out, never counted", () => {
  const both = importSummary("papers", ["papers/a.pdf"], ["Archive is a folder"]);
  equal(both, "Imported papers/a.pdf. Skipped: Archive is a folder.", "landed first, then why not");

  const none = importSummary("papers", [], ["Archive is a folder", "b.pdf: already exists"]);
  equal(
    none,
    "Nothing imported. Archive is a folder; b.pdf: already exists.",
    "every reason survives",
  );
});

check("an empty drop still says something", () => {
  equal(importSummary("papers", [], []), "Nothing to import into papers/.", "no silent no-op");
});

// --- base64 transport ----------------------------------------------------------------

check("bytes round-trip through the transport encoding", () => {
  const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0xff, 0x00]);
  const encoded = bytesToBase64(bytes);
  equal(encoded, "iVBORw0KGgr/AA==", "the PNG header, base64'd");
  const back = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
  equal(back.join(","), bytes.join(","), "decodes byte-identically");
});

check("a payload larger than the argument limit still encodes", () => {
  // The chunking's whole reason: a single spread of this many bytes overflows the
  // call-argument limit, which no small fixture would ever catch.
  const big = new Uint8Array(300_000);
  for (let i = 0; i < big.length; i++) big[i] = i % 256;
  const back = Uint8Array.from(atob(bytesToBase64(big)), (c) => c.charCodeAt(0));
  equal(back.length, big.length, "same length");
  equal(back[299_999], big[299_999], "same last byte");
});

check("an empty file encodes to an empty payload", () => {
  equal(bytesToBase64(new Uint8Array(0)), "", "no bytes, no payload");
});

// --- formatSize ----------------------------------------------------------------------

check("sizes read the way a human would say them", () => {
  equal(formatSize(1), "1 byte", "singular");
  equal(formatSize(512), "512 bytes", "under a KB stays exact");
  equal(formatSize(1024), "1.0 KB", "one decimal while small");
  equal(formatSize(20 * 1024 * 1024), "20 MB", "no decimal once it's big");
});

console.log(`\n${passed} checks passed`);
