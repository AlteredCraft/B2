// TypeScript mirrors of the `b2-core` façade's `Serialize` view types — the IPC
// contract. These are the SAME shapes the CLI's `--json` mode emits (the desktop
// host reuses them verbatim as command payloads, specs/completed/desktop-ui-mvp.md §3), so a
// field here corresponds 1:1 to a Rust struct field. Hand-written for now; if they
// ever churn, `ts-rs`/`tauri-specta` codegen is the later lever (spec §9).

/** `vault_info` — the active vault + whether semantic ranking is live. */
export interface VaultInfo {
  root: string;
  semantic: boolean;
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

/** `Vault::explain` — a note's identity + all its typed edges. */
export interface ExplainView {
  b2id: string;
  path: string;
  title: string | null;
  connections: NeighborView[];
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
 * `Vault::project` — what the fast, model-free projection pass did
 * (projection-embedding-split.md §4). Once this resolves, the tree and keyword
 * search are live; only vectors are missing.
 */
export interface ProjectReport {
  indexed: number;
  stamped: number;
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
