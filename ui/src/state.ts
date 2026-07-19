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
  ResourceLink,
  ResourceSummary,
  SearchResult,
  SimilarView,
  UnresolvedLink,
} from "./types";
import type { GraphLens } from "./graph";

/** Side-pane discovery sections that can be collapsed (foldable headers). */
export type SideSection = "similar" | "connections";

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

/**
 * An open right-click menu, anchored at the cursor (viewport coords, already
 * clamped on-screen when opened). Null when no menu is up. Two surfaces share the
 * one overlay: a discovery **card** (Open / Link… — the whole card is the target,
 * replacing the old inline "Link…" button) and the file **tree** (New note / New
 * folder, targeting the folder under the cursor).
 */
export type ContextMenuState =
  | { kind: "card"; x: number; y: number; path: string; title: string | null }
  | { kind: "tree"; x: number; y: number; dir: string };

/**
 * Appearance preference. `"system"` (the default) defers to the OS via
 * `prefers-color-scheme`; `"light"`/`"dark"` pin the theme regardless. A pure
 * front-end preference persisted in `localStorage` — it's a viewing choice, not
 * vault state, so it never round-trips to the Rust host.
 */
export type ThemePref = "system" | "light" | "dark";

export interface AppState {
  /** Vault root, or null when none is configured (the app shows an actionable state). */
  vaultRoot: string | null;
  /** Whether the real model is installed — drives the "run `b2 init`" search caveat. */
  semantic: boolean;
  /** Notes with a full set of vectors (#26): the "N/M embedded" numerator. */
  notesEmbedded: number;
  /** Every projected note — the "N/M embedded" denominator (0 before the first index). */
  notesTotal: number;
  /** Every indexed note, path-ordered — the file tree's source (from `list_notes`). */
  notes: NoteSummary[];
  /** Every inventoried non-`.md` file — the tree's resource half (slice 1). */
  resources: ResourceSummary[];
  /** Folder paths (vault-relative, no trailing slash) the tree shows expanded. */
  expandedDirs: Set<string>;
  /**
   * The tree's creation context — the folder a new note/folder lands in (⌘N, the
   * tree-head icons). Follows the selection: the open document's folder, or the
   * last folder row clicked/right-clicked. "" is the vault root (the default).
   */
  selectedDir: string;
  /**
   * Staged folders (session-scoped, cleared on vault switch): created in the UI
   * but still empty, so the index-derived tree can't list them and B2 writes no
   * empty dir to disk (nothing durable outside the Markdown). Each materializes
   * for real when its first note is created inside it — `create_note` creates
   * missing parent dirs, exactly like `b2 add`.
   */
  pendingDirs: Set<string>;
  /** An inline name input open in the tree (new note / new folder in `dir`), or null. */
  treeCreate: { kind: "note" | "folder"; dir: string } | null;
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
  /** The open note's outbound resource links (from the same explain, GH #22). */
  resourceLinks: ResourceLink[];
  /**
   * The center pane shows the anchored ghost graph instead of the reading view
   * (GH #22). Sticky across notes like `sourceOpen`, so the vault can be *browsed*
   * in graph mode — a node click re-anchors the graph on the opened note. Renders
   * purely from the discovery state above (`connections`/`resourceLinks`/
   * `unresolved`/`similar`), so toggling costs no IPC.
   */
  graphOpen: boolean;
  /** The graph's typed lens: "all" (ghost graph), "lineage", or "argument". Sticky. */
  graphLens: GraphLens;
  /**
   * Discovery sections the user has collapsed (foldable headers, Obsidian-style).
   * Sticky across notes — a viewing preference — so a collapsed section stays folded
   * as you browse. Empty ⇒ every section expanded (the default).
   */
  collapsedSections: Set<SideSection>;
  /**
   * Per-card fold state: the card keys (`"<section>:<path>"`) whose body (path +
   * snippet) is collapsed to just the title row. Cards default expanded; this tracks
   * the exceptions. Reset on note-open — the keys belong to the note just closed.
   */
  collapsedCards: Set<string>;
  /** An open right-click menu on a discovery card, or null. */
  contextMenu: ContextMenuState | null;
  /**
   * The open note's unresolved (dangling) outbound links — a `[[folder]]` or a typo
   * that resolves to no note or file. Loaded alongside `connections` from the same
   * `explain` read; rendered with a broken-link emblem so they read as broken, not
   * missing (GH #12).
   */
  unresolved: UnresolvedLink[];
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
  /** Appearance preference (System/Light/Dark) — mirrors `localStorage`, shown in Settings. */
  theme: ThemePref;
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
  notesEmbedded: 0,
  notesTotal: 0,
  notes: [],
  resources: [],
  expandedDirs: new Set<string>(),
  selectedDir: "",
  pendingDirs: new Set<string>(),
  treeCreate: null,
  current: null,
  currentResource: null,
  frontmatterOpen: false,
  sourceOpen: false,
  editing: false,
  editConflict: false,
  similar: [],
  connections: [],
  resourceLinks: [],
  graphOpen: false,
  graphLens: "all",
  collapsedSections: new Set<SideSection>(),
  collapsedCards: new Set<string>(),
  contextMenu: null,
  unresolved: [],
  discoveringSimilar: false,
  discoveringConnections: false,
  searchQuery: "",
  searchResults: [],
  linkTarget: null,
  linkRelation: "references",
  settingsOpen: false,
  theme: "system",
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
