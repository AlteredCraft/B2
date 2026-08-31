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

/** One loaded picture, the shape `state.embedImages` holds. */
const PICTURES = new Map([["__Attachments/Screenshot 1.png", "data:image/png;base64,AAAA"]]);

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

// --- the embed with no picture in hand: it reads as its link --------------------------
//
// `![[file]]` is the same link with the core's embed marker in front (link.rs). Until the
// bytes arrive — and forever, for a file that is no image — it renders as the link; the
// `!` is *grammar*, and a grammar character that leaks into the prose is a rendering bug,
// not a partial feature.

const embed = renderMarkdown("![[__Attachments/Screenshot 1.png]]\n");
assertNot(embed, ">!", "the embed marker never reaches the reader as text");
assertNot(embed, "<p>!", "…not even at the head of its own paragraph");
assertHas(embed, 'data-target="__Attachments/Screenshot 1.png"', "the embed carries its target");
assertHas(embed, ">__Attachments/Screenshot 1.png</a>", "…and labels itself with it");
assertNot(embed, "<img", "…with no picture, because none was handed to the render");

// --- an embed's `|`-part is a width, not a label --------------------------------------
//
// `![[img.png|400]]` asks for a 400px-wide render. With no picture there is nothing to
// size, and the unsupported half must not eat the supported one: dropping the hint is
// right, replacing the filename with "400" is the bug this pins.

const sized = renderMarkdown("![[__Attachments/Screenshot 2.png|400]]\n");
assertHas(sized, 'data-target="__Attachments/Screenshot 2.png"', "a sized embed keeps its target");
assertHas(sized, ">__Attachments/Screenshot 2.png</a>", "…and still reads as the filename");
assertNot(sized, ">400<", "the size hint is never a label");
assertNot(sized, "<p>!", "…and the marker is still markup");

// --- the embed with its picture: the image *is* the link's label -----------------------

const shown = renderMarkdown("![[__Attachments/Screenshot 1.png]]\n", PICTURES);
assertHas(shown, '<img class="embed-image"', "an embed whose bytes are in hand draws them");
assertHas(shown, 'src="data:image/png;base64,AAAA"', "…from the picture it was handed");
assertHas(
  shown,
  'alt="Screenshot 1.png"',
  "…labelled with its filename, which is all B2 knows about the picture",
);
assertHas(shown, 'data-target="__Attachments/Screenshot 1.png"', "…still carrying its target");
assertHas(shown, '<a class="wikilink"', "…and still a link: the picture opens the resource card");
assertNot(shown, ">__Attachments/Screenshot 1.png</a>", "the path is no longer the label");
assertNot(shown, "<p>!", "…and the marker is still markup");

// The `data:` src has to survive the sanitizer, not just the renderer — `renderMarkdown`
// runs DOMPurify over its own output (render.ts's postprocess hook), and a URL scheme it
// stripped would leave an `<img>` with nothing to draw.
assertHas(
  renderMarkdown("text ![[__Attachments/Screenshot 1.png]] text\n", PICTURES),
  "data:image/png;base64,AAAA",
  "the sanitizer admits a data: URL on an img (DOMPurify's DATA_URI_TAGS)",
);

// --- the width hint, now that there is something to size -------------------------------

const wide = renderMarkdown("![[__Attachments/Screenshot 1.png|500]]\n", PICTURES);
assertHas(wide, 'width="500"', "`|500` is the width the picture is drawn at");
assertNot(wide, "height=", "…and only the width: the aspect ratio is the CSS's to keep");

const odd = renderMarkdown("![[__Attachments/Screenshot 1.png|500x300]]\n", PICTURES);
assertHas(odd, "<img", "a hint B2 doesn't understand still draws the picture");
assertNot(odd, "width=", "…at its own size, rather than at a width nobody asked for");

// A *plain* wikilink to the same picture stays a link. The marker is the whole difference
// between naming a file and showing it, and a note that meant to link must not sprout an
// image because some other line embedded the same file.
assertNot(
  renderMarkdown("see [[__Attachments/Screenshot 1.png]]\n", PICTURES),
  "<img",
  "a link to a picture is a link, not a picture",
);

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
