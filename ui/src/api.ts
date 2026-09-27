// The one IPC seam (crates/b2-desktop/CLAUDE.md): every `invoke()` lives here, so the rest
// of the UI never imports Tauri and can be tested by mocking this module. Do not call
// `invoke` anywhere else.

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
  NoteSummary,
  NoteView,
  ProjectReport,
  ReindexProgress,
  ResourceDeleteReport,
  ResourceExplainView,
  ResourceMoveReport,
  ResourceSummary,
  SearchEvidenceView,
  SimilarExplainView,
  SimilarView,
  VaultInfo,
  WriteReport,
} from "./types";

// A rejected `invoke` carries the host's user-facing string (CmdError's `user_message`).
export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

/**
 * The host's exact `WriteConflict` message, part of the IPC contract. Pinned host-side by
 * `write_conflict_is_generic_and_recognizable` (`b2-desktop/src/commands.rs`); change both.
 */
export const WRITE_CONFLICT_MESSAGE =
  "This note changed on disk since it was opened. Reload the note, then reapply your edit.";

/** Whether an IPC rejection is a stale-revision save. `startsWith`: `B2_DEBUG` appends detail. */
export function isWriteConflict(e: unknown): boolean {
  return errText(e).startsWith(WRITE_CONFLICT_MESSAGE);
}

/**
 * The host's debounced filesystem-watch pulse (#14): the vault's Markdown changed outside the
 * app. No payload; the frontend re-reads to see what changed. Must equal the host's
 * `VAULT_CHANGED_EVENT` (`b2-desktop/src/watch.rs`, pinned by a test there).
 */
export const VAULT_CHANGED_EVENT = "vault-changed";

/**
 * One of B2's own menu lines was chosen; the payload is its id from `menu.rs`
 * (`view.zoom-in`). A menu accelerator never reaches the webview's keydown, so the host
 * forwards it. Must equal the host's `MENU_COMMAND_EVENT`.
 */
export const MENU_COMMAND_EVENT = "menu-command";

export const api = {
  /** The active vault root + whether semantic ranking is live (real model). */
  vaultInfo: (): Promise<VaultInfo> => invoke("vault_info"),

  /** Native folder picker to switch vault; `null` if the user cancelled. */
  chooseVault: (): Promise<VaultInfo | null> => invoke("choose_vault"),

  /** A note's body + metadata for the left pane, by vault-relative path. */
  readNote: (note: string): Promise<NoteView> => invoke("read_note", { note }),

  /** Every indexed note (path, title; no body) — the file tree's source. */
  listNotes: (): Promise<NoteSummary[]> => invoke("list_notes"),

  /** Every inventoried non-`.md` file — the tree's resource half. */
  listResources: (): Promise<ResourceSummary[]> => invoke("list_resources"),

  /** Every folder, empty ones included, read live off disk (never the index). */
  listDirs: (): Promise<string[]> => invoke("list_dirs"),

  /** The fallback card's data: a resource's metadata + backlinks. */
  explainResource: (path: string): Promise<ResourceExplainView> =>
    invoke("explain_resource", { path }),

  /**
   * A resource's bytes, base64, for the in-app viewer. Copies the whole file across the IPC,
   * so ask only for what will be displayed (the size is on `explainResource`'s view).
   */
  readResource: (path: string): Promise<string> => invoke("read_resource", { path }),

  /** *Open in system default*, host-side (the webview holds no opener permission). */
  openResource: (path: string): Promise<void> => invoke("open_resource", { path }),

  /**
   * Open a web link from inside a note in the default browser. `links.ts` decides what is
   * routed here; the host re-checks the scheme (http / https / mailto) and decides what opens.
   */
  openExternal: (url: string): Promise<void> => invoke("open_external", { url }),

  /**
   * The clipboard's plain text, for ⌘⇧V. Host-side: WebKit gates a programmatic
   * `navigator.clipboard` read behind a native confirmation.
   */
  clipboardText: (): Promise<string> => invoke("clipboard_text"),

  /** Semantically near, not-yet-linked candidates for a note (GH #197). */
  similar: (note: string, limit = 10): Promise<SimilarView[]> =>
    invoke("similar", { note, limit }),

  /** Explain one Similar card (GH #236). `limit` is the pane's `similar` limit, so the
   *  rank matches the card's. */
  explainSimilar: (anchor: string, candidate: string, limit: number): Promise<SimilarExplainView> =>
    invoke("explain_similar", { anchor, candidate, limit }),

  /** Hybrid search plus the query-level evidence verdict (invariants.md D2, GH #202). */
  search: (query: string, limit = 20): Promise<SearchEvidenceView> =>
    invoke("search", { query, limit }),

  /**
   * Save a note's body via `Vault::write`, guarded by the `revision` captured at read.
   * Rejects with `WRITE_CONFLICT_MESSAGE` if the file changed on disk since.
   */
  writeNote: (note: string, body: string, baseRevision: string): Promise<WriteReport> =>
    invoke("write_note", { note, body, baseRevision }),

  /**
   * Save a note's raw frontmatter YAML verbatim (GH #79), under the same `revision` guard.
   * The host's one refusal is a `---` line that would end the block early.
   */
  writeFrontmatter: (
    note: string,
    frontmatter: string,
    baseRevision: string,
  ): Promise<WriteReport> => invoke("write_frontmatter", { note, frontmatter, baseRevision }),

  /**
   * Create an empty note (`.md` optional; missing parents created, like `b2 add`).
   * Model-free: vectors fill on the next embed pass.
   */
  createNote: (path: string): Promise<AddReport> => invoke("create_note", { path }),

  /** Create a folder on disk (missing parents included, an occupied target refused). */
  createDir: (dir: string): Promise<DirCreateReport> => invoke("create_dir", { dir }),

  /**
   * Import a dropped file's bytes into `dir` (`""` for the root). `data` is base64: a drop
   * arrives as content, not a path, and JSON IPC carries no byte array cheaply.
   */
  importFile: (dir: string, name: string, data: string): Promise<ImportReport> =>
    invoke("import_file", { dir, name, data }),

  /** `importFile` from a path (the Import files… picker); the bytes never cross the IPC. */
  importPath: (dir: string, source: string): Promise<ImportReport> =>
    invoke("import_path", { dir, source }),

  /** The native file picker behind Import files… (K1); empty if cancelled. */
  pickImportFiles: (): Promise<string[]> => invoke("pick_import_files"),

  /**
   * Move/rename a note, rewriting inbound links. Needs the real model (rewritten files
   * re-embed), so it can reject with the "run `b2 init`" state.
   */
  moveNote: (note: string, to: string): Promise<MoveReport> =>
    invoke("move_note", { note, to }),

  /** `moveNote` for a resource. */
  moveResource: (path: string, to: string): Promise<ResourceMoveReport> =>
    invoke("move_resource", { path, to }),

  /** Move/rename a whole folder — one rename on disk (unindexed files travel too). */
  moveDir: (from: string, to: string): Promise<DirMoveReport> =>
    invoke("move_dir", { from, to }),

  /** Delete a note from disk. Inbound links dangle; they are never rewritten. */
  deleteNote: (note: string): Promise<DeleteReport> => invoke("delete_note", { note }),

  /** `deleteNote` for a resource. */
  deleteResource: (path: string): Promise<ResourceDeleteReport> =>
    invoke("delete_resource", { path }),

  /** Delete a whole folder and everything inside it (unindexed files go too). */
  deleteDir: (dir: string): Promise<DirDeleteReport> => invoke("delete_dir", { dir }),


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
   * Reindex phase 1: the model-free projection (docs/index-engine.md). Once it resolves the
   * tree and keyword search are live; `embed` fills the vectors.
   */
  project: (): Promise<ProjectReport> => invoke("project"),

  /**
   * Reindex phase 2: fill missing vectors, cancellable. `onProgress` fires per batch; the
   * report's `cancelled` is set if `cancelReindex` was called mid-run.
   */
  embed: (onProgress: (p: ReindexProgress) => void): Promise<EmbedReport> => {
    const channel = new Channel<ReindexProgress>();
    channel.onmessage = onProgress;
    return invoke("embed", { onEvent: channel });
  },

  /** Ask the in-flight embed to stop at its next batch boundary (cooperative). */
  cancelReindex: (): Promise<void> => invoke("cancel_reindex"),

  /**
   * One grounded answer (flow ④, `Vault::ask`), streamed per token. `history` is the
   * caller's and session-only (S4): nothing about a chat is stored.
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
   * The Similar card's **Why?**: a grounded, cited explanation streamed like `ask` and
   * stopped by `cancelAsk`. `limit` is the pane's list length, so the quoted rank matches.
   */
  whySimilar: (
    anchor: string,
    candidate: string,
    limit: number,
    onToken: (text: string) => void,
  ): Promise<AnswerView> => {
    const channel = new Channel<string>();
    channel.onmessage = onToken;
    return invoke("why_similar", { anchor, candidate, limit, onEvent: channel });
  },

  /**
   * Stop the streaming answer (Esc). The in-flight `ask` resolves normally with `cancelled`
   * set and the partial text intact.
   */
  cancelAsk: (): Promise<void> => invoke("cancel_ask"),

  /**
   * The chat configuration in force and, for Ollama, the daemon's inventory. Never rejects:
   * "the daemon isn't running" is an answer the setup card needs.
   */
  chatSetup: (): Promise<ChatSetup> => invoke("chat_setup"),

  /**
   * Save the chat configuration and re-probe it (adapter state; no reindex, contrast M2).
   * `apiKey` and `maxToolCalls` are three-state: `null` keeps what is in force, `""` clears,
   * a value sets. The key never comes back; the setup carries `api_key_source`
   * (`b2-desktop/src/keychain.rs`).
   */
  setChatConfig: (
    baseUrl: string | null,
    model: string | null,
    apiKey: string | null,
    maxToolCalls: string | null = null,
  ): Promise<ChatSetup> =>
    invoke("set_chat_config", { baseUrl, model, apiKey, maxToolCalls }),

  /** The embedding models B2 offers, flagged current + installed (Settings picker). */
  listModels: (): Promise<ModelChoice[]> => invoke("list_models"),

  /**
   * Record the chosen embedding model. A different model takes effect only after it is
   * downloaded (`b2 init`) and the vault is reindexed.
   */
  setModel: (model: string): Promise<ModelChoice[]> => invoke("set_model", { model }),

  /** The in-app `b2 init`: download and verify the selected model. Can take minutes. */
  provisionModel: (): Promise<ModelChoice[]> => invoke("provision_model"),

  /** Per-model cumulative embedding time across sessions (Settings). */
  embedStats: (): Promise<EmbedStat[]> => invoke("embed_stats"),

  /** Where downloaded model files are saved (Settings). */
  modelsDir: (): Promise<string> => invoke("models_dir"),

  /** This build's embedder device, "Metal" or "CPU" (Settings badge). */
  embedDevice: (): Promise<string> => invoke("embed_device"),

  /**
   * Every chord the menu bar takes (#119), the authority on chords the webview never sees.
   * Read once at boot to check `menukeys.ts`'s mirror (`checkMenuDrift`); all else reads
   * the mirror.
   */
  menuChords: (): Promise<MenuChord[]> => invoke("menu_chords"),

  /** WebKit page zoom (`zoom.ts`). `factor` is already a rung of the frontend's ladder. */
  setZoom: (factor: number): Promise<void> => invoke("set_zoom", { factor }),

  /** Subscribe to the filesystem-watch pulse (#14), once per burst of external changes. */
  onVaultChanged: (handler: () => void): Promise<UnlistenFn> =>
    listen(VAULT_CHANGED_EVENT, () => handler()),

  /** Subscribe to B2's own menu lines. `handler` gets the id; ignore unknown ids. */
  onMenuCommand: (handler: (id: string) => void): Promise<UnlistenFn> =>
    listen<string>(MENU_COMMAND_EVENT, (e) => handler(e.payload)),
};
