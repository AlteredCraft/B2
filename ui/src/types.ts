// TypeScript mirrors of the façade's `Serialize` view types: the IPC contract, and the
// same shapes the CLI's `--json` emits (crates/b2-desktop/CLAUDE.md). Fields map 1:1 to
// Rust struct fields. Hand-written; codegen is the lever if they churn (spec §9).

/**
 * `vault_info` — the active vault, whether the real model is installed (`semantic`), and
 * how much is embedded (#26). The fraction says how much semantic ranking is live, so the
 * UI can flag search "keyword-only for now" while the vault embeds.
 */
export interface VaultInfo {
  root: string;
  semantic: boolean;
  notes_embedded: number;
  notes_total: number;
}

/**
 * `menu_chords` — one menu-bar item that carries a chord (b2-desktop `menu.rs`, #119), a
 * host type rather than a `b2-core` one. `keys` is in the registry's chord syntax;
 * `label` is the menu's own text. Checked against `menukeys.ts`.
 */
export interface MenuChord {
  id: string;
  label: string;
  keys: string;
}

// --- chat (flow ④, GH #151/#153/#155) ------------------------------------------------

/**
 * One turn of the conversation, as `ask` takes it (`b2-core`'s `ChatTurn`). History is
 * session-only and the pane's (S4): nothing about a chat is written to the vault, the
 * index, or `localStorage`.
 */
export interface ChatTurn {
  role: "user" | "assistant";
  content: string;
}

/**
 * `ask` — one grounded answer (`b2-core`'s `AnswerView`): what the streamed tokens add up
 * to, plus citations resolved back to notes.
 */
export interface AnswerView {
  /** The answer verbatim, including unresolved `[n]` markers: untrusted (E5), never rewritten. */
  answer: string;
  /** One entry per distinct marker that names a real passage, ascending. */
  citations: Citation[];
  /** The stream was stopped mid-answer (Esc): `answer` is an honest prefix. */
  cancelled: boolean;
  /** The B2 tools that ran, in order. Absent for a plain `ask`, which offers none. */
  tools?: ToolUse[];
}

/** One tool run during a chat turn (`b2-core`'s `ToolUseView`). For a model-made call,
 *  `name` and `arguments` are untrusted. */
export interface ToolUse {
  name: string;
  arguments: string;
  /** `true` for a lookup B2 made itself; `false` for one the model chose. */
  seeded: boolean;
}

/** One resolved `[n]` citation: which marker, which note, and a line of evidence. */
export interface Citation {
  marker: number;
  /** Vault-relative path — the note's identity (L1), and what a click opens in-app. */
  path: string;
  excerpt: string;
}

/** How ready chat is (`b2-llm`'s `ChatState`) — the setup card's branch. `"unreachable"`
 *  covers both nothing listening and a refusal (wrong URL, rejected key); `message` parts
 *  them, so never render advice of your own from this. */
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
 * The Ollama-native half of the setup card. Present only when the endpoint looks like
 * Ollama's, the one runtime B2 guides (GH #151).
 */
export interface OllamaSetup {
  /** The daemon's native root (`http://localhost:11434`). */
  root: string;
  /** Whether the native API answered at all. */
  running: boolean;
  /** What is installed. Empty with `running` is the "no model" card, not "no server". */
  installed: OllamaModel[];
  ram_gb: number | null;
  tiers: ModelTier[];
  /** The rung this machine sits on, or null when memory couldn't be read. */
  suggested: ModelTier | null;
}

/**
 * Where the bearer token in force came from (`b2-llm`'s `ApiKeySource`); only this fact
 * about the key crosses the boundary. Each value is different copy (GH #176):
 *
 * - `"none"` — no key: the Local configuration, and the default.
 * - `"environment"` — `B2_LLM_API_KEY`, which overrides anything stored; the app can
 *   neither replace nor remove it.
 * - `"stored"` — in the macOS Keychain, encrypted at rest.
 * - `"session"` — in memory for this run only, because the Keychain refused.
 */
export type ApiKeySource = "none" | "environment" | "stored" | "session";

/**
 * `chat_setup` / `set_chat_config` — what the chat surface needs before a question
 * (`b2-llm`'s `ChatSetup`). Adapter state, so changing it costs no reindex (contrast M2).
 * The key itself never crosses this boundary (`b2-desktop/src/chat.rs`).
 */
export interface ChatSetup {
  base_url: string;
  model: string;
  /** `false` for Local, `true` for Cloud models; drives the privacy copy (M5). */
  cloud: boolean;
  api_key_source: ApiKeySource;
  state: ChatState;
  /** A generic, actionable sentence when `state` isn't `"ready"` (E4). */
  message: string | null;
  /** Models the endpoint says it serves, when it said. */
  available: string[];
  ollama: OllamaSetup | null;
  /** The tool-call cap and its bounds, from the host so Settings can't advertise a range
   *  the parser refuses. */
  tool_calls: ToolCallCap;
}

/** `b2-llm`'s `ToolCallCap`: the most tool calls one model reply may make. */
export interface ToolCallCap {
  /** The cap in force — Settings over `B2_LLM_MAX_TOOL_CALLS` over the default. */
  in_force: number;
  /** What clearing the field returns to, absent an environment override. */
  default: number;
  /** The highest value any source may set. */
  ceiling: number;
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
 * `embed_stats` — one model's cumulative embedding cost since it was last selected
 * (b2-desktop `stats.rs`), so a model swap can be judged on real speed.
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
  /** Raw frontmatter YAML verbatim, fences excluded, or null. Not a re-serialization, so
   *  unmodeled keys show as written. */
  frontmatter: string | null;
  /** Whether that block reads as YAML metadata (GH #79). `false` ⇒ B2 projected no fields
   *  from it, and the drawer shows a non-blocking warning. */
  frontmatter_readable: boolean;
  /** blake3 of the file bytes at read time: the save-guard token. The host refuses a save
   *  if the file changed since (crates/b2-desktop/CLAUDE.md). */
  revision: string;
}

/** `Vault::list_notes` — one note's identity for the file tree (no body). */
export interface NoteSummary {
  path: string;
  title: string | null;
}

/** `Vault::list_resources` — one non-`.md` vault file; the tree merges it with `NoteSummary`. */
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
  /** Stage-2 best-passage z against the anchor's candidates, for a strength band (GH #192);
   *  gates nothing (GH #197). Absent when ungraded (fake space, tiny pool, no spread). */
  z?: number;
}

/** One passage as stored in the index. */
export interface PassageView {
  heading_path: string | null;
  text: string;
}

/** A candidate passage and the anchor passage nearest to it (`Vault::explain_similar`). */
export interface PassagePairView {
  anchor: PassageView;
  candidate: PassageView;
  score: number;
  /** The pair's z on the card's yardstick; null when the field is ungraded. */
  z: number | null;
  /** Both passages hold the same text (a template, a copy). */
  identical: boolean;
}

/** Where a note stands in an anchor's discovery field: why it is a card, or why not. */
export type SimilarStanding =
  | { kind: "same_note" }
  | { kind: "anchor_unembedded" }
  | { kind: "linked" }
  | { kind: "unembedded" }
  | { kind: "not_shortlisted"; shortlist: number }
  | { kind: "ranked"; rank: number; of: number; served: boolean };

/** `Vault::explain_similar` — the model-free Explain view for one Similar card (GH #236). */
export interface SimilarExplainView {
  anchor: NoteSummary;
  candidate: NoteSummary;
  limit: number;
  standing: SimilarStanding;
  z: number | null;
  /** Rank judged by whole-note average (stage 1), when it entered stage 1. */
  centroid_rank: number | null;
  /** Every scored note's z, nearest first. Empty when ungraded. */
  population: number[];
  pairs: PassagePairView[];
  shared_neighbors: NoteSummary[];
}

/** `Vault::search` — one hybrid-search hit. */
export interface SearchResult {
  path: string;
  title: string | null;
  score: number;
  snippet: string;
}

/** One served row plus the provenance RRF discards: which list ranked its chunk, and its
 *  cosine (`Vault::search_evidence`, GH #201). Flattened host-side onto a `SearchResult`. */
export interface EvidencedResult extends SearchResult {
  /** 0-based rank in the BM25 list; `null` = the lexical half never ranked it. */
  bm25_rank: number | null;
  /** 0-based rank in the dense list; `null` = never ranked it, or never ran. */
  vector_rank: number | null;
  /** This chunk's cosine to the query; `null` whenever `vector_rank` is. */
  cos: number | null;
}

/** One query term's document frequency and weight in the verdict's coverage. */
export interface QueryTermView {
  term: string;
  df: number;
  idf: number;
}

/** `Vault::search_evidence` — the served rows plus D2's verdict (GH #202). `vouched`:
 *    • `true`  — the vault holds evidence; serve the rows.
 *    • `false` — it holds none; show the empty state and none of the rows.
 *    • `null`  — no calibrated bar for this model (M2). Serve the rows; never read this as
 *                "no matches". */
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
  /** The other note's `created` date, resolved host-side (GH #22). */
  created: string | null;
}

/**
 * One outbound link at a resource (any non-`.md` vault file), from `Vault::explain`
 * (GH #22). Always outbound: a resource never authors edges.
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
 * One outbound link that resolves to nothing (a typo, or a `[[Hermes]]` naming a folder).
 * Surfaced as broken rather than dropped (GH #12).
 */
export interface UnresolvedLink {
  /** The target exactly as written in the Markdown (`[[target]]`) — e.g. `Hermes`. */
  target: string;
  /** The relation verb (`references` for a bare link). */
  relation: string;
  origin: string; // "inline" | "frontmatter"
  explanation: string | null;
}

/** `Vault::explain` — a note's identity, its typed edges, and its dangling links (GH #12). */
export interface ExplainView {
  path: string;
  title: string | null;
  connections: NeighborView[];
  /** Outbound links at resources — a note's file links, from the note's side. */
  resources: ResourceLink[];
  unresolved: UnresolvedLink[];
}

/**
 * `Vault::write` — the completed save: the path plus the new `revision`, which the editor
 * chains the next save on so its own saves never self-conflict.
 */
export interface WriteReport {
  path: string;
  revision: string;
}

/** `Vault::create_note` — the created note's `.md`-normalized path, its identity (L1). */
export interface AddReport {
  path: string;
}

/**
 * `Vault::import_file` / `Vault::import_path` — where an imported file landed, and whether
 * it was routed as a note or a resource. Either way the path is its identity (L3).
 */
export interface ImportReport {
  path: string;
  note: boolean;
}

/** `Vault::create_dir` — the created folder's normalized path. Folders have no index row. */
export interface DirCreateReport {
  dir: string;
}

/** `Vault::move_note` — old and new paths, plus which inbound files had links rewritten. */
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

/** `Vault::move_dir` — counts of what travelled, and the rewritten files at their new paths. */
export interface DirMoveReport {
  from: string;
  to: string;
  moved_notes: number;
  moved_resources: number;
  rewrote: string[];
  links_rewritten: number;
}

/** `Vault::delete_note` — the path, plus surviving files whose links now dangle (never rewritten). */
export interface DeleteReport {
  path: string;
  dangled: string[];
}

/** `Vault::delete_resource` — the resource sibling of `DeleteReport` (same shape). */
export interface ResourceDeleteReport {
  path: string;
  dangled: string[];
}

/** `Vault::delete_dir` — counts of what was deleted, and the linkers now dangling. */
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

/** A `.md` file the projection pass skipped. `reason` is a short, file-level phrase
 *  ("permission denied"), safe to show. */
export interface SkippedNote {
  path: string;
  reason: string;
}

/**
 * `Vault::project` — what the model-free projection pass did (docs/index-engine.md). Once
 * it resolves, the tree and keyword search are live. One bad file never aborts it.
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
  /** Cancelled mid-run. The index stays consistent and a re-run finishes the rest. */
  cancelled: boolean;
}

/**
 * `ingest::ReindexProgress` — one per-batch event streamed during an embed. Counts cover
 * only the notes that (re)embed this run, and are determinate from the first batch.
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
