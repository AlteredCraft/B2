// Tests for fenced-code syntax highlighting (highlight.ts), with the real grammars (Lezer
// needs no DOM). `highlightCodeBlocks` touches the DOM and is out of scope. Run directly:
//   node --experimental-strip-types src/highlight.test.ts

import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { highlightCode } from "@lezer/highlight";
import { b2Highlighter, highlightHtml, langFromClass, resolveLang } from "./highlight.ts";

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

// The tag comes back verbatim; the grammar lookup is what's case-insensitive.
assertEq(langFromClass("language-rust"), "rust", "the language class yields its tag");
assertEq(langFromClass("language-TypeScript"), "TypeScript", "case is left to the lookup");
assertEq(langFromClass("hljs language-python"), "python", "found after another class");
assertEq(langFromClass("language-go extra"), "go", "found before another class");
assertEq(langFromClass(""), null, "no class, no language");
assertEq(langFromClass("some-class"), null, "an unrelated class is not a language");
assertEq(langFromClass("language-"), null, "the empty tag is not a language");

// --- resolveLang --------------------------------------------------------------------

const name = (info: string): string | null => resolveLang(info)?.name ?? null;

assertEq(name("rust"), "Rust", "the plain tag");
assertEq(name("JS"), "JavaScript", "an alias, case-insensitively");
assertEq(name("ts"), "TypeScript", "the TypeScript alias");
assertEq(name("sh"), "Shell", "the shell alias");
assertEq(name("yml"), "YAML", "the yaml alias");
assertEq(name("rust,ignore"), "Rust", "only the first word is the tag");
assertEq(name("python title=foo"), "Python", "attributes after the tag are ignored");
assertEq(name(""), null, "an untagged fence resolves to nothing");
assertEq(name("mermaid"), null, "an unknown tag resolves to nothing");
// ```text would otherwise fuzzy-match TeX.
assertEq(name("text"), null, "```text stays plain, not TeX");
assertEq(name("plaintext"), null, "```plaintext stays plain");
assertEq(name("none"), null, "```none stays plain");
assertEq(name("tex"), "LaTeX", "```tex still resolves to the TeX grammar");

// --- highlightHtml ------------------------------------------------------------------

const rust = await hl("fn main() {}", "rust");
assertHas(rust, '<span class="tok-keyword">fn</span>', "a keyword is tagged");
assertHas(rust, '<span class="tok-fn">main</span>', "a definition is tagged");

const src = "def f(x):\n    # a comment\n    return x < 1 & 2 > 3\n";
const py = await hl(src, "python");
assertEq(unspan(py), src, "the source survives a round trip through the highlighter");
assertHas(py, '<span class="tok-comment"># a comment</span>', "a comment is tagged");

// The output goes in through innerHTML, so escaping is the safety property.
const html = await hl('<script>alert("x")</script>', "html");
assertNot(html, "<script", "a script tag in a code block is escaped, never an element");
assertEq(unspan(html), '<script>alert("x")</script>', "escaped markup round-trips");
// Why we ship our own highlighter: `classHighlighter` maps no tag names.
assertHas(html, '<span class="tok-keyword">script</span>', "an html tag name is tagged");

assertHas(await hl("let x = 1", "js"), "tok-keyword", "`js` resolves to JavaScript");
assertHas(await hl("a: 1", "yaml"), "tok-", "`yaml` resolves");

assertEq(await highlightHtml("...", "no-such-language"), null, "an unknown tag yields null");
assertEq(await highlightHtml("x".repeat(100_001), "rust"), null, "an oversized block is skipped");

// --- the editor surfaces ------------------------------------------------------------
//
// The editor paints through the same highlighter over the parser main.ts mounts.

// Grammars load lazily (the fence is opaque until one lands), so preload Rust.
const mdParser = markdown({ base: markdownLanguage, codeLanguages: resolveLang }).language.parser;
await resolveLang("rust")?.load();

/** Every classed token the editor would paint in `doc`, as [text, classes] pairs. */
function editorTokens(doc: string): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  const put = (text: string, cls: string): void => {
    if (cls) out.push([text, cls]);
  };
  highlightCode(doc, mdParser.parse(doc), b2Highlighter, put, () => {});
  return out;
}

/** The classes on the first token reading `text` (trimmed), or null if unclassed. */
function classOf(tokens: Array<[string, string]>, text: string): string | null {
  return tokens.find(([tok]) => tok.trim() === text)?.[1] ?? null;
}

// Source mode paints the whole document, so Markdown marks must be punctuation, not
// keywords. A mark in a styled construct carries both classes; style.css orders
// `tok-punct` after `tok-fn` so the mark stays muted.
const md = editorTokens("# Heading\n\n- **bold** text\n");
assertEq(classOf(md, "#"), "tok-fn tok-punct", "a heading mark is punctuation, not a keyword");
assertEq(classOf(md, "-"), "tok-punct", "a list mark is punctuation");
assertEq(classOf(md, "**"), "tok-strong tok-punct", "an emphasis mark is bold punctuation");
assertEq(classOf(md, "Heading"), "tok-fn", "heading text carries the palette's name colour");

const fenced = editorTokens("```rust\nfn main() {}\n```\n");
assertEq(classOf(fenced, "```"), "tok-punct", "the fence itself is markup");
assertEq(classOf(fenced, "fn"), "tok-keyword", "the fence body is parsed as Rust");
assertEq(classOf(fenced, "main"), "tok-fn", "…including its definitions");

console.log(`highlight.test.ts: ${checks} checks passed`);
