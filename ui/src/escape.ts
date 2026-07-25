// HTML escaping — the one copy, in its own module so both HTML producers can reach it.
//
// It lives here rather than in render.ts because highlight.ts needs it too, and
// highlight.ts is exercised by a node test that runs off the source: render.ts pulls in
// the whole view layer, this pulls in nothing.

/** Escape the five characters that could otherwise break out of text or an attribute. */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
