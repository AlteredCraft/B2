// THE ONE IPC SEAM (specs/completed/desktop-ui-mvp.md §3). Every `invoke()` in the frontend
// lives here — the presentation-side mirror of the `Vault` façade. Keeping it in one
// module means the rest of the UI never imports Tauri directly: it can be unit-tested
// by mocking this module, and a future `serve`/HTTP transport swap touches ~this file
// only. Do not call `invoke` anywhere else.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  EmbedReport,
  EmbedStat,
  ExplainView,
  LinkReport,
  ModelChoice,
  NeighborView,
  NoteSummary,
  NoteView,
  ProjectReport,
  ReindexProgress,
  ResourceExplainView,
  ResourceSummary,
  SearchResult,
  SimilarView,
  VaultInfo,
  WriteReport,
} from "./types";

// A rejected `invoke` resolves to the host's user-facing string (CmdError serializes
// to `user_message`), so surface it directly — it's already generic and actionable.
export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

/**
 * The host's exact `WriteConflict` message — part of the IPC contract
 * (desktop-editing.md §5): the frontend recognizes a save conflict by matching this
 * stable constant. Pinned host-side by the `write_conflict_is_generic_and_recognizable`
 * test in `b2-desktop/src/commands.rs` — change them together.
 */
export const WRITE_CONFLICT_MESSAGE =
  "This note changed on disk since it was opened. Reload the note, then reapply your edit.";

/**
 * Whether an IPC rejection is the save guard refusing a stale revision.
 * `startsWith`, not equality: `B2_DEBUG` appends `\n(debug: …)` to every message.
 */
export function isWriteConflict(e: unknown): boolean {
  return errText(e).startsWith(WRITE_CONFLICT_MESSAGE);
}

/**
 * The host's filesystem-watch pulse (desktop-ui-mvp.md §5 / #14): the Rust watcher emits
 * this event, debounced, whenever the vault's Markdown changes on disk from outside the app
 * (an external editor, a `git pull`). Must equal the host's `VAULT_CHANGED_EVENT`
 * (`b2-desktop/src/watch.rs`) — pinned by the `vault_changed_event_matches_the_frontend`
 * test there; change both together. The pulse carries no payload: it's a bare "reconcile
 * now" signal, and the frontend re-reads through the façade to see *what* changed.
 */
export const VAULT_CHANGED_EVENT = "vault-changed";

export const api = {
  /** Step 0's seam proof: round-trips a trivial command through the Rust host. */
  ping: (): Promise<string> => invoke("ping"),

  /** The active vault root + whether semantic ranking is live (real model). */
  vaultInfo: (): Promise<VaultInfo> => invoke("vault_info"),

  /** Open a native folder picker to switch the active vault. Resolves to the new
   *  `VaultInfo`, or `null` if the user cancelled (the current vault stays put). */
  chooseVault: (): Promise<VaultInfo | null> => invoke("choose_vault"),

  /** A note's body + metadata for the left pane (path or b2id). */
  readNote: (note: string): Promise<NoteView> => invoke("read_note", { note }),

  /** Every indexed note (b2id, path, title; no body) — the file tree's source. */
  listNotes: (): Promise<NoteSummary[]> => invoke("list_notes"),

  /** Every inventoried non-`.md` file — the tree's resource half (slice 1). */
  listResources: (): Promise<ResourceSummary[]> => invoke("list_resources"),

  /** The fallback card's data: a resource's metadata + backlinks. */
  explainResource: (path: string): Promise<ResourceExplainView> =>
    invoke("explain_resource", { path }),

  /**
   * *Open in system default* — an OS handoff performed host-side (the webview holds
   * no opener permission); the host validates the path against the inventory first.
   */
  openResource: (path: string): Promise<void> => invoke("open_resource", { path }),

  /** Semantically-near, not-yet-linked candidates for a note. */
  similar: (note: string, limit = 10): Promise<SimilarView[]> =>
    invoke("similar", { note, limit }),

  /** Hybrid keyword+semantic search across the vault. */
  search: (query: string, limit = 20): Promise<SearchResult[]> =>
    invoke("search", { query, limit }),

  /**
   * Save a note's body — Markdown-first through `Vault::write` (desktop-editing.md
   * §4): a byte-honest body splice guarded by the `revision` captured at read, then
   * a model-free re-projection. Rejects with `WRITE_CONFLICT_MESSAGE` when the file
   * changed on disk since. (Tauri v2 maps camelCase keys to the command's snake_case
   * params — `baseRevision` → `base_revision` — so no hand-written snake_case here.)
   */
  writeNote: (note: string, body: string, baseRevision: string): Promise<WriteReport> =>
    invoke("write_note", { note, body, baseRevision }),

  /** A note's typed neighbors (both directions). */
  neighbors: (note: string): Promise<NeighborView[]> => invoke("neighbors", { note }),

  /** A note's connections with their "why" (outbound + inbound). */
  explain: (note: string): Promise<ExplainView> => invoke("explain", { note }),

  /** Commit a typed connection `src --relation--> dst` into src's frontmatter. */
  link: (
    src: string,
    dst: string,
    relation: string,
    explanation: string | null,
  ): Promise<LinkReport> => invoke("link", { src, dst, relation, explanation }),

  /**
   * Phase 1 of a reindex — the fast, **model-free** projection pass
   * (projection-embedding-split.md §6): notes + keyword index + graph, stamping
   * missing b2ids. Once it resolves, the tree and keyword search are live; call
   * `embed` to fill the vectors behind it.
   */
  project: (): Promise<ProjectReport> => invoke("project"),

  /**
   * Phase 2 of a reindex — fill the missing vectors (real model) as a cancellable
   * background action. `onProgress` fires per embed batch over a typed Tauri
   * `Channel` (async-indexing.md §4), determinate from the first batch; the returned
   * Promise resolves with the final report (its `cancelled` flag set if
   * `cancelReindex` was called mid-run).
   */
  embed: (onProgress: (p: ReindexProgress) => void): Promise<EmbedReport> => {
    const channel = new Channel<ReindexProgress>();
    channel.onmessage = onProgress;
    return invoke("embed", { onEvent: channel });
  },

  /** Ask the in-flight embed to stop at its next batch boundary (cooperative). */
  cancelReindex: (): Promise<void> => invoke("cancel_reindex"),

  /** The embedding models B2 offers, flagged current + installed (Settings picker). */
  listModels: (): Promise<ModelChoice[]> => invoke("list_models"),

  /**
   * Persist the chosen embedding model into the shared config B2 reads. Returns the
   * refreshed list. Selecting a *different* model is a model swap — it takes effect only
   * after that model is downloaded (`b2 init`) and the vault is reindexed; this call
   * just records the choice.
   */
  setModel: (model: string): Promise<ModelChoice[]> => invoke("set_model", { model }),

  /**
   * Download + verify the currently-selected model into the shared cache — the in-app
   * `b2 init`. Idempotent, network-bound (can take minutes). Resolves with the refreshed
   * model list, the just-installed model now flagged `installed`.
   */
  provisionModel: (): Promise<ModelChoice[]> => invoke("provision_model"),

  /** Per-model cumulative embedding time (Settings), accumulated across sessions. */
  embedStats: (): Promise<EmbedStat[]> => invoke("embed_stats"),

  /** The shared directory where downloaded model files are saved (shown in Settings). */
  modelsDir: (): Promise<string> => invoke("models_dir"),

  /**
   * Subscribe to the host's debounced filesystem-watch pulse (#14). `handler` fires once
   * per burst of external Markdown changes; the returned promise resolves to an unlisten
   * function (unused here — the subscription lives for the window's lifetime). This is the
   * only `listen` in the app, kept behind the seam like every `invoke`.
   */
  onVaultChanged: (handler: () => void): Promise<UnlistenFn> =>
    listen(VAULT_CHANGED_EVENT, () => handler()),
};
