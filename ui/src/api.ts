// THE ONE IPC SEAM (crates/b2-desktop/CLAUDE.md). Every `invoke()` in the frontend
// lives here — the presentation-side mirror of the `Vault` façade. Keeping it in one
// module means the rest of the UI never imports Tauri directly: it can be unit-tested
// by mocking this module, and a future `serve`/HTTP transport swap touches ~this file
// only. Do not call `invoke` anywhere else.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AddReport,
  AnswerView,
  ChatSetup,
  ChatTurn,
  DeleteReport,
  DirCreateReport,
  DirDeleteReport,
  DirMoveReport,
  EmbedReport,
  EmbedStat,
  ExplainView,
  ImportReport,
  LinkReport,
  MenuChord,
  ModelChoice,
  MoveReport,
  NeighborView,
  NoteSummary,
  NoteView,
  ProjectReport,
  ReindexProgress,
  ResourceDeleteReport,
  ResourceExplainView,
  ResourceMoveReport,
  ResourceSummary,
  SearchEvidenceView,
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
 * (crates/b2-desktop/CLAUDE.md): the frontend recognizes a save conflict by matching this
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
 * The host's filesystem-watch pulse (crates/b2-desktop/CLAUDE.md / #14): the Rust watcher emits
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

  /** A note's body + metadata for the left pane, by vault-relative path. */
  readNote: (note: string): Promise<NoteView> => invoke("read_note", { note }),

  /** Every indexed note (path, title; no body) — the file tree's source. */
  listNotes: (): Promise<NoteSummary[]> => invoke("list_notes"),

  /** Every inventoried non-`.md` file — the tree's resource half (slice 1). */
  listResources: (): Promise<ResourceSummary[]> => invoke("list_resources"),

  /** Every folder in the vault, empty ones included — the tree's structure half,
   *  read live off the filesystem (never the index) so the tree is one-to-one
   *  with disk in both directions. */
  listDirs: (): Promise<string[]> => invoke("list_dirs"),

  /** The fallback card's data: a resource's metadata + backlinks. */
  explainResource: (path: string): Promise<ResourceExplainView> =>
    invoke("explain_resource", { path }),

  /**
   * *Open in system default* — an OS handoff performed host-side (the webview holds
   * no opener permission); the host validates the path against the inventory first.
   */
  openResource: (path: string): Promise<void> => invoke("open_resource", { path }),

  /**
   * *Open a web link in the system default browser* — `openResource`'s sibling for the
   * links **inside** a note. The same OS handoff, and host-side for the same reason: the
   * webview holds no opener permission, and following the link in place would replace
   * the whole app with a web page. The host re-checks the URL's scheme against its own
   * allow-list (http / https / mailto), so `ui/src/links.ts` decides what gets *routed*
   * here and the host decides what actually opens.
   */
  openExternal: (url: string): Promise<void> => invoke("open_external", { url }),

  /**
   * The clipboard's plain-text flavor, for the editor's ⌘⇧V (paste as plain text).
   * Host-side of necessity: WebKit runs no editing command for a raw ⌘⇧V and gates a
   * programmatic `navigator.clipboard` read behind a native confirmation.
   */
  clipboardText: (): Promise<string> => invoke("clipboard_text"),

  /** Semantically-near, not-yet-linked candidates for a note — the ranked
   *  nearest, always served (GH #197). */
  similar: (note: string, limit = 10): Promise<SimilarView[]> =>
    invoke("similar", { note, limit }),

  /** Hybrid keyword+semantic search across the vault, **with its evidence
   *  reading** (invariants.md D2, GH #202): the same rows in the same order, plus
   *  the query-level verdict that says whether the vault holds evidence for the
   *  query at all. The host returns the view whole — what the pane *shows* on
   *  each of `vouched`'s three states is `doSearch`'s call, in `main.ts`. */
  search: (query: string, limit = 20): Promise<SearchEvidenceView> =>
    invoke("search", { query, limit }),

  /**
   * Save a note's body — Markdown-first through `Vault::write`
   * (crates/b2-desktop/CLAUDE.md): a byte-honest body splice guarded by the `revision` captured at read, then
   * a model-free re-projection. Rejects with `WRITE_CONFLICT_MESSAGE` when the file
   * changed on disk since. (Tauri v2 maps camelCase keys to the command's snake_case
   * params — `baseRevision` → `base_revision` — so no hand-written snake_case here.)
   */
  writeNote: (note: string, body: string, baseRevision: string): Promise<WriteReport> =>
    invoke("write_note", { note, body, baseRevision }),

  /**
   * Save a note's frontmatter — `writeNote`'s frontmatter sibling (GH #79):
   * the raw YAML is spliced verbatim between the fences (`Vault::write_frontmatter`),
   * body untouched, model-free, guarded by the same `revision` contract (rejects
   * with `WRITE_CONFLICT_MESSAGE` on an external change). The host's one refusal is
   * a `---` line that would end the block early and shift bytes into the body; it
   * comes back as an actionable message to show inline. Everything else — including
   * YAML B2 can't read — saves fine: B2 owns no line inside the block.
   */
  writeFrontmatter: (
    note: string,
    frontmatter: string,
    baseRevision: string,
  ): Promise<WriteReport> => invoke("write_frontmatter", { note, frontmatter, baseRevision }),

  /**
   * Create a new, empty note at a vault-relative path (`.md` optional; missing
   * parent folders are created, like `b2 add`). Model-free like `writeNote`: the
   * note is projected immediately (tree/search/graph) and its vectors fill on
   * the next embed pass.
   */
  createNote: (path: string): Promise<AddReport> => invoke("create_note", { path }),

  /**
   * Create a folder — a real on-disk create (missing parents included, an
   * occupied target refused). A folder is user-authored vault structure,
   * immediately visible to Finder, the CLI, and any sync; no index rows are
   * touched.
   */
  createDir: (dir: string): Promise<DirCreateReport> => invoke("create_dir", { dir }),

  /**
   * Import a file from outside the vault into the folder `dir` (`""` for the root),
   * from its **bytes** — the file tree's drop target. `data` is base64 (see
   * `importfiles.ts`): a file dropped on the webview arrives as content, not a path,
   * and Tauri's JSON IPC carries no byte array cheaply. Model-free like `createNote`:
   * the file is projected immediately and any vectors fill on the next embed pass.
   */
  importFile: (dir: string, name: string, data: string): Promise<ImportReport> =>
    invoke("import_file", { dir, name, data }),

  /**
   * `importFile` from a path instead of bytes — what the Import files… picker's
   * selections come back as. Same op, and the bytes never cross the IPC.
   */
  importPath: (dir: string, source: string): Promise<ImportReport> =>
    invoke("import_path", { dir, source }),

  /**
   * Open the native multi-select file picker behind Import files… — the keyboard path
   * to the drop gesture (K1). Resolves to the chosen absolute paths for `importPath`
   * to place, or an empty list if the user cancelled. Host-side like `chooseVault`:
   * the webview holds no dialog permission.
   */
  pickImportFiles: (): Promise<string[]> => invoke("pick_import_files"),

  /**
   * Move/rename a note to a new vault-relative path — inbound
   * links are rewritten and the index re-projects. Needs the real model (the
   * rewritten files re-embed), so it can reject with the "run `b2 init`" state.
   */
  moveNote: (note: string, to: string): Promise<MoveReport> =>
    invoke("move_note", { note, to }),

  /** `moveNote`'s resource sibling — same posture, same report shape. */
  moveResource: (path: string, to: string): Promise<ResourceMoveReport> =>
    invoke("move_resource", { path, to }),

  /** Move/rename a whole folder — one rename on disk (unindexed files travel too). */
  moveDir: (from: string, to: string): Promise<DirMoveReport> =>
    invoke("move_dir", { from, to }),

  /**
   * Delete a note from the vault *and* the disk. Model-free —
   * inbound links dangle (surfacing as unresolved), they are never rewritten.
   */
  deleteNote: (note: string): Promise<DeleteReport> => invoke("delete_note", { note }),

  /** `deleteNote`'s resource sibling — same posture, same report shape. */
  deleteResource: (path: string): Promise<ResourceDeleteReport> =>
    invoke("delete_resource", { path }),

  /** Delete a whole folder and everything inside it (unindexed files go too). */
  deleteDir: (dir: string): Promise<DirDeleteReport> => invoke("delete_dir", { dir }),

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
   * (docs/design/index-engine.md): notes + keyword index + graph, writing nothing
   * to the vault. Once it resolves, the tree and keyword search are live; call
   * `embed` to fill the vectors behind it.
   */
  project: (): Promise<ProjectReport> => invoke("project"),

  /**
   * Phase 2 of a reindex — fill the missing vectors (real model) as a cancellable
   * background action. `onProgress` fires per embed batch over a typed Tauri
   * `Channel` (docs/design/index-engine.md), determinate from the first batch; the returned
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

  /**
   * **Flow ④ — one grounded answer** (GH #151/#153/#155): condense → retrieve → assemble
   * → stream → cite, behind `Vault::ask`. `onToken` fires per token over a typed Tauri
   * `Channel` (the `embed` progress precedent) and the Promise resolves with the resolved
   * [`AnswerView`] — the same two facts `b2 ask --json` emits as a JSONL event stream.
   *
   * `history` is the caller's and session-only (S4): the pane holds the conversation and
   * hands it back each turn. Nothing about a chat is stored anywhere.
   */
  ask: (
    question: string,
    history: ChatTurn[],
    onToken: (text: string) => void,
  ): Promise<AnswerView> => {
    const channel = new Channel<string>();
    channel.onmessage = onToken;
    return invoke("ask", { question, history, onEvent: channel });
  },

  /**
   * Stop the streaming answer at its next token — the pane's Esc. Cooperative: the
   * in-flight `ask` resolves normally, with `cancelled` set and the partial text intact,
   * so a stopped answer renders honestly rather than as a failure.
   */
  cancelAsk: (): Promise<void> => invoke("cancel_ask"),

  /**
   * What the chat surface needs before a question is asked: the endpoint and model in
   * force, whether that is the **Local** or the **Cloud models** configuration, and — for
   * an Ollama endpoint — the native inventory behind the setup card (is the daemon up,
   * what is installed, what to pull on a machine this size).
   *
   * Never rejects: "the daemon isn't running" is the answer the card is asking for.
   */
  chatSetup: (): Promise<ChatSetup> => invoke("chat_setup"),

  /**
   * Save the chat configuration and re-probe it. Adapter state, never vault or index
   * state — a chat model swap costs no reindex (contrast M2).
   *
   * `apiKey` is held in memory for the run and, when the platform will take it, in the
   * macOS Keychain — never in a plain file, and never in a way that comes back here: the
   * returned setup carries `api_key_source`, not the key (`b2-desktop/src/keychain.rs`
   * argues both halves). Passing `null` keeps whatever is already in force, so re-saving
   * the endpoint can't silently sign you out; `""` is the explicit *clear*.
   */
  setChatConfig: (
    baseUrl: string | null,
    model: string | null,
    apiKey: string | null,
  ): Promise<ChatSetup> => invoke("set_chat_config", { baseUrl, model, apiKey }),

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

  /** Compute device the embedder runs on for this build — "Metal" or "CPU" (Settings badge). */
  embedDevice: (): Promise<string> => invoke("embed_device"),

  /**
   * Every chord the app's **menu bar** takes (#119) — the host declares the menu
   * (b2-desktop `menu.rs`), so this is the authority on chords the webview never sees:
   * AppKit dispatches a menu key equivalent before the key window's responder chain.
   * The keyboard reference lists them from here, and `menukeys.ts` holds the mirror this
   * is checked against at boot.
   */
  menuChords: (): Promise<MenuChord[]> => invoke("menu_chords"),

  /**
   * Subscribe to the host's debounced filesystem-watch pulse (#14). `handler` fires once
   * per burst of external Markdown changes; the returned promise resolves to an unlisten
   * function (unused here — the subscription lives for the window's lifetime). This is the
   * only `listen` in the app, kept behind the seam like every `invoke`.
   */
  onVaultChanged: (handler: () => void): Promise<UnlistenFn> =>
    listen(VAULT_CHANGED_EVENT, () => handler()),
};
