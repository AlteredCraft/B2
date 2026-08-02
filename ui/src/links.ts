// Where a link in a note goes.
//
// A note body can carry an ordinary Markdown link, and following one *in place* is the
// one navigation this app must never perform: the webview holds the whole application, so
// a `https://…` click replaces B2 with a web page in a window with no address bar, no
// back button and no way home. The app is simply gone until it is relaunched.
//
// So a web link is an **OS handoff**, exactly as *Open in system default* is on the
// resource card (`open_resource`): main.ts's click delegation asks this module whether an
// anchor is the system's to open, and hands the URL to the host, which opens it in the
// user's browser or mail app.
//
// This is the *routing* half of the rule. The host holds the other half and is the
// authority — `is_openable_link` in `crates/b2-desktop/src/commands.rs` refuses anything
// outside the same three schemes, because a note is untrusted input (invariant E5) and
// `open` launches whatever app has registered a scheme. Two independent layers, the same
// posture as `sanitize.ts` and the CSP; the schemes are spelled on both sides, so **change
// them together**, the way `WRITE_CONFLICT_MESSAGE` and `VAULT_CHANGED_EVENT` are.
//
// Pure string logic, no DOM, so node runs its test straight off the source (`npm test`).

/** The schemes B2 hands to the OS, lowercase, each including the punctuation that ends
 *  it. `mailto:` is here because GFM autolinks a bare email address into one, so a note
 *  produces these without anybody typing a scheme. */
const OPENABLE_SCHEMES = ["http://", "https://", "mailto:"] as const;

/**
 * The URL to hand the host, or `null` if this href isn't the system's to open.
 *
 * Takes the **authored** href (`getAttribute("href")`, not the `href` property): the
 * property resolves a relative link against the app's own origin, which would turn
 * `other.md` into an absolute in-app URL and lose the very distinction being drawn.
 *
 * Null covers the in-page anchors B2 mints (`href="#"` on a wikilink), relative links
 * into the vault, and every scheme outside the three — none of which this app opens
 * outward. A control character refuses outright: it can't appear unencoded in a real
 * URL, and it is how a `\n`-spliced second line would ride along to the OS.
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
