// A single newline in a note is a line break on screen (GH: "new lines render correctly
// in edit mode but not in view mode"). The editor shows the file as typed, one line per
// line; CommonMark's default folds those same lines into one paragraph, so the reading
// view disagreed with the editor about where the author's line ends were. Notes here are
// written the Obsidian way — a keystroke of Enter is a visible break, a blank line is a
// paragraph — and the reading view follows the author, not the spec's fold.
//
// Through `renderMarkdown`, the seam the panes call, for the same reason as
// embedlink.test.ts: the claim is what the note looks like. Run directly:
//   node --experimental-strip-types src/linebreaks.test.ts

import { JSDOM } from "jsdom";
import { renderMarkdown } from "./render.ts";

(globalThis as unknown as { window: unknown }).window = new JSDOM("").window;

let checks = 0;

function assertHas(haystack: string, needle: string, label: string): void {
  if (!haystack.includes(needle)) {
    throw new Error(`${label}\n  missing: ${needle}\n  in:      ${haystack}`);
  }
  checks++;
}

function assertNot(haystack: string, needle: string, label: string): void {
  if (haystack.includes(needle)) {
    throw new Error(`${label}\n  found: ${needle}\n  in:    ${haystack}`);
  }
  checks++;
}

// --- one newline is a break, two are a paragraph ---------------------------------------

const lines = renderMarkdown("Connect: no code connection tool\nDesigner: low code ETL\n");
assertHas(lines, "tool<br>Designer", "a single newline draws as a line break");
assertNot(lines, "tool\nDesigner", "…and is not folded into a space");

const paragraphs = renderMarkdown("first\n\nsecond\n");
assertHas(paragraphs, "<p>first</p>", "a blank line still ends a paragraph");
assertHas(paragraphs, "<p>second</p>", "…and starts the next");
assertNot(paragraphs, "<br>", "…with no stray break between them");

// The shape the report came from: a caption on its own line above an embed. The break
// is what keeps the caption from running into the picture's line.
const captioned = renderMarkdown(
  "LakeFlow is a declarative pipelines add to Spark\n![[__Attachments/Shot.png|400]]\n",
);
assertHas(captioned, "Spark<br>", "a caption line breaks before the embed that follows it");

// --- what a break must not touch ------------------------------------------------------

const fenced = renderMarkdown("```\nline one\nline two\n```\n");
assertNot(fenced, "<br>", "a code fence keeps its newlines literal, never as <br>");

const list = renderMarkdown("- one\n- two\n");
assertNot(list, "<br>", "list items are items, not one item with a break in it");

console.log(`linebreaks: ${checks} checks passed`);
