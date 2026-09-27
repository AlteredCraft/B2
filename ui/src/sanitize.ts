// The Markdown→HTML trust boundary (E5, GH #77). Note content is untrusted, so all
// note-derived HTML crosses here before `innerHTML`; markdown.ts wires it as `marked`'s
// `postprocess` hook so every caller is covered by construction. DOMPurify parses with the
// host's own HTML parser, which survives mutation-XSS a regex allow-list would miss. The
// webview CSP (crates/b2-desktop/tauri.conf.json) is a second, independent layer.
//
// A document renderer, so DOMPurify's permissive default minus what a note has no business
// doing (`CONFIG`), not a narrow allow-list that would eat a hand-authored table.

import DOMPurify, {
  type Config,
  type DOMPurify as Purifier,
  type WindowLike,
} from "dompurify";

/**
 * DOMPurify's defaults, tightened where a note has no business going:
 *
 * - No `data-*` except `data-target`: B2 delegates clicks off `data-*` hooks, and
 *   `data-target` is the wikilink contract (forging one gains nothing over `[[target]]`).
 * - No `<form>`: CSP's `form-action` doesn't fall back to `default-src`, so it is the one
 *   exfiltration sink the policy leaves open. (`<input>` stays for GFM task lists.)
 * - No `id` / `name`: DOM clobbering. The UI re-finds its controls by `id` (GH #91).
 *
 * `style`, remote `<img>` and external links stay: their blast radius is visual, and CSP
 * covers remote loads.
 */
const CONFIG: Config = {
  ALLOW_DATA_ATTR: false,
  ADD_ATTR: ["data-target"],
  FORBID_TAGS: ["form"],
  FORBID_ATTR: ["id", "name"],
};

// Bound on first use, not at import, so the test suite can supply a DOM (jsdom) first and
// exercise the real path.
let purifier: Purifier | null = null;

function resolve(): Purifier {
  if (purifier) return purifier;
  const win: WindowLike | undefined = typeof window === "undefined" ? undefined : window;
  const p = win ? DOMPurify(win) : DOMPurify;
  // Fail loud: without a DOM, `sanitize()` returns its input unchanged.
  if (!p.isSupported) {
    throw new Error("no DOM available to sanitize rendered Markdown");
  }
  purifier = p;
  return p;
}

/** Sanitize note-derived HTML for the DOM: the one crossing of the trust boundary. */
export function sanitizeHtml(html: string): string {
  return resolve().sanitize(html, CONFIG);
}
