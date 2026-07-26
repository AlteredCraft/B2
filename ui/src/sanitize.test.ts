// Tests for the Markdown→HTML trust boundary (sanitize.ts + the `marked` hook it's wired
// into — invariant E5, GH #77). Run directly:
//   node --experimental-strip-types src/sanitize.test.ts
// Hand-rolled asserts, the highlight.test.ts / paste.test.ts idiom.
//
// These go through `renderMarkdown` — the seam the panes actually call — rather than
// `sanitizeHtml` directly, because the claim under test is "no note can reach the DOM
// unsanitized", and that claim is only true if the wiring holds. A test of the sanitizer
// alone would still pass with the hook removed.
//
// The one thing node can't supply is a DOM, and DOMPurify parses with the host's parser.
// jsdom is that host here (the setup below), so the assertions run the *real* sanitizer on
// the *real* renderer output — the paste.test.ts posture, where the fixture is the shim and
// the code under test is untouched. It is a devDependency: nothing ships it.

import { JSDOM } from "jsdom";
import { renderMarkdown } from "./render.ts";

// Before the first render: sanitize.ts binds DOMPurify lazily, precisely so this works.
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

// --- executable markup never survives ------------------------------------------------
//
// `marked` passes raw HTML through untouched by design, so every one of these reaches the
// sanitizer verbatim. The CSP would stop most of them from *firing*; E5's point is that
// they must not be in the document at all.

const script = renderMarkdown("before\n\n<script>alert(1)</script>\n\nafter");
assertNot(script, "<script", "a raw <script> block is dropped");
assertNot(script, "alert(1)", "…along with its payload");
assertHas(script, "before", "and the surrounding prose renders normally");

const handler = renderMarkdown('<img src="x" onerror="alert(1)">');
assertNot(handler, "onerror", "an inline event handler is stripped");
assertHas(handler, "<img", "…while the image element itself stays (CSP owns remote loads)");

// The issue's worked example: a payload smuggled through a GFM table cell, which is the
// live-preview `TableWidget`'s input as well as the reading view's.
const cell = renderMarkdown("| h |\n|---|\n| <img src=x onerror=alert(1)> |\n");
assertNot(cell, "onerror", "a handler inside a table cell is stripped too");
assertHas(cell, '<div class="md-table"><table>', "and B2's own table wrapper survives");

assertNot(renderMarkdown("[click](javascript:alert(1))"), "javascript:", "a javascript: href is dropped");
assertNot(
  renderMarkdown('<a href="data:text/html;base64,PHNjcmlwdD4=">d</a>'),
  "data:text/html",
  "so is a data: document href",
);
assertNot(
  renderMarkdown("<style>body{display:none}</style>"),
  "<style",
  "a <style> block can't restyle the app out from under the user",
);
assertNot(
  renderMarkdown('<iframe src="https://example.invalid"></iframe>'),
  "<iframe",
  "no framing",
);

// `form-action` does not fall back to `default-src`, so the CSP alone would let this one
// post the vault's contents outward — the concrete reason CSP isn't the only layer.
const form = renderMarkdown('<form action="https://example.invalid"><input name="x"></form>');
assertNot(form, "<form", "a <form> exfiltration sink is dropped");

// DOM clobbering: this UI re-finds its own controls by `id` after an innerHTML swap
// (GH #91), so a note must not be able to mint one.
assertNot(
  renderMarkdown('<div id="modal-root">hijack</div>'),
  "modal-root",
  "a note can't declare one of B2's own element ids",
);

// --- the wikilink contract survives ---------------------------------------------------
//
// The sanitizer sits between the wikilink renderer and the click handlers that read its
// output back (`.wikilink` + `dataset.target`, main.ts and livepreview.ts). Both halves of
// that contract have to come through untouched, or in-app navigation silently dies.

const wiki = renderMarkdown("see [[notes/alpha.md|Alpha]] for more");
assertHas(wiki, 'class="wikilink"', "the wikilink keeps its class hook");
assertHas(wiki, 'data-target="notes/alpha.md"', "…and its data-target");
assertHas(wiki, ">Alpha</a>", "…and its label");

const wikiInCell = renderMarkdown("| h |\n|---|\n| [[notes/alpha.md]] |\n");
assertHas(wikiInCell, 'data-target="notes/alpha.md"', "including inside a table widget's cell");

// The allow-list is exactly that one attribute: every *other* `data-*` a note authors is
// dropped, so a note can't forge the delegation hooks the app dispatches on.
assertNot(
  renderMarkdown('<span data-open="notes/secret.md">x</span>'),
  "data-open",
  "a note can't forge B2's other data-* handles",
);

// --- ordinary Markdown is untouched ----------------------------------------------------
//
// A sanitizer that eats the document is a bug too, so the vocabulary the reading view
// leans on is pinned here: fences (highlight.ts re-reads their `language-*` class), GFM
// task lists (a disabled `<input>` — the one form control that stays), and inline HTML.

assertHas(
  renderMarkdown("```rust\nfn main() {}\n```\n"),
  'class="language-rust"',
  "a fence keeps the language class highlight.ts resolves",
);
const tasks = renderMarkdown("- [x] done\n- [ ] todo\n");
assertHas(tasks, 'type="checkbox"', "GFM task lists keep their checkbox");
assertHas(tasks, "disabled", "…still disabled");
assertHas(
  renderMarkdown('<span style="color:red">emphasis</span>'),
  'style="color:red"',
  "hand-authored inline HTML still renders (the blast radius is visual)",
);
assertHas(
  renderMarkdown("# Title\n\n**bold** and `code`\n"),
  "<h1>Title</h1>",
  "and the plain vocabulary is untouched",
);

console.log(`sanitize.test.ts: ${checks} checks passed`);
