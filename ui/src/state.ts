// The app's state — a single mutable object the view renders from. No framework:
// actions (in main.ts) mutate this and call the render hook. Small enough that a
// full-pane re-render on change is imperceptible and keeps the model honest.

import type {
  EmbedStat,
  ModelChoice,
  NeighborView,
  NoteSummary,
  NoteView,
  ReindexProgress,
  ResourceExplainView,
  ResourceSummary,
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
  /** Every indexed note, path-ordered — the file tree's source (from `list_notes`). */
  notes: NoteSummary[];
  /** Every inventoried non-`.md` file — the tree's resource half (slice 1). */
  resources: ResourceSummary[];
  /** Folder paths (vault-relative, no trailing slash) the tree shows expanded. */
  expandedDirs: Set<string>;
  /** The open note (left pane), or null before one is opened. */
  current: NoteView | null;
  /**
   * The selected resource's fallback card (mutually exclusive with `current`:
   * selecting either kind clears the other — the note pane shows one document).
   */
  currentResource: ResourceExplainView | null;
  /** Whether the note pane's frontmatter drawer is expanded (sticky across notes). */
  frontmatterOpen: boolean;
  /** Whether the note body shows raw Markdown source instead of rendered (sticky). */
  sourceOpen: boolean;
  /**
   * Edit mode: the note pane belongs to the live CodeMirror editor, and `render()`
   * must NOT rebuild it (the carve-out, desktop-editing.md §6) — everything else
   * (tree, side pane, toasts) keeps rendering. Only the *renderable* editing state
   * lives here; timers, save flags, and the EditorView are module-locals in main.ts.
   */
  editing: boolean;
  /** A save hit WriteConflict: autosave is paused and the conflict bar is up. */
  editConflict: boolean;
  /** Similar-but-unlinked candidates for the open note. */
  similar: SimilarView[];
  /** The open note's typed edges (from explain). */
  connections: NeighborView[];
  /**
   * Discovery reads in flight for the open note, tracked **per side-pane section** so
   * the fast graph read (`explain` → Connections) paints without waiting on the slower
   * whole-vault scan (`similar` → Similar & unlinked). Both are kept separate from
   * `loading` so the note body paints the instant it's read. Each flag drives its
   * section's "loading…" hint so an empty section mid-load doesn't read as "nothing found".
   */
  discoveringSimilar: boolean;
  discoveringConnections: boolean;
  /** The active search query (empty ⇒ the side pane shows discovery, not results). */
  searchQuery: string;
  searchResults: SearchResult[];
  /** When set, the link modal is open for this target. */
  linkTarget: LinkTarget | null;
  /** The verb selected in the link modal. */
  linkRelation: string;
  /** The settings modal (⌘,) is open. */
  settingsOpen: boolean;
  /** The embedding models offered in Settings — loaded when the modal opens, else empty. */
  models: ModelChoice[];
  /** Per-model cumulative embedding time — loaded alongside `models`, shown in Settings. */
  embedStats: EmbedStat[];
  /** A model download (in-app `b2 init`) is in flight — disables the button, shows a spinner. */
  provisioning: boolean;
  /** The shared directory where model files are saved — loaded with Settings, else null. */
  modelsDir: string | null;
  /** Compute device the embedder runs on ("Metal"/"CPU") — loaded with Settings, else null. */
  embedDevice: string | null;
  /** A slow op is in flight. */
  loading: boolean;
  /**
   * A reindex is in flight. Kept **separate** from `loading` so a reindex does NOT
   * freeze the app (async-indexing.md §2) — only the Reindex action is disabled and a
   * progress + Cancel affordance appears, while reading/searching/navigating stay live.
   */
  reindexing: boolean;
  /** The latest per-batch progress event, or null before embedding starts (or when idle). */
  reindexProgress: ReindexProgress | null;
  /** The user hit Cancel; the request is in flight (disables Cancel, shows "Cancelling…"). */
  reindexCancelling: boolean;
  /** A transient toast message (success or a generic, actionable error). */
  status: string | null;
}

export const state: AppState = {
  vaultRoot: null,
  semantic: true,
  notes: [],
  resources: [],
  expandedDirs: new Set<string>(),
  current: null,
  currentResource: null,
  frontmatterOpen: false,
  sourceOpen: false,
  editing: false,
  editConflict: false,
  similar: [],
  connections: [],
  discoveringSimilar: false,
  discoveringConnections: false,
  searchQuery: "",
  searchResults: [],
  linkTarget: null,
  linkRelation: "references",
  settingsOpen: false,
  models: [],
  embedStats: [],
  provisioning: false,
  modelsDir: null,
  embedDevice: null,
  loading: false,
  reindexing: false,
  reindexProgress: null,
  reindexCancelling: false,
  status: null,
};
