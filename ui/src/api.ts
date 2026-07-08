// THE ONE IPC SEAM (specs/completed/desktop-ui-mvp.md §3). Every `invoke()` in the frontend
// lives here — the presentation-side mirror of the `Vault` façade. Keeping it in one
// module means the rest of the UI never imports Tauri directly: it can be unit-tested
// by mocking this module, and a future `serve`/HTTP transport swap touches ~this file
// only. Do not call `invoke` anywhere else.

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  EmbedReport,
  ExplainView,
  LinkReport,
  NeighborView,
  NoteSummary,
  NoteView,
  ProjectReport,
  ReindexProgress,
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
};
