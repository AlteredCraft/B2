// Tests for the rich-paste conversion (paste.ts) — clipboard HTML → B2 Markdown.
// Run directly:  node --experimental-strip-types src/paste.test.ts
// Hand-rolled asserts, the format.test.ts / wikicomplete.test.ts idiom.
//
// These feed *real* HTML strings: turndown ships domino for non-browser hosts, so the
// same conversion the webview runs is exercised here off the source, no DOM shim.
// The fixtures below are the shapes a real clipboard carries — WebKit's inlined
// computed styles (`<span style="font-weight: 700">`), Google Docs' `<b
// style="font-weight:normal">` wrapper, `<meta charset>` preambles.

import { htmlToMarkdown, markdownForPaste } from "./paste.ts";

let checks = 0;

function assertEq(actual: unknown, expected: unknown, label: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}\n  expected: ${e}\n  actual:   ${a}`);
  checks++;
}

// --- the block vocabulary ------------------------------------------------------------

assertEq(
  htmlToMarkdown("<h1>One</h1><h2>Two</h2><h3>Three</h3>"),
  "# One\n\n## Two\n\n### Three",
  "headings become ATX headings",
);

assertEq(
  htmlToMarkdown("<p>see <a href='https://ex.com/a?b=1'>the docs</a>.</p>"),
  "see [the docs](https://ex.com/a?b=1).",
  "links keep their href",
);

assertEq(
  htmlToMarkdown("<ul><li>one<ul><li>nested</li></ul></li><li>two</li></ul>"),
  "- one\n  - nested\n- two",
  "bullets nest at the marker's own width (not turndown's 4-space default)",
);

assertEq(
  htmlToMarkdown("<ol start='3'><li>a</li><li>b</li></ol>"),
  "3. a\n4. b",
  "ordered lists honour `start`",
);

assertEq(
  htmlToMarkdown("<blockquote><p>quoted</p></blockquote>"),
  "> quoted",
  "blockquotes",
);

assertEq(
  htmlToMarkdown("<p>use <code>foo()</code></p><pre><code class='language-rust'>fn main() {}\n</code></pre>"),
  "use `foo()`\n\n```rust\nfn main() {}\n```",
  "inline code, and a fenced block that keeps its language",
);

assertEq(
  htmlToMarkdown("<pre>line1\nline2</pre>"),
  "```\nline1\nline2\n```",
  "a bare <pre> (no <code>) keeps its newlines as a fenced block",
);

assertEq(
  htmlToMarkdown("<pre>a\n```\nb</pre>"),
  "````\na\n```\nb\n````",
  "a bare <pre> containing a fence gets a longer one",
);

assertEq(
  htmlToMarkdown("<p>a</p><hr><p>b</p>"),
  "a\n\n---\n\nb",
  "thematic break is `---` (what B2's own notes use)",
);

assertEq(
  htmlToMarkdown(
    "<table><thead><tr><th>h1</th><th>h2</th></tr></thead><tbody><tr><td>a</td><td>b</td></tr></tbody></table>",
  ),
  "| h1 | h2 |\n| --- | --- |\n| a | b |",
  "GFM table (the ⌘T scaffold's shape — format.ts)",
);

assertEq(
  htmlToMarkdown("<p><del>gone</del> <s>also</s></p>"),
  "~~gone~~ ~~also~~",
  "GFM strikethrough",
);

assertEq(
  htmlToMarkdown("<p><img src='https://e.com/i.png' alt='pic'></p>"),
  "![pic](https://e.com/i.png)",
  "an image keeps its remote URL — nothing is downloaded into the vault",
);

// --- emphasis, including the styles a real clipboard carries --------------------------

assertEq(
  htmlToMarkdown("<p><strong>bold</strong> and <em>italic</em> and <b>b</b>/<i>i</i></p>"),
  "**bold** and *italic* and **b**/*i*",
  "tag-borne emphasis",
);

assertEq(
  htmlToMarkdown(
    "<p><span style='font-weight: 700'>heavy</span> and <span style='font-style: italic'>slanted</span></p>",
  ),
  "**heavy** and *slanted*",
  "style-borne emphasis — how WebKit serializes a copied fragment",
);

assertEq(
  htmlToMarkdown("<p><span style='font-weight:600;font-style:italic'>both</span></p>",),
  "***both***",
  "one span can carry both marks",
);

assertEq(
  htmlToMarkdown(
    "<b style='font-weight:normal' id='docs-internal-guid-x'><span style='font-weight:400'>plain</span> <span style='font-weight:700'>bold</span></b>",
  ),
  "plain **bold**",
  "Google Docs' `<b style=font-weight:normal>` wrapper is not bold (an explicit style wins over the tag)",
);

assertEq(
  htmlToMarkdown("<h2><span style='font-weight:700'>Title</span></h2><h3><strong>T</strong></h3>"),
  "## Title\n\n### T",
  "a heading is already emphatic — no `## **Title**`",
);

assertEq(
  htmlToMarkdown("<p><strong>a <b>b</b></strong></p>"),
  "**a b**",
  "nested bold marks once",
);

assertEq(
  htmlToMarkdown(
    "<table><tr><th><span style='font-weight:700'>head</span></th></tr><tr><td>body</td></tr></table>",
  ),
  "| head |\n| --- |\n| body |",
  "a table header cell is bold by CSS, never by markers",
);

// --- text fidelity --------------------------------------------------------------------

assertEq(
  htmlToMarkdown("<p>2 * 3 and [[wiki]] and _under_</p>"),
  "2 \\* 3 and \\[\\[wiki\\]\\] and \\_under\\_",
  "Markdown syntax in pasted prose is escaped — a pasted `[[x]]` must not author an edge",
);

assertEq(
  htmlToMarkdown("<p>a&nbsp;b</p>"),
  "a b",
  "non-breaking spaces (endemic in web copy) become plain spaces",
);

assertEq(
  htmlToMarkdown("<style>p{color:red}</style><p>hi</p><script>alert(1)</script>"),
  "hi",
  "<style>/<script> text never lands in the note",
);

assertEq(
  htmlToMarkdown("\n\n  <p>a</p>\n\n\n<p>b</p>  \n"),
  "a\n\nb",
  "surrounding blank space is trimmed and runs collapse to one blank line",
);

assertEq(htmlToMarkdown("<p>   </p>"), "", "a blank fragment converts to nothing");

// --- markdownForPaste: the intervene-or-not decision ----------------------------------

assertEq(
  markdownForPaste("", "hello"),
  null,
  "no text/html on the clipboard — the editor's own plain paste runs",
);

assertEq(
  markdownForPaste("<meta charset='utf-8'><span>2 * 3</span>", "2 * 3"),
  null,
  "unformatted HTML: the plain text is better than escaped Markdown, so don't intervene",
);

assertEq(
  markdownForPaste("<p>a</p><p>b</p>", "a\n\nb"),
  null,
  "block structure alone is not formatting — paragraphs already survive as plain text",
);

assertEq(markdownForPaste("<h2>Hi</h2>", "Hi"), "## Hi", "real formatting converts");

assertEq(
  markdownForPaste("<ul><li>one</li><li>two</li></ul>", "one\ntwo"),
  "- one\n- two",
  "a list converts",
);

assertEq(
  markdownForPaste("<p><img src='x.png' alt='a'></p>", ""),
  "![a](x.png)",
  "an HTML-only clipboard (an image) has no plain fallback to prefer",
);

assertEq(markdownForPaste("   ", "  "), null, "a blank clipboard is nobody's business");

console.log(`paste.test.ts: ${checks} checks passed`);
