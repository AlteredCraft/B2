// The `![[…]]` image embed: the grammar it shares with the plain wikilink, which targets
// are drawable pictures, how wide to draw them, and which a note will hold in memory.
// Pure logic shared by render.ts and livepreview.ts. The bytes arrive via `read_resource`
// into `state.embedImages`; this module decides what to ask for and how to draw it.

import type { ResourceSummary } from "./types.ts";

/** The pictures a note has loaded: vault-relative path → `data:` URL. Read by both the
 *  reading view and the live preview, so a note looks the same read or edited. */
export type EmbedImages = ReadonlyMap<string, string>;

/** Nothing loaded yet: an embed with no picture reads as its link. */
export const NO_EMBED_IMAGES: EmbedImages = new Map<string, string>();

// --- the grammar ---------------------------------------------------------------------
//
// `[[target]]`, `[[target|label]]`, each optionally with the `!` marker. Spelled once so the
// tokenizer and the loader's scan can't disagree about which images exist.
//
// Group 1 is the marker (`!` or empty), 2 the target, 3 the optional `|`-part.
const WIKILINK = String.raw`(!?)\[\[([^\]|]+)(?:\|([^\]]+))?\]\]`;

/** The tokenizer's form — anchored at the head of the source it is handed. */
export const WIKILINK_ANCHORED = new RegExp(`^${WIKILINK}`);

/** The scanner's form. `g` is stateful (`lastIndex`), so use only with `matchAll`. */
const WIKILINK_GLOBAL = new RegExp(WIKILINK, "g");

/** Both ends pinned, for matching a whole `Wikilink` node (livepreview.ts). */
export const WIKILINK_EXACT = new RegExp(`^${WIKILINK}$`);

/**
 * The display width an embed asks for — `![[shot.png|500]]` → 500 CSS pixels. Only a bare
 * integer counts; anything else (Obsidian's `|500x300`, free text) renders at natural
 * size. Height is never taken, so aspect ratio is kept.
 */
export function embedWidth(hint: string | null | undefined): number | null {
  if (!hint) return null;
  const t = hint.trim();
  if (!/^[0-9]+$/.test(t)) return null;
  const n = Number(t);
  return n > 0 ? n : null;
}

// --- what is a picture ---------------------------------------------------------------

/** Extension → MIME type for the images this webview draws. Extension-only, like the core
 *  (`resource.rs`). Change it with `ResourceClass::Image`, or an image the core names has
 *  nowhere to put its bytes. */
const IMAGE_MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
  avif: "image/avif",
};

/** The MIME type this path's extension names, or null when it names no image B2 draws. */
export function imageMime(path: string): string | null {
  const ext = path.includes(".") ? (path.split(".").pop() ?? "").toLowerCase() : "";
  return IMAGE_MIME[ext] ?? null;
}

/**
 * A resource's base64 bytes as an `<img>` `src`, or null for a non-image. A `data:` URL
 * because the webview can't fetch vault files, and the CSP admits exactly this shape
 * (`img-src 'self' data:` in `tauri.conf.json`).
 */
export function imageDataUrl(path: string, base64: string): string | null {
  const mime = imageMime(path);
  return mime ? `data:${mime};base64,${base64}` : null;
}

// --- how much of it B2 will hold -----------------------------------------------------

/**
 * The largest single image B2 will pull across the IPC and hold: a memory bound, since the
 * `data:` URL lives in the webview's heap. Past it an embed reads as its link.
 */
export const IMAGE_VIEWER_MAX_BYTES = 25 * 1024 * 1024;

/**
 * The image budget for one note, across all its embeds. Spent in document order, so what
 * is visible when the note opens gets drawn; the rest read as their links.
 */
export const NOTE_IMAGES_MAX_BYTES = 96 * 1024 * 1024;

/**
 * Every image the note embeds (`!` form only), de-duplicated, in document order. A regex
 * scan, not a parse, because it runs per keystroke; an embed inside a code fence costs a
 * wasted read, never a wrong draw.
 */
export function imageEmbedTargets(md: string): string[] {
  const seen = new Set<string>();
  for (const m of md.matchAll(WIKILINK_GLOBAL)) {
    if (m[1] !== "!") continue;
    const target = m[2].trim();
    if (imageMime(target)) seen.add(target);
  }
  return [...seen];
}

/**
 * Which of those targets the app will fetch, in document order, decided off the inventory
 * (`list_resources`) so the bounds apply before any byte is read. A target drops out (and
 * reads as its link) if the inventory lacks it, the core doesn't class it an image, or it
 * exceeds either bound.
 */
export function inlineImagePlan(
  targets: readonly string[],
  resources: readonly ResourceSummary[],
): string[] {
  const inventory = new Map(resources.map((r) => [r.path, r]));
  const plan: string[] = [];
  let budget = NOTE_IMAGES_MAX_BYTES;
  for (const target of targets) {
    const r = inventory.get(target);
    if (!r || r.class !== "image") continue;
    if (r.size > IMAGE_VIEWER_MAX_BYTES || r.size > budget) continue;
    budget -= r.size;
    plan.push(target);
  }
  return plan;
}
