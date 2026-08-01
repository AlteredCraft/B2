// The icon registry — one family, one wrapper, one place a glyph means something.
//
// B2's chrome used to carry two kinds of icon and neither could be reasoned about. The
// fold affordance and the resource markers were **Unicode text** (`▶ ▼`, and `▣ ▶ ▤ ◇ ≡ ◆`
// for a resource's class), which meant a row's marker told you only "this is not a note" —
// the shape carried no information about *what* the file was, and rendered at whatever
// weight the system font felt like. Everything else was **hand-rolled inline SVG**, pasted
// at each call site with its own `stroke-width` and its own idea of 15px vs 16px, so a
// second copy of a chevron could — and did — drift from the first.
//
// Both are now the same thing: a name in this module, resolving to path data vendored from
// **Bootstrap Icons** (twbs/icons, MIT). `scripts/gen-icons.ts` is the vendoring step and
// explains why the data is generated into `icons.gen.ts` rather than imported from the
// package at runtime (the short version: the webview CSP forbids a CDN, and `npm test` runs
// off the source through node, where a Vite `?raw` import would not resolve).
//
// **The split.** `icons.gen.ts` holds *shapes*; this file holds *meanings*. A caller asks
// for `resourceIcon(r.class)` or `foldChevron(open)`, not for `"file-earmark-pdf"` — so the
// question "what does a PDF look like in B2" has exactly one answer, and changing it is a
// one-line edit here rather than a grep across three panes and a graph.

import { ICON_BODIES } from "./icons.gen.ts";

/** Every vendored icon. A typo is a type error; adding one is a name in the generator's
 *  manifest plus `just icons`. */
export type IconName = keyof typeof ICON_BODIES;

export interface IconOptions {
  /** Rendered box, in px (the art is a 16×16 grid regardless). Defaults to 14 — the tree
   *  row's size, and the one most chrome uses. */
  size?: number;
  /** Extra class(es) on the `<svg>`, alongside the base `icon`. */
  class?: string;
}

/**
 * One icon as inline SVG markup, for interpolation into the panes' HTML.
 *
 * `aria-hidden` on every one of them, without an opt-out: an icon in this UI is always
 * *beside* its accessible name, never instead of it — a control names itself with
 * `aria-label` (or its visible text), so an icon that also announced would double up. If a
 * new control has no name but its icon, the fix is the label, not a title here (K1's
 * "reachable" obligation — crates/b2-desktop/CLAUDE.md).
 *
 * `fill="currentColor"` is what makes one icon work on both themes and in every state: the
 * glyph is whatever color its container's text is, so hover, `is-active` and the dangling
 * graph node all tint it by setting `color`/`fill` on the parent and nothing else.
 */
export function icon(name: IconName, opts: IconOptions = {}): string {
  const size = opts.size ?? 14;
  const cls = opts.class ? `icon ${opts.class}` : "icon";
  return (
    `<svg class="${cls}" width="${size}" height="${size}" viewBox="0 0 16 16"` +
    ` fill="currentColor" aria-hidden="true">${ICON_BODIES[name]}</svg>`
  );
}

/**
 * The same icon placed inside an *existing* SVG scene — the connection graph, whose nodes
 * are drawn in scene coordinates rather than laid out by CSS. A nested `<svg>` re-origins
 * the 16×16 art at (x, y), which is why this can center on a node with no transform math.
 *
 * No `fill` here, unlike `icon()`: the scene's glyph color is CSS's (`.gglyph`, and
 * `.gnode.is-dangling .gglyph`), and a presentation attribute the stylesheet has to
 * out-specify is a trap for the next person who adds a node state.
 */
export function sceneIcon(name: IconName, cx: number, cy: number, size: number, cls: string): string {
  const half = size / 2;
  return (
    `<svg class="${cls}" x="${cx - half}" y="${cy - half}" width="${size}" height="${size}"` +
    ` viewBox="0 0 16 16" aria-hidden="true">${ICON_BODIES[name]}</svg>`
  );
}

// --- what a thing looks like ---------------------------------------------------------

/** A note. The vault's *authored* subset is Markdown (data-model.md), so the written-on
 *  page is the note's, and a plain-text resource gets the lettered page below. */
export const NOTE_ICON: IconName = "file-earmark-text";

/**
 * A resource's icon, by the class the index assigns it (`ResourceSummary.class`).
 *
 * The map is the whole point of moving off `▣ ▶ ▤ ◇ ≡ ◆`: those six shapes were mutually
 * distinct but individually meaningless, so a tree of them told you where the resources
 * were and nothing more. These say *what*, at a glance and at 14px.
 */
export const RESOURCE_ICONS: Record<string, IconName> = {
  image: "file-earmark-image",
  media: "file-earmark-play",
  pdf: "file-earmark-pdf",
  html: "file-earmark-code",
  text: "file-earmark-font",
  binary: "file-earmark-binary",
};

/** The icon for a resource class, falling back to `binary` — the class comes from the
 *  host, so an unknown one is a vault B2 hasn't learned about yet, not a bug to throw on. */
export function resourceIcon(cls: string | null | undefined): IconName {
  return RESOURCE_ICONS[cls ?? ""] ?? RESOURCE_ICONS.binary;
}

/** A folder, open or closed. Paired with — not replaced by — the fold chevron: the chevron
 *  is the *affordance* (this row folds), the folder is the *thing*, and a row that shows
 *  only one of them makes you learn which of its two jobs it is doing today. */
export function folderIcon(open: boolean): IconName {
  return open ? "folder2-open" : "folder";
}

/** The fold chevron, shared by every foldable row in the app — the file tree, the
 *  frontmatter drawer, and the discovery pane's sections and cards. One function so they
 *  cannot drift apart the way three pasted `▶`/`▼` pairs quietly could. */
export function foldChevron(open: boolean): IconName {
  return open ? "chevron-down" : "chevron-right";
}

/** A connection's direction on its discovery card: out of the open note, or into it. */
export function directionIcon(direction: string): IconName {
  return direction === "outbound" ? "arrow-right" : "arrow-left";
}
