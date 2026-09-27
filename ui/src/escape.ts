// HTML escaping, the one copy. Its own module so highlight.ts's node test can import it
// without render.ts's whole view layer.

/** Escape the five characters that could otherwise break out of text or an attribute. */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
