// TypeScript mirrors of the `b2-core` façade's `Serialize` view types — the IPC
// contract. These are the SAME shapes the CLI's `--json` mode emits (the desktop
// host reuses them verbatim as command payloads, specs/completed/desktop-ui-mvp.md §3), so a
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
  b2id: string;
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
   * the fields above — so `relations:` and any unmodeled keys show as written. The
   * note pane renders it in a collapsible drawer.
   */
  frontmatter: string | null;
  /**
   * blake3 of the raw file bytes at read time — the save-guard token
   * (desktop-editing.md §3): a save presents it, and the host refuses if the file
   * changed on disk since, so an external edit is never silently clobbered.
   */
  revision: string;
}

/** `Vault::list_notes` — one note's identity for the file tree (no body). */
export interface NoteSummary {
  b2id: string;
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
  b2id: string;
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
  b2id: string;
  path: string;
  title: string | null;
  score: number;
  evidence: string;
}

/** `Vault::search` — one hybrid-search hit. */
export interface SearchResult {
  b2id: string;
  path: string;
  title: string | null;
  score: number;
  snippet: string;
}

/** One typed edge of a note, resolved for display (from `Vault::explain`). */
export interface NeighborView {
  b2id: string;
  path: string;
  title: string | null;
  relation: string;
  direction: string; // "outbound" | "inbound"
  label: string;
  explanation: string | null;
  origin: string; // "inline" | "frontmatter"
}

/**
 * One outbound link that resolves to nothing — no note and no resource exists at
 * its target (a `[[Hermes]]` naming a *folder*, or a typo). A note is one `.md` file,
 * so a folder is never a valid target; B2 surfaces the link as broken rather than
 * dropping it (GH #12). Has no `b2id`/`path` — nothing resolved.
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
  b2id: string;
  path: string;
  title: string | null;
  connections: NeighborView[];
  unresolved: UnresolvedLink[];
}

/**
 * `Vault::write` — the completed body save (desktop-editing.md §4): the note's path
 * plus the new `revision` (blake3 of the final on-disk bytes), the token the editor
 * chains the next save on so its own saves never self-conflict.
 */
export interface WriteReport {
  path: string;
  revision: string;
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
 * (projection-embedding-split.md §4). Once this resolves, the tree and keyword
 * search are live; only vectors are missing. `skipped` names any unreadable files the
 * pass left out — one bad file never aborts the whole reindex (empty on a clean vault).
 */
export interface ProjectReport {
  indexed: number;
  stamped: number;
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
   * and re-running finishes the rest (async-indexing.md §3).
   */
  cancelled: boolean;
}

/**
 * `ingest::ReindexProgress` — one per-batch progress event streamed over a Tauri
 * `Channel` during an embed (async-indexing.md §4). The counts describe the notes
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
