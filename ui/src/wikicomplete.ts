// Wikilink completion, the pure half — typing `[[` in the editor offers the vault's
// notes and files, Obsidian-style. This module is the *logic*: detect the open `[[`
// the cursor sits in, rank candidates against the query, and compute the insertion
// that closes the brackets. main.ts wraps it in a CodeMirror completion source; the
// split keeps this node-testable with no editor dependency (the move.ts pattern).
//
// Targets follow the engine's resolution rules (`db::resolve_link_target` /
// `resolve_resource_target`): a wikilink is a **vault-root-relative path**, `.md`
// optional for notes (the Obsidian habit — we omit it), extension required for
// resources (extension-only kind dispatch, slice-1 spec §3). Titles are display-only;
// inserting one would author a dangling link.

import { baseName } from "./move.ts";
import { parentDir } from "./newentry.ts";
import type { NoteSummary, ResourceSummary } from "./types.ts";

/** One completion row: what gets inserted, and the two display lines. */
export interface WikiCandidate {
  /** The wikilink target to insert — a vault-relative path (notes minus `.md`). */
  target: string;
  /** The primary display line — the note's title, or the filename. */
  label: string;
  /** The muted second line — the containing folder, `dir/` style ("" at root). */
  detail: string;
}

/**
 * Find the open `[[` the cursor is typing into. `textBefore` is the current line up
 * to the cursor; the return's `from` is the query's start offset within it (i.e.
 * just past the `[[`), ready to become the completion's replace-from. `null` means
 * no trigger: no `[[`, already closed, or past a `|` (the target is fixed and the
 * user is typing display text).
 */
export function wikiQueryAt(textBefore: string): { from: number; query: string } | null {
  const open = textBefore.lastIndexOf("[[");
  if (open < 0) return null;
  const query = textBefore.slice(open + 2);
  if (/[\][|]/.test(query)) return null;
  return { from: open + 2, query };
}

/** A note's display title — its `title`, or the filename minus `.md`. */
function noteLabel(n: NoteSummary): string {
  return n.title ?? baseName(n.path).replace(/\.md$/, "");
}

/** The `dir/` detail line under a label ("" for a root-level entry). */
function dirDetail(path: string): string {
  const dir = parentDir(path);
  return dir === "" ? "" : `${dir}/`;
}

// Ranking tiers: a label (title/filename) prefix beats a label substring beats a
// path-only match; anything else is dropped. Within a tier, label order.
function tierOf(label: string, path: string, query: string): number | null {
  const l = label.toLowerCase();
  if (l.startsWith(query)) return 0;
  if (l.includes(query)) return 1;
  if (path.toLowerCase().includes(query)) return 2;
  return null;
}

/**
 * Rank the vault's notes + resources against `query` (case-insensitive), best first.
 * An empty query lists everything label-sorted, so the menu opens useful the moment
 * `[[` is typed. `limit` caps the list — the menu is a picker, not an inventory.
 */
export function wikiCandidates(
  notes: NoteSummary[],
  resources: ResourceSummary[],
  query: string,
  limit = 50,
): WikiCandidate[] {
  const q = query.toLowerCase();
  const ranked: { tier: number; c: WikiCandidate }[] = [];
  for (const n of notes) {
    const label = noteLabel(n);
    const tier = tierOf(label, n.path, q);
    if (tier === null) continue;
    ranked.push({
      tier,
      c: { target: n.path.replace(/\.md$/, ""), label, detail: dirDetail(n.path) },
    });
  }
  for (const r of resources) {
    const label = baseName(r.path);
    const tier = tierOf(label, r.path, q);
    if (tier === null) continue;
    ranked.push({ tier, c: { target: r.path, label, detail: dirDetail(r.path) } });
  }
  ranked.sort(
    (a, b) =>
      a.tier - b.tier ||
      a.c.label.toLowerCase().localeCompare(b.c.label.toLowerCase()) ||
      a.c.target.localeCompare(b.c.target),
  );
  return ranked.slice(0, limit).map((r) => r.c);
}

/**
 * The text that completes a picked target, given what already follows the cursor:
 * append `]]`, finish a lone `]`, or reuse an existing `]]` — never a stray third
 * bracket. `cursor` is where the caret lands, relative to the insertion start —
 * always just past the closing brackets, ready to keep typing prose.
 */
export function wikiInsertion(
  target: string,
  after: string,
): { insert: string; cursor: number } {
  const insert = after.startsWith("]]")
    ? target
    : after.startsWith("]")
      ? `${target}]`
      : `${target}]]`;
  return { insert, cursor: target.length + 2 };
}
