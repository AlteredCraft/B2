// Where a link in a note goes. Following a web link in place would replace the whole app
// with a page it can't get back from, so a web link is an OS handoff (main.ts delegates the
// click, the host opens it). The host's `is_openable_link` (b2-desktop commands.rs)
// re-checks the same schemes, since a note is untrusted input (E5): change them together.

/** The schemes B2 hands to the OS, lowercase. GFM autolinks bare emails to `mailto:`. */
const OPENABLE_SCHEMES = ["http://", "https://", "mailto:"] as const;

/**
 * The URL to hand the host, or `null` if this href isn't the system's to open. Takes the
 * authored href (`getAttribute`): the `href` property would resolve relative links
 * against the app's origin. Control characters refuse (e.g. a `\n`-spliced second line).
 */
export function externalUrl(href: string | null | undefined): string | null {
  if (!href) return null;
  if (/[\u0000-\u001f\u007f]/.test(href)) return null;
  const openable = OPENABLE_SCHEMES.some(
    // `>` not `>=`: a bare "https://" names nothing to open.
    (scheme) => href.length > scheme.length && href.slice(0, scheme.length).toLowerCase() === scheme,
  );
  return openable ? href : null;
}

/**
 * Does this href point inside the document on screen (including B2's wikilink `#`)? The
 * only in-place navigation allowed. Anything else the sanitizer lets through but
 * `externalUrl` won't open (relative paths, `tel:`, …) must be cancelled by the click
 * handler, or the webview navigates away from the app.
 */
export function isInPageAnchor(href: string | null | undefined): boolean {
  return typeof href === "string" && href.startsWith("#");
}
