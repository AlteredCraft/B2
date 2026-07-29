// The customization layer over the keyboard registry (#121) — the algebra that turns
// "the keyboard B2 ships with" into "the keyboard this user has", and the judgement that
// decides whether a chord they just pressed may become part of it.
//
// Mostly pure, so node runs its test straight off the source (`npm test`); the two
// functions that touch `localStorage` are at the bottom, the same shape panes.ts uses for
// column widths. That is deliberate as to *where the preference lives*, not just how it's
// stored: a keyboard layout is a viewing choice, never vault state, so it does not touch
// the host, the index, or a byte of Markdown. Drop the vault on another machine and the
// notes are identical; the chords are that machine's business.
//
// **The four checkers already existed.** #118 built `conflicts()` and `shadows()` here in
// bindings.ts, `editorOverlaps()` in editorkeys.ts and `menuOverlaps()` in menukeys.ts —
// pure functions over a table, shaped for exactly this and until now run only as a CI
// gate over the shipped defaults. `chordProblems` is what finally asks them a *question*:
// it lays the candidate chord over the current table and asks all four what that table
// would be like, so the answer a user gets is the same answer the gate would give.
//
// The two tiers are theirs, not new policy. `conflicts()` is refusal — two commands
// answering to one keystroke in one scope, where which one runs depends on the order of
// branches in main.ts's handler, which is not a contract anyone can read. `menuOverlaps()`
// is refusal for a blunter reason: AppKit dispatches a menu accelerator before the key
// window's responder chain, so a B2 chord spelled ⌘W is not a chord that loses a race, it
// is a chord that never happens. `shadows()` and `editorOverlaps()` are advisory —
// legal, frequently deliberate, and worth saying out loud.
import {
  type Binding,
  DEFAULT_BINDINGS,
  activeBindings,
  conflicts,
  displayChord,
  findBinding,
  parseChord,
  shadows,
} from "./bindings.ts";
import { editorOverlaps } from "./editorkeys.ts";
import { MENU_CHORDS, menuOverlaps } from "./menukeys.ts";
import type { MenuChord } from "./types.ts";

/** The user's rebindings: command id → the chords that now fire it.
 *
 *  Sparse on purpose — an unrebound command has no entry, so "reset this chord" is a
 *  delete and "reset everything" is an empty object. There is no stored copy of a default
 *  to fall out of date with the table. */
export type Overrides = Readonly<Record<string, readonly string[]>>;

const KEY = "b2:keymap";

/** Can this command's chord be changed? See `Binding.fixed` for what earns a no. */
export function isRebindable(b: Binding): boolean {
  return b.fixed === undefined;
}

// --- the algebra ---------------------------------------------------------------------

/**
 * The defaults with the user's rebindings laid over them — the table `setActiveBindings`
 * installs, and the one every checker below reasons about.
 *
 * An override replaces `keys` and leaves `aliases` alone (`Binding.aliases` says why),
 * and an empty or absent entry means "unchanged", so a malformed store degrades to the
 * default keyboard rather than to no keyboard.
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

/** The commands the user has moved, in the table's own order — what the panel marks as
 *  changed, and what "Reset all" has to have something to do. */
export function customized(
  base: readonly Binding[] = DEFAULT_BINDINGS,
  overrides: Overrides = {},
): Binding[] {
  return base.filter((b) => overrides[b.id] !== undefined);
}

/** Is this list simply what `id` already ships with?
 *
 *  The store has two ways in — the recorder and a hand-edited file — and this is the rule
 *  they have to agree on. `adoptOverrides` has always refused to *read* a restatement back
 *  (a "changed" badge over an unmoved chord is a lie, and "Reset all (1)" would offer to
 *  undo nothing); `withOverride` now refuses to *write* one, which is where it was getting
 *  in. Order counts: `keys[0]` is what the sheet leads with and what CodeMirror is handed
 *  (`chordFor`), so the same chords in another order really is a change.
 *
 *  Module-private: the two writers above are the only callers, and a rule the store
 *  enforces for itself is not a question its callers should be asking separately. */
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

/** `overrides` with `id` bound to `chords`, or — for an empty list, or one that restates
 *  what the command ships with — reset to its default.
 *  Returns a new object; nothing here mutates its argument. */
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
 * Everything the four checkers have to say about binding `spec` to `id`, given the
 * rebindings already in force. Empty means "nothing to report".
 *
 * Asked of the *candidate* table rather than the live one, which is the whole point: the
 * question is not "does the keyboard conflict today" but "would it, if this were saved".
 * Each checker is filtered to rows naming `id`, since the pre-existing shadows in the
 * shipped table are not this user's problem.
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

  // Refusal, tier one: the menu bar got there first, and it wins in every scope at once.
  for (const o of menuOverlaps(candidate, menu)) {
    if (o.id !== id) continue;
    out.push({
      tier: "refuse",
      message: `${shown} belongs to the menu bar (${menuLabel(menu, o.item)}). macOS runs it before B2 sees the key.`,
    });
  }
  // Refusal, tier two: two commands, one keystroke, one scope.
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
  // Advisory: the editor's own keyboard. Which side wins is install order and differs row
  // by row (editorkeys.ts), so this names the overlap rather than predicting it.
  for (const o of editorOverlaps(candidate)) {
    if (o.id !== id) continue;
    out.push({
      tier: "warn",
      message: `CodeMirror binds ${shown} while you're editing (${o.command}).`,
    });
  }
  // Advisory: no modifier at all. Legal — `?` is a shipped default — but whether it fires
  // mid-sentence depends on the guard beside its branch in main.ts's handler, which is
  // exactly the thing this table deliberately doesn't model. So: say so, don't refuse.
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
 * A stored blob, read defensively into overrides B2 will actually honour.
 *
 * Two passes, and the second is the one that matters. The first drops what is malformed
 * *in itself* — an unknown command id, a chord that no longer parses, an entry on a
 * `fixed` binding, one that merely restates the default. The second folds the survivors
 * in **one at a time**, keeping an entry only if the table it produces is still free of
 * refusals. That is what makes `conflicts(activeBindings())` empty by construction rather
 * than by trusting the recorder: the recorder refuses a bad chord at record time, but the
 * store is a file a human can edit, and a keyboard where two commands answer to ⌘F is not
 * a keyboard anyone can use to fix itself.
 *
 * `dropped` names what didn't survive, so the caller can say so instead of silently
 * handing back a preference the user thought they had.
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
    // Restating the default is not an override — dropping it here is what keeps "changed"
    // an honest badge and "Reset all" a real no-op when there is nothing to reset. Not a
    // loss, so not `dropped`; `withOverride` holds the same line on the recorder's side.
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

/** Persist the keyboard. An empty set removes the entry rather than storing `{}`, so a
 *  user who resets everything leaves no trace behind to go stale against a future table. */
export function saveOverrides(overrides: Overrides): void {
  try {
    if (Object.keys(overrides).length === 0) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, JSON.stringify(overrides));
  } catch {
    // Non-fatal: the chords still hold for this session if they can't persist.
  }
}

/** The live table's chords for one command, for the panel's "currently" column. */
export function currentKeys(id: string): readonly string[] {
  return findBinding(activeBindings(), id)?.keys ?? [];
}
