// Tests for fenced-code syntax highlighting (highlight.ts). Run directly:
//   node --experimental-strip-types src/highlight.test.ts
// Hand-rolled asserts, the format.test.ts / paste.test.ts idiom.
//
// These run the *real* grammars: a Lezer parser is plain data-in/data-out with no DOM, so
// `@codemirror/language-data` loads and parses in bare node exactly as it does in the
// webview. `highlightCodeBlocks` is the one DOM-touching export and stays out of scope.

import { highlightHtml, langFromClass, resolveLang } from "./highlight.ts";

let checks = 0;

function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
  checks++;
}

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

/** Strip the `tok-*` spans back out, undoing the escaping, to recover the source text. */
function unspan(s: string): string {
  return s
    .replace(/<span class="[^"]*">/g, "")
    .replace(/<\/span>/g, "")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&");
}

/** `highlightHtml`, asserting the language resolved (the null cases are tested apart). */
async function hl(code: string, lang: string): Promise<string> {
  const html = await highlightHtml(code, lang);
  if (html === null) throw new Error(`no grammar resolved for \`${lang}\``);
  return html;
}

// --- langFromClass ------------------------------------------------------------------

// What `marked` writes for a ```rust fence — the tag comes back verbatim, case included
// (the grammar lookup is what's case-insensitive).
assertEq(langFromClass("language-rust"), "rust", "the language class yields its tag");
assertEq(langFromClass("language-TypeScript"), "TypeScript", "case is left to the lookup");
// The class sits among others, in either order.
assertEq(langFromClass("hljs language-python"), "python", "found after another class");
assertEq(langFromClass("language-go extra"), "go", "found before another class");
// An untagged fence carries no language class at all; a bare `language-` names nothing.
assertEq(langFromClass(""), null, "no class, no language");
assertEq(langFromClass("some-class"), null, "an unrelated class is not a language");
assertEq(langFromClass("language-"), null, "the empty tag is not a language");

// --- resolveLang --------------------------------------------------------------------

const name = (info: string): string | null => resolveLang(info)?.name ?? null;

// The tags a vault's fences actually carry, including the aliases and casings.
assertEq(name("rust"), "Rust", "the plain tag");
assertEq(name("JS"), "JavaScript", "an alias, case-insensitively");
assertEq(name("ts"), "TypeScript", "the TypeScript alias");
assertEq(name("sh"), "Shell", "the shell alias");
assertEq(name("yml"), "YAML", "the yaml alias");
// A fence's info string can carry attributes past the tag (```rust,ignore is rustdoc's).
assertEq(name("rust,ignore"), "Rust", "only the first word is the tag");
assertEq(name("python title=foo"), "Python", "attributes after the tag are ignored");
// An untagged fence, and a tag no grammar claims, both mean "leave it plain".
assertEq(name(""), null, "an untagged fence resolves to nothing");
assertEq(name("mermaid"), null, "an unknown tag resolves to nothing");
// ```text means "don't highlight" — and would otherwise fuzzy-match TeX, whose `tex`
// alias is a substring of it. The guard is why this resolver exists rather than a bare
// `matchLanguageName` call on each surface.
assertEq(name("text"), null, "```text stays plain, not TeX");
assertEq(name("plaintext"), null, "```plaintext stays plain");
assertEq(name("none"), null, "```none stays plain");
// The real TeX tag still resolves — the guard is a list, not a heuristic.
assertEq(name("tex"), "LaTeX", "```tex still resolves to the TeX grammar");

// --- highlightHtml ------------------------------------------------------------------

const rust = await hl("fn main() {}", "rust");
assertHas(rust, '<span class="tok-keyword">fn</span>', "a keyword is tagged");
assertHas(rust, '<span class="tok-fn">main</span>', "a definition is tagged");

// The text is reproduced byte for byte once the spans come off. Highlighting is pure
// presentation — the buffer B2 writes back is never involved, and neither is this output.
const src = "def f(x):\n    # a comment\n    return x < 1 & 2 > 3\n";
const py = await hl(src, "python");
assertEq(unspan(py), src, "the source survives a round trip through the highlighter");
assertHas(py, '<span class="tok-comment"># a comment</span>', "a comment is tagged");

// `<`, `>` and `&` in code are escaped, not injected. The output goes in through
// innerHTML, so this is the safety property, not a cosmetic one.
const html = await hl('<script>alert("x")</script>', "html");
assertNot(html, "<script", "a script tag in a code block is escaped, never an element");
assertEq(unspan(html), '<script>alert("x")</script>', "escaped markup round-trips");
// A coloured `tagName` is why we ship our own highlighter rather than `classHighlighter`,
// which maps neither tag nor attribute names — an HTML fence would come back near-plain.
assertHas(html, '<span class="tok-keyword">script</span>', "an html tag name is tagged");

// Tags resolve through aliases, the same lookup `@codemirror/lang-markdown` runs for the
// editor's fences — so a fence highlights identically in the reading view and the editor.
assertHas(await hl("let x = 1", "js"), "tok-keyword", "`js` resolves to JavaScript");
assertHas(await hl("a: 1", "yaml"), "tok-", "`yaml` resolves");

// An unknown tag isn't an error — the block just stays plain.
assertEq(await highlightHtml("...", "no-such-language"), null, "an unknown tag yields null");
// Nor is an oversized block: past MAX_CHARS it's a data dump, not a code sample, and
// parsing it would block the paint.
assertEq(await highlightHtml("x".repeat(100_001), "rust"), null, "an oversized block is skipped");

console.log(`highlight.test.ts: ${checks} checks passed`);
