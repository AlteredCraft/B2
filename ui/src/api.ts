// THE ONE IPC SEAM (specs/desktop-ui-mvp.md §3). Every `invoke()` in the frontend
// lives here — the presentation-side mirror of the `Vault` façade. Keeping it in one
// module means the rest of the UI never imports Tauri directly: it can be unit-tested
// by mocking this module, and a future `serve`/HTTP transport swap touches ~this file
// only. Do not call `invoke` anywhere else.

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ExplainView,
  LinkReport,
  NeighborView,
  NoteSummary,
  NoteView,
  ReindexProgress,
  ReindexReport,
  SearchResult,
  SimilarView,
  VaultInfo,
} from "./types";

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

  /** Semantically-near, not-yet-linked candidates for a note. */
  similar: (note: string, limit = 10): Promise<SimilarView[]> =>
    invoke("similar", { note, limit }),

  /** Hybrid keyword+semantic search across the vault. */
  search: (query: string, limit = 20): Promise<SearchResult[]> =>
    invoke("search", { query, limit }),

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
   * Re-project the vault into the index (stamps missing b2ids; embeds changes) as a
   * cancellable background action. `onProgress` fires per embed batch over a typed
   * Tauri `Channel` (async-indexing.md §4); the returned Promise resolves with the
   * final report (its `cancelled` flag set if `cancelReindex` was called mid-run).
   */
  reindex: (onProgress: (p: ReindexProgress) => void): Promise<ReindexReport> => {
    const channel = new Channel<ReindexProgress>();
    channel.onmessage = onProgress;
    return invoke("reindex", { onEvent: channel });
  },

  /** Ask the in-flight reindex to stop at its next batch boundary (cooperative). */
  cancelReindex: (): Promise<void> => invoke("cancel_reindex"),
};
