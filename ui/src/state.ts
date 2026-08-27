// The app's state — a single mutable object the view renders from. No framework:
// actions (in main.ts) mutate this and call the render hook. Small enough that a
// full-pane re-render on change is imperceptible and keeps the model honest.

import type { ChatMessage } from "./chat";
import type {
  ChatSetup,
  EmbedStat,
  MenuChord,
  ModelChoice,
  NeighborView,
  NoteSummary,
  NoteView,
  ReindexProgress,
  ResourceExplainView,
  ResourceLink,
  ResourceSummary,
  EvidencedResult,
  SimilarView,
  UnresolvedLink,
} from "./types";
import type { NodeKind } from "./move";
// Carries its `.ts` because it is a *value* import (the others here are type-only, and
// erase): render.test.ts / sanitize.test.ts reach this module through render.ts under
// node's type-stripping, which resolves by real filename. render.ts says the same.
import { DEFAULT_SETTINGS_TAB, type SettingsTabId } from "./settingstabs.ts";
import type { BindingId } from "./bindings";
import type { ChordProblem, Overrides } from "./keymap";

/** Side-pane discovery sections that can be collapsed (foldable headers). */
export type SideSection = "similar" | "connections";

/**
 * The closed three-verb stance core (b2-core `relation.rs` CORE — data-model.md §2):
 * neutral / for / against. The link picker offers exactly these; the Rust host
 * re-validates `is_core`, so a drifted entry here is *refused*, never silently
 * stored (a bad verb → a generic, actionable error). `references` is the default,
 * matching `b2 link`.
 */
export const RELATION_VERBS = ["references", "supports", "contradicts"] as const;

/** The note the link modal targets (the source is always the open note). */
export interface LinkTarget {
  path: string;
  title: string | null;
}

/**
 * The tree node a move/rename gesture targets — a note, resource, or folder row,
 * identified by its vault-relative path (the tree's DOM identity).
 */
export interface TreeNodeRef {
  path: string;
  nodeKind: NodeKind;
  label: string;
}

/**
 * An open right-click menu, anchored at the cursor (viewport coords, already
 * clamped on-screen when opened). Null when no menu is up. Two surfaces share the
 * one overlay: a discovery **card** (Open / Link… — the whole card is the target,
 * replacing the old inline "Link…" button) and the file **tree** (New note / New
 * folder, targeting the folder under the cursor; over a concrete row, `node` is
 * that row and the menu grows Rename / Move…).
 */
export type ContextMenuState =
  | { kind: "card"; x: number; y: number; path: string; title: string | null }
  | { kind: "tree"; x: number; y: number; dir: string; node: TreeNodeRef | null };

/**
 * Appearance preference. `"system"` (the default) defers to the OS via
 * `prefers-color-scheme`; `"light"`/`"dark"` pin the theme regardless. A pure
 * front-end preference persisted in `localStorage` — it's a viewing choice, not
 * vault state, so it never round-trips to the Rust host.
 */
export type ThemePref = "system" | "light" | "dark";

/**
 * The chord recorder in Settings → Keyboard, while it is open (#121).
 *
 * `candidate` is null until a chord arrives, and that gap is a state worth modelling
 * rather than a loading spinner: a chord that never arrives is the recorder's one real
 * observation about the world outside B2 (recorder.ts's header), and `hint` is where that
 * reading goes. Once a chord *has* arrived, `problems` is what the four checkers make of
 * it — a refusal disables Save, an advisory is said and saved anyway.
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
   * The window lost focus while this recording was waiting — the probe's one *positive*
   * signal (recorder.ts).
   *
   * Remembered rather than passed at the moment it happens, because two things read
   * silence: the blur itself, and a timer set when the recorder opened. Held as a
   * parameter, the later of the two would answer the question without knowing what the
   * earlier one saw — which is exactly how the strong "something outside B2 answered"
   * reading got overwritten by the weaker guess (GH #125). As state, every reader reaches
   * the same conclusion whatever order they run in.
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
  /**
   * Every folder in the vault, empty ones included — the tree's structure half
   * (from `list_dirs`, a live filesystem walk). Folders are user-authored
   * structure with the fs authoritative, so this list is one-to-one with disk:
   * a Finder `mkdir` or a folder emptied by a move shows exactly as it is.
   */
  dirs: string[];
  /** Folder paths (vault-relative, no trailing slash) the tree shows expanded. */
  expandedDirs: Set<string>;
  /**
   * The tree's creation context — the folder a new note/folder lands in (⌘N, the
   * tree-head icons). Follows the selection: the open document's folder, or the
   * last folder row clicked/right-clicked. "" is the vault root (the default).
   */
  selectedDir: string;
  /**
   * The tree row the keyboard is on (its vault-relative path), or null before any
   * arrow key has been pressed. Distinct from `selectedDir` (the *create* context)
   * and from `current` (the *open* document): a keyboard user arrows across rows
   * without opening them, which is the whole point of arrow navigation. Drives the
   * roving `tabindex` (treenav.ts `rovingPath`), so the tree is one Tab stop rather
   * than one per file — invariant K1, GH #78.
   */
  treeFocus: string | null;
  /**
   * The discovery row the keyboard is on (a `sidenav.ts` row key), or null before any
   * arrow key has been pressed there — `treeFocus`'s counterpart for the right column.
   * Drives that pane's roving `tabindex` (`rovingSideKey`), so the whole card list is one
   * Tab stop rather than three per card, and it is what `paintSide` restores focus *by*:
   * the element holding focus never survives the pane's `innerHTML` swap.
   */
  sideFocus: string | null;
  /** An inline name input open in the tree (new note / new folder in `dir`), or null. */
  treeCreate: { kind: "note" | "folder"; dir: string } | null;
  /** An inline rename input open on a tree row, or null. */
  treeRename: TreeNodeRef | null;
  /** When set, the Move… modal is open for this tree node. */
  moveTarget: TreeNodeRef | null;
  /**
   * When set, the delete-confirm modal is open for this tree node. Only folders
   * land here (a subtree is a bigger loss); files delete without a dialog — the
   * gesture itself is the intent.
   */
  deleteTarget: TreeNodeRef | null;
  /** The open note (left pane), or null before one is opened. */
  current: NoteView | null;
  /**
   * The selected resource's fallback card (mutually exclusive with `current`:
   * selecting either kind clears the other — the note pane shows one document).
   */
  currentResource: ResourceExplainView | null;
  /**
   * The open resource's bytes as a `data:` URL, when its class has an in-app viewer and
   * the read succeeded — otherwise null, and the card shows its *Open in system default*
   * fallback. Loaded alongside `currentResource` and cleared with it, so the pane can
   * never paint one document's picture over another's card.
   */
  resourceImage: string | null;
  /** Whether the note pane's frontmatter drawer is expanded (sticky across notes). */
  frontmatterOpen: boolean;
  /**
   * The drawer's frontmatter mini-editor is live (GH #79): the note pane belongs
   * to it — `render()` must NOT rebuild the pane (the same carve-out as
   * `editing`), and pane-changing actions (navigation, the view toggles) resolve
   * the edit first (`fmEditGuard` in main.ts). Only this renderable flag lives
   * here; the buffer is the textarea's DOM value, and the inline error is painted
   * imperatively — both die with the editor.
   */
  fmEditing: boolean;
  /** Whether the note body shows raw Markdown source instead of rendered (sticky). */
  sourceOpen: boolean;
  /**
   * Edit mode: the note pane belongs to the live CodeMirror editor, and `render()`
   * must NOT rebuild it (the carve-out, crates/b2-desktop/CLAUDE.md) — everything else
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
  /**
   * The right column is showing **chat** instead of discovery or search results
   * (GH #155). Chat lives there so a citation can open its note in the centre pane
   * *without the conversation leaving the screen* — see chat.ts's header. It owns the
   * whole column, so opening chat and running a search close each other (main.ts).
   */
  chatOpen: boolean;
  /**
   * The conversation — **session-only** (invariant S4): it lives here and dies with the
   * window. Never persisted, not even to `localStorage` where the theme and the keymap
   * live: a saved transcript would be B2-derived state outside the Markdown.
   */
  chatMessages: ChatMessage[];
  /**
   * The answer streaming right now, as it accumulates, or null between turns. Rendered
   * as **text**, never parsed — a partial answer is not a document, and the finished one
   * goes through the sanitizing `renderMarkdown` seam like every other untrusted string
   * (E5). Tokens land here without a full render (`paintChatStream`), so the composer
   * keeps its caret and the pane keeps its scroll while an answer arrives.
   */
  chatStreaming: string | null;
  /** The chat provider's status — endpoint, model, Local vs Cloud, and the Ollama-native
   *  setup card's data. Null until the first probe lands (the "loading" empty state). */
  chatSetup: ChatSetup | null;
  /**
   * The Settings → Chat section is showing the **Cloud models** fields. A pure view flag,
   * initialized from `chatSetup.cloud`: the configuration itself is just the endpoint, so
   * this decides which fields (and which privacy copy) are on screen while the user types,
   * with nothing to keep in sync afterwards.
   */
  chatCloud: boolean;
  /**
   * Settings → Chat is showing the **Model** field as a text box rather than the picker.
   *
   * The picker exists only when there is an inventory to pick from (a local Ollama daemon
   * that answered `/api/tags`), so this flag is what lets a user name a model that *isn't*
   * installed yet — the one thing a list of installed models structurally cannot offer,
   * and exactly what you do while a `ollama pull` is still running. A view flag like
   * [`chatCloud`]: the configuration is just a model string either way.
   */
  chatModelTyped: boolean;
  /** The active search query (empty ⇒ the side pane shows discovery, not results). */
  searchQuery: string;
  /**
   * The rows the search pane **serves** — which is not always every row the host
   * returned. On an unvouched query (`searchVouched === false`) this is emptied at
   * the boundary in `doSearch`, so the paint and the arrow walk cannot disagree
   * about what is on screen: `render.ts` and `sidenav.ts` both derive from this one
   * list, exactly as they do for every other pane (invariants.md D2, GH #202).
   */
  searchResults: EvidencedResult[];
  /**
   * D2's verdict for the current query — three-state, and each state is different
   * copy in the empty branch (`false` = "no matches", `null` = no calibrated bar
   * for this model, so no verdict was offered at all; see `SearchEvidenceView`).
   */
  searchVouched: boolean | null;
  /** When set, the link modal is open for this target. */
  linkTarget: LinkTarget | null;
  /** The verb selected in the link modal. */
  linkRelation: string;
  /** The settings modal (⌘,) is open. */
  settingsOpen: boolean;
  /**
   * Which section of the settings dialog is showing (settingstabs.ts). Outlives a
   * close, so ⌘, comes back where you left it — a dialog that resets to page one every
   * time is a dialog you re-navigate on every visit. `?` overrides it to "keyboard",
   * which is the chord's whole meaning.
   */
  settingsTab: SettingsTabId;
  /** Appearance preference (System/Light/Dark) — mirrors `localStorage`, shown in Settings. */
  theme: ThemePref;
  /**
   * The user's keyboard rebindings (#121): command id → the chords that now fire it.
   * Mirrors `localStorage` — a viewing choice like the theme, never vault state — and is
   * what `applyOverrides` lays over the shipped table to build the live registry.
   *
   * Held in state rather than module-locally (the way panes.ts holds column widths)
   * because the Keyboard section renders from it: which rows are marked changed, and
   * whether "Reset all" has anything to do.
   */
  keyOverrides: Overrides;
  /** The chord recorder, while Settings → Keyboard has one open. Null the rest of the time. */
  recorder: RecorderState | null;
  /** The embedding models offered in Settings — loaded when the modal opens, else empty. */
  models: ModelChoice[];
  /** Per-model cumulative embedding time — loaded alongside `models`, shown in Settings. */
  embedStats: EmbedStat[];
  /** A model download (in-app `b2 init`) is in flight — disables the button, shows a spinner. */
  provisioning: boolean;
  /**
   * The "semantic search is off — install the model" banner has been dismissed. Set by
   * the banner's ✕ (this session only) or its "Don't remind me again" checkbox (also
   * persisted to `localStorage`, so a keyword-only user stays opted out across launches).
   * Initialized from that persisted flag on boot; see `embedreminder.ts` for the gate.
   */
  embedReminderDismissed: boolean;
  /** The shared directory where model files are saved — loaded with Settings, else null. */
  modelsDir: string | null;
  /** Compute device the embedder runs on ("Metal"/"CPU") — loaded with Settings, else null. */
  embedDevice: string | null;
  /**
   * The app menu bar's chords, as the **host** declares them (b2-desktop `menu.rs`, #119)
   * — the keyboard reference's last group. Null until the boot fetch lands, and the sheet
   * falls back to `menukeys.ts`'s mirror for that window. Fetched once: a menu is
   * compiled-in data, not something that changes under the app.
   */
  menuChords: MenuChord[] | null;
  /** A slow op is in flight. */
  loading: boolean;
  /**
   * A reindex is in flight. Kept **separate** from `loading` so a reindex does NOT
   * freeze the app (docs/index-engine.md) — only the Reindex action is disabled and a
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
  frontmatterOpen: false,
  fmEditing: false,
  sourceOpen: false,
  editing: false,
  editConflict: false,
  similar: [],
  connections: [],
  resourceLinks: [],
  graphOpen: false,
  collapsedSections: new Set<SideSection>(),
  collapsedCards: new Set<string>(),
  contextMenu: null,
  unresolved: [],
  discoveringSimilar: false,
  discoveringConnections: false,
  chatOpen: false,
  chatMessages: [],
  chatStreaming: null,
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
  models: [],
  embedStats: [],
  provisioning: false,
  embedReminderDismissed: false,
  modelsDir: null,
  embedDevice: null,
  menuChords: null,
  loading: false,
  reindexing: false,
  reindexProgress: null,
  reindexCancelling: false,
  status: null,
};
