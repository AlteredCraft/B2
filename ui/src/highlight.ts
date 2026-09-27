// Syntax highlighting: one engine and palette for the reading view, live preview's fences
// and source mode (wired in main.ts). Uses CodeMirror's own grammar registry
// (`@codemirror/language-data`, each grammar a lazy `import()`), so read and edit highlight
// identically with no second engine. A post-render pass over already-escaped HTML: every
// text run is re-escaped, and no byte of the note changes.

import { LanguageDescription, type LanguageSupport } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightCode, tagHighlighter, tags as t } from "@lezer/highlight";
// Full filename, for node's test runner (see tsconfig.json).
import { escapeHtml } from "./escape.ts";

/**
 * Lezer highlight tags → the `tok-*` classes style.css colors. Ours because the stock
 * `classHighlighter` misses `tagName` and `attributeName`. Unmapped tags inherit the body
 * color on purpose.
 */
export const b2Highlighter = tagHighlighter([
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], class: "tok-comment" },
  {
    // Tag names sit here too: `<div>` reads as a keyword the way `if` does.
    tag: [
      t.keyword,
      t.modifier,
      t.controlKeyword,
      t.operatorKeyword,
      t.definitionKeyword,
      t.moduleKeyword,
      t.tagName,
      t.meta,
      t.documentMeta,
      t.annotation,
    ],
    class: "tok-keyword",
  },
  {
    tag: [t.string, t.docString, t.character, t.attributeValue, t.regexp, t.escape],
    class: "tok-string",
  },
  {
    // Literals as one colour: `42`, `true`, `None`, `self`, `#fff`, `10px`.
    tag: [t.number, t.integer, t.float, t.literal, t.bool, t.atom, t.self, t.null, t.unit, t.color],
    class: "tok-number",
  },
  { tag: [t.typeName, t.className, t.namespace, t.labelName], class: "tok-type" },
  {
    // Defined or called names share one colour.
    tag: [
      t.definition(t.variableName),
      t.definition(t.propertyName),
      t.function(t.variableName),
      t.function(t.propertyName),
      t.macroName,
      t.heading,
    ],
    class: "tok-fn",
  },
  { tag: [t.propertyName, t.attributeName, t.special(t.variableName)], class: "tok-name" },
  { tag: [t.link, t.url], class: "tok-link" },
  {
    // `processingInstruction` is lezer-markdown's own markup (`#`, `**`, fences); muted so
    // source mode's markup doesn't drown the prose.
    tag: [
      t.operator,
      t.derefOperator,
      t.punctuation,
      t.separator,
      t.bracket,
      t.angleBracket,
      t.processingInstruction,
    ],
    class: "tok-punct",
  },
  { tag: t.strong, class: "tok-strong" },
  { tag: t.emphasis, class: "tok-emphasis" },
  { tag: t.strikethrough, class: "tok-strike" },
  { tag: t.inserted, class: "tok-ins" },
  { tag: t.deleted, class: "tok-del" },
  { tag: t.invalid, class: "tok-invalid" },
]);

/** Grammars load lazily and once: the promise is cached, so blocks share one fetch. A
 *  failed load caches `null`, and the block stays plain. */
const grammars = new Map<string, Promise<LanguageSupport | null>>();

function grammar(desc: LanguageDescription): Promise<LanguageSupport | null> {
  const cached = grammars.get(desc.name);
  if (cached) return cached;
  const loading = desc.load().catch(() => null);
  grammars.set(desc.name, loading);
  return loading;
}

/** Past this a fence stays plain, so parsing doesn't block the UI thread. */
const MAX_CHARS = 100_000;

/** Tags that mean "don't highlight". Needed because the fuzzy match would map ```text
 *  to TeX. */
const PLAIN = new Set(["text", "plain", "plaintext", "txt", "none"]);

/**
 * A fence's info string → its grammar, or null to leave it plain. The one resolver for
 * both the editor (`markdown({ codeLanguages })`) and the reading view.
 */
export function resolveLang(info: string): LanguageDescription | null {
  // The first word (```rust,ignore), as lang-markdown takes it.
  const tag = /^\S*/.exec(info.trim())?.[0].toLowerCase() ?? "";
  if (!tag || PLAIN.has(tag)) return null;
  return LanguageDescription.matchLanguageName(languages, tag, true);
}

/** The language tag `marked` wrote onto a fence (`language-rust`), or null. */
export function langFromClass(cls: string): string | null {
  for (const c of cls.split(/\s+/)) {
    if (c.startsWith("language-") && c.length > 9) return c.slice(9);
  }
  return null;
}

/**
 * Highlight `code` as `lang` into escaped HTML with `tok-*` spans, or null (no grammar,
 * failed load, or over `MAX_CHARS`) to leave the block as rendered.
 */
export async function highlightHtml(code: string, lang: string): Promise<string | null> {
  if (code.length > MAX_CHARS) return null;
  const desc = resolveLang(lang);
  if (!desc) return null;
  const support = await grammar(desc);
  if (!support) return null;

  let html = "";
  highlightCode(
    code,
    support.language.parser.parse(code),
    b2Highlighter,
    // `classes` are B2's constants; only text runs need escaping.
    (text, classes) => {
      const esc = escapeHtml(text);
      html += classes ? `<span class="${classes}">${esc}</span>` : esc;
    },
    () => {
      html += "\n";
    },
  );
  return html;
}

/**
 * Highlight every language-tagged fence under `root`, in place. Resolves true when a
 * block was repainted, so the caller can re-derive find-in-note's Ranges. A block that
 * left the DOM while its grammar loaded is skipped.
 */
export async function highlightCodeBlocks(root: ParentNode): Promise<boolean> {
  const pending: Array<[HTMLElement, string]> = [];
  for (const node of root.querySelectorAll('pre > code[class*="language-"]')) {
    // `data-lang` marks a block already painted.
    if (!(node instanceof HTMLElement) || node.dataset.lang) continue;
    const lang = langFromClass(node.className);
    if (lang) pending.push([node, lang]);
  }

  let painted = false;
  await Promise.all(
    pending.map(async ([node, lang]) => {
      const html = await highlightHtml(node.textContent ?? "", lang);
      if (html === null || !node.isConnected) return;
      node.innerHTML = html;
      node.dataset.lang = lang;
      painted = true;
    }),
  );
  return painted;
}
