// Pure sequencing for the `vault-changed` reconcile's index refresh (no DOM, no IPC).
//
// A pulse means the disk changed, but the tree lists come from the index, so the index is
// re-projected first or an external add stays invisible (#65). Re-projecting a note clears
// its vectors (`db::replace_chunks`) and projection is model-free, so the reconcile also
// schedules the trailing embed the in-app save path uses, or the note drops out of
// discovery.

/** The four thunks the sequence composes, plus the one gate it respects. */
export interface ReconcileIndexDeps {
  /** A reindex is in flight and owns the index, so reconcile neither projects nor heals. */
  reindexing: boolean;
  /** The model-free projection pass (`api.project`). Can't loop the watcher: `.b2/` writes
   *  are filtered host-side and projection writes nothing to the vault (W1, GH #170). */
  project: () => Promise<unknown>;
  /** Re-fetch the tree lists (`loadNotes`). Its errors pass through to the caller. */
  list: () => Promise<unknown>;
  /** Whether any note still lacks vectors, read after the projection (`vault_info`'s
   *  model-free coverage, #26). */
  vectorsPending: () => Promise<boolean>;
  /** Schedule the trailing embed (`scheduleTrailingEmbed`). Fire-and-forget: it debounces
   *  and refreshes discovery itself. */
  healVectors: () => void;
}

/**
 * Project (unless a reindex owns the index), re-list even if that failed, then schedule
 * an embed iff some note is missing vectors.
 */
export async function reconcileIndex(deps: ReconcileIndexDeps): Promise<void> {
  if (!deps.reindexing) {
    try {
      await deps.project();
    } catch {
      // Best-effort: the next reindex or pulse heals the rest.
    }
  }
  await deps.list();

  // Gated like the projection, but not on its success: the pending set is DB-derived.
  if (deps.reindexing) return;
  try {
    if (await deps.vectorsPending()) deps.healVectors();
  } catch {
    // Best-effort, as above.
  }
}
