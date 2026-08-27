// Tests for the resource card's **image viewer** (render.ts) — the `data:` URL rule and
// what the card shows in each of its states. Run directly:
//   node --experimental-strip-types src/resourceview.test.ts
// Hand-rolled asserts, the sanitize.test.ts / render.test.ts idiom.
//
// The card is chrome, not note content, so it never passes through the sanitizer — but
// `notePaneHtml` is one function over the whole pane and the note branch does, so jsdom
// is here for the same reason it is in render.test.ts.

import { JSDOM } from "jsdom";
import { imageDataUrl, IMAGE_VIEWER_MAX_BYTES, notePaneHtml } from "./render.ts";
import { state, type AppState } from "./state.ts";
import type { ResourceExplainView } from "./types.ts";

(globalThis as unknown as { window: unknown }).window = new JSDOM("").window;

let checks = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
  checks++;
}

function resource(over: Partial<ResourceExplainView> = {}): ResourceExplainView {
  return {
    path: "__Attachments/Screenshot 1.png",
    class: "image",
    size: 1_200_000,
    mtime: 0,
    content_hash: "e".repeat(64),
    backlinks: [],
    ...over,
  };
}

/** The pane rendered for one resource, with `image` as its loaded picture. */
function pane(r: ResourceExplainView, image: string | null): string {
  const s: AppState = { ...state, currentResource: r, resourceImage: image, current: null };
  return notePaneHtml(s);
}

// --- imageDataUrl: extension → the one src shape the CSP admits -----------------------

assert(
  imageDataUrl("__Attachments/Shot.png", "AAAA") === "data:image/png;base64,AAAA",
  "a png's bytes become a png data URL",
);
assert(
  imageDataUrl("a/b/PHOTO.JPEG", "AAAA") === "data:image/jpeg;base64,AAAA",
  "the extension is matched case-insensitively, and jpeg is jpg's MIME type",
);
assert(
  imageDataUrl("drawing.svg", "AAAA") === "data:image/svg+xml;base64,AAAA",
  "svg is an image the webview renders, so it gets its own MIME type",
);
assert(imageDataUrl("notes/paper.pdf", "AAAA") === null, "a class with no viewer gets no URL");
assert(imageDataUrl("Makefile", "AAAA") === null, "…and neither does an extensionless file");

// --- the card's three states ----------------------------------------------------------

const shown = pane(resource(), "data:image/png;base64,AAAA");
assert(shown.includes('<img class="resource-image"'), "a loaded image is shown in the card");
assert(shown.includes('src="data:image/png;base64,AAAA"'), "…from the bytes that were read");
assert(
  shown.includes('alt="Screenshot 1.png"'),
  "…labelled with its filename, which is all B2 knows about the picture",
);
assert(!shown.includes("No viewer available"), "the viewer replaces the no-viewer line");
assert(
  shown.includes("Open in system default"),
  "…and only that line: the OS handoff is still how you get to a real image app",
);
assert(shown.includes("BACKLINKS") || shown.includes("Backlinks"), "…as are the backlinks");

const unread = pane(resource(), null);
assert(
  unread.includes("No viewer available"),
  "an image whose bytes could not be read falls back to the card, never to a broken frame",
);
assert(!unread.includes("resource-image"), "…with no empty image element left behind");

const pdf = pane(resource({ path: "resources/paper.pdf", class: "pdf" }), null);
assert(pdf.includes("No viewer available"), "a class with no viewer is still the fallback card");

// --- the size bound is a real number, not a placeholder -------------------------------

assert(
  IMAGE_VIEWER_MAX_BYTES > 5 * 1024 * 1024,
  "the bound clears the screenshots a vault actually accumulates",
);
assert(
  IMAGE_VIEWER_MAX_BYTES < 200 * 1024 * 1024,
  "…and is small enough to still be a bound (base64 inflates it by a third in the heap)",
);

console.log(`resourceview.test.ts: ${checks} checks passed`);
