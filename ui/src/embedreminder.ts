// Pure gating logic for the "semantic search is off" install banner — no DOM, no IPC —
// so node runs its test straight off the source (`npm test`), like newentry.ts/panes.ts.
//
// The problem it addresses: on a fresh install with no embedding model, opening a vault
// runs the model-free projection pass (keyword index + graph) and then *silently* stops
// before embedding (`autoIndexOnOpen` bails on `!semantic`). Discovery and semantic
// ranking are simply off, and the only prior signal was the small search caveat (#26),
// which is easy to miss. This predicate decides when to surface a prominent, dismissible
// prompt pointing at Settings → Download instead.

/** Inputs the banner keys on — plain primitives so this stays node-testable. */
export interface EmbedReminderInputs {
  /** A vault is open (null root ⇒ nothing to embed, nothing to prompt about). */
  hasVault: boolean;
  /** The real embedding model is installed (`VaultInfo.semantic`). When true the
   *  problem doesn't exist — semantic ranking is (or is becoming) live. */
  semantic: boolean;
  /** Projected notes (`VaultInfo.notes_total`). Zero means either an empty vault or one
   *  still mid-projection: there is nothing to embed yet, so don't nag prematurely. */
  notesTotal: number;
  /** A model download is already in flight — the Settings modal owns the flow; the
   *  banner's "go download it" ask would be stale, so stand down while it runs. */
  provisioning: boolean;
  /** The user has dismissed the reminder (this session's ✕, or a persisted
   *  "Don't remind me again" — a keyword-only user opting out for good). */
  dismissed: boolean;
}

/**
 * Whether to show the install banner. True only when there is a real, actionable gap:
 * a vault with content is open, the model is genuinely absent, no download is already
 * running, and the user hasn't opted out. Kept intentionally narrow so the banner never
 * fires on an empty vault, mid-projection, mid-download, or once semantic is live.
 */
export function shouldPromptEmbedInstall(i: EmbedReminderInputs): boolean {
  return (
    i.hasVault &&
    !i.semantic &&
    !i.provisioning &&
    !i.dismissed &&
    i.notesTotal > 0
  );
}
