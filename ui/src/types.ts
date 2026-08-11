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
  /** Stage-1 z-score against the anchor's candidate population — the number the
   *  discovery floor judged, and the honest input for a strength band (GH #150).
   *  Absent when no floor statistics were computed (floor off / tiny pool). */
  z?: number;
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
  /**
   * The other note's `created` date, if it has one — resolved by the host so a
   * client never re-reads the file just for a date (GH #22).
   */
  created: string | null;
}

/**
 * One outbound link a note authors at a **resource** (an image, a PDF — any
 * non-`.md` vault file), from `Vault::explain` — the third target kind an edge
 * can have (note / resource / dangling, GH #22). No `b2id`/direction: a resource
 * never authors edges, so these are always outbound.
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
 * `Vault::create_note` — the created note's identity: the `b2id` projection
 * stamped, and the vault-relative path (`.md`-normalized) to open it by.
 */
export interface AddReport {
  b2id: string;
  path: string;
}

/**
 * `Vault::import_file` / `Vault::import_path` — where an imported file landed, and
 * the `b2id` its projection stamped. `null` for a resource: a non-`.md` file is a
 * path-keyed peer with no identity to stamp (data-model.md §10).
 */
export interface ImportReport {
  path: string;
  b2id: string | null;
}

/**
 * `Vault::create_dir` — the created folder's normalized vault-relative path. A
 * folder is user-authored structure (a real `mkdir` on disk), so there is no
 * b2id and no index row to report.
 */
export interface DirCreateReport {
  dir: string;
}

/**
 * `Vault::move_note` — the completed move/rename: old and new vault-relative
 * paths, plus which inbound files had their link text rewritten.
 */
export interface MoveReport {
  b2id: string;
  from: string;
  to: string;
  rewrote: string[];
  links_rewritten: number;
}

/** `Vault::move_resource` — the resource sibling of `MoveReport` (no b2id). */
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
 * `Vault::delete_note` — the completed delete: the note's identity, plus the
 * surviving files whose links at it now dangle (they are never rewritten).
 */
export interface DeleteReport {
  b2id: string;
  path: string;
  dangled: string[];
}

/** `Vault::delete_resource` — the resource sibling of `DeleteReport` (no b2id). */
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
 * Why the kept path kept a contested `b2id` (GH #81): `incumbent` — the index
 * already attributed the id to that file, the one confident signal (a copy
 * preserves every byte, so nothing in the vault distinguishes it from the
 * original); `tie_break` — no incumbent (a fresh index), first-in-path-order
 * kept purely so the pass is reproducible, NOT an identity ruling.
 */
export type CollisionPrecedence = "incumbent" | "tie_break";

/**
 * A cross-note `b2id` collision the projection pass surfaced (GH #81) — e.g. a
 * note duplicated in Finder. One file keeps the identity; the `shadowed_paths`
 * stay on disk but are NOT indexed until the human resolves: delete the copy,
 * remove its `b2id:` line (next pass stamps a fresh identity), or delete the
 * original (the copy inherits). Surfacing only — B2 never edits either file of
 * its own accord.
 *
 * With no index row a shadowed path is also absent from the file tree (which lists
 * `list_notes`), so the review panel (GH #88) can offer it for *copying* but never
 * for opening — see `anomalies.ts`.
 */
export interface B2idCollision {
  b2id: string;
  kept_path: string;
  precedence: CollisionPrecedence;
  shadowed_paths: string[];
}

/**
 * An identity restamp the projection pass surfaced (GH #81): the file's `b2id`
 * line was removed or blanked outside b2, so the pass stamped a fresh id — the
 * note's identity changed, and inbound links keyed to `old_b2id` now dangle.
 */
export interface RestampedNote {
  path: string;
  old_b2id: string;
  new_b2id: string;
}

/**
 * `Vault::project` — what the fast, model-free projection pass did
 * (docs/design/index-engine.md). Once this resolves, the tree and keyword
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
  /** Cross-note b2id collisions this pass (GH #81) — re-surfaced every pass until resolved. */
  collisions: B2idCollision[];
  /** Identity restamps this pass (GH #81) — per-pass events; the dangling links persist. */
  restamped: RestampedNote[];
}

/** `Vault::embed` — what the embed pass did: notes whose missing vectors it filled. */
export interface EmbedReport {
  embedded: number;
  /**
   * The embed was cancelled mid-run (the user hit Cancel). The index is still
   * consistent — keyword search + graph are complete, a prefix of notes is embedded —
   * and re-running finishes the rest (docs/design/index-engine.md).
   */
  cancelled: boolean;
}

/**
 * `ingest::ReindexProgress` — one per-batch progress event streamed over a Tauri
 * `Channel` during an embed (docs/design/index-engine.md). The counts describe the notes
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
