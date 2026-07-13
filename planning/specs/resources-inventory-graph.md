---
title: "B2 — Resources slice 1: inventory & graph"
type: note
tags: [b2, resources, file-types, ingest, edges, schema, fallback-card, spec]
created: 2026-07-12
status: draft
---

# B2 — Resources slice 1: inventory & graph

> **The build spec for the first file-type-support slice: the vault stops lying about its own
> contents.** Every non-`.md` file becomes a walked, classified, path-keyed row in a new
> `resources` table; the parser learns Markdown's own link forms (`![alt](path)`, `[text](path)`,
> `![[file.ext]]`) so notes' references to resources become real, resolved edges with captured
> captions; the desktop file tree shows every file and selecting one opens the **fallback card**
> (metadata + backlinks + *Open in system default*); `b2 explain` and `b2 mv` cover resources.
> **Model-free, no new engine deps** — the one new adapter dep is the Tauri opener plugin.
>
> **This doc owns:** the v4 schema delta (`resources` table, `edges` widening), the walk/pruning
> changes, the parser forms + resolution rules, the façade additions (`list_resources`,
> `explain_resource`, `move_resource`, the `doc_kind` dispatch helper), the CLI/desktop wiring, and
> the build order.
>
> **It does not own:** the design and its rationale
> ([research/file-type-support.md](../research/file-type-support.md), locked 2026-07-08, §9b
> 2026-07-12; mirrored in [data-model.md](../data-model.md) §10,
> [index-engine.md](../index-engine.md) §3); rendering/viewers (slice 2); extraction, chunks,
> vectors, centroids, search/similar over resources (slice 3 — slice 1 touches **no chunk**); PDF
> text (slice 4); the semantic seams (slice 5).

## 0. Scope & ground rules

Every existing decision holds; this slice adds inventory + graph only:

- **The invariant widens, nothing else moves:** `index = projection of (the vault directory)`. A
  resource contributes only derived rows — no `b2id` stamp, no writes to non-`.md` bytes ever
  (`b2 mv` moves the file; that is the one operation, and it is path-only).
- **The core stays model-free and deterministic.** No wall-clock (`indexed_at` follows the
  `notes.indexed_at` pattern — passed in), no randomness; classification is by extension only.
- **Adapters stay dumb.** The one new rule adapters need — is this argument a note or a resource? —
  lives in core as a pure helper (`doc_kind`, §4), so the CLI and desktop can't drift.
- **Degrade, never abort.** An unreadable file gets a `skipped` entry (the `SkippedNote` pattern,
  reused); one bad file never fails the index.

## 1. Schema — v4

`SCHEMA_VERSION` 3 → 4; the version gate drops and rebuilds as always (**no migration code, ever**).

```sql
-- New: the resource inventory (data-model.md §10; path-keyed, index-only identity)
CREATE TABLE IF NOT EXISTS resources (
  path         TEXT PRIMARY KEY,     -- vault-relative, '/'-separated
  class        TEXT NOT NULL CHECK (class IN
                 ('text','html','pdf','image','media','binary')),
  size         INTEGER NOT NULL,
  mtime        INTEGER,
  content_hash TEXT NOT NULL,        -- blake3 of the file bytes (move-repair assist, §2)
  indexed_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS resources_class_idx ON resources(class);
```

`edges` widens (dst-only, per the locked model — `src_id` stays `NOT NULL REFERENCES notes`):

```sql
  dst_resource_path TEXT REFERENCES resources(path),  -- resolved resource target, or NULL
  embed             INTEGER NOT NULL DEFAULT 0,       -- ![...]  / ![[...]] (display nicety, not a verb)
  caption           TEXT,                              -- captured alt/link text (slice 3's image index text)
```

- **Dangling** now means `dst_id IS NULL AND dst_resource_path IS NULL`; the partial index
  `edges_dangling_idx` gains the second condition. A resolved-resource edge and a dangling path are
  distinguishable, closing the `[[photo.png]]` half of
  [#12](https://github.com/AlteredCraft/B2/issues/12)'s UX hole.
- **Dedup/identity:** the edge id derivation keeps its identity tuple, with the resolved resource
  path standing in the `dst` position for resource edges; a partial unique index
  `(src_id, dst_resource_path, type, occurrence_index) WHERE dst_resource_path IS NOT NULL` mirrors
  the existing note-edge constraint (SQLite's UNIQUE treats NULLs as distinct, so the existing
  constraint can't cover them).
- **No FK cascade from `resources` deletes is relied on** — pruning (§2) re-resolves inbound edges
  to dangling in the same pass, keeping `full-reindex ≡ incremental-update` exact.

Classification lives in a new `resource.rs` (or `classify.rs`) module: the extension → class table
from [research/file-type-support.md](../research/file-type-support.md) §3, `binary` the total
fallback, `.md` handled by the walk's note route (never a `resources` row). Case-insensitive
extensions; no content sniffing.

## 2. The walk — inventory, hashing, pruning

`collect_md_files` generalizes to **one walk** that routes: dot-prefixed components skipped (as
today), `.md` → the existing note pipeline, everything else → a `resources` upsert. Two-phase
ingest is preserved: all rows (notes *and* resources) land before phase-2 link resolution, so
resolution stays independent of file order.

- **Change detection:** an existing row with unchanged `(size, mtime)` is not re-read; otherwise
  the file's bytes are read once to recompute `content_hash`. (Hashing is the only byte-read this
  slice performs — there is no extraction until slice 3. blake3 makes even media files cheap, and
  the `(size, mtime)` short-circuit makes it once per change.)
- **Pruning:** after the walk, `resources` rows whose paths were not seen are deleted, and inbound
  edges re-resolve to dangling. This is the resource half of
  [#31](https://github.com/AlteredCraft/B2/issues/31)'s ghost-row gap, fixed from day one because
  resources churn more than notes; the note half stays tracked in #31.
- **Skips:** a file whose metadata or bytes can't be read gets a `skipped` entry (path + short
  clean reason — the existing classifier) in the report; the walk continues.
- **Reporting:** `ProjectReport` gains `resources_indexed` and `resources_pruned` counts alongside
  the existing fields, surfaced by both adapters.
- **Move repair (flag-only):** with `content_hash` stored, a dangling resource edge whose hash
  reappears at exactly one new path can be *proposed* as a repair. Slice 1 stores the hash and
  keeps the door open; the surfacing UX ships with the reconcile surfaces later — nothing here
  blocks it.

## 3. The parser — Markdown-native forms, captions, resolution

`link.rs` learns two new inline forms plus the embed marker; `ParsedLink` gains `embed: bool` and
`caption: Option<String>` (the existing `alias` keeps wikilink semantics):

| Form | Edge | `embed` | `caption` |
|---|---|---|---|
| `![alt](path)` | `references` | 1 | alt text |
| `[text](path)` | `references` | 0 | link text |
| `![[file.ext]]` / `![[file.ext\|alias]]` | `references` | 1 | alias if present |
| `[[file.ext]]` (bare wikilink, non-md target) | `references` | 0 | alias if present |

- **Skipped targets:** anything with a scheme (`http://`, `https://`, `mailto:`, …), absolute
  paths, and empty/fragment-only targets. A `#fragment` suffix on a relative target is stripped
  before resolution and preserved in `dst_path_raw`.
- **Resolution base:** a Markdown-form target resolves **note-relative first** (standard Markdown
  semantics, `..` normalized), then vault-root-relative (the wikilink habit) — the same
  try-then-fallback laddering `resolve_link_target` already uses for `+ ".md"`. Wikilink targets
  keep today's vault-root resolution.
- **Kind dispatch is uniform (§9b #8):** a resolved target ending `.md` resolves against `notes`
  (so `[text](other.md)` now projects an ordinary note edge — a small bonus: Markdown-native note
  links stop being invisible); any other extension resolves against `resources` →
  `dst_resource_path`. Unresolved either way → dangling, `dst_path_raw` retained.
- **Scanner parity:** the new forms are scanned with the same line discipline as today's bare
  wikilinks — no new code-fence handling is invented in this slice.

## 4. Façade — three additions and one helper

Per the charter (grow only what commands need) and the locked calls (§9b #8, #10):

```rust
/// Pure dispatch rule, in core so adapters can't drift (research §9b #8), the
/// SAME rule link resolution uses: an extension other than `md` → Resource;
/// `.md` or no extension → Note. Extensionless covers both the wikilink habit
/// (`b2 explain concepts/memory`) and a b2id (ULIDs carry no dot).
pub fn doc_kind(arg: &str) -> DocKind;

impl Vault {
    pub fn list_resources(&self) -> Result<Vec<ResourceSummary>>;   // path, class, size, mtime
    pub fn explain_resource(&self, path: &str) -> Result<ResourceExplainView>;
    pub fn move_resource(&self, path: &str, to: &str) -> Result<MoveReport>;
}
```

- `ResourceExplainView` = the fallback card's data: `path`, `class`, `size`, `mtime`,
  `content_hash`, and `backlinks` — each inbound edge's source note (`b2id`, `path`, `title`) with
  its `type`, `caption`, and `embed` marker. Backlinks come straight off `edges_dst_*` lookups —
  the materialized-graph payoff, no parsing.
- `move_resource` is the note move minus the identity step (data-model.md §10): collect inbound
  link sites from `edges` (the `mv.rs` machinery generalized), rewrite each referencing note's
  link text (all four forms), move the file, re-project the touched notes (their changed chunks
  re-embed through the existing flow), upsert the `resources` row under the new path. Refuses on
  overwrite, like notes.
- `list_notes`, `explain`, `read`, `similar` are **untouched** — note semantics guaranteed, per
  §9b #10. `b2 similar <resource>` errors with a clear "resources become discoverable in a later
  release" message until slice 3 (never a silent empty result).
- Known limit, accepted: an extensionless *file* (`Makefile`, `LICENSE`) dispatches as a note ref
  in CLI arguments — it is still walked, inventoried, and reachable through surfaces that know its
  kind (the desktop tree calls `explain_resource` directly). Documented on `doc_kind`; revisit if a
  real vault hurts.

## 5. CLI

- `b2 explain <arg>` and `b2 mv <arg> <to>` dispatch via `doc_kind` to the note or resource arm;
  `--json` emits the per-kind view types verbatim (they remain the IPC contract). Human output for
  a resource explain: the card's fields + a backlinks list.
- No new subcommand. `list_resources` has no CLI consumer yet — it ships for the desktop; a
  `b2 ls` can adopt it later if a need appears.
- Errors stay generic-and-actionable through `user_message`; a `ResourceNotFound` variant joins
  `CliError`/core errors (thiserror, matched like `NoteNotFound`).

## 6. Desktop

- **Tree:** a `list_resources` command joins `list_notes`; the frontend merges both flat lists into
  the one path-ordered tree (`render.ts` already builds from flat paths), resources tagged with a
  class icon.
- **Fallback card:** selecting any resource (all classes, this slice — viewers land in slice 2)
  renders the card from `explain_resource`: filename, class, size, modified, content hash,
  backlinks with captions, and **Open in system default** via `tauri-plugin-opener` (the one new
  adapter dep; an OS handoff, never in-webview execution — research §6 security posture).
- **Watcher inversion:** `touches_markdown` (allowlist `.md`) becomes the walk's own rule — any
  event path with no dot-prefixed component counts (denylist dotfolders, which already covers
  `.b2/` and `.git/`). A Finder-dropped PNG now pulses `vault-changed`; the existing debounce
  absorbs the extra chatter. The watcher filter and the walk filter are now the same predicate —
  keep them literally shared.
- No CSP/asset-protocol change in this slice — the card is metadata-only. The asset protocol is
  slice 2's key.

## 7. Testing

House pattern throughout — fast suite, fake embedder, fixtures:

- **Fixtures:** `fixtures/golden-vault/` gains `resources/` — a tiny PNG, an HTML snippet, a
  `.txt`, and an unknown-extension blob (`binary`) — plus notes exercising every parser form
  (embed with alt, plain link, `![[…]]`, a dangling resource target, an external URL to skip).
- **Unit:** classification table (parameterized over extensions, per the testing convention);
  `doc_kind` dispatch incl. the extensionless cases; parser forms/captions/fragment-stripping;
  watcher predicate.
- **Integration:** inventory walk + counts; `(size, mtime)` short-circuit vs. hash refresh;
  pruning on delete (edges re-dangle); resolution note-relative → root-relative laddering;
  `explain_resource` backlinks; `move_resource` rewrites all four forms and re-projects
  referencing notes; `full-reindex ≡ incremental-update` extended over resource add/change/delete.
- **Property held everywhere:** drop `b2.sqlite`, reindex, identical — including `resources` and
  the widened edges.

## 8. Build order

| Step | Lands | Proof |
|---|---|---|
| **0** | v4 schema: `resources` + `edges` widening + version bump | reopen-stable; gate drops v3 |
| **1** | `resource.rs`: class table + `doc_kind` | parameterized unit tests |
| **2** | walk generalizes: inventory, hashing, pruning, report counts | golden-vault counts; prune test |
| **3** | parser: new forms, `embed`/`caption` on `ParsedLink` | parser unit tests |
| **4** | resolution: kind dispatch, `dst_resource_path`, dangling semantics | resolution + dangling tests |
| **5** | façade: `list_resources` / `explain_resource` / `move_resource` | integration tests |
| **6** | CLI: explain/mv dispatch, `--json` views, error variant | CLI-level tests |
| **7** | desktop: tree merge, fallback card, opener, watcher inversion | `just check-app`; manual dogfood |

Each step compiles green with the full suite passing before the next begins; steps 0–6 never touch
`b2-desktop`, so `cargo clippy --workspace --exclude b2-desktop` stays the fast gate until step 7.

## 9. Out of scope (owned by later slices)

Viewers and the asset protocol (slice 2) · extraction, resource chunks/vectors, `resource_centroids`,
search/similar coverage, `kind`-tagged search results (slice 3, DDL locked in research §9b #7) · PDF
text + dependency choice (slice 4) · Describer / multimodal embedder (slice 5) · move-repair
*surfacing* UX (hash stored now; proposal surface later) · `.b2ignore` (separate, deferred idea).
