// The app's state — a single mutable object the view renders from. No framework:
// actions (in main.ts) mutate this and call the render hook. Small enough that a
// full-pane re-render on change is imperceptible and keeps the model honest.

import type {
  NeighborView,
  NoteView,
  SearchResult,
  SimilarView,
} from "./types";

/**
 * The closed 10-verb relation core (b2-core `relation.rs` CORE — data-model.md §2).
 * The link picker offers exactly these; the Rust host re-validates `is_core`, so a
 * drifted entry here is *refused*, never silently stored (a bad verb → a generic,
 * actionable error). `references` is the default, matching `b2 link`.
 */
export const RELATION_VERBS = [
  "references",
  "relates",
  "elaborates",
  "supports",
  "refutes",
  "contradicts",
  "example-of",
  "part-of",
  "supersedes",
  "derived-from",
] as const;

/** The note the link modal targets (the source is always the open note). */
export interface LinkTarget {
  path: string;
  title: string | null;
}

export interface AppState {
  /** Vault root, or null when none is configured (the app shows an actionable state). */
  vaultRoot: string | null;
  /** Whether semantic ranking is live (real model) — drives the honest search caveat. */
  semantic: boolean;
  /** The open note (left pane), or null before one is opened. */
  current: NoteView | null;
  /** Similar-but-unlinked candidates for the open note. */
  similar: SimilarView[];
  /** The open note's typed edges (from explain). */
  connections: NeighborView[];
  /** The active search query (empty ⇒ the side pane shows discovery, not results). */
  searchQuery: string;
  searchResults: SearchResult[];
  /** When set, the link modal is open for this target. */
  linkTarget: LinkTarget | null;
  /** The verb selected in the link modal. */
  linkRelation: string;
  /** A slow op is in flight. */
  loading: boolean;
  /** A transient toast message (success or a generic, actionable error). */
  status: string | null;
}

export const state: AppState = {
  vaultRoot: null,
  semantic: true,
  current: null,
  similar: [],
  connections: [],
  searchQuery: "",
  searchResults: [],
  linkTarget: null,
  linkRelation: "references",
  loading: false,
  status: null,
};
