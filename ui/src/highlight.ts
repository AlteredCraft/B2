// Syntax highlighting — one engine, one palette, every surface: the reading view, live
// preview's fences, and source mode's whole document (main.ts wires all three; style.css
// scopes each).
//
// The dependency is CodeMirror's own registry: `@codemirror/language-data` (MIT, the CM6
// project's ~140 Lezer grammars, each behind its own dynamic `import()` so a vault of
// plain prose downloads none of them). It's the pragmatic pick *because the editor is
// already CodeMirror*: handing `resolveLang` to `markdown({ codeLanguages })` highlights
// the editor's fenced blocks with no second engine, and running the same grammars over the
// reading view's `<pre><code>` here keeps read ↔ edit identical — the property the
// live-preview table widget already leans on. A standalone highlighter (highlight.js,
// Shiki) would have meant a second grammar set, a second token vocabulary, and read/edit
// drift.
//
// Nothing here changes a byte of the note: this is a post-render pass over B2's own
// already-escaped HTML (render.ts), and every text run is re-escaped on the way back in.

import { LanguageDescription, type LanguageSupport } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightCode, tagHighlighter, tags as t } from "@lezer/highlight";
// Imported by full filename so highlight.test.ts can run off the source under node's
// type-stripping — the same reason panes.test.ts does (see tsconfig.json).
import { escapeHtml } from "./escape.ts";

/**
 * Lezer highlight tags → the `tok-*` class vocabulary style.css colors.
 *
 * Ours rather than `@lezer/highlight`'s stock `classHighlighter` because that one covers
 * neither `tagName` nor `attributeName` — an HTML or XML fence would come back nearly
 * uncolored. Unmapped tags (a plain `variableName`, say) simply inherit the body color,
 * which is deliberate: coloring every identifier turns a code block into confetti.
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
    // The name of a thing being defined or called — one colour for both; the distinction
    // isn't worth a second hue in a note's code sample.
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
    // `processingInstruction` is here rather than with the keywords because it's what
    // lezer-markdown tags *its own* markup with (`#`, `**`, `-`, the fences) — and source
    // mode paints the whole document, where loud markup would drown the prose.
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

/** Grammars load lazily and once — the *promise* is the cache entry, so ten Rust blocks
 *  share one chunk fetch. A grammar that fails to load caches `null` rather than
 *  retrying on every render; the block just stays plain. */
const grammars = new Map<string, Promise<LanguageSupport | null>>();

function grammar(desc: LanguageDescription): Promise<LanguageSupport | null> {
  const cached = grammars.get(desc.name);
  if (cached) return cached;
  const loading = desc.load().catch(() => null);
  grammars.set(desc.name, loading);
  return loading;
}

/** Past this, a fence is a data dump rather than a code sample — parse it and the paint
 *  pass would block the UI thread, so it stays plain text. */
const MAX_CHARS = 100_000;

/** Tags that mean "don't highlight this". Worth naming: `matchLanguageName`'s fuzzy pass
 *  is a substring match, so ```text would otherwise land on TeX (`text`.indexOf(`tex`)). */
const PLAIN = new Set(["text", "plain", "plaintext", "txt", "none"]);

/**
 * A fence's info string → the grammar to parse its body with, or null to leave it plain.
 *
 * The *one* resolver: `markdown({ codeLanguages })` takes it for the editor and
 * `highlightHtml` calls it for the reading view, so a given fence resolves the same way on
 * both surfaces — including the fuzzy alias matching (```js → JavaScript) that
 * `@codemirror/lang-markdown` would otherwise apply on the editor side alone.
 */
export function resolveLang(info: string): LanguageDescription | null {
  // A fence's info string can carry more than the tag (```rust,ignore) — take the first
  // word, the same slice lang-markdown makes.
  const tag = /^\S*/.exec(info.trim())?.[0].toLowerCase() ?? "";
  if (!tag || PLAIN.has(tag)) return null;
  return LanguageDescription.matchLanguageName(languages, tag, true);
}

/** The language tag `marked` wrote onto a fence (`<code class="language-rust">`), or null
 *  for an untagged block. Resolving that tag to a grammar is `resolveLang`'s job — this
 *  only reads the class. */
export function langFromClass(cls: string): string | null {
  for (const c of cls.split(/\s+/)) {
    if (c.startsWith("language-") && c.length > 9) return c.slice(9);
  }
  return null;
}

/**
 * Highlight `code` as `lang`, returning HTML — escaped text wrapped in `tok-*` spans —
 * or null when the tag resolves to no grammar, the grammar won't load, or the block is
 * over `MAX_CHARS` (in every one of those cases the caller leaves the block as rendered).
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
    // `classes` comes from the table above — B2's own constants, never note content —
    // so only the text runs need escaping.
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
 * Highlight every language-tagged fence under `root`, in place. Resolves true when at
 * least one block was repainted, so the caller can re-derive anything anchored to the
 * pane's text nodes (find-in-note's Ranges).
 *
 * Progressive enhancement, by construction: grammars arrive a tick after the render, so a
 * block paints plain first and gains its colours when its chunk lands. A block whose
 * element left the DOM while that chunk loaded (the note was closed, the pane rebuilt) is
 * dropped — never written back into a stale tree.
 */
export async function highlightCodeBlocks(root: ParentNode): Promise<boolean> {
  const pending: Array<[HTMLElement, string]> = [];
  for (const node of root.querySelectorAll('pre > code[class*="language-"]')) {
    // `data-lang` marks a block this pass already painted — its text is unchanged, so
    // re-highlighting would be pure work.
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
