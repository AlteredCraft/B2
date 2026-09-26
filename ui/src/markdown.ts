// Markdown → HTML: the one path a note body (or a chat answer) takes into the window, and
// the trust boundary it crosses on the way (invariant E5 — **note content is untrusted
// input**). Authorship is not trust: a `.md` can come from anyone (a shared vault, a
// downloaded or clipped note), so every body is treated as hostile, and the output is
// sanitized by `sanitize.ts`, wired below as `marked`'s `postprocess` hook — so *every*
// caller is covered by construction rather than by remembering. The webview CSP is a
// second, independent layer, never the only one (crates/b2-desktop/CLAUDE.md, GH #77).
//
// Its own module so every surface that renders Markdown (the note pane, chat answers,
// live preview's widgets) imports the one seam without importing the panes; render.ts
// re-exports `renderMarkdown`, so the view layer's single import surface is unchanged.

import { marked, type Tokens, type TokenizerAndRendererExtension } from "marked";
// Relative imports carry their `.ts` here (the idiom highlight.ts and reconcile.ts already
// use) because render.test.ts runs this module off the source under node's type-stripping,
// which resolves by real filename — a bundler-style extensionless value import doesn't
// resolve there. tsc rewrites nothing (noEmit).
import { escapeHtml } from "./escape.ts";
import { sanitizeHtml } from "./sanitize.ts";
import { embedWidth, NO_EMBED_IMAGES, WIKILINK_ANCHORED, type EmbedImages } from "./embeds.ts";
import { baseName } from "./move.ts";

// A `[[target]]` / `[[target|label]]` wikilink becomes an in-app anchor carrying the
// raw target; main.ts delegates a click on `.wikilink` to open that note. This is the
// MVP's in-app navigation (spec §4) — the buffer stays byte-honest Markdown.
//
// `![[target]]` is the **embed** form of the same link — the core reads the `!` as the
// embed marker and records it on the edge (`link.rs`). Where the target is a picture the
// note has loaded, the embed *shows* it: the anchor stays (so the image is still the
// link, and clicking it still opens the resource card), and the `<img>` becomes its
// label. Everything else about an embed is unchanged — with no picture in hand (not an
// image, not indexed, too large, or simply not read yet) it reads as its link, which is
// what B2 showed before there was a viewer.
//
// The marker is *grammar* either way, so it is consumed rather than left beside the
// anchor as a stray `!`. The `|`-part changes meaning with it: on a plain wikilink it is
// the display **label**, on an embed a display **width** (`![[shot.png|500]]`, aspect
// ratio kept — `embedWidth` in embeds.ts). So an embed never labels itself "500"; it
// draws itself 500px wide, or falls back to naming its target.
const wikilink: TokenizerAndRendererExtension = {
  name: "wikilink",
  level: "inline",
  start(src: string) {
    const i = src.indexOf("[[");
    if (i < 0) return undefined;
    // Back up over the embed marker: this offset is where marked stops emitting plain
    // text, so anchoring it at the `[[` hands the reader the `!` before we ever tokenize.
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
    // The alt text is the filename: it is all B2 knows about the picture, and it is what
    // the embed would have read as had the bytes not arrived (the resource card's viewer
    // makes the same choice, for the same reason).
    const name = baseName(target);
    const width = typeof token.width === "number" ? ` width="${token.width}"` : "";
    return `${anchor}<img class="embed-image" src="${escapeHtml(src)}" alt="${escapeHtml(
      name,
    )}"${width}></a>`;
  },
};

// The pictures the *current* `renderMarkdown` call may draw.
//
// A module-local rather than a parameter because `marked`'s extensions are registered
// once, globally (the `marked.use` below), so the renderer above has no way to be handed
// per-call data. It is safe to hold it here for exactly one reason, and the reason is
// worth stating: `marked.parse(…, { async: false })` runs to completion synchronously,
// so between the assignment and the `finally` no other render can interleave. Every
// caller still passes its images as an argument — the seam is `renderMarkdown`, and this
// variable never outlives one of its calls.
let renderImages: EmbedImages = NO_EMBED_IMAGES;

// Wrap each table in a scroll box so a wide one scrolls *within* its column instead of
// stretching the pane. The table itself must stay a real `display: table` (the wrapper
// is what's `display: block; overflow-x: auto`) — a `display: block` table splits
// marked's whitespace-separated `<thead>`/`<tbody>` into two anonymous tables, so
// `border-collapse` can't join the header row onto the body (the gap bug). marked
// escapes cell content, so these are the only literal `<table>` tags in the output.
function wrapTables(html: string): string {
  return html
    .replace(/<table>/g, '<div class="md-table"><table>')
    .replace(/<\/table>/g, "</table></div>");
}

// The postprocess hook is where the trust boundary sits (E5, GH #77). Two properties come
// from putting it *here* rather than inside `renderMarkdown`: sanitizing is the last thing
// that happens to the HTML — nothing, not even B2's own table wrapper, is spliced in
// afterwards — and it holds for every `marked.parse` in the app, so a future second call
// site cannot render an unsanitized note by forgetting a step.
//
// `breaks: true` makes a single newline a `<br>`, the Obsidian reading of a note. The
// editor shows the file one line per line, and the default CommonMark fold — one newline
// is a space, only a blank line ends a paragraph — had the reading view disagreeing with
// it about where the author's lines end (linebreaks.test.ts pins both halves: a break for
// one newline, a paragraph for two, and neither inside a fence or a list).
marked.use({
  extensions: [wikilink],
  gfm: true,
  breaks: true,
  hooks: { postprocess: (html: string) => sanitizeHtml(wrapTables(html)) },
});

/**
 * Note body → the HTML the panes write into the DOM. Sanitized (see the hook above).
 *
 * `images` is what an `![[picture.png]]` embed draws with — the note's loaded pictures,
 * keyed by vault-relative path (`state.embedImages`). Omitted, every embed reads as its
 * link, which is both the honest state before the bytes arrive and what a caller with no
 * pictures to offer wants.
 */
export function renderMarkdown(md: string, images: EmbedImages = NO_EMBED_IMAGES): string {
  renderImages = images;
  try {
    return marked.parse(md, { async: false }) as string;
  } finally {
    renderImages = NO_EMBED_IMAGES;
  }
}
