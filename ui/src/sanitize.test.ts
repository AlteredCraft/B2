// The Markdown→HTML trust boundary (E5, GH #77). Tests go through `renderMarkdown`, not
// `sanitizeHtml`, so they fail if the hook is unwired. jsdom supplies the DOM.

import { JSDOM } from "jsdom";
import { renderMarkdown } from "./render.ts";

// Before the first render: sanitize.ts binds DOMPurify lazily.
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
// `marked` passes raw HTML through, so these reach the sanitizer verbatim. E5: they must
// not be in the document at all, whatever CSP would stop.

const script = renderMarkdown("before\n\n<script>alert(1)</script>\n\nafter");
assertNot(script, "<script", "a raw <script> block is dropped");
assertNot(script, "alert(1)", "…along with its payload");
assertHas(script, "before", "and the surrounding prose renders normally");

const handler = renderMarkdown('<img src="x" onerror="alert(1)">');
assertNot(handler, "onerror", "an inline event handler is stripped");
assertHas(handler, "<img", "…while the image element itself stays (CSP owns remote loads)");

// A payload in a GFM table cell (also the live-preview `TableWidget`'s input).
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

// `form-action` doesn't fall back to `default-src`, so CSP alone would let this post out.
const form = renderMarkdown('<form action="https://example.invalid"><input name="x"></form>');
assertNot(form, "<form", "a <form> exfiltration sink is dropped");

// DOM clobbering: the UI re-finds its controls by `id` (GH #91).
assertNot(
  renderMarkdown('<div id="modal-root">hijack</div>'),
  "modal-root",
  "a note can't declare one of B2's own element ids",
);

// --- the wikilink contract survives ---------------------------------------------------
//
// `.wikilink` and `data-target` must survive, or in-app navigation silently dies.

const wiki = renderMarkdown("see [[notes/alpha.md|Alpha]] for more");
assertHas(wiki, 'class="wikilink"', "the wikilink keeps its class hook");
assertHas(wiki, 'data-target="notes/alpha.md"', "…and its data-target");
assertHas(wiki, ">Alpha</a>", "…and its label");

const wikiInCell = renderMarkdown("| h |\n|---|\n| [[notes/alpha.md]] |\n");
assertHas(wikiInCell, 'data-target="notes/alpha.md"', "including inside a table widget's cell");

// Every other `data-*` is dropped, so a note can't forge the app's delegation hooks.
assertNot(
  renderMarkdown('<span data-open="notes/secret.md">x</span>'),
  "data-open",
  "a note can't forge B2's other data-* handles",
);

// --- ordinary Markdown is untouched ----------------------------------------------------
//
// A sanitizer that eats the document is a bug too: fences (highlight.ts reads
// `language-*`), GFM task lists, and inline HTML survive.

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
