// The `![[…]]` image embed's pure rules (embeds.ts) — the width hint, what counts as a
// picture, and which of a note's embeds the app agrees to hold. Run directly:
//   node --experimental-strip-types src/embeds.test.ts
// Hand-rolled asserts, the sanitize.test.ts / links.test.ts idiom.
//
// No DOM here on purpose. These are the decisions made *before* anything is rendered or
// read — the two surfaces that draw an embed (render.ts, livepreview.ts) have their own
// files, and each of those tests what the rules *look like* once applied.

import {
  embedWidth,
  imageDataUrl,
  imageEmbedTargets,
  imageMime,
  inlineImagePlan,
  IMAGE_VIEWER_MAX_BYTES,
  NOTE_IMAGES_MAX_BYTES,
} from "./embeds.ts";
import type { ResourceSummary } from "./types.ts";

let checks = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
  checks++;
}

// --- the width hint --------------------------------------------------------------------
//
// `![[shot.png|500]]` asks for a 500px-wide render. One number, because the ask is a width
// that *maintains aspect ratio* — the height is the CSS's to derive.

assertEq(embedWidth("500"), 500, "a bare integer is a width");
assertEq(embedWidth(" 500 "), 500, "hand-spacing is not a different answer");
assertEq(embedWidth(undefined), null, "no hint, no width — the embed draws at its own size");
assertEq(embedWidth(""), null, "…and neither is an empty one");
// Every one of these would be a *drawn* size if a leading-digit parse were used, and each
// would be wrong in its own way: a distorted picture, or one 500px wide that asked for 100.
assertEq(embedWidth("500x300"), null, "Obsidian's two-axis form is not a width B2 understands");
assertEq(embedWidth("100px"), null, "a unit is not a number");
assertEq(embedWidth("Label"), null, "a human who wrote a label on an embed gets no size");
assertEq(embedWidth("-40"), null, "a negative is not a width");
assertEq(embedWidth("0"), null, "…and neither is zero, which would draw nothing at all");
assertEq(embedWidth("1e3"), null, "no exponents: the hint is digits, not a JS number literal");

// --- what names a picture ---------------------------------------------------------------
//
// Extension-only, deliberately: the same rule the core classifies on (`resource.rs`), so a
// mislabeled file degrades to a broken `<img>` rather than to a guess about its bytes.

assertEq(imageMime("__Attachments/Shot.png"), "image/png", "png");
assertEq(imageMime("a/b/PHOTO.JPEG"), "image/jpeg", "case-insensitive, and jpeg is jpg's type");
assertEq(imageMime("drawing.svg"), "image/svg+xml", "svg is an image the webview renders");
assertEq(imageMime("notes/paper.pdf"), null, "a class with no viewer names no picture");
assertEq(imageMime("Makefile"), null, "…and neither does an extensionless file");
assertEq(
  imageDataUrl("shot.png", "AAAA"),
  "data:image/png;base64,AAAA",
  "the bytes become the one src shape the CSP admits",
);
assertEq(imageDataUrl("paper.pdf", "AAAA"), null, "no MIME type, no URL to hand an `<img>`");

// --- scanning a body for embeds ----------------------------------------------------------

assertEq(
  imageEmbedTargets("intro\n\n![[__Attachments/Pasted image 1.png|500]]\n\ntail\n"),
  ["__Attachments/Pasted image 1.png"],
  "the target is taken, the width hint is not part of it",
);
assertEq(
  imageEmbedTargets("![[ a/shot.png ]]\n"),
  ["a/shot.png"],
  "trimmed, the same way the renderers resolve a hand-spaced target",
);
assertEq(
  imageEmbedTargets("![[a.png]] ![[b.png]] ![[a.png]]\n"),
  ["a.png", "b.png"],
  "document order, and a picture embedded twice is read once",
);
assertEq(
  imageEmbedTargets("see [[a/shot.png]] and [[b.png|Bee]]\n"),
  [],
  "a *link* to a picture is a link — the marker is the whole difference",
);
assertEq(
  imageEmbedTargets("![[notes/paper.pdf]] ![[clip.mp4]] ![[some/note]]\n"),
  [],
  "an embed of something B2 can't draw asks for nothing",
);
assertEq(
  imageEmbedTargets("![alt](shot.png)\n"),
  [],
  "Markdown's own image form is not a wikilink embed",
);
assertEq(imageEmbedTargets("brackets ![[not closed\n"), [], "an unclosed `[[` names nothing");

// --- the plan: what the app will actually hold -------------------------------------------

function res(path: string, over: Partial<ResourceSummary> = {}): ResourceSummary {
  return { path, class: "image", size: 1_000, mtime: 0, ...over };
}

const INVENTORY: ResourceSummary[] = [
  res("a.png"),
  res("b.png"),
  res("huge.png", { size: IMAGE_VIEWER_MAX_BYTES + 1 }),
  res("paper.pdf", { class: "pdf" }),
];

assertEq(
  inlineImagePlan(["a.png", "b.png"], INVENTORY),
  ["a.png", "b.png"],
  "two ordinary pictures, both read",
);
assertEq(
  inlineImagePlan(["missing.png"], INVENTORY),
  [],
  "a file the vault has never inventoried is not asked for — the host would refuse it",
);
assertEq(
  inlineImagePlan(["paper.pdf"], INVENTORY),
  [],
  "the core's class has the last word, whatever the extension claimed",
);
assertEq(
  inlineImagePlan(["huge.png", "a.png"], INVENTORY),
  ["a.png"],
  "one picture over the per-image bound drops out; the note's others are unaffected",
);

// The per-note budget, which is what a photo log costs. Taken in document order, so what
// the reader sees when the note opens is what got drawn.
const many = Array.from({ length: 8 }, (_, i) =>
  res(`p${i}.png`, { size: NOTE_IMAGES_MAX_BYTES / 4 }),
);
assertEq(
  inlineImagePlan(
    many.map((r) => r.path),
    many,
  ),
  ["p0.png", "p1.png", "p2.png", "p3.png"],
  "the budget is spent from the top of the note, and the rest read as their links",
);
assert(
  NOTE_IMAGES_MAX_BYTES >= IMAGE_VIEWER_MAX_BYTES,
  "a note must be able to hold at least one picture of the largest permitted size",
);
checks++;

console.log(`embeds.test.ts: ${checks} checks passed`);
