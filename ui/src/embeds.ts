// The `![[…]]` **image embed**: the grammar it shares with the plain wikilink, which
// targets name a picture this webview can draw, how wide to draw it, and which of a
// note's embeds the app will actually hold in memory.
//
// Pure string/number logic over the vault's own inventory — no DOM, no IPC — so node
// runs its test straight off the source (`npm test`), and the three surfaces that need
// these answers can share one set of them: the reading view (render.ts), the editor's
// live preview (livepreview.ts) and the resource card's viewer (render.ts again).
//
// The bytes themselves are *not* here. An embed's picture arrives over the same
// `read_resource` command the resource card uses, is base64 on the wire, and lands in
// `state.embedImages` keyed by vault-relative path; everything below decides **what to
// ask for** and **how to draw what came back**.

import type { ResourceSummary } from "./types.ts";

/** The pictures a note has loaded: vault-relative path → the `data:` URL to draw it
 *  with. One map, read by the reading view and the live preview alike, so a note looks
 *  the same read or edited. */
export type EmbedImages = ReadonlyMap<string, string>;

/** The empty map, shared — the "nothing loaded yet" value every surface degrades to
 *  (an embed with no picture reads as its link, which is what B2 showed before there
 *  was a viewer at all). */
export const NO_EMBED_IMAGES: EmbedImages = new Map<string, string>();

// --- the grammar ---------------------------------------------------------------------
//
// `[[target]]`, `[[target|label]]`, and each with the embed marker in front. Spelled
// **once** and given two anchorings, because the two readers of it must not drift: the
// reading view tokenizes at the head of the remaining source (marked hands it a suffix),
// and the loader scans a whole body for the images to fetch. A grammar that agreed on
// paper and disagreed in code would show a picture that was never asked for, or ask for
// one it then can't draw.
//
// Group 1 is the marker (`!` or empty), 2 the target, 3 the optional `|`-part.
const WIKILINK = String.raw`(!?)\[\[([^\]|]+)(?:\|([^\]]+))?\]\]`;

/** The tokenizer's form — anchored at the head of the source it is handed. */
export const WIKILINK_ANCHORED = new RegExp(`^${WIKILINK}`);

/** The scanner's form. `g` is stateful (`lastIndex`), so this is only ever used with
 *  `matchAll`, which iterates a fresh walk rather than leaving the flag mid-string. */
const WIKILINK_GLOBAL = new RegExp(WIKILINK, "g");

/** Both ends pinned — for a reader that already knows where the construct starts and
 *  ends. The editor's live preview matches this against a whole `Wikilink` node
 *  (livepreview.ts, which exports it under the name its handlers use). */
export const WIKILINK_EXACT = new RegExp(`^${WIKILINK}$`);

/**
 * The display width an embed asks for — `![[shot.png|500]]` → 500, in CSS pixels.
 *
 * Only a bare integer is a width. The `|`-part means something different on either side
 * of the marker (a plain wikilink's is its **label**), and even on an embed it is a
 * free-text field: Obsidian's `|500x300` and anything a human typed by hand land here
 * too. Refusing everything that isn't a width is what keeps a hint the app doesn't
 * understand from being *drawn* — the image simply renders at its own size, which is
 * the honest fallback and the one that can't distort the picture.
 *
 * Height is deliberately not taken even when written: the ask is a width that
 * **maintains aspect ratio**, and that is one number plus `height: auto` in the CSS.
 */
export function embedWidth(hint: string | null | undefined): number | null {
  if (!hint) return null;
  const t = hint.trim();
  if (!/^[0-9]+$/.test(t)) return null;
  const n = Number(t);
  return n > 0 ? n : null;
}

// --- what is a picture ---------------------------------------------------------------

/** Extension → MIME type for the image classes this webview draws in place.
 *  Extension-only, the same rule the core classifies on (`resource.rs`) and the same
 *  table of extensions — no content sniffing, so a mislabeled file degrades to a broken
 *  `<img>` rather than being guessed at. **Change it with `ResourceClass::Image`**: a
 *  class the core calls an image and this table doesn't know gets asked for, and then
 *  has nowhere to put its bytes. */
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
 * A resource's base64 bytes as the `src` an `<img>` can use, or `null` when the
 * extension names no image this webview renders.
 *
 * A `data:` URL rather than a path, because the webview's origin is the *app bundle*,
 * not the vault: there is no URL by which it could fetch a vault file, and the CSP
 * admits exactly this one shape (`img-src 'self' data:` in `tauri.conf.json`) — so the
 * bytes travel through the same command seam as everything else the host lends the UI.
 */
export function imageDataUrl(path: string, base64: string): string | null {
  const mime = imageMime(path);
  return mime ? `data:${mime};base64,${base64}` : null;
}

// --- how much of it B2 will hold -----------------------------------------------------

/**
 * How large a *single* image B2 will pull across the IPC and hold on screen.
 *
 * The bytes cross as base64 (a third larger) and the `data:` URL lives in the webview's
 * heap for as long as the document is open, so this is a memory bound, not a taste one.
 * Past it the resource card keeps the *Open in system default* handoff it has always
 * had, and an inline embed keeps reading as its link — both cost nothing and both still
 * reach the file. Screenshots — what a vault actually accumulates — are a couple of
 * megabytes.
 */
export const IMAGE_VIEWER_MAX_BYTES = 25 * 1024 * 1024;

/**
 * How much image B2 will hold for **one note**, across every embed in it.
 *
 * The per-image bound above says nothing about a note holding forty of them, and a
 * photo log is a perfectly ordinary note. This is the bound on the sum, and it is what
 * makes the memory cost of opening a note something that can be stated rather than
 * discovered. Embeds are taken in document order until it is spent, so what the reader
 * is looking at when the note opens is what got drawn; the rest read as their links.
 */
export const NOTE_IMAGES_MAX_BYTES = 96 * 1024 * 1024;

/**
 * Every image the note **embeds**, vault-relative, de-duplicated, in document order.
 *
 * A scan of the raw Markdown rather than a parse: this runs on every buffer change
 * while editing, so it has to be cheap, and one regex walk over the body is. The cost
 * of not parsing is precision — an `![[shot.png]]` written *inside a fenced code block*
 * is prose to the renderer but a hit here, so its bytes get read and then never drawn.
 * That is a wasted read of a file the vault already holds, which is the cheaper side of
 * the trade; the expensive side would be re-lexing the note per keystroke.
 *
 * Only the marked-up form counts. A plain `[[shot.png]]` is a *link* to the picture and
 * B2 keeps it one — the marker is the whole difference between naming a file and
 * showing it.
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
 * Which of those targets the app will actually fetch, in document order.
 *
 * Everything is decided off the **inventory** B2 already has in hand (`list_resources`,
 * which carries each file's class and size), so planning costs no IPC and the bounds are
 * applied before a single byte is read rather than after. Three ways a target drops out,
 * and all three leave the embed reading as its link:
 *
 * - the vault has no such file — a typo, or a picture added since the last index pass
 *   (the host validates against the same inventory, so asking anyway would only fail);
 * - the core doesn't call it an image, whatever its extension claims;
 * - it is larger than one image may be, or the note has already spent its budget.
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
