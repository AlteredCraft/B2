// The icon registry. Shapes are Bootstrap Icons (MIT) path data vendored into
// `icons.gen.ts` by `scripts/gen-icons.ts` (the CSP forbids a CDN, and node tests can't
// resolve Vite imports). This file holds meanings: callers ask for `resourceIcon(cls)` or
// `foldChevron(open)`, so what a PDF looks like has one answer.

import { escapeHtml } from "./escape.ts";
import { ICON_BODIES } from "./icons.gen.ts";

/** Every vendored icon. Add one to the generator's manifest, then `make icons`. */
export type IconName = keyof typeof ICON_BODIES;

export interface IconOptions {
  /** Rendered box in px (the art is a 16×16 grid). Defaults to 14. */
  size?: number;
  /** Extra class(es) on the `<svg>`, alongside the base `icon`. */
  class?: string;
}

/**
 * One icon as inline SVG markup. Always `aria-hidden`: a control names itself with
 * `aria-label` or visible text, so a nameless control needs a label, not a title here
 * (crates/b2-desktop/CLAUDE.md). `currentColor` lets the container's color tint it.
 * The class is escaped at this seam so no caller has to remember to (E5).
 */
export function icon(name: IconName, opts: IconOptions = {}): string {
  const size = opts.size ?? 14;
  const cls = opts.class ? `icon ${escapeHtml(opts.class)}` : "icon";
  return (
    `<svg class="${cls}" width="${size}" height="${size}" viewBox="0 0 16 16"` +
    ` fill="currentColor" aria-hidden="true">${ICON_BODIES[name]}</svg>`
  );
}

/**
 * The icon inside an existing SVG scene (the graph), centred on (cx, cy) by a nested
 * `<svg>`. No `fill`: the stylesheet colors it (`.gglyph`), and a presentation attribute
 * would have to be out-specified.
 */
export function sceneIcon(name: IconName, cx: number, cy: number, size: number, cls: string): string {
  const half = size / 2;
  return (
    `<svg class="${escapeHtml(cls)}" x="${cx - half}" y="${cy - half}" width="${size}" height="${size}"` +
    ` viewBox="0 0 16 16" aria-hidden="true">${ICON_BODIES[name]}</svg>`
  );
}

// --- what a thing looks like ---------------------------------------------------------

/** A note (a plain-text resource gets the lettered page below). */
export const NOTE_ICON: IconName = "file-earmark-text";

/** A resource's icon, by the class the index assigns it (`ResourceSummary.class`). */
export const RESOURCE_ICONS: Record<string, IconName> = {
  image: "file-earmark-image",
  media: "file-earmark-play",
  pdf: "file-earmark-pdf",
  html: "file-earmark-code",
  text: "file-earmark-font",
  binary: "file-earmark-binary",
};

/** The icon for a resource class, falling back to `binary` for a class from the host this
 *  UI doesn't know yet. */
export function resourceIcon(cls: string | null | undefined): IconName {
  return RESOURCE_ICONS[cls ?? ""] ?? RESOURCE_ICONS.binary;
}

/** A folder, open or closed. Shown with the fold chevron, not instead of it. */
export function folderIcon(open: boolean): IconName {
  return open ? "folder2-open" : "folder";
}

/** The fold chevron, shared by every foldable row in the app. */
export function foldChevron(open: boolean): IconName {
  return open ? "chevron-down" : "chevron-right";
}

/** A connection's direction on its discovery card: out of the open note, or into it. */
export function directionIcon(direction: string): IconName {
  return direction === "outbound" ? "arrow-right" : "arrow-left";
}
