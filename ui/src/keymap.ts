// The customization layer over the keyboard registry (#121): user rebindings laid over the
// defaults, and the judgement on whether a pressed chord may join them. A keyboard layout
// is a viewing choice, so it lives in `localStorage`, never in the vault or the host.
//
// `chordProblems` asks the four CI checkers (#118) about the candidate table, so the user
// gets the gate's answer. `conflicts()` and `menuOverlaps()` refuse (AppKit runs a menu
// accelerator before B2 sees the key); `shadows()` and `editorOverlaps()` only warn.
import {
  type Binding,
  DEFAULT_BINDINGS,
  conflicts,
  displayChord,
  findBinding,
  parseChord,
  shadows,
} from "./bindings.ts";
import { editorOverlaps } from "./editorkeys.ts";
import { MENU_CHORDS, menuOverlaps } from "./menukeys.ts";
import type { MenuChord } from "./types.ts";

/** The user's rebindings: command id → the chords that now fire it. Sparse, so a reset is
 *  a delete and no stored default can go stale. */
export type Overrides = Readonly<Record<string, readonly string[]>>;

const KEY = "b2:keymap";

/** Can this command's chord be changed? See `Binding.fixed` for what earns a no. */
export function isRebindable(b: Binding): boolean {
  return b.fixed === undefined;
}

// --- the algebra ---------------------------------------------------------------------

/**
 * The defaults with the user's rebindings laid over them. An override replaces `keys` and
 * leaves `aliases` alone; an empty entry means "unchanged", so a malformed store degrades
 * to the default keyboard.
 */
export function applyOverrides(
  base: readonly Binding[] = DEFAULT_BINDINGS,
  overrides: Overrides = {},
): Binding[] {
  return base.map((b) => {
    const keys = overrides[b.id];
    return keys && keys.length > 0 ? { ...b, keys } : b;
  });
}

/** The commands the user has moved, in the table's own order. */
export function customized(
  base: readonly Binding[] = DEFAULT_BINDINGS,
  overrides: Overrides = {},
): Binding[] {
  return base.filter((b) => overrides[b.id] !== undefined);
}

/** Is this list simply what `id` already ships with? Both ways into the store (the recorder
 *  and a hand-edited file) refuse a restatement, so "changed" stays honest. Order counts:
 *  `keys[0]` leads the sheet and is what CodeMirror gets. */
function restatesDefault(
  base: readonly Binding[],
  id: string,
  chords: readonly string[],
): boolean {
  const b = findBinding(base, id);
  return (
    b !== undefined && chords.length === b.keys.length && chords.every((c, i) => c === b.keys[i])
  );
}

/** `overrides` with `id` bound to `chords`, or reset to default for an empty list or a
 *  restatement. Returns a new object. */
export function withOverride(
  overrides: Overrides,
  id: string,
  chords: readonly string[],
  base: readonly Binding[] = DEFAULT_BINDINGS,
): Overrides {
  const next: Record<string, readonly string[]> = { ...overrides };
  if (chords.length === 0 || restatesDefault(base, id, chords)) delete next[id];
  else next[id] = chords;
  return next;
}

// --- judging a candidate chord ---------------------------------------------------------

/** Something the user should know before this chord is saved. `refuse` blocks the save;
 *  `warn` is said out loud and saved anyway — the human is the gate. */
export interface ChordProblem {
  tier: "refuse" | "warn";
  message: string;
}

const menuLabel = (menu: readonly MenuChord[], id: string): string =>
  menu.find((c) => c.id === id)?.label ?? id;

/**
 * Everything the four checkers say about binding `spec` to `id`, asked of the candidate
 * table (as if saved). Filtered to rows naming `id`, since the shipped table's own shadows
 * are not this user's problem.
 */
export function chordProblems(
  id: string,
  spec: string,
  base: readonly Binding[] = DEFAULT_BINDINGS,
  overrides: Overrides = {},
  menu: readonly MenuChord[] = MENU_CHORDS,
): ChordProblem[] {
  const out: ChordProblem[] = [];
  let chord: ReturnType<typeof parseChord>;
  try {
    chord = parseChord(spec);
  } catch {
    return [{ tier: "refuse", message: "B2 can't build a shortcut out of that key." }];
  }
  const candidate = applyOverrides(base, withOverride(overrides, id, [spec], base));
  const shown = displayChord(spec);
  const labelOf = (other: string): string => findBinding(candidate, other)?.label ?? other;

  // Refuse: the menu bar wins in every scope.
  for (const o of menuOverlaps(candidate, menu)) {
    if (o.id !== id) continue;
    out.push({
      tier: "refuse",
      message: `${shown} belongs to the menu bar (${menuLabel(menu, o.item)}). macOS runs it before B2 sees the key.`,
    });
  }
  // Refuse: two commands, one keystroke, one scope.
  for (const c of conflicts(candidate)) {
    if (c.a !== id && c.b !== id) continue;
    out.push({
      tier: "refuse",
      message: `${shown} already runs ${labelOf(c.a === id ? c.b : c.a)} here.`,
    });
  }
  // Advisory: a surface nearer the user takes this keystroke first, or gives it up.
  for (const s of shadows(candidate)) {
    if (s.inner === id) {
      out.push({
        tier: "warn",
        message: `${shown} will run this instead of ${labelOf(s.outer)} while that surface has the keyboard.`,
      });
    } else if (s.outer === id) {
      out.push({
        tier: "warn",
        message: `${labelOf(s.inner)} takes ${shown} first while that surface has the keyboard.`,
      });
    }
  }
  // Advisory: which side wins differs row by row (editorkeys.ts), so name it, don't predict.
  for (const o of editorOverlaps(candidate)) {
    if (o.id !== id) continue;
    out.push({
      tier: "warn",
      message: `CodeMirror binds ${shown} while you're editing (${o.command}).`,
    });
  }
  // Advisory: no modifier. Legal (`?` ships), but whether it fires mid-sentence depends on
  // a guard in main.ts's handler, which the table doesn't model.
  if (!chord.any && !chord.mod && !chord.ctrl && !chord.alt && chord.key.length === 1) {
    out.push({
      tier: "warn",
      message: `${shown} has no modifier, so it can fire while you're typing.`,
    });
  }
  return out;
}

/** Would saving this chord be refused? */
export function refused(problems: readonly ChordProblem[]): boolean {
  return problems.some((p) => p.tier === "refuse");
}

// --- persistence -----------------------------------------------------------------------

/**
 * A stored blob, read defensively into overrides B2 will honour. First drops malformed
 * entries (unknown id, unparseable chord, `fixed` binding), then folds survivors in one at a
 * time, keeping each only if the table stays refusal-free. The store is hand-editable, so
 * this is what keeps `conflicts(activeBindings())` empty. `dropped` names what was lost.
 */
export function adoptOverrides(
  raw: unknown,
  base: readonly Binding[] = DEFAULT_BINDINGS,
  menu: readonly MenuChord[] = MENU_CHORDS,
): { overrides: Overrides; dropped: string[] } {
  const dropped: string[] = [];
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return { overrides: {}, dropped };

  const wanted: [string, string[]][] = [];
  for (const [id, value] of Object.entries(raw as Record<string, unknown>)) {
    const b = findBinding(base, id);
    if (!b || !isRebindable(b)) {
      dropped.push(id);
      continue;
    }
    if (!Array.isArray(value) || value.length === 0 || !value.every((v) => typeof v === "string")) {
      dropped.push(id);
      continue;
    }
    const chords = value as string[];
    if (!chords.every((spec) => tryParse(spec))) {
      dropped.push(id);
      continue;
    }
    // Not an override, and not a loss, so not `dropped`.
    if (restatesDefault(base, id, chords)) continue;
    wanted.push([id, chords]);
  }

  let overrides: Overrides = {};
  for (const [id, chords] of wanted) {
    const problems = chords.flatMap((spec) => chordProblems(id, spec, base, overrides, menu));
    if (refused(problems)) dropped.push(id);
    else overrides = withOverride(overrides, id, chords, base);
  }
  return { overrides, dropped };
}

function tryParse(spec: string): boolean {
  try {
    parseChord(spec);
    return true;
  } catch {
    return false;
  }
}

/** Read the saved keyboard. Unreadable or unavailable storage is the default keyboard —
 *  never a thrown boot. */
export function loadOverrides(
  base: readonly Binding[] = DEFAULT_BINDINGS,
  menu: readonly MenuChord[] = MENU_CHORDS,
): { overrides: Overrides; dropped: string[] } {
  let raw: unknown = null;
  try {
    const text = localStorage.getItem(KEY);
    if (!text) return { overrides: {}, dropped: [] };
    raw = JSON.parse(text);
  } catch {
    // Unavailable (private mode) or not JSON at all: the shipped keyboard.
    return { overrides: {}, dropped: [] };
  }
  return adoptOverrides(raw, base, menu);
}

/** Persist the keyboard. An empty set removes the entry rather than storing `{}`. */
export function saveOverrides(overrides: Overrides): void {
  try {
    if (Object.keys(overrides).length === 0) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, JSON.stringify(overrides));
  } catch {
    // Non-fatal: the chords still hold for this session if they can't persist.
  }
}
