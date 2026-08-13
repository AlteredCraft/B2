// Pure sequencing for the `vault-changed` reconcile's index refresh — no DOM, no IPC —
// so node runs its test straight off the source (`npm test`), like embedreminder.ts.
//
// Why a projection belongs here at all (#65 dogfood, item 4): the tree lists come from
// the index (`list_notes` / `list_resources` are index-first by design — a
// never-projected vault lists nothing), but the pulse means the *disk* changed. An
// externally added file (a Finder-dropped PNG, a new `.md` from another editor) has no
// index row until something projects it, so a re-list alone repaints the same tree and
// the add is invisible until a manual reindex. Re-deriving first keeps
// `index = projection of (the vault)` honest at the exact moment the vault changed.
//
// And why the *heal* belongs here too: that projection re-chunks whatever changed, and
// re-chunking **clears** the note's vectors — `db::replace_chunks` drops its chunk rows
// (their `embeddings` cascade with them) and its `note_centroids` row. Projection is
// model-free, so nothing fills them back in. A note edited outside the app therefore
// comes back semantically invisible: `discover::candidates` has no anchor vectors to
// rank *from*, and no centroid for the note to be ranked *against* as a candidate for
// anything else — so the discovery pane empties out to "Nothing similar-but-unlinked,
// or the vault isn't embedded yet" until a manual reindex. The in-app save path
// already answers this with its trailing embed; a pulse is the same invalidation
// arriving from the other side (an external editor, a `git pull`, a Finder drop, an
// import), so it owes the same answer.

/** The four thunks the sequence composes, plus the one gate it respects. */
export interface ReconcileIndexDeps {
  /** A reindex (manual, auto-on-open, or trailing embed) is in flight: that run owns
   *  the index — and its own embed and UI refresh — so reconcile must neither project
   *  nor heal under it. */
  reindexing: boolean;
  /** The model-free projection pass (`api.project`) — cheap (no model load),
   *  idempotent, and host-safe outside the reindex slot, the same op the first tree
   *  paint uses. Its own side effects can't loop this: `.b2/` writes are filtered
   *  host-side, and a b2id stamp on a new note re-pulses into a no-op pass. */
  project: () => Promise<unknown>;
  /** Re-fetch the tree lists (`loadNotes`). Its errors are the caller's contract
   *  (toast + empty tree) and pass through untouched. */
  list: () => Promise<unknown>;
  /** Read back, *after* the projection, whether any projected note is still missing
   *  vectors (`vault_info`'s model-free N/M coverage — #26). The pending set is
   *  DB-derived, so this answers honestly whether or not the projection above
   *  succeeded — and it costs no model load, which is the whole point of asking
   *  before scheduling one. */
  vectorsPending: () => Promise<boolean>;
  /** Schedule the trailing embed (`scheduleTrailingEmbed`) to fill them. Deliberately
   *  fire-and-forget: it debounces, coalesces with the save path's own timer, and
   *  refreshes discovery when it lands — none of which the reconcile should wait on. */
  healVectors: () => void;
}

/**
 * Re-derive the index from disk, re-list it, then heal the vectors the re-derivation
 * cleared: project (unless a reindex owns the index) and fall through to the list
 * either way — a failed projection is a background hum, never a reason to skip the
 * tree refresh — then schedule an embed iff something is actually missing one.
 */
export async function reconcileIndex(deps: ReconcileIndexDeps): Promise<void> {
  if (!deps.reindexing) {
    try {
      await deps.project();
    } catch {
      // Best-effort: refused (a reindex won the race) or failed (unreadable vault) —
      // re-list whatever the index has; the next reindex/pulse heals the rest.
    }
  }
  await deps.list();

  // The heal answers to the same gate as the projection, but not to its success: the
  // pending set is DB-derived, so a pass that failed halfway still leaves vectors owed,
  // while a pass that changed nothing reports nothing pending and schedules nothing.
  if (deps.reindexing) return;
  try {
    if (await deps.vectorsPending()) deps.healVectors();
  } catch {
    // Same posture as the projection: a coverage read that fails leaves the vectors to
    // the next pulse or reindex rather than failing the reconcile over a hint.
  }
}
