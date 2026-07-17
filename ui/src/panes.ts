// Draggable column widths for the three-pane layout.
//
// The two side columns are resizable; the center is not — it takes whatever is left
// (`minmax(0, 1fr)`), because the note is the reading surface and everything else is
// chrome. Widths ride on CSS custom properties (`--tree-w` / `--side-w`) that the grid
// reads, so a drag is one style write and never a re-render — which also means the
// gutters must live in the shell, not inside a pane whose innerHTML `render()` swaps.
//
// Sizes are a viewing choice, never vault state, so they persist in localStorage and
// never touch the host — the same shape as the appearance preference in `main.ts`. They
// stay module-local rather than in `state.ts` for the same reason: nothing renders from
// them, so putting them in the model would only invite a needless re-render.

/** Which of the two resizable columns. The center is deliberately not one of them. */
export type Pane = "tree" | "side";

/** Which panes the stylesheet is actually rendering — the breakpoints drop them. */
export interface Shown {
  tree: boolean;
  side: boolean;
}

export interface PaneWidths {
  tree: number;
  side: number;
}

/** Per-pane travel. Mins keep a pane useful (a tree that can't show a filename is a
 *  handle, not a pane); maxes stop one column from eating the window. */
export const BOUNDS: Record<Pane, { min: number; max: number; default: number }> = {
  tree: { min: 160, max: 420, default: 240 },
  side: { min: 240, max: 560, default: 380 },
};

/** The center's floor. Below this the reading measure stops being a measure, so this is
 *  the constraint the side columns yield to — the center wins every contest. */
export const CENTER_MIN = 360;

/** Grab-strip width; must match `--gutter-w` in style.css (it's a grid track). */
export const GUTTER = 6;

const KEY = "b2:panes";

/** The widest `pane` may be right now: its own max, or whatever the center can spare
 *  with the *other* pane held fixed. Holding the other fixed is what makes a drag feel
 *  like a wall rather than a lever that secretly moves the far column. */
export function ceilingFor(pane: Pane, otherW: number, avail: number, show: Shown): number {
  let room = avail - CENTER_MIN;
  if (show.tree) room -= GUTTER;
  if (show.side) room -= GUTTER;
  if (pane === "tree" ? show.side : show.tree) room -= otherW;
  return Math.min(BOUNDS[pane].max, room);
}

/** Settle both widths against the window. The side pane yields first, then the tree;
 *  each pane's own min outranks the center's (there is nothing useful below it, and the
 *  stylesheet's breakpoints drop a pane entirely long before it gets that tight).
 *  A pane the breakpoints have hidden reserves no room and keeps its stored width, so it
 *  comes back the size the user left it. */
export function fit(want: PaneWidths, avail: number, show: Shown): PaneWidths {
  // Bound each pane on its own *first*: a neighbor is only ever weighed at a width it
  // could actually occupy, so a wild stored value can't crush the other pane on its way
  // to being capped itself.
  const bound = (pane: Pane, w: number): number =>
    Math.min(Math.max(w, BOUNDS[pane].min), BOUNDS[pane].max);
  const settle = (pane: Pane, w: number, otherW: number): number =>
    show[pane] // off-screen: nothing to compete over
      ? Math.max(BOUNDS[pane].min, Math.min(w, ceilingFor(pane, otherW, avail, show)))
      : w;

  const side = settle("side", bound("side", want.side), bound("tree", want.tree));
  const tree = settle("tree", bound("tree", want.tree), side);
  return { tree, side };
}

// --- persistence --------------------------------------------------------------------

function defaults(): PaneWidths {
  return { tree: BOUNDS.tree.default, side: BOUNDS.side.default };
}

function load(): PaneWidths {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return defaults();
    const saved: unknown = JSON.parse(raw);
    if (!saved || typeof saved !== "object") return defaults();
    const { tree, side } = saved as Partial<PaneWidths>;
    return {
      tree: typeof tree === "number" && Number.isFinite(tree) ? tree : BOUNDS.tree.default,
      side: typeof side === "number" && Number.isFinite(side) ? side : BOUNDS.side.default,
    };
  } catch {
    // Unreadable or unavailable (private mode, hand-edited value): fall back to defaults.
    return defaults();
  }
}

function save(w: PaneWidths): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(w));
  } catch {
    // Non-fatal: the sizes still hold for this session if they can't persist.
  }
}

// --- the live layout ----------------------------------------------------------------

/**
 * What the user *asked for* — not necessarily what's on screen. The two diverge whenever
 * the window is too narrow to honor the request, and keeping them apart is what lets a
 * pane spring back to its chosen width once the room returns: `fit()` is re-derived from
 * this on every relayout, so a squeeze is never written back over the intent. (Fold the
 * two together and the first narrow window silently becomes the new preference.)
 */
let desired = defaults();

/** Is this pane on screen? The breakpoints drop panes, and a dropped pane reserves no room. */
function shown(el: HTMLElement | null): boolean {
  return !!el && getComputedStyle(el).display !== "none";
}

/**
 * Wire the gutters and start tracking the layout. Call once, after the shell exists.
 * `root` is the `.layout` grid; it owns the width vars and is what we measure against.
 */
export function initPanes(root: HTMLElement): void {
  const paneEl = (pane: Pane): HTMLElement | null =>
    document.getElementById(pane === "tree" ? "tree-pane" : "side-pane");

  const visible = (): Shown => ({ tree: shown(paneEl("tree")), side: shown(paneEl("side")) });

  /** What's actually on screen: the request, settled against the window as it is now. */
  const effective = (): PaneWidths => fit(desired, root.clientWidth, visible());

  /** Derive from `desired` and paint. Never writes back — see `desired`'s note. */
  const apply = (): void => {
    const w = effective();
    root.style.setProperty("--tree-w", `${w.tree}px`);
    root.style.setProperty("--side-w", `${w.side}px`);
    for (const pane of ["tree", "side"] as const) {
      // Report the width the user can actually see and act on, not the one we're holding.
      document.getElementById(`gutter-${pane}`)?.setAttribute("aria-valuenow", String(w[pane]));
    }
  };

  desired = load();
  apply();

  // The window (and the breakpoints) can invalidate a width at any time; re-deriving is
  // the whole response. Nothing is persisted here — a temporarily-narrow window must not
  // overwrite what the user chose.
  window.addEventListener("resize", apply);

  for (const pane of ["tree", "side"] as const) {
    const gutter = document.getElementById(`gutter-${pane}`);
    if (!gutter) continue;

    // Drag. Pointer capture keeps the stream coming when the cursor outruns the 6px
    // strip; `is-resizing` on <body> stops the panes text-selecting under the drag.
    gutter.addEventListener("pointerdown", (e: PointerEvent) => {
      if (e.button !== 0) return;
      e.preventDefault();
      const startX = e.clientX;
      // Start from what's on screen, not from `desired`: grabbing a pane the window has
      // squeezed must move it from where the user sees it, not jump to a held-back width.
      const start = effective();
      const startW = start[pane];
      const cap = ceilingFor(pane, pane === "tree" ? start.side : start.tree, root.clientWidth, visible());
      gutter.setPointerCapture(e.pointerId);
      gutter.classList.add("is-dragging");
      document.body.classList.add("is-resizing");

      const onMove = (ev: PointerEvent): void => {
        // The left gutter grows its pane rightward; the right one is mirrored.
        const dx = pane === "tree" ? ev.clientX - startX : startX - ev.clientX;
        desired[pane] = Math.max(BOUNDS[pane].min, Math.min(startW + dx, cap));
        apply();
      };
      const onUp = (): void => {
        gutter.removeEventListener("pointermove", onMove);
        gutter.classList.remove("is-dragging");
        document.body.classList.remove("is-resizing");
        save(desired);
      };
      gutter.addEventListener("pointermove", onMove);
      gutter.addEventListener("pointerup", onUp, { once: true });
      gutter.addEventListener("pointercancel", onUp, { once: true });
    });

    // Double-click restores the default — the cheap way back from a bad drag.
    gutter.addEventListener("dblclick", () => {
      desired[pane] = BOUNDS[pane].default;
      apply();
      save(desired);
    });

    // Keyboard: a separator that only responds to a mouse is a separator half the
    // people here can't move.
    gutter.addEventListener("keydown", (e: KeyboardEvent) => {
      const step = e.shiftKey ? 48 : 16;
      let delta = 0;
      if (e.key === "ArrowLeft") delta = pane === "tree" ? -step : step;
      else if (e.key === "ArrowRight") delta = pane === "tree" ? step : -step;
      else if (e.key === "Home") delta = -Infinity;
      else if (e.key === "End") delta = Infinity;
      else return;
      e.preventDefault();
      const now = effective(); // step from what's on screen, as the drag does
      const cap = ceilingFor(pane, pane === "tree" ? now.side : now.tree, root.clientWidth, visible());
      desired[pane] = Math.max(BOUNDS[pane].min, Math.min(now[pane] + delta, cap));
      apply();
      save(desired);
    });
  }

  // --- flowing the center -----------------------------------------------------------
  //
  // The note pane's side padding is generous by design, but at a narrow center it is
  // just margin eating the measure. Track the pane's *own* width (not the window's — a
  // drag can squeeze the center on a wide screen) and taper the padding as it closes in.
  // This can't be `clamp(20px, 6%, 48px)` in CSS: the full-bleed bars cancel this padding
  // with negative margins, and a margin % resolves against the pane's *content* box while
  // a padding % resolves against its *grid area* — the two would disagree and the bars'
  // dividers would stop short of the edge. A px value resolves identically for both.
  const note = document.getElementById("note-pane");
  if (note && "ResizeObserver" in window) {
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        // The *border* box, not `contentRect`: content width is measured inside the very
        // padding we're about to set, so feeding it back here would chase its own tail
        // (each pass shrinks the pad, which widens the content, which grows the pad...).
        // The border box is fixed by the grid track, so it's a stable input.
        const w = entry.borderBoxSize?.[0]?.inlineSize ?? (entry.target as HTMLElement).clientWidth;
        // Full 48px from ~800px up (a default layout on any normal window keeps today's
        // reading surface untouched); tapers to 20px as the center approaches its floor.
        const pad = Math.round(Math.min(48, Math.max(20, w * 0.06)));
        note.style.setProperty("--note-pad-x", `${pad}px`);
      }
    });
    ro.observe(note);
  }
}
