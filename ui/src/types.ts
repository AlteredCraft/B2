// TypeScript mirrors of the `b2-core` façade's `Serialize` view types — the IPC
// contract. These are the SAME shapes the CLI's `--json` mode emits (the desktop
// host reuses them verbatim as command payloads, crates/b2-desktop/CLAUDE.md), so a
// field here corresponds 1:1 to a Rust struct field. Hand-written for now; if they
// ever churn, `ts-rs`/`tauri-specta` codegen is the later lever (spec §9).

/**
 * `vault_info` — the active vault, whether the real model is installed (`semantic`),
 * and how much of the vault is actually embedded (`notes_embedded`/`notes_total`, #26).
 * The fraction is the precise honesty signal: `semantic` says a model *exists*, the
 * fraction says how much semantic ranking is *live*, so the UI can flag search
 * "keyword-only for now" while a projected vault embeds behind the first tree paint.
 */
export interface VaultInfo {
  root: string;
  semantic: boolean;
  notes_embedded: number;
  notes_total: number;
}

/**
 * `menu_chords` — one item of the app's **menu bar** that carries a chord
 * (b2-desktop `menu.rs`, #119). Not a `b2-core` view type: the menu is the host's own
 * surface, and this is the only shape it exports. `keys` is spelled in the keyboard
 * registry's chord syntax (`Mod-Shift-z`), so `bindings.ts` can parse it with the same
 * parser it uses for B2's own chords; `label` is the text the menu itself shows, and the
 * keyboard reference prints it verbatim. See `menukeys.ts` for the mirror this is
 * checked against.
 */
export interface MenuChord {
  id: string;
  label: string;
  keys: string;
}

// --- chat (flow ④, GH #151/#153/#155) ------------------------------------------------

/**
 * One turn of the conversation, as `ask` takes it (`b2-core`'s `ChatTurn`). The only
 * shape here that crosses the seam *inbound*: history is the **adapter's**, session-only
 * (invariant S4), so the pane holds it and hands it back turn by turn — nothing about a
 * chat is ever written to the vault, the index, or `localStorage`.
 */
export interface ChatTurn {
  role: "user" | "assistant";
  content: string;
}

/**
 * `ask` — one grounded answer (`b2-core`'s `AnswerView`), the same view `b2 ask --json`
 * ends its event stream with. The tokens arrive first over the command's channel; this is
 * what they add up to, plus the citations resolved back to notes.
 */
export interface AnswerView {
  /** The answer verbatim, including any `[n]` marker that resolved to nothing —
   *  model output is untrusted (E5) but it is never rewritten. */
  answer: string;
  /** One entry per distinct marker that names a real passage, ascending. */
  citations: Citation[];
  /** The stream was stopped mid-answer (Esc): `answer` is an honest prefix. */
  cancelled: boolean;
}

/** One resolved `[n]` citation: which marker, which note, and a line of evidence. */
export interface Citation {
  marker: number;
  /** Vault-relative path — the note's identity (L1), and what a click **opens in-app**. */
  path: string;
  excerpt: string;
}

/** How ready chat is right now (`b2-llm`'s `ChatState`) — the setup card's branch.
 *
 *  `"unreachable"` covers both *nothing is listening* and *something answered and
 *  refused* (a base URL that isn't a chat API, a rejected key): the card is the same,
 *  and `message` is what parts them — never render advice of your own from this. */
export type ChatState = "ready" | "unreachable" | "model_missing" | "fake";

/** One model an Ollama daemon has installed, from its native `/api/tags`. */
export interface OllamaModel {
  name: string;
  /** On-disk size in bytes. */
  size: number;
  /** Ollama's own parameter label ("3.2B"), when it gives one. */
  parameters: string | null;
}

/** One rung of the pull heuristic — illustrative and non-binding (GH #151). */
export interface ModelTier {
  min_ram_gb: number;
  ram: string;
  size: string;
  model: string;
}

/**
 * The Ollama-native half of the setup card. Present only when the configured endpoint
 * looks like Ollama's: guided setup is a per-runtime feature, and Ollama is the runtime
 * B2 guides (GH #151) — there is nothing honest to say about pulling a model into
 * LM Studio.
 */
export interface OllamaSetup {
  /** The daemon's native root (`http://localhost:11434`). */
  root: string;
  /** Whether the native API answered at all. */
  running: boolean;
  /** What is installed. Empty *with* `running` is the "no model" card, which is a
   *  different sentence from the "no server" one. */
  installed: OllamaModel[];
  ram_gb: number | null;
  tiers: ModelTier[];
  /** The rung this machine sits on, or null when memory couldn't be read. */
  suggested: ModelTier | null;
}

/**
 * Where the bearer token in force came from (`b2-llm`'s `ApiKeySource`) — a fact *about*
 * the key, which is the only part of one that may cross this boundary.
 *
 * Each value is different copy, which is why this isn't a boolean (GH #176):
 *
 * - `"none"` — no key. The **Local** configuration, and the default.
 * - `"environment"` — `B2_LLM_API_KEY`, which **overrides** anything B2 remembered. It is
 *   the user's own configuration, so the app can neither replace nor remove it.
 * - `"stored"` — remembered in the macOS Keychain: encrypted at rest, and there next launch.
 * - `"session"` — held in memory for this run only, because the Keychain was unavailable or
 *   refused. Chat works; the key is gone at quit.
 */
export type ApiKeySource = "none" | "environment" | "stored" | "session";

/**
 * `chat_setup` / `set_chat_config` — everything the chat surface needs before a question
 * is asked (`b2-llm`'s `ChatSetup`). Adapter-level state, never vault or index state, so
 * changing it costs no reindex (contrast M2).
 *
 * `api_key_source` and never the key: the token does not cross this boundary in either
 * direction (`b2-desktop/src/chat.rs`).
 */
export interface ChatSetup {
  base_url: string;
  model: string;
  /** `false` for **Local**, `true` for **Cloud models** — what the privacy copy hangs
   *  off (invariant M5). */
  cloud: boolean;
  api_key_source: ApiKeySource;
  state: ChatState;
  /** A generic, actionable sentence when `state` isn't `"ready"` (E4). */
  message: string | null;
  /** Models the endpoint says it serves, when it said. */
  available: string[];
  ollama: OllamaSetup | null;
}

/**
 * `list_models` / `set_model` — one embedding model the settings picker offers
 * (b2-embed `ModelChoice`). `current` is the model B2 is configured to use now;
 * `installed` is whether it's been downloaded (`b2 init`) yet.
 */
export interface ModelChoice {
  id: string;
  label: string;
  dim: number;
  description: string;
  current: boolean;
  installed: boolean;
}

/**
 * `embed_stats` — one model's cumulative embedding cost (b2-desktop `stats.rs`): a running
 * total summed across every reindex since the model was selected, shown in Settings so a
 * model swap can be judged on real speed. Switching *to* a model restarts its total, so a
 * bucket covers only the model's current stint. `total_ms / chunks` is throughput; `runs`
 * counts contributing embed passes.
 */
export interface EmbedStat {
  model: string;
  total_ms: number;
  chunks: number;
  runs: number;
}

/** `Vault::read` — a note's body + display metadata for the left pane. */
export interface NoteView {
  path: string;
  title: string | null;
  type: string | null;
  created: string | null;
  updated: string | null;
  tags: string[];
  /** Raw Markdown body (frontmatter stripped), verbatim from disk. */
  body: string;
  /**
   * Raw frontmatter YAML verbatim (between the `---` fences, fences excluded), or
   * null when the note has none. The byte-honest block — not a re-serialization of
   * the fields above — so `b2_relations:` and any unmodeled keys show as written. The
   * note pane renders it in a collapsible drawer.
   */
  frontmatter: string | null;
  /**
   * Whether that block reads as YAML metadata (GH #79). `false` ⇒ the raw bytes
   * above round-trip verbatim but B2 projected no fields from them (malformed
   * YAML, or not a key/value mapping) — the drawer shows a non-blocking warning.
   * Because every read carries it, an external hand-edit surfaces the same
   * warning as an in-app save.
   */
  frontmatter_readable: boolean;
  /**
   * blake3 of the raw file bytes at read time — the save-guard token
   * (crates/b2-desktop/CLAUDE.md): a save presents it, and the host refuses if the file
   * changed on disk since, so an external edit is never silently clobbered.
   */
  revision: string;
}

/** `Vault::list_notes` — one note's identity for the file tree (no body). */
export interface NoteSummary {
  path: string;
  title: string | null;
}

/**
 * `Vault::list_resources` — one non-`.md` vault file for the file tree (file-type
 * slice 1). The per-kind sibling of `NoteSummary`; the tree merges the two lists.
 */
export interface ResourceSummary {
  path: string;
  class: string; // "text" | "html" | "pdf" | "image" | "media" | "binary"
  size: number;
  mtime: number | null;
}

/** One note linking at a resource, with the edge's authored context. */
export interface ResourceBacklink {
  path: string;
  title: string | null;
  type: string;
  caption: string | null;
  embed: boolean;
}

/** `Vault::explain_resource` — the fallback card: inventory metadata + backlinks. */
export interface ResourceExplainView {
  path: string;
  class: string;
  size: number;
  mtime: number | null;
  content_hash: string;
  backlinks: ResourceBacklink[];
}

/** `Vault::similar` — a semantically-near, not-yet-linked candidate. */
export interface SimilarView {
  path: string;
  title: string | null;
  score: number;
  evidence: string;
  /** Stage-2 best-passage z against the anchor's candidate population — the
   *  honest input for a strength band (GH #150/#192), gating nothing since
   *  GH #197. Absent when no statistic was computed (fake space / tiny pool /
   *  no spread) — the pane's cue to say the list is ungraded. */
  z?: number;
}

/** `Vault::search` — one hybrid-search hit. */
export interface SearchResult {
  path: string;
  title: string | null;
  score: number;
  snippet: string;
}

/** One served row with the provenance RRF discarded — which list ranked its chunk,
 *  and how near its vector actually was (`Vault::search_evidence`, GH #201). The
 *  fields are flattened onto the row host-side, so this *is* a `SearchResult`. */
export interface EvidencedResult extends SearchResult {
  /** 0-based rank in the BM25 list; `null` = the lexical half never ranked it. */
  bm25_rank: number | null;
  /** 0-based rank in the dense list; `null` = the vector half never ranked it,
   *  or never ran. */
  vector_rank: number | null;
  /** This chunk's cosine to the query; `null` whenever `vector_rank` is. */
  cos: number | null;
}

/** One query term's lexical reading — its document frequency and the weight that
 *  gives it in the coverage the verdict reads (`Vault::search_evidence`). */
export interface QueryTermView {
  term: string;
  df: number;
  idf: number;
}

/** `Vault::search_evidence` — the served rows plus D2's query-level verdict.
 *
 *  `vouched` is three-state and each state is different behavior (invariants.md
 *  D2, GH #202):
 *    • `true`  — the vault holds lexical or semantic evidence; serve the rows.
 *    • `false` — it holds neither; the pane shows the honest empty state and
 *                **none** of the rows (strict, no reveal).
 *    • `null`  — no calibrated bar for the active model (the fake embedder, or
 *                any model until the harness measures one — M2). Serve the rows
 *                exactly as before; never read this as "no matches", which would
 *                blank every dev vault. */
export interface SearchEvidenceView {
  results: EvidencedResult[];
  vouched: boolean | null;
  chunk_total: number;
  terms: QueryTermView[];
  best_cos: number | null;
}

/** One typed edge of a note, resolved for display (from `Vault::explain`). */
export interface NeighborView {
  path: string;
  title: string | null;
  relation: string;
  direction: string; // "outbound" | "inbound"
  label: string;
  explanation: string | null;
  origin: string; // "inline" | "frontmatter"
  /**
   * The other note's `created` date, if it has one — resolved by the host so a
   * client never re-reads the file just for a date (GH #22).
   */
  created: string | null;
}

/**
 * One outbound link a note authors at a **resource** (an image, a PDF — any
 * non-`.md` vault file), from `Vault::explain` — the third target kind an edge
 * can have (note / resource / dangling, GH #22). No direction: a resource never
 * authors edges, so these are always outbound.
 */
export interface ResourceLink {
  path: string;
  class: string; // "text" | "html" | "pdf" | "image" | "media" | "binary"
  relation: string;
  origin: string; // "inline" | "frontmatter"
  caption: string | null;
  embed: boolean;
  explanation: string | null;
}

/**
 * One outbound link that resolves to nothing — no note and no resource exists at
 * its target (a `[[Hermes]]` naming a *folder*, or a typo). A note is one `.md` file,
 * so a folder is never a valid target; B2 surfaces the link as broken rather than
 * dropping it (GH #12). Has no `path` — nothing resolved.
 */
export interface UnresolvedLink {
  /** The target exactly as written in the Markdown (`[[target]]`) — e.g. `Hermes`. */
  target: string;
  /** The relation verb (`references` for a bare link). */
  relation: string;
  origin: string; // "inline" | "frontmatter"
  explanation: string | null;
}

/**
 * `Vault::explain` — a note's identity, its typed edges, and any unresolved
 * (dangling) outbound links. `connections` are resolved neighbors; `unresolved` are
 * links whose target names no note or file, shown with a broken-link emblem (GH #12).
 */
export interface ExplainView {
  path: string;
  title: string | null;
  connections: NeighborView[];
  /** Outbound links at resources — a note's file links, from the note's side. */
  resources: ResourceLink[];
  unresolved: UnresolvedLink[];
}

/**
 * `Vault::write` — the completed body save (crates/b2-desktop/CLAUDE.md): the note's path
 * plus the new `revision` (blake3 of the final on-disk bytes), the token the editor
 * chains the next save on so its own saves never self-conflict.
 */
export interface WriteReport {
  path: string;
  revision: string;
}

/**
 * `Vault::create_note` — the created note's vault-relative path
 * (`.md`-normalized), which is its identity (L1) and how it is opened.
 */
export interface AddReport {
  path: string;
}

/**
 * `Vault::import_file` / `Vault::import_path` — where an imported file landed, and
 * whether it was routed as a note (a `.md`, projected) or a resource (one inventory
 * row). Both are path-keyed peers (L3), so the path is the whole identity either way.
 */
export interface ImportReport {
  path: string;
  note: boolean;
}

/**
 * `Vault::create_dir` — the created folder's normalized vault-relative path. A
 * folder is user-authored structure (a real `mkdir` on disk), so it has no index
 * row to report at all.
 */
export interface DirCreateReport {
  dir: string;
}

/**
 * `Vault::move_note` — the completed move/rename: old and new vault-relative
 * paths, plus which inbound files had their link text rewritten.
 */
export interface MoveReport {
  from: string;
  to: string;
  rewrote: string[];
  links_rewritten: number;
}

/** `Vault::move_resource` — the resource sibling of `MoveReport` (same shape). */
export interface ResourceMoveReport {
  from: string;
  to: string;
  rewrote: string[];
  links_rewritten: number;
}

/**
 * `Vault::move_dir` — a whole-folder move: how many indexed notes/resources
 * travelled, and the rewritten files at their post-move paths.
 */
export interface DirMoveReport {
  from: string;
  to: string;
  moved_notes: number;
  moved_resources: number;
  rewrote: string[];
  links_rewritten: number;
}

/**
 * `Vault::delete_note` — the completed delete: the note's path, plus the surviving
 * files whose links at it now dangle (they are never rewritten).
 */
export interface DeleteReport {
  path: string;
  dangled: string[];
}

/** `Vault::delete_resource` — the resource sibling of `DeleteReport` (same shape). */
export interface ResourceDeleteReport {
  path: string;
  dangled: string[];
}

/** `Vault::delete_dir` — a whole-folder delete: how many indexed notes/resources
 *  died with it, and the surviving linkers whose links now dangle. */
export interface DirDeleteReport {
  dir: string;
  deleted_notes: number;
  deleted_resources: number;
  dangled: string[];
}

/** `Vault::link` — the committed edge (idempotent: `created=false` if it existed). */
export interface LinkReport {
  src_path: string;
  dst_path: string;
  relation: string;
  created: boolean;
}

/**
 * A `.md` file the projection pass couldn't read and skipped (see `ProjectReport`).
 * `reason` is a short, file-level phrase — "not valid UTF-8 text", "permission
 * denied" — safe to show; never a B2 internal.
 */
export interface SkippedNote {
  path: string;
  reason: string;
}

/**
 * `Vault::project` — what the fast, model-free projection pass did
 * (docs/index-engine.md). Once this resolves, the tree and keyword
 * search are live; only vectors are missing. `skipped` names any unreadable files the
 * pass left out — one bad file never aborts the whole reindex (empty on a clean vault).
 */
export interface ProjectReport {
  indexed: number;
  skipped: SkippedNote[];
  /** Ghost note rows pruned this pass — files deleted outside b2 (#31). */
  notes_pruned: number;
  /** Resources inventoried this pass, and stale inventory rows pruned (slice 1). */
  resources_indexed: number;
  resources_pruned: number;
}

/** `Vault::embed` — what the embed pass did: notes whose missing vectors it filled. */
export interface EmbedReport {
  embedded: number;
  /**
   * The embed was cancelled mid-run (the user hit Cancel). The index is still
   * consistent — keyword search + graph are complete, a prefix of notes is embedded —
   * and re-running finishes the rest (docs/index-engine.md).
   */
  cancelled: boolean;
}

/**
 * `ingest::ReindexProgress` — one per-batch progress event streamed over a Tauri
 * `Channel` during an embed (docs/index-engine.md). The counts describe the notes
 * that actually (re)embed this run, not every note (an incremental run reuses most
 * vectors untouched), and are determinate from the first batch.
 */
export interface ReindexProgress {
  /** Vault-relative path of the note currently embedding. */
  note_path: string;
  /** Number of chunks in the current note. */
  note_chunks: number;
  /** How many notes have begun embedding so far (1-based)… */
  notes_embedded: number;
  /** …out of this many notes that need (re)embedding this run — the progress denominator. */
  notes_to_embed: number;
  /** Chunks embedded so far, cumulative across every note this run. */
  chunks_done: number;
}
