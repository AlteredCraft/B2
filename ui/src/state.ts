// The app's state: a single mutable object the view renders from. Actions (main.ts)
// mutate it and call the render hook.

import type { ChatMessage } from "./chat";
import type {
  ChatSetup,
  EmbedStat,
  ModelChoice,
  NeighborView,
  NoteSummary,
  NoteView,
  ReindexProgress,
  ResourceExplainView,
  ResourceLink,
  ResourceSummary,
  EvidencedResult,
  SimilarExplainView,
  SimilarView,
  UnresolvedLink,
} from "./types";
import type { NodeKind } from "./move";
// `.ts` because it is a value import reached by node's test runner (via render.ts).
import { DEFAULT_SETTINGS_TAB, type SettingsTabId } from "./settingstabs.ts";
import type { BindingId } from "./bindings";
import type { ChordProblem, Overrides } from "./keymap";

/** Side-pane discovery sections that can be collapsed (foldable headers). */
export type SideSection = "similar" | "connections";

/**
 * The closed stance core (b2-core `relation.rs` CORE, data-model.md §2). The host
 * re-validates `is_core`, so a drifted entry here is refused, never stored. `references`
 * is the default, matching `b2 link`.
 */
export const RELATION_VERBS = ["references", "supports", "contradicts"] as const;

/** The note the link modal targets (the source is always the open note). */
export interface LinkTarget {
  path: string;
  title: string | null;
}

/** The tree node (note, resource or folder) a move/rename gesture targets. */
export interface TreeNodeRef {
  path: string;
  nodeKind: NodeKind;
  label: string;
}

/**
 * An open right-click menu at viewport coords (clamped on-screen). On a discovery card,
 * or on the file tree: `dir` is the folder under the cursor and `node` the row, if any.
 */
export type ContextMenuState =
  | { kind: "card"; x: number; y: number; path: string; title: string | null }
  | { kind: "tree"; x: number; y: number; dir: string; node: TreeNodeRef | null };

/**
 * Appearance preference; `"system"` defers to `prefers-color-scheme`. Persisted in
 * `localStorage`: a viewing choice, not vault state.
 */
export type ThemePref = "system" | "light" | "dark";

/**
 * The chord recorder in Settings → Keyboard, while it is open (#121). A chord that never
 * arrives is itself a signal (recorder.ts), reported in `hint`.
 */
export interface RecorderState {
  /** The command being rebound. */
  id: BindingId;
  /** The chord captured so far, in the registry's syntax, or null while waiting. */
  candidate: string | null;
  /** What binding `candidate` to `id` would mean (keymap.ts `chordProblems`). */
  problems: ChordProblem[];
  /** The probe's reading of silence, or why a pressed key can't hold a chord. */
  hint: string | null;
  /**
   * The window lost focus while this recording was waiting (recorder.ts). State, not a
   * parameter, so the blur and the open-timer reach the same reading in either order
   * (GH #125).
   */
  blurred: boolean;
}

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
  /** Every folder in the vault, empty ones included (`list_dirs`, a live fs walk). */
  dirs: string[];
  /** Folder paths (vault-relative, no trailing slash) the tree shows expanded. */
  expandedDirs: Set<string>;
  /**
   * The folder a new note/folder lands in: the open document's folder, or the last
   * folder row clicked. "" is the vault root.
   */
  selectedDir: string;
  /**
   * The tree row the keyboard is on, or null before any arrow key. Distinct from
   * `current`: arrowing doesn't open. Drives the roving `tabindex` (K1, GH #78).
   */
  treeFocus: string | null;
  /**
   * `treeFocus` for the right column (a `sidenav.ts` row key). Also what `paintSide`
   * restores focus by, since the focused element doesn't survive the `innerHTML` swap.
   */
  sideFocus: string | null;
  /** An inline name input open in the tree (new note / new folder in `dir`), or null. */
  treeCreate: { kind: "note" | "folder"; dir: string } | null;
  /** An inline rename input open on a tree row, or null. */
  treeRename: TreeNodeRef | null;
  /** When set, the Move… modal is open for this tree node. */
  moveTarget: TreeNodeRef | null;
  /** When set, the delete-confirm modal is open. Only folders confirm; files don't. */
  deleteTarget: TreeNodeRef | null;
  /** The note the centre pane shows, or null (nothing open yet, or a resource is). */
  current: NoteView | null;
  /** The selected resource's card (mutually exclusive with `current`). */
  currentResource: ResourceExplainView | null;
  /**
   * The open resource's bytes as a `data:` URL when it has an in-app viewer, else null.
   * Cleared with `currentResource`, so one document's picture never paints over another.
   */
  resourceImage: string | null;
  /**
   * The open note's pictures: path → `data:` URL for each embed read (embeds.ts
   * `inlineImagePlan`). Shared by the reading view and live preview; an embed with no
   * entry reads as its link. Cleared when the document changes.
   */
  embedImages: Map<string, string>;
  /** Whether the note pane's frontmatter drawer is expanded (sticky across notes). */
  frontmatterOpen: boolean;
  /**
   * The frontmatter mini-editor is live (GH #79): `render()` must not rebuild the note
   * pane (as for `editing`), and pane-changing actions resolve the edit first
   * (`fmEditGuard`, main.ts). The buffer itself is the textarea's DOM value.
   */
  fmEditing: boolean;
  /** Whether the note body shows raw Markdown source instead of rendered (sticky). */
  sourceOpen: boolean;
  /**
   * Edit mode: the note pane belongs to CodeMirror and `render()` must not rebuild it
   * (crates/b2-desktop/CLAUDE.md). Timers, save flags and the EditorView live in main.ts.
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
   * The centre pane shows the ghost graph instead of the reading view (GH #22). Sticky
   * across notes; renders from the discovery state, so toggling costs no IPC.
   */
  graphOpen: boolean;
  /**
   * The Explain view (GH #236): the open note compared with one Similar card. `view` is
   * null while loading. Not sticky: navigation, the graph toggle and edit mode close it.
   */
  explainCard: {
    anchor: string;
    candidate: string;
    view: SimilarExplainView | null;
    error: string | null;
    /** Every passage pair shown, not only the first few. */
    allPairs: boolean;
    /** The strip's "?" is open: its longer account is shown. */
    help: boolean;
  } | null;
  /** Discovery sections the user has collapsed. Sticky across notes. */
  collapsedSections: Set<SideSection>;
  /** Card keys (`"<section>:<path>"`) collapsed to their title row. Reset on note-open. */
  collapsedCards: Set<string>;
  /** An open right-click menu, or null. */
  contextMenu: ContextMenuState | null;
  /** The open note's links that resolve to nothing, shown as broken (GH #12). */
  unresolved: UnresolvedLink[];
  /**
   * Discovery reads in flight, per side-pane section, so the fast `explain` read paints
   * without waiting on `similar`. Separate from `loading` so the note body paints at once.
   */
  discoveringSimilar: boolean;
  discoveringConnections: boolean;
  /**
   * The right column shows chat (GH #155), so a citation opens in the centre pane without
   * the conversation leaving the screen. Chat and search close each other (main.ts).
   */
  chatOpen: boolean;
  /**
   * The conversation, session-only (S4): never persisted, not even to `localStorage`,
   * since a saved transcript would be B2-derived state outside the Markdown.
   */
  chatMessages: ChatMessage[];
  /**
   * The answer streaming now, or null between turns. Rendered as text, never parsed; the
   * finished answer goes through `renderMarkdown` (E5). Painted by `paintChatStream`
   * without a full render, so the composer keeps its caret.
   */
  chatStreaming: string | null;
  /** What the live row says until the first token, so tool-running time doesn't look hung. */
  chatWaiting: string;
  /** The chat provider's status. Null until the first probe lands. */
  chatSetup: ChatSetup | null;
  /** Settings → Chat shows the Cloud fields. A view flag seeded from `chatSetup.cloud`. */
  chatCloud: boolean;
  /**
   * Settings → Chat shows Model as a text box rather than the installed-model picker, so
   * a user can name a model not installed yet (e.g. mid `ollama pull`). A view flag.
   */
  chatModelTyped: boolean;
  /** The active search query (empty ⇒ the side pane shows discovery, not results). */
  searchQuery: string;
  /**
   * The rows the search pane serves. Emptied in `doSearch` for an unvouched query, so the
   * paint and the arrow walk derive from one list (D2, GH #202).
   */
  searchResults: EvidencedResult[];
  /**
   * D2's verdict: `false` = "no matches", `null` = no calibrated bar for this model, so no
   * verdict (`SearchEvidenceView`).
   */
  searchVouched: boolean | null;
  /** When set, the link modal is open for this target. */
  linkTarget: LinkTarget | null;
  /** The verb selected in the link modal. */
  linkRelation: string;
  /** Settings is open, as a full-window surface (settingsview.ts). */
  settingsOpen: boolean;
  /** The settings section showing (settingstabs.ts). Outlives a close; `?` forces "keyboard". */
  settingsTab: SettingsTabId;
  /** Appearance preference — mirrors `localStorage`. */
  theme: ThemePref;
  /**
   * The user's keyboard rebindings (#121): command id → chords. Mirrors `localStorage`;
   * held in state because the Keyboard section renders from it.
   */
  keyOverrides: Overrides;
  /** The chord recorder, while Settings → Keyboard has one open. */
  recorder: RecorderState | null;
  /**
   * The ⌘-hold sheet is up (cmdhold.ts). Transient, never persisted; in state so render.ts
   * can own its markup (main.ts `paintCmdSheet`).
   */
  cmdSheet: boolean;
  /** The embedding models offered in Settings — loaded when the modal opens, else empty. */
  models: ModelChoice[];
  /** Per-model cumulative embedding time — loaded alongside `models`, shown in Settings. */
  embedStats: EmbedStat[];
  /** A model download (in-app `b2 init`) is in flight — disables the button, shows a spinner. */
  provisioning: boolean;
  /**
   * The "install the model" banner was dismissed, for this session (✕) or for good (the
   * checkbox, persisted to `localStorage`). See `embedreminder.ts`.
   */
  embedReminderDismissed: boolean;
  /** The shared directory where model files are saved — loaded with Settings, else null. */
  modelsDir: string | null;
  /** Compute device the embedder runs on ("Metal"/"CPU") — loaded with Settings, else null. */
  embedDevice: string | null;
  /** A slow op is in flight. */
  loading: boolean;
  /** A reindex is in flight. Separate from `loading` so it doesn't freeze the app. */
  reindexing: boolean;
  /** The latest per-batch progress event, or null before embedding starts (or when idle). */
  reindexProgress: ReindexProgress | null;
  /** The user hit Cancel; the request is in flight (disables Cancel, shows "Cancelling…"). */
  reindexCancelling: boolean;
  /** A transient toast message (success or a generic, actionable error). */
  status: string | null;
}

/** The path of the document the note pane shows — a note or a resource — or null. */
export function openDocPath(s: AppState): string | null {
  return s.current?.path ?? s.currentResource?.path ?? null;
}

export const state: AppState = {
  vaultRoot: null,
  semantic: true,
  notesEmbedded: 0,
  notesTotal: 0,
  notes: [],
  resources: [],
  dirs: [],
  expandedDirs: new Set<string>(),
  selectedDir: "",
  treeFocus: null,
  sideFocus: null,
  treeCreate: null,
  treeRename: null,
  moveTarget: null,
  deleteTarget: null,
  current: null,
  currentResource: null,
  resourceImage: null,
  embedImages: new Map<string, string>(),
  frontmatterOpen: false,
  fmEditing: false,
  sourceOpen: false,
  editing: false,
  editConflict: false,
  similar: [],
  connections: [],
  resourceLinks: [],
  graphOpen: false,
  explainCard: null,
  collapsedSections: new Set<SideSection>(),
  collapsedCards: new Set<string>(),
  contextMenu: null,
  unresolved: [],
  discoveringSimilar: false,
  discoveringConnections: false,
  chatOpen: false,
  chatMessages: [],
  chatStreaming: null,
  chatWaiting: "",
  chatSetup: null,
  chatCloud: false,
  chatModelTyped: false,
  searchQuery: "",
  searchResults: [],
  searchVouched: null,
  linkTarget: null,
  linkRelation: "references",
  settingsOpen: false,
  settingsTab: DEFAULT_SETTINGS_TAB,
  theme: "system",
  keyOverrides: {},
  recorder: null,
  cmdSheet: false,
  models: [],
  embedStats: [],
  provisioning: false,
  embedReminderDismissed: false,
  modelsDir: null,
  embedDevice: null,
  loading: false,
  reindexing: false,
  reindexProgress: null,
  reindexCancelling: false,
  status: null,
};
