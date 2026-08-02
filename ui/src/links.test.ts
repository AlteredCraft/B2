// The note-link routing rule (links.ts), pinned. Pure string logic — no DOM — so node
// runs it straight off the source: `npm test`. Dependency-free like move.test.ts.
//
// The acceptances are the feature; the refusals are the point. Every `null` below is a
// click that must NOT become an OS handoff, and the host refuses the same set a second
// time (`only_web_links_are_openable` in crates/b2-desktop/src/commands.rs) — the two
// halves of one rule, spelled on both sides of the seam, so these cases are deliberately
// the same cases.
import { externalUrl } from "./links.ts";

let passed = 0;

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function check(name: string, fn: () => void): void {
  fn();
  passed++;
  console.log(`  ok  ${name}`);
}

// --- what B2 hands to the OS --------------------------------------------------------

check("a web link comes back verbatim, query and fragment intact", () => {
  const url = "https://example.com/docs?q=b2#install";
  assert(externalUrl(url) === url, "the href is passed through unchanged");
  assert(externalUrl("http://example.com") === "http://example.com", "plain http too");
});

check("a scheme is case-insensitive, the rest of the URL is not", () => {
  assert(externalUrl("HTTPS://Example.COM/A") === "HTTPS://Example.COM/A", "kept as authored");
  assert(externalUrl("MailTo:someone@example.com") !== null, "mailto in any casing");
});

check("an email address is a link too — GFM autolinks one into mailto:", () => {
  const url = "mailto:someone@example.com?subject=hi";
  assert(externalUrl(url) === url, "mailto is handed over");
});

// --- what stays inside the app ------------------------------------------------------

check("B2's own in-page anchors are not the system's to open", () => {
  // Every wikilink renders as `href="#"` (render.ts) and is handled before this ever runs;
  // it must not *also* look like a web link if the ordering ever changes.
  assert(externalUrl("#") === null, "a bare fragment");
  assert(externalUrl("#a-heading") === null, "a heading anchor");
});

check("a relative link into the vault is not opened outward", () => {
  for (const href of ["other.md", "./notes/other.md", "../assets/report.pdf", "/absolute"]) {
    assert(externalUrl(href) === null, `${href} stays in the app`);
  }
});

check("no other scheme reaches the OS", () => {
  // Each of these would hand a note the ability to name a program for the OS to launch.
  for (const href of [
    "file:///etc/passwd",
    "javascript:alert(1)", // the sanitizer drops it first; this is the second layer
    "data:text/html;base64,PHNjcmlwdD4=",
    "vscode://file/etc/passwd",
    "x-b2-evil://run",
    "tel:+15551234",
  ]) {
    assert(externalUrl(href) === null, `${href} is refused`);
  }
});

check("a scheme naming nothing is not a link", () => {
  assert(externalUrl("https://") === null, "no host to open");
  assert(externalUrl("mailto:") === null, "no address to write to");
});

check("whitespace and control characters refuse rather than being trimmed away", () => {
  // Leading space: a scheme starts a URL or it isn't one. A newline: the shape of a
  // second line smuggled into an href, which is never something to hand to the OS.
  assert(externalUrl(" https://example.com") === null, "a leading space is not a scheme");
  assert(externalUrl("https://ok.example\nmailto:x@y.z") === null, "a spliced newline");
  assert(externalUrl("https://ok.example\u0000") === null, "a NUL byte");
});

check("an absent href is not a link", () => {
  assert(externalUrl(null) === null, "no attribute at all");
  assert(externalUrl(undefined) === null, "nor an undefined one");
  assert(externalUrl("") === null, "nor an empty one");
});

console.log(`\nlinks.test.ts: ${passed} checks passed`);
