// The "semantic search is off" install banner's gate, and where its opt-out persists (no
// DOM, no IPC). Without a model, opening a vault projects and then silently skips
// embedding, so discovery is off; this decides when to prompt for Settings → Download.

/** Inputs the banner keys on — plain primitives so this stays node-testable. */
export interface EmbedReminderInputs {
  /** A vault is open. */
  hasVault: boolean;
  /** The real embedding model is installed (`VaultInfo.semantic`). */
  semantic: boolean;
  /** Projected notes (`VaultInfo.notes_total`). Zero (empty or mid-projection) doesn't nag. */
  notesTotal: number;
  /** A model download is in flight, so the ask would be stale. */
  provisioning: boolean;
  /** Dismissed, for this session (✕) or for good ("Don't remind me again"). */
  dismissed: boolean;
}

/** Whether to show the install banner: only for a real, actionable gap. */
export function shouldPromptEmbedInstall(i: EmbedReminderInputs): boolean {
  return (
    i.hasVault &&
    !i.semantic &&
    !i.provisioning &&
    !i.dismissed &&
    i.notesTotal > 0
  );
}

// --- persistence -----------------------------------------------------------------------
//
// The persisted half of `dismissed`. localStorage: a viewing choice, never vault state.

const KEY = "b2:embed-reminder-off";

/** Has the user opted out for good? Unavailable storage reads as no. */
export function loadReminderOptOut(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function saveReminderOptOut(): void {
  try {
    localStorage.setItem(KEY, "1");
  } catch {
    // Non-fatal: the opt-out still holds for this session.
  }
}
