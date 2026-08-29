// Global text size — ⌘= / ⌘- / ⌘0.
//
// **What actually scales, and why it isn't CSS.** The stylesheet sizes everything in
// px, deliberately (a 13px chip and a 15px body are drawn to a pixel grid, not derived
// from a root em), so there is no `html { font-size }` to turn. Growing text alone would
// also be the wrong answer: at 140% the note reads bigger while the icons, the row
// heights and the pane gutters stay put, and the layout comes apart. So this is real
// **page zoom** — WebKit's `pageZoom`, reached through the host's `set_zoom` command —
// which scales the whole rendering the way ⌘+ does in Safari. Everything grows together,
// `100vh` still means the window, and no rule in style.css has to know it happened.
//
// **Where the rules live.** The host is a pass-through: it hands a number to the webview
// and nothing else (crates/b2-desktop/CLAUDE.md — hold no logic). The ladder, the walls
// and the reading-back of a stored value are here, pure and tested, which is the same
// split panes.ts makes for column widths.
//
// **Where the preference lives.** localStorage, like the theme and the pane widths and
// the rebound chords: a viewing choice, never vault state. It never touches the host's
// config, the index, or a byte of Markdown — drop the vault on another machine and the
// notes are identical; the size you read them at is that machine's business.

const KEY = "b2:zoom";

/** 100% — where B2 starts, and where ⌘0 goes back to. A rung by construction (the suite
 *  pins it), so a reset always lands the ladder somewhere ⌘=/⌘- can walk from. */
export const DEFAULT_ZOOM = 1;

/**
 * The rungs, ascending. Browser-ish spacing — fine steps around 100% where a small
 * adjustment is the whole point, coarser out at the ends where the next useful size is
 * further away.
 *
 * A fixed ladder rather than "multiply by 1.1": a multiplier accumulates float dust
 * across a dozen presses, and it gives ⌘0 nothing exact to return to. It also makes the
 * walls honest — the ends of this list *are* the limits, so there is no separate min/max
 * to keep in step with it.
 */
export const STEPS: readonly number[] = [0.75, 0.85, 0.9, 1, 1.1, 1.25, 1.4, 1.6, 1.8, 2];

/** Which way a step goes. */
export type Direction = 1 | -1;

/**
 * One rung `dir` from `current`.
 *
 * Off-ladder input is the interesting case, and the rule is *never overshoot*: a value
 * between two rungs steps to the one on that side, so a size that drifted (a hand-edited
 * store, a ladder that changed under an old preference) is back on the ladder after a
 * single keypress instead of jumping two sizes. Past either end it settles onto that end
 * — pressing ⌘- at 300% should bring you to the top rung, not to 200%'s neighbour.
 *
 * At the ends it is a wall, not a wrap: ⌘= held down should stop, never snap back to
 * tiny.
 */
export function stepZoom(current: number, dir: Direction): number {
  if (dir === 1) {
    const next = STEPS.find((s) => s > current);
    return next ?? STEPS[STEPS.length - 1];
  }
  // Scan from the top for the first rung strictly below — the mirror of `find` above.
  for (let i = STEPS.length - 1; i >= 0; i--) {
    if (STEPS[i] < current) return STEPS[i];
  }
  return STEPS[0];
}

/**
 * An unknown value, read into a size B2 will actually apply.
 *
 * Defensive in the same shape as `adoptOverrides` and panes.ts's `load`, and for the same
 * reason: localStorage is a file a human can edit, and this one is handed straight to the
 * renderer. A non-number, a NaN, an infinity or a zero-or-negative scale is not a small
 * window — it is an unrenderable one — so those are refused outright and become the
 * default. A finite positive number that simply isn't a rung is snapped to the nearest,
 * which is what lets the ladder be re-tuned later without stranding anyone's preference.
 */
export function adoptZoom(raw: unknown): number {
  if (typeof raw !== "number" || !Number.isFinite(raw) || raw <= 0) return DEFAULT_ZOOM;
  let best = STEPS[0];
  for (const s of STEPS) {
    if (Math.abs(s - raw) < Math.abs(best - raw)) best = s;
  }
  return best;
}

// --- what a step costs -------------------------------------------------------------------

/**
 * Which side columns the stylesheet is currently drawing — `panes.ts`'s `Shown`, and
 * deliberately the same shape, because it is the same question asked by the same means:
 * ask the browser, never compute it.
 *
 * That is the whole design of the notice below. The breakpoints live in `style.css`
 * (discovery goes below 1040px of layout width, the tree below 720px) and page zoom
 * divides the window by the scale, so "how far can I zoom before a column goes" is
 * `window width ÷ 1040` — a number that moves with every window resize and that this
 * module would have to keep in step with a stylesheet it can't see. Measuring the result
 * instead means there is no second copy of a breakpoint anywhere, and no arithmetic to be
 * wrong about.
 */
export interface Columns {
  tree: boolean;
  side: boolean;
}

/**
 * What to say about a zoom step that hid a column, or `null` when it hid none.
 *
 * A note rather than a wall, which is the choice worth writing down. B2 could refuse the
 * step instead — but the ceiling is the window's width divided by the breakpoint, so it
 * moves whenever the window is resized, and a ⌘= that worked yesterday and does nothing
 * today is a worse surprise than a column that goes away with a reason. The columns come
 * back on ⌘-, and someone on a small screen who would rather have big text than a side
 * column is allowed to have it.
 *
 * Only *losses* are announced. A column reappearing announces itself.
 */
export function hiddenNotice(before: Columns, after: Columns): string | null {
  const lost = (k: keyof Columns): boolean => before[k] && !after[k];
  const tree = lost("tree");
  const side = lost("side");
  if (tree && side) return "The file tree and discovery are hidden at this size.";
  if (tree) return "The file tree is hidden at this size.";
  if (side) return "Discovery is hidden at this size.";
  return null;
}

// --- persistence -----------------------------------------------------------------------

/** Read the saved size. Unreadable or unavailable storage is 100% — never a thrown boot. */
export function loadZoom(): number {
  try {
    const text = localStorage.getItem(KEY);
    if (!text) return DEFAULT_ZOOM;
    return adoptZoom(JSON.parse(text));
  } catch {
    // Unavailable (private mode) or not JSON at all: the size B2 ships with.
    return DEFAULT_ZOOM;
  }
}

/** Persist the size. The default removes the entry rather than storing `1`, so a user
 *  who presses ⌘0 leaves nothing behind to go stale against a future ladder. */
export function saveZoom(zoom: number): void {
  try {
    if (zoom === DEFAULT_ZOOM) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, JSON.stringify(zoom));
  } catch {
    // Non-fatal: the size still holds for this session if it can't persist.
  }
}
