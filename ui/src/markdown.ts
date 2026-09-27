// Markdown → HTML: the one path a note body or chat answer takes into the window. Note
// content is untrusted (E5, GH #77), so output is sanitized by sanitize.ts, wired below as
// `marked`'s `postprocess` hook to cover every caller by construction.

import { marked, type Tokens, type TokenizerAndRendererExtension } from "marked";
// Relative imports carry `.ts`: node's type-stripping resolves by real filename.
import { escapeHtml } from "./escape.ts";
import { sanitizeHtml } from "./sanitize.ts";
import { embedWidth, NO_EMBED_IMAGES, WIKILINK_ANCHORED, type EmbedImages } from "./embeds.ts";
import { baseName } from "./move.ts";

// A `[[target]]` / `[[target|label]]` wikilink becomes an in-app anchor carrying the raw
// target (spec §4); main.ts opens it on click.
//
// `![[target]]` is the embed form: with a loaded picture the `<img>` becomes the anchor's
// label, otherwise it reads as its link. The `!` is consumed, and on an embed the `|`-part
// is a width (`![[shot.png|500]]`, `embedWidth`), not a label.
const wikilink: TokenizerAndRendererExtension = {
  name: "wikilink",
  level: "inline",
  start(src: string) {
    const i = src.indexOf("[[");
    if (i < 0) return undefined;
    // Back up over the embed `!`, or marked emits it as plain text first.
    return i > 0 && src[i - 1] === "!" ? i - 1 : i;
  },
  tokenizer(src: string) {
    const m = WIKILINK_ANCHORED.exec(src);
    if (!m) return undefined;
    const embed = m[1] === "!";
    const target = m[2].trim();
    return {
      type: "wikilink",
      raw: m[0],
      embed,
      target,
      width: embed ? embedWidth(m[3]) : null,
      label: embed ? target : (m[3] ?? m[2]).trim(),
    } as Tokens.Generic;
  },
  renderer(token: Tokens.Generic) {
    const target = String(token.target);
    const anchor = `<a class="wikilink" data-target="${escapeHtml(target)}" href="#">`;
    const src = token.embed ? renderImages.get(target) : undefined;
    if (!src) return `${anchor}${escapeHtml(String(token.label))}</a>`;
    // Alt text is the filename: all B2 knows about the picture.
    const name = baseName(target);
    const width = typeof token.width === "number" ? ` width="${token.width}"` : "";
    return `${anchor}<img class="embed-image" src="${escapeHtml(src)}" alt="${escapeHtml(
      name,
    )}"${width}></a>`;
  },
};

// The pictures the current `renderMarkdown` call may draw. Module-local because `marked`'s
// extensions are registered globally; safe only because `marked.parse(…, { async: false })`
// is synchronous, so no other render can interleave before the `finally`.
let renderImages: EmbedImages = NO_EMBED_IMAGES;

// Wrap each table in a scroll box so a wide one scrolls within its column. The table must
// stay `display: table`: as a block, `<thead>`/`<tbody>` split into two anonymous tables
// and `border-collapse` leaves a gap. marked escapes cell content, so these are the only
// literal `<table>` tags.
function wrapTables(html: string): string {
  return html
    .replace(/<table>/g, '<div class="md-table"><table>')
    .replace(/<\/table>/g, "</table></div>");
}

// The trust boundary (E5, GH #77): sanitizing is the last step, after the table wrapper,
// for every `marked.parse` in the app.
//
// `breaks: true` makes one newline a `<br>` (Obsidian's reading), so the reading view
// agrees with the editor on where lines end (linebreaks.test.ts).
marked.use({
  extensions: [wikilink],
  gfm: true,
  breaks: true,
  hooks: { postprocess: (html: string) => sanitizeHtml(wrapTables(html)) },
});

/**
 * Note body → sanitized HTML. `images` are the note's loaded pictures by vault path, for
 * `![[…]]` embeds; without one, an embed reads as its link.
 */
export function renderMarkdown(md: string, images: EmbedImages = NO_EMBED_IMAGES): string {
  renderImages = images;
  try {
    return marked.parse(md, { async: false }) as string;
  } finally {
    renderImages = NO_EMBED_IMAGES;
  }
}
