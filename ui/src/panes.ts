// Draggable column widths for the three-pane layout. The two side columns resize; the
// center takes the rest. Widths are CSS custom properties (`--tree-w` / `--side-w`), so a
// drag is one style write, never a re-render, and the gutters must live in the shell, not
// in a pane `render()` swaps. A viewing choice, so localStorage, not vault or `state.ts`.

/** Which of the two resizable columns. */
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

/** Per-pane travel: mins keep a pane useful, maxes stop one eating the window. */
export const BOUNDS: Record<Pane, { min: number; max: number; default: number }> = {
  tree: { min: 160, max: 420, default: 240 },
  side: { min: 240, max: 560, default: 380 },
};

/** The center's floor; the side columns yield to it. */
export const CENTER_MIN = 360;

/** Grab-strip width; must match `--gutter-w` in style.css (it's a grid track). */
export const GUTTER = 6;

const KEY = "b2:panes";

/** The widest `pane` may be now: its max, or what the center can spare with the other pane
 *  held fixed (so a drag never moves the far column). */
export function ceilingFor(pane: Pane, otherW: number, avail: number, show: Shown): number {
  let room = avail - CENTER_MIN;
  if (show.tree) room -= GUTTER;
  if (show.side) room -= GUTTER;
  if (pane === "tree" ? show.side : show.tree) room -= otherW;
  return Math.min(BOUNDS[pane].max, room);
}

/** Settle both widths against the window. The side pane yields first, then the tree; a
 *  pane's own min outranks the center's. A hidden pane reserves no room and keeps its
 *  stored width. */
export function fit(want: PaneWidths, avail: number, show: Shown): PaneWidths {
  // Bound each pane first, so a wild stored value can't crush its neighbor.
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
 * What the user asked for, not necessarily what's on screen. `fit()` re-derives from this
 * on every relayout and never writes back, so a pane springs back once room returns.
 */
let desired = defaults();

/** Is this pane on screen? The breakpoints drop panes, and a dropped pane reserves no room. */
function shown(el: HTMLElement | null): boolean {
  return !!el && getComputedStyle(el).display !== "none";
}

/** The pane element the breakpoints act on, by name. */
function paneEl(pane: Pane): HTMLElement | null {
  return document.getElementById(pane === "tree" ? "tree-pane" : "side-pane");
}

/**
 * Which side columns the stylesheet is drawing: read, never computed, since the
 * breakpoints live in style.css. `main.ts` also compares it across a zoom step, which can
 * cross a breakpoint too.
 */
export function visiblePanes(): Shown {
  return { tree: shown(paneEl("tree")), side: shown(paneEl("side")) };
}

/**
 * Wire the gutters and start tracking the layout. Call once, after the shell exists.
 * `root` is the `.layout` grid; it owns the width vars and is what we measure against.
 */
export function initPanes(root: HTMLElement): void {
  const visible = visiblePanes;

  /** What's actually on screen: the request, settled against the window as it is now. */
  const effective = (): PaneWidths => fit(desired, root.clientWidth, visible());

  /** Derive from `desired` and paint. Never writes back — see `desired`'s note. */
  const apply = (): void => {
    const w = effective();
    root.style.setProperty("--tree-w", `${w.tree}px`);
    root.style.setProperty("--side-w", `${w.side}px`);
    for (const pane of ["tree", "side"] as const) {
      document.getElementById(`gutter-${pane}`)?.setAttribute("aria-valuenow", String(w[pane]));
    }
  };

  desired = load();
  apply();

  // Nothing is persisted here: a narrow window must not overwrite what the user chose.
  window.addEventListener("resize", apply);

  for (const pane of ["tree", "side"] as const) {
    const gutter = document.getElementById(`gutter-${pane}`);
    if (!gutter) continue;

    // Pointer capture keeps events coming when the cursor outruns the strip; `is-resizing`
    // stops text selection under the drag.
    gutter.addEventListener("pointerdown", (e: PointerEvent) => {
      if (e.button !== 0) return;
      e.preventDefault();
      const startX = e.clientX;
      // Start from what's on screen, not `desired`, so a squeezed pane doesn't jump.
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

    gutter.addEventListener("dblclick", () => {
      desired[pane] = BOUNDS[pane].default;
      apply();
      save(desired);
    });

    // Keyboard access (K1); Enter is the double-click reset.
    gutter.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        desired[pane] = BOUNDS[pane].default;
        apply();
        save(desired);
        return;
      }
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
  // Taper the note pane's side padding with its own width. Not a CSS `clamp(…6%…)`: the
  // full-bleed bars cancel this padding with negative margins, and margin % and padding %
  // resolve against different boxes, so the bars would stop short. A px value agrees.
  const note = document.getElementById("note-pane");
  if (note && "ResizeObserver" in window) {
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        // The border box, not `contentRect`, which depends on the padding set here and
        // would feed back on itself.
        const w = entry.borderBoxSize?.[0]?.inlineSize ?? (entry.target as HTMLElement).clientWidth;
        // 48px from ~800px up, tapering to 20px.
        const pad = Math.round(Math.min(48, Math.max(20, w * 0.06)));
        note.style.setProperty("--note-pad-x", `${pad}px`);
      }
    });
    ro.observe(note);
  }
}
