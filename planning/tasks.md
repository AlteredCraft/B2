---
b2id: 01KWSRNSA6ZVQEM14NSS345RRY
title: "B2 — Tasks"
type: note
tags: [b2, tasks, planning]
created: 2026-06-28
updated: 2026-07-05
status: active
---

# B2 — Tasks

Working task queue for B2. Start at [README.md](../README.md) for the map; context lives in
[vision-and-scope.md](vision-and-scope.md) (motivations, principles, design philosophy, capability
areas, v1 scope, locked decisions).

## ⚠ Course correction — 2026-07-04: discovery is similarity + human judgment

**Read this before the Done list below.** Dogfooding the LLM relator on a real 1000+ note vault showed its
per-pair latency and dollar cost don't scale. Decision (mirrored in
[vision-and-scope.md](vision-and-scope.md) "Decisions locked (2026-07-04)", and across
[data-model.md](data-model.md) / [index-engine.md](index-engine.md) /
[specs/index-engine-build.md](specs/index-engine-build.md) / [user-stories.md](user-stories.md) /
[specs/eval-strategy.md](specs/eval-strategy.md)):

- **Cut the LLM relator entirely** — the `Relator` seam, the `b2-relate` crate, and the suggestion queue
  (`b2 suggest` / `accept` / `reject`).
- **Discovery is now `b2 similar`** (surface the semantically-nearest *unlinked* notes — vector KNN over
  stored embeddings, no model, no network) **+ `b2 link`** (you commit a typed relation to frontmatter).
  The human is the precision gate the relator used to be.
- **Collapse to two storage tiers** — drop the `.b2/log/` event log + `replay.rs`; the invariant
  simplifies to `index = projection of (Markdown)`.
- **Backlinks retained** unchanged (`b2 neighbors` / `b2 explain`, inbound + outbound).

So several **Done** items below — *Relator seam*, *Connection-discovery ①/②/③*, *suggest cost controls*,
*Suggestion-quality eval* — were **reverted in code** (commit `2cda889`, 2026-07-04; see "Shipped — the
discovery pivot" below). They stay listed as history; only the code that implemented them was removed.

## Shipped — Desktop UI MVP, read-only (2026-07-05); editing is next

The headless phase is done and the first UI's **read-only MVP has shipped** (Steps 0→3 below, all green).
Full spec + rationale:
[specs/desktop-ui-mvp.md](specs/desktop-ui-mvp.md). It is the **second dumb adapter over the
[`Vault`](../crates/b2-core/src/vault.rs) façade** — a **Tauri** desktop app (`crates/b2-desktop`, thin-host
charter in [crates/b2-desktop/CLAUDE.md](../crates/b2-desktop/CLAUDE.md)) + a **CodeMirror** frontend
(`ui/`), talking to the core over Tauri IPC. The MVP is **read-only-first**: render a note, surface its
similar-but-unlinked notes, commit a typed link — the connection-discovery loop, made visual.

- [x] **Step 0 — Scaffold & wiring.** `crates/b2-desktop` (Tauri **v2** host) + `ui/` (Vite frontend) added
  to the explicit workspace `members`; the window boots and `invoke('ping')` round-trips — the Rust↔JS seam
  proven end-to-end (boot-smoke verified, no startup panic). **`ui/` framework resolved (spec §9): vanilla
  TypeScript** — no framework, the thinnest dep tree and cleanest CSP story, matching the repo's
  no-speculative-abstraction ethos. (CodeMirror is **not** pulled in yet; it enters `ui/` at Step 4 when
  editing lands — the read-only MVP renders Markdown → HTML via `marked`, so an unused editor dep would be
  speculative.)
- [x] **Step 1 — Read a note.** The one new façade op [`Vault::read(note) → NoteView`](../crates/b2-core/src/vault.rs)
  (body from disk + display metadata; a pure model-free read) + a `read_note` command; the left pane renders
  the note (Markdown → HTML) with clickable in-app wikilinks (a `marked` inline extension → `.wikilink`
  anchors that navigate via `read_note`). 6 `read` integration tests in [tests/read.rs](../crates/b2-core/tests/read.rs).
- [x] **Step 2 — The related pane.** `similar` / `search` / `explain` / `neighbors` commands (all existing
  façade ops); the right pane shows similar-but-unlinked candidates and hybrid-search results with snippets
  (click to open), plus the open note's typed connections. Honest "semantic off (run `b2 init`)" caveat when
  the fake embedder is active, mirroring `b2 search`.
- [x] **Step 3 — Commit a link (read-only MVP done).** A `link` command + a modal **verb picker over the
  closed 10-verb core**; committing writes the frontmatter `relations:` through the façade and the discovery
  pane refreshes (the linked candidate leaves "similar", appears in "connections"). The read → discover →
  link loop is visual end-to-end. The host stays a **dumb adapter** (each command is deserialize → one façade
  call → serialize; charter: [crates/b2-desktop/CLAUDE.md](../crates/b2-desktop/CLAUDE.md)); errors funnel
  through a `user_message` mirror of the CLI's, never leaking internals. **4 command-layer tests** +
  clippy/fmt clean; the desktop build is a separate heavier job, out of the fast `cargo test -p b2-core` gate.
- [x] **File-tree navigation pane.** A left-most pane that lists the vault as collapsible folders — navigate,
  open a folder, click a file to read it — so the app is browsable, not just searchable. One new façade op
  [`Vault::list_notes() → Vec<NoteSummary>`](../crates/b2-core/src/vault.rs) (b2id/path/title, **no body**;
  a pure model-free read, path-ordered) + a thin `list_notes` command; the tree is folded from the flat list
  **in `ui/`** (presentation logic stays out of the host). Index-first like `search`: it shows exactly the
  notes the index knows, so every entry is `read`-resolvable (a click always opens); opening from search /
  wikilink auto-expands the note's ancestor folders and highlights it. **3 `list_notes` integration tests**
  ([tests/list.rs](../crates/b2-core/tests/list.rs)) + a command-layer test; clippy/fmt clean.
- [x] **Frontmatter drawer.** A full-bleed, collapsible strip across the top of the note pane showing the
  note's **raw frontmatter YAML verbatim** — the block between the `---` fences, so `relations:`, `aliases:`,
  and any keys the projected fields don't model show *as written*. Extends the existing read op rather than
  adding one: `NoteView` gains a byte-honest [`frontmatter: Option<String>`](../crates/b2-core/src/vault.rs)
  field, sourced from a new [`ParsedNote::frontmatter()`](../crates/b2-core/src/note.rs) accessor (raw span,
  lossless like `body()`). Always present so the pane chrome is stable; a note with no block unfolds to an
  explicit "No frontmatter." The host is untouched — it passes the widened `NoteView` straight through
  (dumb-adapter rule holds). **1 `read` test (verbatim block) + 3 `note` unit tests**; the drawer render is
  state-controlled in `ui/` (survives the full-pane re-renders a toast/tree-toggle triggers). Read-only —
  editing frontmatter arrives with the CodeMirror editor (Step 4).
- [x] **View-source toggle.** A `</>` button pinned top-right of the note pane's bar (opposite the Frontmatter
  toggle) flips the note body between rendered Markdown and its **raw source** (`NoteView.body` verbatim in a
  `<pre>`, HTML-escaped, wikilinks shown literally). Presentation-only — **no new façade op**, reuses the body
  `Vault::read` already returns; state-controlled and sticky across notes like the frontmatter drawer. A
  read-only peek at the on-disk Markdown ahead of the CodeMirror editor (Step 4).

**Now the fast-follow (specced, next up):** CodeMirror 6 body editing + `Vault::write` + an `mtime` guard
(Step 4); native fs-watch auto-reload (Step 5). **Also not yet:** an in-app vault picker (today it's a launch
arg / `B2_VAULT_PATH`); live reindex progress (a long reindex runs off the main thread, but with no progress
bar — background reindex is on the Backlog). **Deferred:** packaging/signing/distribution; a `serve` HTTP
adapter; graph visualization ([specs/desktop-ui-mvp.md](specs/desktop-ui-mvp.md) §9).

## Done

- [x] **Motivations & problem** — folded into [vision-and-scope.md](vision-and-scope.md)
  ("Why I'm building it").
- [x] **Vision & scope** — [vision-and-scope.md](vision-and-scope.md), including v1 scope and the
  three locked decisions (2026-06-28: semantic is engine-gated, full CRUD in CLI, v1 discovery =
  links only).
- [x] **Data model** — [data-model.md](data-model.md): note + edge as the Markdown source of truth,
  `[[path|title]]` links keyed by `b2id`, inline typed relations, the three-tier model (Markdown /
  disposable index / durable `.b2/` event log), provenance + suggestion lifecycle, OKF compatibility,
  and a golden-vault fixture. All judgment calls resolved 2026-06-29: edge-provenance → event log
  (accepted edges stay pristine); `b2id` is B2's one always-allowed write; bare links = directed
  `references`; a 10-verb relation core + tolerated tail. Identity key in
  [index-engine.md](index-engine.md) realigned to `b2id`. **§0 revised 2026-06-30** (Decision 1–3): B2
  never authors the body; accepted edges live in frontmatter `relations:` (Format A); graph = union of
  body ∪ frontmatter ∪ log, inline-wins dedup.
- [x] **Language gate** — **Rust** (`crates/b2-core`), per the single-binary goal
  ([index-engine.md](index-engine.md) §7). rusqlite (bundled SQLite + FTS5) + `sqlite-vec`.
- [x] **Index-engine build, steps 0→5** — [specs/index-engine-build.md](specs/index-engine-build.md), all
  green against the golden-vault fixture: (0) DB substrate proving FTS5 + `sqlite-vec` coexist; (1)
  lossless parse/serialize, `b2id` stamping, `b2id ⇄ path` resolver; (2) `chunks` (+FTS5) + the typed
  `edges` graph + `neighbors` (minimal paragraph chunker; qmd heuristic deferred to a real-embedder eval);
  (3) `chunks_vec` + the embedder seam (deterministic fake; real model deferred); (4) the `.b2/` JSONL
  event log + replay (suggestions inert; drop→replay reproduces the queue; rejection tombstones); (5)
  hybrid retrieval — BM25 ⊕ vector → RRF (k=60) + the graph⨝vector join.
- [x] **Suggestion lifecycle, end-to-end** — generate → list → **accept** (append to frontmatter
  `relations:`, Markdown-first, re-project as `origin=frontmatter`) / reject (tombstone). Frontmatter
  `relations:` reader + inline-wins dedup. Survives drop→rebuild→replay; accepted edges stay pristine.
- [x] **`b2` CLI over a typed Core API** — the walking skeleton. A `b2_core::Vault` façade
  (`open`/`reindex`/`neighbors`/`search`; a note ref resolves by path **or** `b2id`) is now the one
  typed contract, and a `b2-cli` crate (binary `b2`) is a *dumb* adapter over it — parse args, call the
  façade, print — with a `--json` mode for agents. Index + log live in `<vault>/.b2/` (one portable
  folder). Ships the deterministic `FakeEmbedder`: `search`'s BM25 half is real, the vector half is not
  yet semantic (the CLI says so, never overstating). First real dogfooding moment — point B2 at a folder
  and explore its graph + search from the terminal. Façade + CLI-level tests (67 total).
- [x] **Real embedder + eval suite** — honest semantic `search` now ships. A new **`b2-embed`** crate
  holds the candle-backed **`LocalEmbedder`** behind the existing [`Embedder`](../crates/b2-core/src/embed.rs)
  seam (CLS-pool + L2-normalize, asymmetric `embed_query` prefix), so `b2-core` stays candle-free and the
  fast CI suite runs only the fake. `b2 init` downloads + **verifies** (loads + embeds a probe) the model
  into a shared XDG cache; `reindex`/`search` **fail fast** with "run `b2 init`" if absent. Config is a
  global TOML (`[embedder] model / source / cache_dir`), source overridable to a mirror/repo/local path.
  The `open()`-time drop is fixed: `open` never mutates the vector space; a model/dim mismatch **fails
  fast** on `search` and re-embeds only on `reindex`. Eval is a separate `--example eval` (out of CI)
  scoring precision/MRR over a hand-labelled set. **Decision change (2026-07-01):** EmbeddingGemma-300M is
  **gated** on HF (HTTP 401 without a token + license click — defeats a friction-free `b2 init`), so the
  default is the pre-authorized fallback **BAAI/bge-base-en-v1.5** (BERT, 768-dim, ungated), validated in
  the spike. Also fixed a real bug the eval surfaced: NL queries with punctuation crashed FTS5 —
  `keyword_search` now sanitizes to a safe `MATCH`. **73 tests** (all fake/deterministic in CI); the
  real model is exercised only by `b2 init` and the eval example.
- [x] **Relator seam** — the classify/explain step of connection discovery now sits behind a swappable
  **`Relator`** trait ([relate.rs](../crates/b2-core/src/relate.rs)), mirroring
  [`Embedder`](../crates/b2-core/src/embed.rs): `relate(anchor, candidate) -> Result<Option<Proposal>>`,
  **pairwise**, with `Ok(None)` as a first-class **decline** — candidate generation over-produces, and the
  relator is the precision gate that prunes. `Proposal { edge_type, explanation, confidence }` maps 1:1 onto
  [`generate_suggestion`](../crates/b2-core/src/suggest.rs) (relator owns type/explanation/confidence + `by`
  via `model_id`; candidate-gen owns src/dst/`source`). Ships the deterministic **`FakeRelator`**
  (content-addressed on the `b2id` pair like `FakeEmbedder`; always emits a **core** verb, declines 1-in-4
  to exercise the prune path) so the pipeline is provable with no LLM. The real LLM-backed relator is
  deferred to its own crate (the `LocalEmbedder`/`b2-embed` precedent), keeping `b2-core` model-free. 5
  relator tests; **78** workspace tests green.
- [x] **Connection-discovery ① + candidate generation** — the first discovery stage now exists. **① resolved
  2026-07-01**, mirrored to [index-engine.md](index-engine.md) §3 + [docs/architecture.html](../docs/architecture.html)
  (new Connection-discovery section + relator seam): a candidate is the graph's *complement* — **near ∖
  connected** — not the intersection ([`graph_filtered_search`](../crates/b2-core/src/search.rs) is a
  scoped-traversal primitive, the wrong tool). Mechanism: per anchor chunk, KNN its **stored** `chunks_vec`
  vector (vector-only, **no re-embed**, passage↔passage — `embed_query`'s asymmetric prefix is the wrong
  side); score each other note by its **best** chunk-pair (**max-sim**); subtract
  [`reachable_within`](../crates/b2-core/src/graph.rs)`(anchor, 1)` (distance is **exclusion-only** — 2-hop
  triadic-closure candidates survive unboosted; distance-weighting is a backlog eval experiment); rank →
  top-N. Anchor text is **per-chunk**, not whole-note. Built
  [`discover::candidates`](../crates/b2-core/src/discover.rs) (+ db readers `chunks_for_note` / `chunk_vector`,
  `embed::unpack_f32`); 7 discover tests, **85** workspace tests green.
- [x] **Connection-discovery ② — the generate pipeline, wired end-to-end** — the glue that finally turns the
  three built pieces into suggestions now exists:
  [`discover::generate_for_anchor`](../crates/b2-core/src/discover.rs) + a
  [`generate_all`](../crates/b2-core/src/discover.rs) over the vault. Per anchor:
  [`candidates`](../crates/b2-core/src/discover.rs) → assemble the relator's borrowed inputs (anchor +
  per-candidate [`NoteCtx`](../crates/b2-core/src/relate.rs) / [`Candidate`](../crates/b2-core/src/relate.rs),
  `evidence_chunk` = [`db::chunk_text`](../crates/b2-core/src/db.rs), `signal="semantic:maxsim"` → the
  suggestion's `source`) → [`Relator::relate`](../crates/b2-core/src/relate.rs) → on `Some`, **validate
  [`relation::is_core`](../crates/b2-core/src/relation.rs)** (a real relator's verb is checked, not trusted —
  a non-core proposal is dropped + counted, never persisted) →
  [`suggest::generate_suggestion`](../crates/b2-core/src/suggest.rs) (`by="agent:<model_id>"`). Deterministic
  + idempotent like the rest of the core: `created`/`IdGen` passed in, anchors iterated in **sorted b2id
  order** ([`db::all_note_ids`](../crates/b2-core/src/db.rs)), and `generate_suggestion`'s `edge_exists` guard
  means a re-run proposes nothing new — every candidate lands in exactly one of
  `{generated, declined, non_core, existing}` ([`GenerateOutcome`](../crates/b2-core/src/discover.rs)).
  **Sub-decision resolved:** `NoteCtx.text` is the note's chunks joined
  ([`db::note_text`](../crates/b2-core/src/db.rs)) — the body as the index already holds it, cheapest-correct
  (a real relator reads it; `FakeRelator` ignores it, content-addressed on b2ids). Runs fully on
  `FakeRelator`, no LLM. **7 pipeline tests** (purpose-built relator stubs drive fire-core / decline /
  tail-verb exactly; `FakeRelator` proves the seam runs through; determinism across rebuild; idempotent
  re-run; queue survives drop→rebuild→replay); **92** workspace tests green.

- [x] **Connection-discovery ③ — the CLI + façade surface** — `suggest` / `accept` / `reject` now ship, so
  the review queue is reachable from the terminal. Four ops on the [`Vault`](../crates/b2-core/src/vault.rs)
  façade (`generate_suggestions` wrapping [`discover::generate_all`](../crates/b2-core/src/discover.rs) on the
  `FakeRelator`; `list_suggestions` resolving both ends to path+title as `SuggestionView`;
  `accept_suggestion` / `reject_suggestion`), and the `b2 suggest` (generate-then-list, idempotent) /
  `b2 accept <id>` / `b2 reject <id>` commands with `--json`. Wiring decisions: `suggest` needs **no model**
  (candidate-gen reads stored vectors, the relator is a stub) so it opens with the fake like `neighbors`;
  `accept` re-projects (re-embeds) the source note so it loads the **same embedder the index was built with**
  (real model, like `reindex`); `reject` touches no vectors. Timestamps come from **SQLite** (the
  `indexed_at` clock) via a façade `now()`, keeping `b2-core` wall-clock-free (engine ops still take
  `created`/`decided`). Honest to the user: `suggest` prints a loud **stub-relator caveat** + a generation
  summary on stderr (stdout stays pure results); a bad `accept`/`reject` id is a clean nonzero exit
  (`CliError::SuggestionNotFound`), no internals leaked. **6 CLI tests** (generate+list human/JSON,
  empty-before-reindex, accept writes the frontmatter link + leaves the queue, reject tombstones,
  accept/reject JSON shapes, unknown-id fails cleanly); **98** workspace tests green.

- [x] **Reindex performance + progress (fast-follow)** — dogfooding a ~1000-doc vault surfaced that
  `reindex` *looked* frozen and was genuinely glacial. Three fixes: (1) a **live progress line** — the embed
  phase reports per batch ([`ingest::ReindexProgress`](../crates/b2-core/src/ingest.rs) via
  `ingest_vault_with_progress` / [`Vault::reindex_with_progress`](../crates/b2-core/src/vault.rs)); the CLI
  prints a live `embedding n/N · <path> (k chunks)` line on an interactive stderr (TTY-gated — off in `--json`
  and pipes, so tests stay clean) — refined 2026-07-05 to count notes that *actually* embed + name the vault
  (see the entry below). (2) **Batched embedding** — a new [`Embedder::embed_batch`](../crates/b2-core/src/embed.rs)
  (default maps `embed`; `LocalEmbedder` overrides with one padded forward pass — CLS + attention mask, so a
  batched row equals the single embed, proven by an out-of-CI `--ignored` test). (3) **Apple Accelerate** for
  candle's CPU matmuls (macOS-gated in `b2-embed`), the real multiplier: a 160-chunk reindex went **84s → 11s**
  wall (~70× less CPU work) with retrieval-eval quality unchanged. **100** workspace tests green.

- [x] **Incremental reindex + `--force` (fast-follow)** — reindex no longer re-embeds the whole vault every
  run. A note whose body hash (stored in `notes.body_hash`;
  [`db::note_body_hash`](../crates/b2-core/src/db.rs)) is unchanged *and* whose chunks all still have vectors
  ([`db::note_fully_embedded`](../crates/b2-core/src/db.rs)) reuses them verbatim — its `pending` embed work is
  empty, so nothing is re-embedded. A model swap (which empties `chunks_vec`) or `b2 reindex --force`
  re-embeds everything; `ReindexReport` now reports `embedded` next to `indexed`/`stamped`. Frontmatter-only
  edits (e.g. an accepted relation) still re-project the note + edges but skip re-embedding — so `accept` got
  cheaper too. The invariant (`incremental ≡ full rebuild`) holds because the reused vectors are byte-identical
  to a fresh embed. Real-model check: an unchanged reindex of a 4-doc / 160-chunk vault dropped **2.7s →
  0.09s** (mmap means the weights aren't even faulted in when there's nothing to embed); editing one note
  re-embeds only it. **102** workspace tests green.

- [x] **`b2 mv` — move/rename + inbound-link repair (Story 1)** — the first note-authoring kernel op ships,
  directly realizing the locked invariant **"rename keeps every backlink resolving"**
  ([user-stories.md](user-stories.md) Story 1). A new [`mv::move_note`](../crates/b2-core/src/mv.rs) on the
  [`Vault`](../crates/b2-core/src/vault.rs) façade + a `b2 mv <from> <to>` command (`--json`). The graph
  **never breaks** because edges key on `b2id`: the move leaves the target's id untouched, so `neighbors` /
  backlinks show the same set before and after — only the human convenience-copy `[[oldpath|alias]]` text is
  repaired. Mechanism, **Markdown-first** (mirrors `accept`): [`db::inbound_edge_targets`](../crates/b2-core/src/db.rs)
  reads the materialized graph to name **exactly** the inbound files + link strings to rewrite (bounded, never
  an O(vault) scan — [index-engine.md](index-engine.md) §8) → a byte-preserving `rewrite_links` swaps only the
  target token (surrounding whitespace + `|alias` kept; a prefix-sharing `[[foo-bar]]` is never touched when
  moving `foo`; each link keeps its own `.md`-or-not convention) → move the file (creating parent dirs) →
  re-project the moved note first (its `notes.path` current before inbound links re-resolve) then each rewritten
  file. **Not logged** — a move is fully reconstructible from Markdown (files at new paths, `b2id`s intact), so
  replay is untouched. Destination is validated (empty / absolute / `..`-escaping / onto-an-existing-file all
  refused with clean generic errors: [`Error::MoveDestination`](../crates/b2-core/src/error.rs) /
  `MoveTargetExists`); `.md` is optional. The CLI opens the real model (rewriting an inbound file changes its
  body → it re-embeds), like `reindex`/`accept`. **15 new tests** (6 `rewrite_links` unit: alias/whitespace
  preservation, prefix-safety, `.md` variants; 9 façade integration: graph-unchanged, byte-exact inbound diff,
  unrelated files untouched, subdir creation, `.md`-optional, clobber/invalid/unknown-src errors, prefix-sibling
  safety); **117** workspace tests green.

- [x] **The real relator — Claude-backed, in its own crate** — the intelligence is no longer a stub: `b2 suggest`
  now makes genuine typed judgments. A new **`b2-relate`** crate holds the **`ClaudeRelator`** behind the existing
  [`Relator`](../crates/b2-core/src/relate.rs) seam (the `b2-embed`/`LocalEmbedder` precedent — `b2-core` stays
  model-free; heavy/IO deps live only here). **Decisions:** backend is **pluggable, Claude first** — a config
  `[relator] backend` selects it ([`RelateConfig`](../crates/b2-relate/src/config.rs), same global TOML the embedder
  reads), so a local/Ollama backend drops in behind the same seam later. Rust has no official Anthropic SDK, so the
  transport is **raw HTTP over `ureq`** (already in the lock tree via `hf-hub`; synchronous — no `tokio`, per the
  no-speculative-async rule). Structured output is **forced tool use**: one `classify_relation` tool whose
  `input_schema` pins `relation` to the closed core set ([`relation::CORE`](../crates/b2-core/src/relation.rs)) via
  `enum` + `tool_choice`, so the model returns a typed verdict, not free text — and the pipeline still **re-validates
  [`is_core`](../crates/b2-core/src/relation.rs)** (a real relator's verb is checked, never trusted). Default model
  **`claude-opus-4-8`** (config-overridable to `claude-haiku-4-5` for cheap high-volume runs); the **API key comes
  from `ANTHROPIC_API_KEY`** (never the config file — secrets policy) and is validated at construction, so a missing
  key **fails fast** with an actionable message, never a mid-run 401. **Injection sub-decision:** the relator is
  passed **as an argument** to [`Vault::generate_suggestions`](../crates/b2-core/src/vault.rs)`(&dyn Relator, top_n)`,
  *not* held on the façade like the embedder — it has a single consumer, so an argument keeps the façade surface
  minimal while still keeping `b2-core` model-free (the façade already reads `NoteCtx.text` from
  [`db::note_text`](../crates/b2-core/src/db.rs)). New [`Error::Relator`](../crates/b2-core/src/error.rs) (the
  discovery-seam parallel of `Error::Embed`); the CLI adds `CliError::Relate` + generic, no-internals-leaked messages,
  selects the real relator by default and the deterministic stub under **`B2_RELATOR=fake`** (or `B2_EMBEDDER=fake`,
  keeping the model-free CLI suite driving the stub), and the loud stub caveat **comes off** under the real relator.
  **10 model-free `b2-relate` tests** (config defaults/overrides/unknown-backend/`[relator]`-table parse; request
  forces the tool + pins the verb enum + carries the evidence chunk; response parse: fired proposal / decline /
  verb-less-degrades-to-decline / confidence clamp+default / no-tool-call-is-an-error) + a **`#[ignore]` live smoke**
  test (real key, out of CI); **127** workspace tests green.

- [x] **`b2 suggest` cost controls — progress, token usage, pre-call dedup (fast-follow)** — dogfooding the real
  relator on a live vault surfaced that `suggest` is the one **paid, network-bound** command, and it was neither
  observable nor incremental. Three fixes: (1) **live progress** — a [`SuggestProgress`](../crates/b2-core/src/discover.rs)
  callback (the [`ReindexProgress`](../crates/b2-core/src/ingest.rs) analog) via `generate_all_with_progress` /
  [`Vault::generate_suggestions_with_progress`](../crates/b2-core/src/vault.rs); the CLI renders
  `judging… note i/N · k call(s) · g new` on an interactive stderr (TTY-gated, off in `--json`/pipes). (2) **Token
  usage** — [`ClaudeRelator`](../crates/b2-relate/src/claude.rs) sums each response's `usage` block into atomics
  (so `&self` suffices — no `Relator` trait change) and exposes [`Usage`](../crates/b2-relate/src/claude.rs); the CLI
  prints `~ N input + M output tokens over C call(s)` for the real relator (tokens, not dollars — pricing drifts).
  The full tally (`generated · declined · non_core · existing`) is surfaced, not just `generated`. (3) **Pre-call
  dedup — idempotent in _cost_, not just effect.** The idempotency guard fired *after* the paid call, so a re-run
  re-classified every pending/rejected pair. Now [`generate_for_anchor`](../crates/b2-core/src/discover.rs) checks
  [`db::edge_exists_for_pair`](../crates/b2-core/src/db.rs) (any type, any status) **before** `relate()` and skips a
  settled pair (pending suggestion or rejection tombstone) with no model call — so a re-run pays only for genuinely
  new pairs. Deliberately **pair-level** (the type isn't known until after the call), a small strengthening of the
  per-`(pair,type)` tombstone. *Declines* leave no edge so they still re-pay (the `body_hash` anchor-skip below is
  the follow-up). Instrumentation + dedup mirrored to [docs/discovery.html](../docs/discovery.html). **2 new tests**
  (re-run makes zero relator calls for pending pairs; a rejected pair is never re-judged); **129** workspace tests green.

- [x] **Suggestion-quality eval — the harness + seed labelled set** — the relator makes typed judgments but nothing
  scored them; now the measurement exists. A new **`cargo run -p b2-relate --example suggest-eval`**
  ([suggest-eval.rs](../crates/b2-relate/examples/suggest-eval.rs)), the relator-side parallel of the retrieval eval
  in `b2-embed` — an **example, not a test**, so a real key + spend + model non-determinism never touch the
  deterministic CI suite. **Decision (2026-07-03), mirroring the "isolated pairs" answer:** it scores the
  [`Relator`](../crates/b2-core/src/relate.rs)'s **judgment in isolation** — it does **not** build a vault or run
  candidate-gen/the embedder — so the number measures the precision gate itself, not entangled candidate-gen quality
  (that stays a separate, separately-tuned concern). It feeds hand-labelled note pairs
  ([evals/pairs.json](../crates/b2-relate/evals/pairs.json): 22 pairs over an 18-note
  [evals/corpus/](../crates/b2-relate/evals/corpus)) straight to the real
  [`ClaudeRelator`](../crates/b2-relate/src/claude.rs) and scores **firing precision** (the over-firing gate — the
  relator's whole job), **firing recall**, and **verb accuracy** (gold lists *every defensible* core verb per pair,
  most-apt first, because the vocabulary genuinely overlaps — exact-match-only would report fake errors). Declines
  deliberately include "same-topic but not connected" traps (sibling brewing methods, a direction-reversed pair)
  since over-firing is the primary failure mode; the seed set covers all 10 core verbs. Honest engineering: labels are
  **validated up front** (unknown note / non-core verb / evidence-not-a-substring all fail fast **before any paid
  call** — a data typo costs nothing), a per-pair table + a "misses" block (with the labeller's comment) make tuning
  legible, per-run token usage is reported, and a soft precision floor (0.75, the retrieval eval's `p@1` precedent)
  exits non-zero for a manual gate. **8 model-free scoring tests** (gold parse/validate, the four confusion quadrants,
  frontmatter parse, evidence resolution) run via `cargo test -p b2-relate --examples`, **out of** the default
  suite like the retrieval eval — **129** workspace tests unchanged. Strategy + first-run baseline documented in
  [specs/eval-strategy.md](specs/eval-strategy.md) (covers both evals — retrieval + suggestion — as one out-of-CI
  model-quality pass).

- [x] **`b2 add` + `b2 explain` — the note-authoring kernel CRUD** — the two remaining note-authoring ops ship, so a
  vault can be **created and understood** from the terminal, not just moved/searched. **`b2 add <path> [--title]
  [--content]`** ([`add::add_note`](../crates/b2-core/src/add.rs) on the [`Vault`](../crates/b2-core/src/vault.rs)
  façade) creates a new note and projects it — it's immediately in the graph + searchable. **Markdown-first** (mirrors
  `mv`/`accept`): render a minimal valid frontmatter (`type: note`, an optional YAML-quoted `title`, today's `created`
  from the façade's SQLite clock — [`Vault::today`](../crates/b2-core/src/vault.rs), the `now()` precedent keeping
  `b2-core` wall-clock-free) with **no `b2id`**, write the file, then re-project via
  [`ingest::ingest_file`](../crates/b2-core/src/ingest.rs) — which **stamps the `b2id`** (and logs it): one
  "stamp on first sight" identity path for every note ([data-model.md](data-model.md) §1), so a created note is fully
  reconstructible from Markdown ∪ log and needs no bespoke event. Refuses to clobber
  ([`Error::AddTargetExists`](../crates/b2-core/src/error.rs)) and validates the path
  ([`Error::AddDestination`](../crates/b2-core/src/error.rs)); the empty/absolute/`..`-escaping + `.md`-append
  normalizer is now shared with `mv` ([`pathspec::normalize_rel_md`](../crates/b2-core/src/pathspec.rs)). Creates missing
  parent dirs; embeds the new body → opens the real model like `reindex`/`accept`/`mv`. **`b2 explain <note>`**
  ([`Vault::explain`](../crates/b2-core/src/vault.rs) → `ExplainView`) presents a note's connections **with their
  "why"**: the note's header (title/path/`b2id`) + every active typed edge, grouped by direction with each edge's
  explanation, and an **orphan flag** when nothing points at it (surfaced, never acted on — [user-stories.md](user-stories.md)
  Story 2). A pure graph read (no embed, like `neighbors`), reusing [`NeighborView`](../crates/b2-core/src/vault.rs) via a
  shared `neighbors_of`. [`graph::Neighbor`](../crates/b2-core/src/graph.rs) gained **`origin`** so `explain` shows
  `inline` (a human body link) vs `frontmatter` (a B2-committed relation) provenance — the distinction
  [data-model.md](data-model.md) §0 says `explain` surfaces. **26 new tests** (3 `pathspec` + 4 `add` unit; 7 `add` + 6
  `explain` façade: stamp-logged, edge projection, clobber/invalid-path/unknown-note clean errors, `.md`-optional,
  subdir creation, provenance, orphan; 6 CLI: add/explain human+JSON, clobber/orphan/unknown clean exits); **155**
  workspace tests green.

- [x] **`b2 reindex --dry-run` — a read-only preview (fast-follow)** — the last open CLI knob ships: a dry-run that
  reports what a reindex **would** do (`would_index` / `would_embed` / `would_stamp`) while writing **nothing** — no
  `b2id` stamped to the Markdown (B2's one vault write, [data-model.md](data-model.md) §1), no index/log mutation, no
  embedding — so a user can preview against a pristine vault. [`ingest::plan_reindex`](../crates/b2-core/src/ingest.rs)
  walks the files (same sorted order + dotfolder skip as ingest) and decides, per note, would-stamp (the file lacks a
  `b2id`) and would-(re)embed. **No drift:** the embed decision is a new
  [`would_reembed`](../crates/b2-core/src/ingest.rs) predicate now **shared** with the real incremental path
  (`project_note_and_chunks` was refactored to call it — behavior-identical, proven by the unchanged incremental/force
  tests), so the preview can't diverge from the run; a `space_exists` guard lets a never-embedded vault short-circuit
  without querying a not-yet-existing `chunks_vec`. The façade
  [`Vault::plan_reindex`](../crates/b2-core/src/vault.rs) returns a dedicated **`ReindexPlan`** whose past-tense-free
  `would_*` keys are the honesty signal (distinct from `ReindexReport`); a dry-run **needs no model** (pure read, opens
  with the fake like `neighbors`), so previewing never forces a `b2 init`. **Documented limitation:** it previews an
  incremental run under the embedder the index was built with and does **not** detect a pending model swap (that needs
  the real model loaded, which a dry-run avoids). **5 new tests** (4 façade: counts match a real reindex then go to
  zero, `--force` would-re-embed-all, edit flags exactly one note, and byte-identical files + empty index prove no
  write; 1 CLI: `would_*` JSON shape + "No changes made" + a following real reindex still does all the work); **160**
  workspace tests green.

## Shipped — the discovery pivot (2026-07-04), in three phases

Phase 0 (docs) mirrored this file and six others to the pivot; the code landed in commit `2cda889`
(129 workspace tests green, clippy + fmt clean). What shipped:

- [x] **Phase 1 — additive code (`b2 similar` + `b2 link`); the new surface, built before deleting anything.**
  - `Vault::similar(note, limit)` over [`discover::candidates`](../crates/b2-core/src/discover.rs)
    (model-free) → **`b2 similar <note>`**: notes ranked by embedding proximity minus the
    already-connected, each with path · title · score · evidence snippet; default 10; no model call; `--json`.
  - The frontmatter-append extracted into `Vault::link(src, dst, type=references, explanation?)`
    ([`note::add_relation`](../crates/b2-core/src/note.rs)) → **`b2 link <src> <dst> [--type] [--explanation]`**:
    Markdown-first write to `relations:` + re-project; idempotent. A body link stays a manual edit,
    picked up on the next reindex.
- [x] **Phase 2 — deletions.** Removed the `b2-relate` crate; `relate.rs`; the generate pipeline in
  `discover.rs` (`generate_for_anchor` / `generate_all*` / `GenerateOutcome` / `SuggestProgress`) — **kept
  `candidates`**; the suggestion lifecycle in `suggest.rs` (**kept** the extracted link primitive); `replay.rs`;
  the log tier in `event.rs`; the suggestion DB machinery (`status`, `origin='suggested'`, `edge_provenance`,
  the review-queue readers); the façade + CLI suggestion surface (`suggest`/`accept`/`reject`, `B2_RELATOR`);
  and the `relate`/`generate`/`suggestions` tests (`accept` tests adapted → `link`). Schema bumped to **v2**
  with a disposable-index rebuild gate.
- [x] **Phase 3 — verify.** `cargo build` / `test` / `clippy --workspace` / `fmt` all clean; the two-tier
  invariant confirmed (drop `b2.sqlite` → reindex identical, backlinks intact) and the
  `similar` → `link` → `neighbors` loop exercised end-to-end.

**Not in scope (keep discovery thin):** a reranker (a one-stage insertion after RRF,
[index-engine.md](index-engine.md) §5, still a clean fast-follow); query expansion; the actual
packaging/distribution build. **Unlocks (now available):** the qmd chunker upgrade — a real embedder can
finally score paragraph vs. qmd chunking (build spec §1.2); and ranking-quality tuning the retrieval eval can
now measure (e.g. the keyword-half stopword noise the first eval pass surfaced) — which lifts `b2 similar`
directly, since it reuses the same stored vectors.

## Shipped — reindex UX fixes (2026-07-05)

- [x] **Reindex progress line — count real work, name the target (fast-follow).** Dogfooding surfaced that the
  progress line reported *position in the full note list* (`note 14/18`), which jumped and disagreed with the
  final `embedded` count on an incremental run. [`ingest::ReindexProgress`](../crates/b2-core/src/ingest.rs) now
  carries `note_path` / `note_chunks` / `notes_embedded` / `notes_to_embed`; `ingest_vault_with_progress` stages
  all projections before embedding so the `n/N` denominator is the notes that actually embed (it equals the
  report's `embedded`). The CLI prints an `Indexing <vault>` header + `embedding n/N · <path> (k chunks)` on an
  interactive stderr. Test `reindex_with_progress_reports_cumulative_and_fully_embeds` updated.
- [x] **Write commands require an explicit vault — no silent cwd fallback.** A stale binary (built before the
  `B2_VAULT_PATH` env landed) and a mistyped var both silently indexed the *current directory*, leaving a stray
  `.b2/`. The global `--vault` is now `Option<PathBuf>` (no `default_value`); read-only commands (`search`,
  `neighbors`, `explain`, `similar`) still fall back to `.` (a pure read can't pollute), but **every command that
  writes** — `reindex`, `add`, `mv`, `link` — resolves through `Cli::require_vault` (positional → `-C`/
  `$B2_VAULT_PATH`, else `CliError::VaultRequired`). CLI test `write_commands_refuse_without_an_explicit_vault`
  covers all four; README + CLAUDE.md + the docs site (quickstart, indexing) mirrored.

## Backlog (later, not now)

- **Non-blocking embedding — deferred approaches** (incremental reindex is *done*; these tackle the one part
  it can't, the first cold index of a large vault, and all compose with it):
  - **Background reindex + `b2 status`** — `b2 reindex` detaches and returns immediately; a separate process
    embeds while `search`/`suggest` read the index live (SQLite WAL already permits one writer + concurrent
    readers across processes). Cost: a background-process lifecycle + cross-process progress. The most direct
    answer to "embedding can't block" for a cold index.
  - **Progressive (keyword-first) index** — insert all chunk text + FTS up front so BM25 keyword search works
    immediately, then embed vectors in the background so the semantic half fills in behind it. Best
    "usable during a long cold index" feel; pairs with the background runner.
  - **Faster / smaller embedder** — swap bge-base (768-dim) for bge-small (384-dim, ~3× faster) or a
    quantized / ONNX path to cut per-chunk cost. A raw-speed lever behind the existing `Embedder` seam, not a
    structural fix — measure retrieval quality (the eval) before switching the default.
- **Docs: keep the HTML + test-count badge in sync.** The 2026-07-04 pivot returns the workspace to **three**
  crates (`b2-core`/`b2-embed`/`b2-cli`), so `architecture.html`'s "Three crates" framing is correct again once
  the discovery narrative is updated (done in the pivot's Phase 0). The `index.html` test-count badge remains a
  manual snapshot — refresh it whenever tallies move.
- Property tests for the invariants — round-trip, `full-reindex ≡ incremental` (now real, worth pinning),
  rename-keeps-backlinks as property tests over generated vaults (golden-vault scenarios exist; property
  coverage is the gap).
- qmd chunker upgrade — replace the minimal paragraph chunker once the real-embedder retrieval eval can score it
  (build spec §1.2).
- Distance-weighting for `b2 similar` ranking — today candidates are ranked by semantic max-sim alone (graph
  distance is exclusion-only, ① resolved 2026-07-01). A possible knob: boost graph-*close* (triadic closure) or
  graph-*distant* (serendipity/bridging) candidates. With no automated accept-precision signal (the human is the
  gate), this is a dogfooding-judged experiment — add the knob only if it visibly improves the surfaced list.
- GUI (beyond the discovery-loop MVP) — the broader **editor + graph/review** surface stays deferred; the
  read-only first cut has **shipped** (see "Shipped — Desktop UI MVP, read-only" above and
  [specs/desktop-ui-mvp.md](specs/desktop-ui-mvp.md)). The immediate next cut is **editing** (Step 4).
