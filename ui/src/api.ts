// THE ONE IPC SEAM (specs/desktop-ui-mvp.md §3). Every `invoke()` in the frontend
// lives here — the presentation-side mirror of the `Vault` façade. Keeping it in one
// module means the rest of the UI never imports Tauri directly: it can be unit-tested
// by mocking this module, and a future `serve`/HTTP transport swap touches ~this file
// only. Do not call `invoke` anywhere else.

import { invoke } from "@tauri-apps/api/core";
import type {
  ExplainView,
  LinkReport,
  NeighborView,
  NoteView,
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

  /** A note's body + metadata for the left pane (path or b2id). */
  readNote: (note: string): Promise<NoteView> => invoke("read_note", { note }),

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

  /** Re-project the vault into the index (stamps missing b2ids; embeds changes). */
  reindex: (): Promise<ReindexReport> => invoke("reindex"),
};
