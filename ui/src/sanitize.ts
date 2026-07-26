// The Markdown→HTML trust boundary (invariant **E5**, GH #77).
//
// Note content is **untrusted input**: authorship is not trust, and a `.md` can arrive
// from a shared vault, a download, or a web clip. So every byte of note-derived HTML
// crosses this seam before it can reach `innerHTML`. render.ts wires it as `marked`'s
// `postprocess` hook, which is what makes it *unavoidable* rather than remembered — the
// reading view, the live-preview table widget, and any future call site are covered by
// construction, not by each caller choosing to sanitize.
//
// The sanitizer is DOMPurify because parsing with the host's own HTML parser and walking
// the resulting tree is the only approach that survives the mutation-XSS shapes a regex
// allow-list has never heard of. It is a *layer*, not the layer: the webview CSP
// (`default-src 'self'`, no inline scripts — crates/b2-desktop/tauri.conf.json) stays as
// the second, independent guard.
//
// Ours is a *document* renderer, so the posture is DOMPurify's permissive HTML default
// minus the four things a note has no business doing (see `CONFIG`) — not a narrow
// allow-list that would quietly eat a hand-authored table or an inline `<span>`.

import DOMPurify, {
  type Config,
  type DOMPurify as Purifier,
  type WindowLike,
} from "dompurify";

/**
 * DOMPurify's defaults, tightened at exactly four points. Each entry is a claim about
 * what a *note* may do, and each costs nothing a Markdown renderer emits:
 *
 * - `ALLOW_DATA_ATTR: false` + `ADD_ATTR: ["data-target"]` — B2 delegates clicks off
 *   `data-*` hooks, so leaving the default (every `data-*` allowed) would let a note mint
 *   the app's own handles. `data-target` is the one exception because it *is* the
 *   wikilink contract: render.ts emits `<a class="wikilink" data-target="…">` and both
 *   the reading view's follow handler and livepreview's `TableWidget` read it back. A
 *   note forging one gains nothing — it can write `[[target]]` and get the same anchor.
 * - `FORBID_TAGS: ["form"]` — the CSP hole this issue named: `form-action` does *not*
 *   fall back to `default-src`, so a note-authored `<form action="https://…">` is the one
 *   exfiltration sink the webview policy leaves open. Markdown never produces one.
 *   (`<input>` stays: GFM task lists render as disabled checkboxes.)
 * - `FORBID_ATTR: ["id", "name"]` — DOM clobbering. This UI paints by `innerHTML` swap
 *   and re-finds its controls by stable `id` (`getElementById`, the GH #91 focus
 *   restoration); a note declaring `id="modal-root"` or `id="fm-save"` would shadow the
 *   real one in document order. `marked` emits no ids, so nothing legitimate is lost.
 *
 * Deliberately *not* forbidden: the `style` attribute, remote `<img>`, and external
 * links. Those are what makes a document a document; their blast radius is visual, and
 * CSP (`img-src 'self' data:`) already answers the remote-load half.
 */
const CONFIG: Config = {
  ALLOW_DATA_ATTR: false,
  ADD_ATTR: ["data-target"],
  FORBID_TAGS: ["form"],
  FORBID_ATTR: ["id", "name"],
};

// Bound on first use, not at import. DOMPurify's default export binds to `globalThis.window`
// when the *module* loads — the real one in the webview, nothing at all under node's test
// runner — so resolving late is what lets the suite hand it a DOM and then exercise the
// very path the app runs (src/sanitize.test.ts), instead of testing a stand-in.
let purifier: Purifier | null = null;

function resolve(): Purifier {
  if (purifier) return purifier;
  const win: WindowLike | undefined = typeof window === "undefined" ? undefined : window;
  const p = win ? DOMPurify(win) : DOMPurify;
  // Fail loud, never quiet. Without a DOM, DOMPurify sets `isSupported = false` and
  // `sanitize()` returns its input **unchanged** — an advisory-but-exit-0 hole of exactly
  // the shape the root CLAUDE.md warns about, and here it would be a security control
  // reporting success while doing nothing. A host with no DOM cannot render a note at all,
  // so throwing loses no working case.
  if (!p.isSupported) {
    throw new Error("no DOM available to sanitize rendered Markdown");
  }
  purifier = p;
  return p;
}

/** Sanitize note-derived HTML for the DOM. The one crossing of the trust boundary. */
export function sanitizeHtml(html: string): string {
  return resolve().sanitize(html, CONFIG);
}
