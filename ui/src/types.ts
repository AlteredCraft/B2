// TypeScript mirrors of the `b2-core` façade's `Serialize` view types — the IPC
// contract. These are the SAME shapes the CLI's `--json` mode emits (the desktop
// host reuses them verbatim as command payloads, specs/desktop-ui-mvp.md §3), so a
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

/** `Vault::link` — the committed edge (idempotent: `created=false` if it existed). */
export interface LinkReport {
  src_path: string;
  dst_path: string;
  relation: string;
  created: boolean;
}

/** `Vault::reindex` — what a reindex did. */
export interface ReindexReport {
  indexed: number;
  embedded: number;
  stamped: number;
}
