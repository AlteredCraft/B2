// Rich paste, the pure half: the clipboard's `text/html` flavor → B2 Markdown, the vault's
// only authored format (data-model.md). main.ts wraps it in a CodeMirror `paste` handler.
//
// Markdown syntax in pasted prose is escaped, so a pasted `[[Rust]]` never authors an edge.
// And `markdownForPaste` returns null when nothing was formatted, so a plain paste keeps
// the editor's own path.

import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";

/** A `font-weight` / `font-style` verdict: asserted, denied, or not spoken to. */
type Mark = "on" | "off" | "unset";

/** Elements whose emphasis we decide ourselves (tag *and* inline style — see `boldState`). */
const EMPHASIS_TAGS = new Set(["B", "STRONG", "I", "EM", "SPAN", "FONT"]);

/**
 * Ancestors already bold by CSS, so marking inside them is noise (`## **Title**`). `<b>` is
 * not listed: Google Docs' wrapper is a `<b>` that isn't bold (see `boldState`).
 */
const BOLD_ABOVE = new Set(["H1", "H2", "H3", "H4", "H5", "H6", "TH"]);

/**
 * Read one inline-style property off the element. Parses the `style` attribute rather
 * than `node.style`, so it also runs on the tests' node DOM (turndown's domino).
 */
function styleProp(node: HTMLElement, prop: string): string {
  const style = node.getAttribute("style");
  if (!style) return "";
  const m = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, "i").exec(style);
  return m ? m[1].trim().toLowerCase() : "";
}

/**
 * Is this element bold? An inline style wins over the tag: Google Docs wraps its whole
 * fragment in `<b style="font-weight:normal">` and marks bold runs with styled spans.
 */
function boldState(node: HTMLElement): Mark {
  const w = styleProp(node, "font-weight");
  if (w) {
    const n = Number.parseInt(w, 10);
    if (!Number.isNaN(n)) return n >= 600 ? "on" : "off";
    if (w === "bold" || w === "bolder") return "on";
    if (w === "normal" || w === "lighter") return "off";
  }
  return node.nodeName === "B" || node.nodeName === "STRONG" ? "on" : "unset";
}

/** The italic twin of `boldState`. */
function italicState(node: HTMLElement): Mark {
  const s = styleProp(node, "font-style");
  if (s) return s === "italic" || s === "oblique" ? "on" : "off";
  return node.nodeName === "I" || node.nodeName === "EM" ? "on" : "unset";
}

/** Is the mark already carried by an ancestor — so wrapping again would only add markers? */
function markedAbove(node: HTMLElement, state: (n: HTMLElement) => Mark, tags: Set<string>): boolean {
  for (let p: Node | null = node.parentNode; p && p.nodeType === 1; p = p.parentNode) {
    const el = p as HTMLElement;
    if (tags.has(el.nodeName) || state(el) === "on") return true;
  }
  return false;
}

/** A backtick fence longer than any run inside `text`, so embedded fences survive. */
function fenceFor(text: string): string {
  let longest = 0;
  for (const m of text.matchAll(/`{3,}/g)) longest = Math.max(longest, m[0].length);
  return "`".repeat(Math.max(3, longest + 1));
}

/** The converter, configured for the Markdown dialect B2's own notes use (GFM included). */
function makeService(): TurndownService {
  const service = new TurndownService({
    headingStyle: "atx",
    hr: "---",
    bulletListMarker: "-",
    codeBlockStyle: "fenced",
    emDelimiter: "*",
    strongDelimiter: "**",
    linkStyle: "inlined",
  });
  service.use(gfm);
  // A copied fragment can carry these wholesale; their text is not the human's prose.
  service.remove(["script", "style", "noscript"]);

  // Rules added last are matched first, so these three shadow the defaults they replace.

  // Emphasis per element, not per tag (see `boldState`); one rule so a span can carry both.
  service.addRule("emphasis", {
    filter: (node) => EMPHASIS_TAGS.has(node.nodeName),
    replacement: (content, node) => {
      if (!content.trim()) return content;
      let out = content;
      if (italicState(node) === "on" && !markedAbove(node, italicState, new Set(["I", "EM"]))) {
        out = `*${out}*`;
      }
      if (boldState(node) === "on" && !markedAbove(node, boldState, BOLD_ABOVE)) {
        out = `**${out}**`;
      }
      return out;
    },
  });

  // `~~`, not the gfm plugin's single `~`, which other Markdown tools may not read.
  service.addRule("strikethrough", {
    filter: (node) => node.nodeName === "DEL" || node.nodeName === "S" || node.nodeName === "STRIKE",
    replacement: (content) => (content.trim() ? `~~${content}~~` : content),
  });

  // Indent by the marker's width (`- ` → 2, `12. ` → 4), as a hand writes, not turndown's
  // fixed `-   `.
  service.addRule("listItem", {
    filter: "li",
    replacement: (content, node) => {
      const body = content.replace(/^\n+/, "").replace(/\n+$/, "\n");
      const parent = node.parentNode as HTMLElement | null;
      let prefix = "- ";
      if (parent && parent.nodeName === "OL") {
        const start = Number.parseInt(parent.getAttribute("start") ?? "1", 10);
        const index = Array.prototype.indexOf.call(parent.children, node);
        prefix = `${(Number.isNaN(start) ? 1 : start) + index}. `;
      }
      const pad = " ".repeat(prefix.length);
      const indented = body
        .split("\n")
        .map((line, i) => (i === 0 || line === "" ? line : pad + line))
        .join("\n");
      return prefix + indented + (node.nextSibling && !/\n$/.test(indented) ? "\n" : "");
    },
  });

  // A `<pre>` without `<code>`: turndown's fenced rule only matches `pre > code`, and would
  // collapse this into a paragraph.
  service.addRule("bareCodeBlock", {
    filter: (node) =>
      node.nodeName === "PRE" && !(node.firstChild && node.firstChild.nodeName === "CODE"),
    replacement: (_content, node) => {
      const text = (node.textContent ?? "").replace(/\n+$/, "");
      const fence = fenceFor(text);
      return `\n\n${fence}\n${text}\n${fence}\n\n`;
    },
  });

  return service;
}

const service = makeService();

/** Convert a clipboard HTML fragment to Markdown. Pure: same html in, same bytes out. */
export function htmlToMarkdown(html: string): string {
  return service
    .turndown(html)
    .replace(/\u00a0/g, " ") // web copy is full of &nbsp;, invisible and syntax-breaking
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/** Drop the backslashes the converter added, for comparison against the plain flavor. */
function unescapeMarkdown(md: string): string {
  return md.replace(/\\([\\`*_{}[\]()#+\-.!>~|])/g, "$1");
}

/** Whitespace-insensitive shape of a string — line breaks are not formatting. */
function collapse(s: string): string {
  return s.replace(/\s+/g, " ").trim();
}

/**
 * What a paste should insert, or null to leave it to the editor's plain-text path: null
 * whenever the conversion captured nothing the plain flavor lacks.
 */
export function markdownForPaste(html: string, text: string): string | null {
  if (!html.trim()) return null;
  const md = htmlToMarkdown(html);
  if (!md) return null;
  if (!text.trim()) return md;
  return collapse(unescapeMarkdown(md)) === collapse(text) ? null : md;
}
