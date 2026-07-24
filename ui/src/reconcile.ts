// Pure sequencing for the `vault-changed` reconcile's tree refresh — no DOM, no IPC —
// so node runs its test straight off the source (`npm test`), like embedreminder.ts.
//
// Why a projection belongs here at all (#65 dogfood, item 4): the tree lists come from
// the index (`list_notes` / `list_resources` are index-first by design — a
// never-projected vault lists nothing), but the pulse means the *disk* changed. An
// externally added file (a Finder-dropped PNG, a new `.md` from another editor) has no
// index row until something projects it, so a re-list alone repaints the same tree and
// the add is invisible until a manual reindex. Re-deriving first keeps
// `index = projection of (the vault)` honest at the exact moment the vault changed.

import type { B2idCollision, RestampedNote } from "./types.ts";

/** The two thunks the sequence composes, plus the one gate it respects. */
export interface ReconcileListDeps {
  /** A reindex (manual, auto-on-open, or trailing embed) is in flight: that run owns
   *  the index and its own UI refresh, so reconcile must not project under it. */
  reindexing: boolean;
  /** The model-free projection pass (`api.project`) — cheap (no model load),
   *  idempotent, and host-safe outside the reindex slot, the same op the first tree
   *  paint uses. Its own side effects can't loop this: `.b2/` writes are filtered
   *  host-side, and a b2id stamp on a new note re-pulses into a no-op pass. */
  project: () => Promise<unknown>;
  /** Re-fetch the tree lists (`loadNotes`). Its errors are the caller's contract
   *  (toast + empty tree) and pass through untouched. */
  list: () => Promise<unknown>;
  /** Surface a completed pulse-projection's report — the GH #81 anomaly notices
   *  (a Finder-duplicated note collides *here*, on the very pulse the copy
   *  landed). Optional; never called when the projection was skipped (a reindex
   *  owns the index and its own reporting) or failed (best-effort hum). */
  onReport?: (report: unknown) => void;
}

/**
 * Re-derive the index from disk, then re-list it: project (unless a reindex owns the
 * index) and fall through to the list either way — a failed projection is a background
 * hum, never a reason to skip the tree refresh.
 */
export async function reprojectThenList(deps: ReconcileListDeps): Promise<void> {
  if (!deps.reindexing) {
    try {
      const report = await deps.project();
      deps.onReport?.(report);
    } catch {
      // Best-effort: refused (a reindex won the race) or failed (unreadable vault) —
      // re-list whatever the index has; the next reindex/pulse heals the rest.
    }
  }
  await deps.list();
}

/** The slice of a `ProjectReport` the anomaly notice reads (GH #81). */
export interface IndexAnomalies {
  collisions: B2idCollision[];
  restamped: RestampedNote[];
}

/**
 * The GH #81 anomaly flash — one compact, actionable line per anomaly, or `null`
 * when the pass was clean (the common case: pulses are frequent and must stay
 * silent). Pure string-building, so node tests pin the wording contract:
 * name the un-indexed file, say who kept the identity and why, say the fix.
 */
export function anomalyNotice(report: IndexAnomalies): string | null {
  const parts: string[] = [];
  for (const c of report.collisions) {
    const kept =
      c.precedence === "incumbent"
        ? `${c.kept_path} kept it`
        : `${c.kept_path} kept it (path order — b2 can't tell which is the original)`;
    parts.push(
      `Duplicate b2id: ${c.shadowed_paths.join(", ")} not indexed — ${kept}. ` +
        `Delete the copy, or remove its b2id line for a fresh identity.`,
    );
  }
  for (const r of report.restamped) {
    parts.push(
      `${r.path} was re-stamped a new identity (its b2id line was removed externally) — links to its old identity are now broken.`,
    );
  }
  return parts.length ? parts.join(" ") : null;
}
