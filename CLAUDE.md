---
b2id: 01KWSRHGY9XCT43ZW22W73QBY4
---
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What B2 is

A personal, local-first Markdown knowledge vault with an AI layer that **surfaces semantically similar
notes** for you to connect. The Markdown files stay plain and yours; B2 is the intelligence layer
over them, not a container around them. This Cargo workspace is the **index engine + its two dumb
adapters** — the `b2` CLI and the Tauri desktop app (with the `ui/` frontend); the design lives in
`docs/design/`.

## Design docs are the source of truth

The code is a *projection of the spec*, and comments cite it constantly (e.g. `data-model.md §2`,
`index-engine.md §6`). Before changing behavior, read the relevant doc — the schema must satisfy the
data model, never the reverse. The three canonical docs live in `docs/design/`:

- `docs/design/invariants.md` — the **invariant register**: the one-page normative list of what must
  always be true, and the source of *why* (cited by id — S2, G2, …). On conflict with any other doc, it wins.
- `docs/design/data-model.md` — the *what*: note + connection in Markdown, the two storage tiers, the relation vocabulary.
- `docs/design/index-engine.md` — the *how*: SQLite (FTS5 + in-process vector scan) projection, table DDL, data flows.

Planned-but-unstarted work and the backlog live in [GitHub Issues](https://github.com/AlteredCraft/B2/issues);
shipped build history lives in git. Model quality (the `Embedder` seam) is measured out-of-CI by the
eval harness under `crates/b2-embed/evals/` — the hand-labelled retrieval + discovery evals
(BM25-vs-hybrid ablation, note & passage ranks, `b2 similar`), the chunker-sweep gate, and the results log.

## Commands

```bash
# Build the workspace / the `b2` binary
cargo build
cargo run -p b2-cli -- --help          # run the CLI in place (binary is named `b2`)
cargo install --path crates/b2-cli --locked --force   # install `b2` onto PATH (~/.cargo/bin)

# `just` (optional) wraps these: `just install`, `just test`, `just check`, `just eval`, …

# Fast test suite — deterministic, model-free, what CI runs
cargo test -p b2-core                   # the engine suite (fake embedder; no ML deps)
cargo test                              # whole workspace (compiles candle in b2-embed, and b2-desktop
                                        # embeds ui/dist — run `just ui-build` once first)
cargo test -p b2-core --test discover   # one integration-test file (targets in tests/*.rs)
cargo test -p b2-core one_note_reindex  # filter by test-name substring

# Real embedder (out of CI; needs the model provisioned first)
cargo run -p b2-cli -- init             # download + verify bge-base-en-v1.5 into the XDG cache
cargo run -p b2-embed --example eval    # retrieval + discovery quality eval (never in `cargo test`):
                                        # BM25-vs-hybrid lift, passage ranks, `similar`; appends each
                                        # run to crates/b2-embed/evals/results.jsonl (gitignored)
cargo run -p b2-embed --example eval -- --sweep   # + in-process ChunkConfig A/B (the GH #44 gate)

# Metal GPU embedder — research lever (GH #40, macOS-only). The `metal` cargo feature moves the
# BERT forward pass to the Apple-Silicon GPU (default build stays CPU + Accelerate). It's a
# BUILD switch, not runtime: recompile to flip. Selecting Metal tags the recorded model id
# `…@metal`, so switching device re-embeds the vault (a model swap) and `search` fails fast
# rather than mixing CPU/GPU vectors — no silent staleness. The desktop Settings pane (⌘,) shows
# a subtle CPU/Metal badge for the running build (`b2_embed::active_device_label`).
cargo run -p b2-cli --features metal -- reindex   # embed on the GPU (else identical to reindex)
just eval-metal        # retrieval-quality eval on Metal (compare to `just eval` on CPU)
just app               # desktop app — auto-selects Metal on Apple Silicon (`just app-cpu` forces CPU)
just compare-device    # CPU-vs-Metal embed A/B on fixtures/test-vault → chunks/s + speedup

# Desktop app (crates/b2-desktop + ui/; needs Node + `cargo install tauri-cli --locked`)
just ui-install                         # one-time: install the frontend's npm deps
B2_VAULT_PATH=~/notes just app          # run the app in dev (Vite HMR + a live Tauri window)
B2_LOG_FILE=$PWD/logs/desktop.jsonl B2_VAULT_PATH=~/notes just app   # + structured JSONL log ($PWD: cwd is crates/b2-desktop)
just check-app                          # clippy for b2-desktop (builds ui/dist first)
(cd ui && npm test)                     # the frontend's pure-logic suite (pane sizing); no deps —
                                        # node strips the TS types and runs off the source

cargo fmt
cargo clippy --workspace --exclude b2-desktop   # fast lint gate (desktop needs ui/dist; see check-app)
```

Env vars: `B2_VAULT_PATH` sets the vault root so commands need no `-C`/`--vault` (an explicit flag wins).
Read-only commands fall back to the current dir; commands that write (`reindex`/`add`/`mv`/`rm`/`link`) require
an explicit vault (flag, positional, or env) and refuse otherwise, so a stale binary or typo'd var can't
silently touch the wrong dir (`Cli::require_vault`).
`B2_EMBEDDER=fake` forces the deterministic fake embedder everywhere
(offline/dev mode, and what the test suite runs under); `B2_DEBUG` makes the CLI print internal error
detail after the generic message.
`B2_LOG` turns on structured debug logging: **JSON Lines** (stdout stays pure data), one flat object
per event — pipe into jq/DuckDB/pandas for reporting/plotting. Sink is stderr by default;
`B2_LOG_FILE=<path>` writes there instead (append mode, so runs accumulate into one reportable
dataset, and the capture is pure JSONL even when stderr carries human notices). Its value is a tracing
filter directive (`debug`, `b2::sqlite=debug`, `warn`, …); `B2_DEBUG` or `B2_LOG_FILE` alone implies
`B2_LOG=debug`. The kernel
emits: per-statement SQLite timings from SQLite's own profiler (`sqlite3_trace_v2` +
`SQLITE_TRACE_PROFILE`, wired in `db::open` — target `b2::sqlite`, SQL template + numeric `duration_us`
+ `vm_steps`/`fullscan_steps`; statements at/over `B2_SLOW_QUERY_MS` (default 100) log at WARN with
`slow=true`), a span per `Vault` façade op (`b2::vault`; close events carry the op's duration), and
flow milestones (`b2::ingest`, `b2::search`). The core only *emits* — the subscriber (and its clock)
lives in the adapter (`init_logging`, in **both** `b2-cli/src/main.rs` and `b2-desktop/src/logging.rs`),
so `b2-core` stays wall-clock-free and the instrumentation is inert unless an adapter opts in. The two
sinks emit the same JSONL shape; they differ only where the host demands it — the desktop uses a
non-blocking writer (long-lived, multi-threaded) and, for the *implied* default, scopes to `b2=debug`
so Tauri/wry tracing doesn't pollute the file (an explicit `B2_LOG` is honored verbatim in both).
**Desktop path quirk:** `just app` runs `cargo tauri dev` with cwd `crates/b2-desktop/`, so a *relative*
`B2_LOG_FILE=./logs/x.jsonl` lands under that crate dir, not the repo root — pass an **absolute** path
(e.g. `B2_LOG_FILE=$PWD/logs/desktop.jsonl B2_VAULT_PATH=~/notes just app`) to write where you expect.

## Architecture

### The core invariant

**`index = a pure projection of (the vault directory)`.** (The full register: `docs/design/invariants.md`.) Two storage tiers:

1. **The vault directory** — the source of truth. **Markdown is its sole authored subset** — the only
   format whose bytes B2 may write; non-`.md` files are *resources* (path-keyed peers contributing
   derived rows only). Every committed connection lives in the Markdown: a body `[[link]]` (always
   untyped), or a frontmatter `b2_relations:` entry (the sole home of a typed relation; written by
   `b2 link` or by hand).
2. **Disposable SQLite index** (`<vault>/.b2/b2.sqlite`) — FTS5 + plain-table vectors (`embeddings` + `note_centroids`, scored in-process) + the typed `edges` graph.
   Drop it and `reindex` rebuilds it identical. Nothing here is authoritative, and **no durable
   B2-derived state lives outside the Markdown** (the human's own directory tree is vault material,
   not B2 state — see the folders paragraph below).

Consequences that shape the code: incremental re-index must equal a full rebuild (idempotency); every
edge is re-derived from Markdown on every reindex; the only write B2 makes to a note *of its own accord*
is stamping a missing `b2id` (a ULID) — every other write is the mechanics of a command: `b2 link`
appending a frontmatter `b2_relations:` entry, the move-repair of inbound link paths, the desktop
editor's saves through `Vault::write` (a byte-honest splice of the **human's own** body edit, guarded
by a content-hash revision; B2 never authors body content itself), and the frontmatter drawer's saves
through `Vault::write_frontmatter` (the same-guard splice of the **human's own** frontmatter bytes,
body untouched, refusing only a changed/removed `b2id` — GH #79).

**Folders are user-authored structure, and the filesystem is authoritative for them** (data-model.md
"Folders"): a folder — empty or not — is vault material like a note, never projected
into the index (nothing to chunk, embed, or link). The tree's structure listing (`Vault::list_dirs`) is a
**live fs walk**, so the desktop file tree is one-to-one with the vault's managed (non-dot) subtree by
construction; `create_dir` / `move_dir` / `delete_dir` proxy the OS (create-with-parents — an occupied
target refused — / `rename` / `remove_dir_all`) and resolve targets against the disk, so empty folders
work everywhere (`b2 mv`, `b2 rm -r`, the tree).

### The one seam (Bitter-Lesson tenet: build for tomorrow's model)

The AI part sits behind a swappable trait; the engine is built and tested against a deterministic fake,
and a real model drops in through the same seam with no schema or flow change.

- **`Embedder`** (`b2-core/src/embed.rs`) — text → vector. Real impl is `b2-embed`'s candle-backed
  `LocalEmbedder` (bge-base-en-v1.5, 768-dim); test/dev impl is `FakeEmbedder` (blake3-hashed,
  content-addressed, *not* semantic). The fake is content-addressed so drop→rebuild is reproducible.

*(Connection discovery is deliberately model-free at surface time: `b2 similar` surfaces candidates,
`b2 link` is the human committing — the human is the precision gate. A reranker would be the next seam
if/when one lands — `index-engine.md` §5.)*

### Workspace crates

- **`b2-core`** — the whole index engine and the typed `Vault` façade. Deliberately **model-free**
  (no candle) so its test suite stays fast and deterministic. Deps: rusqlite (bundled SQLite + FTS5),
  blake3, ulid, yaml-rust2. (No vector extension: vectors are plain BLOB tables scored in-process —
  `embed::l2_sq` over `db::for_each_stored_vector`/`for_each_note_centroid`.)
- **`b2-embed`** — the real candle-backed embedder. Heavy ML deps (candle, tokenizers, hf-hub) live
  **only here**. `provision` (`b2 init`) downloads + verifies the model into a shared XDG cache;
  `LocalEmbedder::load` fails fast with "run `b2 init`" if absent.
- **`b2-cli`** — the `b2` binary. A *dumb* adapter over the façade: parse args, pick + inject the
  embedder, call `Vault`, print (human-readable, or `--json` for agents). Holds no engine logic.
- **`b2-desktop`** — the Tauri host: the *second* dumb adapter, the GUI sibling of `b2-cli`. Each
  `#[tauri::command]` is deserialize → one `Vault` call → serialize, reusing the CLI's `--json` view
  types as the IPC contract; it also owns host-only infrastructure (the async cancellable reindex task,
  the fs-watch `vault-changed` pulse, the OS folder dialog). Has its own `CLAUDE.md` with the
  thin-adapter rules — read it before touching this crate.
- **`ui/`** (not a crate) — the desktop frontend: Vite + vanilla TS + CodeMirror 6, a separate npm
  toolchain talking to the host over Tauri IPC (`ui/src/api.ts` is the seam).

### The `Vault` façade (`b2-core/src/vault.rs`)

The **one typed API**. The CLI and the desktop host are its only clients; every other `b2-core`
module is called directly only by the integration tests. Surface: lifecycle + indexing (`open` /
`open_with_embedder` / `reindex` / `reindex_with_progress` / `plan_reindex` / `project` / `embed`),
reads (`read` / `list_notes` / `list_resources` / `list_dirs` / `neighbors` / `explain` /
`explain_resource` / `search` / `similar`), writes (`add_note` / `create_note` / `create_dir` /
`move_note` / `move_resource` / `move_dir` / `link` / `write` / `delete_note` / `delete_resource` /
`delete_dir`).
**Add operations when a command needs them; do not
pre-build a broad surface.** The embedder is injected here: `open` defaults to the fake, `open_with_embedder` is how the
adapters wire the real model.

### Data flows

- **Flow ① ingest/reindex** (`ingest.rs`) — parse → stamp missing `b2id` (write file) → project
  notes, chunks (+FTS), embeddings, and the typed `edges` graph. Two-phase so link resolution is
  independent of file order. It is **two separately-invokable passes**
  (the `project`/`embed` split, #15): model-free `project_vault` (notes/chunks/FTS/edges) and
  `embed_vault` (fills the DB-derived missing-vector set); `reindex` composes them, and `search`
  falls back to BM25-only on a projected-but-unembedded vault.
- **Flow ② hybrid search** (`search.rs`) — BM25 (`chunks_fts`) ⊕ vector KNN (an exact in-process scan
  of `embeddings`) fused with Reciprocal Rank Fusion (k=60), resolved from chunks up to notes. Raw NL
  queries are sanitized into a safe FTS5 `MATCH` expression (punctuation is FTS5 syntax and would
  otherwise crash the parse).
- **Flow ③ connection discovery** — **`b2 similar`** (`discover::candidates`) surfaces the semantically
  nearest *unlinked* notes in **two stages** (#38): a coarse O(notes) scan over per-note centroids
  (`note_centroids`, maintained by the embed pass) shortlists candidates, then exact max-sim over only
  the shortlist's chunk vectors — minus the anchor's 1-hop graph neighbors, no model call;
  **`b2 link`** appends a typed `b2_relations:` entry to the source note's frontmatter
  (`note::add_relation`, Markdown-first, **never the body**) and re-projects it as an `origin=frontmatter`
  active edge. No suggestion queue — a connection exists only once you author it.
- **`graph_filtered_search`** (`search.rs`) — the vector⨝graph join: nearest chunks whose note is
  within *k* typed hops of an anchor (scoped traversal). `b2 similar`'s candidate generation is its
  *complement* (`discover::candidates` — nearest notes *not* already connected).

### The typed graph & relation vocabulary

`edges` carries `origin` (`inline`/`frontmatter`) and a deterministic id derived from the identity tuple
`(src, dst, type, occurrence)`. There is **no `status` column** — every edge is authored and active. The
edge set = union of body links (`inline`, all untyped `references` — **the body carries no B2 syntax**,
so no verb/explanation is ever parsed from prose) and frontmatter `b2_relations:` (`frontmatter`, the
sole typed home), with **frontmatter-wins dedup** on same-`(target, type)` overlap (the frontmatter row
alone can carry an explanation; a *different* verb over a body-linked target simply coexists — the
"augment" case, data-model §2). Backlinks are why the graph is materialized rather than parsed at read
time. The relation vocabulary (`relation.rs`) is a **closed three-verb stance core** — `references`
(neutral), `supports` (for), `contradicts` (against) — plus a tolerated tail stored verbatim; the core
is your typing palette on `b2 link` (and what queries rely on). Edges are stored once, directed;
inverse labels are display-only.

### Embedding-space discipline

Vectors live in **plain tables** — `embeddings(chunk_id, vector)` and `note_centroids(note_b2id,
centroid)` — created at **embed time**, not in the base migration: their existence is the "this vault
has an embedding space" signal the projected-but-unembedded fallbacks key on. Every distance is
computed **in-process** (`embed::l2_sq`, one sequential scan statement; rationale:
#38). `meta` records `(embed_model_id,
embed_dim)` — the only place a model swap is detectable. The compute **device** folds into this
identity: the real embedder tags its recorded `embed_model_id` with the resolved device (CPU stays the
bare repo id; a `--features metal` GPU build appends `@metal`, `b2-embed/src/model.rs`), so a
device/precision change that alters vectors *is* a model swap — GH #40. A swap drops both tables and
re-embeds on `reindex`; `search` **fails fast** on a mismatch rather than returning silently-wrong
results. `open`
never mutates the vector space (so changing the configured model can't wipe vectors on the next
command). Centroids are derived data with the vectors' own lifecycle: the embed pass refreshes a
note's centroid after filling its vectors; a re-chunk drops it (`db::replace_chunks`) — no separate
invalidation exists or is needed.

## Conventions

- **Determinism is a hard requirement of the core.** No wall-clock and no randomness inside `b2-core`:
  timestamps and ids are passed in (see `IdGen`, and the `created` param on write ops), so operations are
  reproducible and unit-testable. Tests assert against fixed ids (`FixedId`, the golden-vault b2ids in
  `tests/common/mod.rs`).
- **Keep `cargo test` fast, deterministic, and model-free.** Real-model work belongs out of CI —
  behind `b2 init`, `--example eval`, or manual runs. Never add candle/tokenizers deps to `b2-core`.
- **Never `#[ignore]` a test, and a hard-to-write test is a signal, not a chore.** `#[ignore]` hides a
  test from the suite while leaving it looking present — a silent gap. If a test is difficult to write,
  keep faithful, or make pass, *stop and reflect*: is the test valuable; are we testing the right thing;
  is the fault in the test or in the system under test? A test that fights you is usually coupled to an
  implementation detail (a retired dependency's constant, a since-changed fixture assumption) rather than
  a real invariant — re-anchor it on the invariant, or fix the system. When the resolution isn't obvious,
  **open a conversation with the user** to work through it; do not reach for `#[ignore]`, a slow/brittle
  fixture, or a weakened assertion to move on. 
- **User-facing errors are generic and actionable, never leaking internals** (sqlite/io/serde). The
  CLI funnels everything through `user_message` (`b2-cli/src/main.rs`); `B2_DEBUG` opts into detail.
  This matches the repo-wide logging policy in the parent `CLAUDE.md`.
- Integration tests copy the committed `fixtures/golden-vault/` into a tempdir first, so ingest (which
  may stamp a `b2id`) never mutates the repo fixtures. `fixtures/test-vault/` is a *separate*,
  larger synthetic fixture (~200 notes) for **out-of-CI throughput/quality experiments**, not the
  deterministic suite — see `fixtures/README.md` and `just compare-device` (the CPU-vs-Metal embed A/B).

## Idiomatic Rust

### Rust data modeling

- Ownership forms a tree/DAG, never a cycle. One clear owner per value.
- For references between values (or any logical cycle): use `slotmap` keys, or `Vec` indices only if nothing is ever removed. Do NOT default to `Rc<RefCell<T>>`; treat `Rc` / `Arc<Mutex<T>>` as last resorts after trying ownership + keys.
- Prefer owned fields over borrowed (`&'a T`) fields. If a struct sprouts a lifetime parameter, reconsider — it usually wants owned data or a key. Legitimate exception: a short-lived, `Copy`, read-only *view* struct passed into one call and never stored (e.g. `NoteRow` in `db.rs`) — borrowing there avoids a needless clone; keep it, and say so in the doc-comment.
- Never silence the borrow checker with a reflexive `.clone()`. Diagnose ownership first: should this be a key instead of a reference?
- No self-referential structs in safe Rust; restructure with indices.
- No `.unwrap()` / `.expect()` in production paths; handle via `match`, `if let`, or `?`. This holds even for an invariant you believe can't fail (e.g. `strip_prefix` on a path you just walked) — degrade gracefully (skip it) rather than panic.
- When stuck, ask: "Who owns this, and can the relationship be an ID instead of a pointer?"

### Rust style & structure

- Errors: reach for `thiserror` typed enums wherever error *variants get matched on* — every library, **and any binary that does too**: the CLI maps variants to user-facing messages in `user_message`, so `CliError` is a `thiserror` enum, not `anyhow` (which erases the type and would force `downcast_ref`). Use `anyhow` only where errors are merely propagated and printed. Never hand-roll `From`/`Display` impls — `#[from]` and `#[error("…")]` generate them.
- Signatures: accept `&str` not `&String`, `&[T]` not `&Vec<T>`. Return owned types and let callers borrow.
- Prefer iterator chains over manual index loops (`for x in &items`, not `for i in 0..items.len()`).
- Do NOT introduce `async`/`tokio`, generics, traits, or macros until there's a concrete need. No speculative abstraction.
- `unsafe` requires an explicit `// SAFETY:` comment stating the invariant that makes it sound (see `model.rs`'s weights mmap); otherwise disallowed.
- Derive `Debug` on public data types (and `Clone`/`PartialEq` where it makes sense).
- Keep modules small and domain-named; document public items with `///` comments stating intent, not mechanics.
