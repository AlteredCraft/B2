// Wikilink completion, the pure half: typing `[[` offers the vault's notes and files.
// main.ts wraps it in a CodeMirror completion source.
//
// Targets follow the engine's resolution (`db::resolve_link_target` /
// `resolve_resource_target`): a vault-root-relative path, `.md` omitted for notes,
// extension required for resources. Titles are display-only; inserting one would dangle.

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
 * Find the open `[[` the cursor is typing into. `textBefore` is the line up to the
 * cursor; `from` is just past the `[[`. Null for no `[[`, already closed, or past a `|`.
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

/** The wikilink target that names a note: its path minus `.md`. Shared with droplink.ts. */
export function noteTarget(path: string): string {
  return path.replace(/\.md$/, "");
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
 * Rank the vault's notes and resources against `query` (case-insensitive), best first. An
 * empty query lists everything label-sorted.
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
      c: { target: noteTarget(n.path), label, detail: dirDetail(n.path) },
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
 * The text that completes a picked target: append `]]`, finish a lone `]`, or reuse an
 * existing `]]`. `cursor` lands just past the closing brackets, relative to the insertion.
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
