---
b2id: 01KWSRHGY9XCT43ZW22W73QBY4
---
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What B2 is

A personal, local-first Markdown knowledge vault with an AI layer that **surfaces semantically similar
notes** for you to connect. The Markdown files stay plain and yours; B2 is the intelligence layer
over them, not a container around them. This Cargo workspace is the **index engine + CLI**; the
design lives in `planning/`.

## Design docs are the source of truth

The code is a *projection of the spec*, and comments cite it constantly (e.g. `data-model.md §2`,
`build spec §1.2`, `index-engine.md §6`). Before changing behavior, read the relevant doc — the
schema must satisfy the data model, never the reverse.

- `planning/vision-and-scope.md` — the *why*: principles, the two design tenets, v1 scope, locked decisions.
- `planning/data-model.md` — the *what*: note + connection in Markdown, the two storage tiers, the relation vocabulary.
- `planning/index-engine.md` + `planning/specs/index-engine-build.md` — the *how*: SQLite (FTS5 + `sqlite-vec`) projection, table DDL, the build order, data flows.
- `planning/tasks.md` — the working queue (what's done, what's next). **Read this first to know current state.**
- `planning/user-stories.md` — kernel behavior as testable scenarios.
- `planning/specs/eval-strategy.md` — how model quality (the `Embedder` seam) is measured out-of-CI:
  the hand-labelled retrieval eval, its metrics, and how to run/grow it.

## Commands

```bash
# Build the workspace / the `b2` binary
cargo build
cargo run -p b2-cli -- --help          # run the CLI in place (binary is named `b2`)
cargo install --path crates/b2-cli --locked --force   # install `b2` onto PATH (~/.cargo/bin)

# `just` (optional) wraps these: `just install`, `just test`, `just check`, `just eval`, …

# Fast test suite — deterministic, model-free, what CI runs
cargo test -p b2-core                   # the engine suite (fake embedder; no ML deps)
cargo test                              # whole workspace (compiles candle in b2-embed — slower)
cargo test -p b2-core --test relate     # one integration-test file (targets in tests/*.rs)
cargo test -p b2-core same_pair         # filter by test-name substring

# Real embedder (out of CI; needs the model provisioned first)
cargo run -p b2-cli -- init             # download + verify bge-base-en-v1.5 into the XDG cache
cargo run -p b2-embed --example eval    # semantic-retrieval quality eval (never in `cargo test`)

cargo fmt
cargo clippy --workspace
```

Env vars: `B2_VAULT_PATH` sets the vault root so commands need no `-C`/`--vault` (an explicit flag wins).
Read-only commands fall back to the current dir; commands that write (`reindex`/`add`/`mv`/`link`) require
an explicit vault (flag, positional, or env) and refuse otherwise, so a stale binary or typo'd var can't
silently touch the wrong dir (`Cli::require_vault`).
`B2_EMBEDDER=fake` forces the deterministic fake embedder everywhere
(offline/dev mode, and what the test suite runs under); `B2_DEBUG` makes the CLI print internal error
detail after the generic message.

## Architecture

### The core invariant

**`index = a pure projection of (Markdown)`.** Two storage tiers:

1. **Markdown files** (`<vault>/*.md`) — the source of truth, plain and portable. Every committed
   connection lives here: a body `[[link]]`, or a frontmatter `relations:` entry (written by `b2 link`).
2. **Disposable SQLite index** (`<vault>/.b2/b2.sqlite`) — FTS5 + `sqlite-vec` + the typed `edges` graph.
   Drop it and `reindex` rebuilds it identical. Nothing here is authoritative, and **no durable state
   lives outside the Markdown.**

Consequences that shape the code: incremental re-index must equal a full rebuild (idempotency); every
edge is re-derived from Markdown on every reindex; the only write B2 ever makes to a note is stamping a
missing `b2id` (a ULID) or, on `b2 link`, appending a frontmatter `relations:` entry.

*(Through 2026-06-30 there was a third tier — a durable `.b2/log/` event log holding the suggestion queue
+ rejection memory — and the invariant was `(Markdown ∪ log)`. The 2026-07-04 relator cut removed it;
see `vision-and-scope.md` "Decisions locked (2026-07-04)".)*

### The one seam (Bitter-Lesson tenet: build for tomorrow's model)

The AI part sits behind a swappable trait; the engine is built and tested against a deterministic fake,
and a real model drops in through the same seam with no schema or flow change.

- **`Embedder`** (`b2-core/src/embed.rs`) — text → vector. Real impl is `b2-embed`'s candle-backed
  `LocalEmbedder` (bge-base-en-v1.5, 768-dim); test/dev impl is `FakeEmbedder` (blake3-hashed,
  content-addressed, *not* semantic). The fake is content-addressed so drop→rebuild is reproducible.

*(A second seam — `Relator`, an LLM that typed/explained candidate note pairs — was cut 2026-07-04: its
per-pair cost didn't scale to a real vault. Connection discovery is now `b2 similar` (surface candidates,
no model) + `b2 link` (the human commits). A reranker would be the next seam if/when one lands —
`index-engine.md` §5.)*

### Workspace crates

- **`b2-core`** — the whole index engine and the typed `Vault` façade. Deliberately **model-free**
  (no candle) so its test suite stays fast and deterministic. Deps: rusqlite (bundled SQLite + FTS5),
  `sqlite-vec`, blake3, ulid, yaml-rust2.
- **`b2-embed`** — the real candle-backed embedder. Heavy ML deps (candle, tokenizers, hf-hub) live
  **only here**. `provision` (`b2 init`) downloads + verifies the model into a shared XDG cache;
  `LocalEmbedder::load` fails fast with "run `b2 init`" if absent.
- **`b2-cli`** — the `b2` binary. A *dumb* adapter over the façade: parse args, pick + inject the
  embedder, call `Vault`, print (human-readable, or `--json` for agents). Holds no engine logic.

### The `Vault` façade (`b2-core/src/vault.rs`)

The **one typed API**. The CLI and any future adapter are its only clients; every other `b2-core`
module is called directly only by the integration tests. Surface is intentionally minimal —
`open` / `open_with_embedder` / `reindex` / `project` / `embed` / `neighbors` / `search`. **Add
operations when a command needs them; do not pre-build a broad surface.** The embedder is injected
here: `open` defaults to the fake, `open_with_embedder` is how the CLI wires the real model.

### Data flows

- **Flow ① ingest/reindex** (`ingest.rs`) — parse → stamp missing `b2id` (write file) → project
  notes, chunks (+FTS), embeddings, and the typed `edges` graph. Two-phase so link resolution is
  independent of file order. Since 2026-07-07 it is **two separately-invokable passes**
  (`specs/completed/projection-embedding-split.md`): model-free `project_vault` (notes/chunks/FTS/edges) and
  `embed_vault` (fills the DB-derived missing-vector set); `reindex` composes them, and `search`
  falls back to BM25-only on a projected-but-unembedded vault.
- **Flow ② hybrid search** (`search.rs`) — BM25 (`chunks_fts`) ⊕ vector KNN (`chunks_vec`) fused with
  Reciprocal Rank Fusion (k=60), resolved from chunks up to notes. Raw NL queries are sanitized into a
  safe FTS5 `MATCH` expression (punctuation is FTS5 syntax and would otherwise crash the parse).
- **Flow ③ connection discovery** — **`b2 similar`** (`discover::candidates`) surfaces the semantically
  nearest *unlinked* notes (vector KNN over stored embeddings, minus the anchor's 1-hop graph neighbors —
  no model call); **`b2 link`** appends a typed `relations:` entry to the source note's frontmatter
  (`note::add_relation`, Markdown-first, **never the body**) and re-projects it as an `origin=frontmatter`
  active edge. No suggestion queue — a connection exists only once you author it.
- **`graph_filtered_search`** (`search.rs`) — the vector⨝graph join: nearest chunks whose note is
  within *k* typed hops of an anchor (scoped traversal). `b2 similar`'s candidate generation is its
  *complement* (`discover::candidates` — nearest notes *not* already connected).

### The typed graph & relation vocabulary

`edges` carries `origin` (`inline`/`frontmatter`) and a deterministic id derived from the identity tuple
`(src, dst, type, occurrence)`. There is **no `status` column** — every edge is authored and active. The
edge set = union of body links (`inline`) and frontmatter `relations:` (`frontmatter`), with **inline-wins
dedup**. Backlinks are why the graph is materialized rather than parsed at read time. The relation
vocabulary (`relation.rs`) is a **closed 10-verb core** plus a tolerated tail; the core is your typing
palette on `b2 link` (and what queries rely on). Edges are stored once, directed; inverse labels are
display-only.

### Embedding-space discipline

`chunks_vec` (a `vec0` virtual table) is created at **embed time**, not in the base migration, because
its dimension is a DDL literal pinned to the embedder's `dim`. `meta` records `(embed_model_id,
embed_dim)` — the only place a model swap is detectable. A swap drops `chunks_vec` and re-embeds on
`reindex`; `search` **fails fast** on a mismatch rather than returning silently-wrong results. `open`
never mutates the vector space (so changing the configured model can't wipe vectors on the next
command).

## Conventions

- **Determinism is a hard requirement of the core.** No wall-clock and no randomness inside `b2-core`:
  timestamps and ids are passed in (see `IdGen`, and the `created` param on write ops), so operations are
  reproducible and unit-testable. Tests assert against fixed ids (`FixedId`, the golden-vault b2ids in
  `tests/common/mod.rs`).
- **Keep `cargo test` fast, deterministic, and model-free.** Real-model work belongs out of CI —
  behind `b2 init`, `--example eval`, or manual runs. Never add candle/tokenizers deps to `b2-core`.
- **User-facing errors are generic and actionable, never leaking internals** (sqlite/io/serde). The
  CLI funnels everything through `user_message` (`b2-cli/src/main.rs`); `B2_DEBUG` opts into detail.
  This matches the repo-wide logging policy in the parent `CLAUDE.md`.
- Integration tests copy the committed `fixtures/golden-vault/` into a tempdir first, so ingest (which
  may stamp a `b2id`) never mutates the repo fixtures.

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
- `unsafe` requires an explicit `// SAFETY:` comment stating the invariant that makes it sound (see `db.rs`'s `sqlite-vec` registration and `model.rs`'s weights mmap); otherwise disallowed.
- Derive `Debug` on public data types (and `Clone`/`PartialEq` where it makes sense).
- Keep modules small and domain-named; document public items with `///` comments stating intent, not mechanics.
