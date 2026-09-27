// Global text size: ⌘= / ⌘- / ⌘0.
//
// Real page zoom (WebKit `pageZoom` via the host's `set_zoom`), not CSS: the stylesheet
// sizes in px, and growing text alone would break the layout. The host is a pass-through;
// the ladder and the parsing of a stored value live here, pure and tested. The preference
// lives in localStorage: a viewing choice, never vault state.

const KEY = "b2:zoom";

/** 100%, where ⌘0 returns. A rung of the ladder (the suite pins it). */
export const DEFAULT_ZOOM = 1;

/**
 * The rungs, ascending; its ends are the limits. A fixed ladder rather than a multiplier,
 * which would accumulate float error and give ⌘0 nothing exact to return to.
 */
export const STEPS: readonly number[] = [0.75, 0.85, 0.9, 1, 1.1, 1.25, 1.4, 1.6, 1.8, 2];

/** Which way a step goes. */
export type Direction = 1 | -1;

/**
 * One rung `dir` from `current`. Off-ladder input never overshoots: it lands on the
 * nearest rung on that side (or the end, if past it). The ends are walls, not wraps.
 */
export function stepZoom(current: number, dir: Direction): number {
  if (dir === 1) {
    const next = STEPS.find((s) => s > current);
    return next ?? STEPS[STEPS.length - 1];
  }
  for (let i = STEPS.length - 1; i >= 0; i--) {
    if (STEPS[i] < current) return STEPS[i];
  }
  return STEPS[0];
}

/**
 * An unknown (hand-editable) stored value, read into a size B2 will apply. Unrenderable
 * values become the default; other numbers snap to the nearest rung, so the ladder can be
 * re-tuned without stranding a preference.
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
 * Which side columns the stylesheet is drawing, as measured from the browser (like
 * panes.ts's `Shown`), so no breakpoint from style.css is copied here.
 */
export interface Columns {
  tree: boolean;
  side: boolean;
}

/**
 * What to say about a zoom step that hid a column, or `null`. A notice rather than
 * refusing the step: the ceiling moves with window width. Only losses are announced.
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
    return DEFAULT_ZOOM;
  }
}

/** Persist the size. The default removes the entry, leaving nothing to go stale. */
export function saveZoom(zoom: number): void {
  try {
    if (zoom === DEFAULT_ZOOM) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, JSON.stringify(zoom));
  } catch {
    // Non-fatal: the size still holds for this session.
  }
}
