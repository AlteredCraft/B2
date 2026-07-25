// Rich paste, the pure half — the clipboard's `text/html` flavor → B2 Markdown.
// main.ts wraps this in a CodeMirror `paste` DOM handler; the split keeps the
// conversion node-testable with no editor dependency (the format.ts / move.ts pattern).
//
// Why it exists: copying from a web page loses every heading, bold and list, because
// the editor's default paste takes `text/plain`. Markdown is the vault's *sole authored
// subset* (data-model.md), so the fix is a conversion at the seam, not a rich-text
// buffer: what the paste inserts is ordinary Markdown bytes the human could have typed.
//
// Two properties this owes the vault:
//   - **Faithful, not authoritative.** Markdown syntax inside pasted prose is escaped
//     (turndown's default), so a page containing `[[Rust]]` pastes as literal text and
//     never authors an edge — a connection exists only once *you* write it.
//   - **Plain wins when nothing was formatted.** `markdownForPaste` returns null unless
//     the conversion actually captured something, so an ordinary text paste keeps the
//     editor's own path — no escape noise, no reflow.

import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";

/** A `font-weight` / `font-style` verdict: asserted, denied, or not spoken to. */
type Mark = "on" | "off" | "unset";

/** Elements whose emphasis we decide ourselves (tag *and* inline style — see `boldState`). */
const EMPHASIS_TAGS = new Set(["B", "STRONG", "I", "EM", "SPAN", "FONT"]);

/**
 * Ancestors that are already bold *by CSS* — marking inside them is noise, not fidelity
 * (`## **Title**`). A `<b>`/`<strong>` ancestor is not listed: whether it is really bold
 * is `boldState`'s question, since Google Docs' wrapper is a `<b>` that isn't.
 */
const BOLD_ABOVE = new Set(["H1", "H2", "H3", "H4", "H5", "H6", "TH"]);

/**
 * Read one inline-style property off the element.
 *
 * Deliberately parses the `style` *attribute* rather than touching `node.style`: the
 * same code then runs against the webview's DOM and the tests' node DOM (turndown's
 * bundled domino), and against the copied fragment's own declarations only — never a
 * stylesheet's, which the clipboard never carries.
 */
function styleProp(node: HTMLElement, prop: string): string {
  const style = node.getAttribute("style");
  if (!style) return "";
  const m = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, "i").exec(style);
  return m ? m[1].trim().toLowerCase() : "";
}

/**
 * Is this element bold? An explicit inline style **wins over the tag** — that one rule
 * is what makes a Google Docs paste readable, since Docs wraps its whole fragment in
 * `<b style="font-weight:normal">` and marks the genuinely bold runs with styled spans.
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

/**
 * The converter, configured to emit the Markdown dialect B2's own notes use: ATX
 * headings, `-` bullets, `---` breaks, fenced code, `*`/`**` emphasis, GFM tables and
 * strikethrough (the reading view parses with `gfm: true`, and ⌘T writes pipe tables).
 */
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

  // Emphasis, decided per element instead of per tag — see `boldState`. One rule for
  // both marks so a single span can carry both (`***both***`).
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

  // Strikethrough with the doubled tilde. The gfm plugin emits a single `~`, which the
  // GFM spec allows but nothing else in B2 writes — the reading view's `~~` is the form
  // that survives every other Markdown tool the vault's files may pass through.
  service.addRule("strikethrough", {
    filter: (node) => node.nodeName === "DEL" || node.nodeName === "S" || node.nodeName === "STRIKE",
    replacement: (content) => (content.trim() ? `~~${content}~~` : content),
  });

  // List items indented by the marker's own width (`- ` → 2, `12. ` → 4) rather than
  // turndown's fixed `-   ` + 4 spaces: the source pane shows these bytes, and this is
  // the shape a hand writes.
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

  // A `<pre>` with no leading `<code>` — turndown's fenced rule only matches `pre >
  // code`, and everything else would have its newlines collapsed into a paragraph.
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
 * What a paste should insert, or `null` to leave it to the editor's plain-text path.
 *
 * `null` is the answer whenever the conversion captured nothing the plain flavor didn't
 * already carry — an unformatted copy, or one whose only structure is paragraph breaks.
 * Intervening there would only add escape backslashes to the note.
 */
export function markdownForPaste(html: string, text: string): string | null {
  if (!html.trim()) return null;
  const md = htmlToMarkdown(html);
  if (!md) return null;
  if (!text.trim()) return md;
  return collapse(unescapeMarkdown(md)) === collapse(text) ? null : md;
}
