// Tests for the `[[wikilink]]` / `![[embed]]` grammar in the reading view (render.ts's
// `marked` extension). Run directly:
//   node --experimental-strip-types src/embedlink.test.ts
// Hand-rolled asserts, the sanitize.test.ts idiom.
//
// Through `renderMarkdown` rather than the extension object, because the claim under test
// is what a note *looks like on screen*, and that is the composition of the extension, the
// tokenizer ordering against marked's own image rule, and the sanitizer hook. The
// extension in isolation would still pass with the `start` hook mis-anchored, which is
// exactly the bug that put a stray `!` in front of every embed.
//
// jsdom supplies the DOM DOMPurify parses with — a devDependency, nothing ships it.

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

// --- the plain wikilink: target carried, label shown ----------------------------------

const plain = renderMarkdown("see [[concepts/memory]] today\n");
assertHas(plain, 'data-target="concepts/memory"', "a bare wikilink carries its target");
assertHas(plain, ">concepts/memory</a>", "…and labels itself with it");

const labelled = renderMarkdown("see [[concepts/memory|how it works]]\n");
assertHas(labelled, 'data-target="concepts/memory"', "a `|` alias leaves the target alone");
assertHas(labelled, ">how it works</a>", "…and is what the reader sees");

// --- the embed form: the `!` is markup, not text --------------------------------------
//
// `![[file]]` is the same link with the core's embed marker in front (link.rs). B2 has no
// inline viewer for it yet, so it renders as the link — but the `!` is *grammar*, and a
// grammar character that leaks into the prose is a rendering bug, not a partial feature.

const embed = renderMarkdown("![[__Attachments/Screenshot 1.png]]\n");
assertNot(embed, ">!", "the embed marker never reaches the reader as text");
assertNot(embed, "<p>!", "…not even at the head of its own paragraph");
assertHas(embed, 'data-target="__Attachments/Screenshot 1.png"', "the embed carries its target");
assertHas(embed, ">__Attachments/Screenshot 1.png</a>", "…and labels itself with it");

// --- an embed's `|`-part is a size hint, not a label ----------------------------------
//
// `![[img.png|400]]` asks for a 400px-wide render. B2 does not size images, and the
// unsupported half must not eat the supported one: dropping the hint is right, replacing
// the filename with "400" is the bug this pins.

const sized = renderMarkdown("![[__Attachments/Screenshot 2.png|400]]\n");
assertHas(sized, 'data-target="__Attachments/Screenshot 2.png"', "a sized embed keeps its target");
assertHas(sized, ">__Attachments/Screenshot 2.png</a>", "…and still reads as the filename");
assertNot(sized, ">400<", "the size hint is dropped, not shown");
assertNot(sized, "<p>!", "…and the marker is still markup");

// --- neighbours the grammar must not swallow ------------------------------------------

assertHas(
  renderMarkdown("![alt text](resources/diagram.png)\n"),
  '<img src="resources/diagram.png"',
  "Markdown's own image form is untouched — the `!` there belongs to marked",
);
assertHas(
  renderMarkdown("a shout! [[concepts/memory]]\n"),
  "a shout! ",
  "a `!` that isn't a marker stays in the prose",
);
assertHas(
  renderMarkdown("brackets [[not closed here\n"),
  "[[not closed",
  "an unclosed `[[` is prose, and the marker rule doesn't change that",
);

console.log(`embedlink.test.ts: ${checks} checks passed`);
