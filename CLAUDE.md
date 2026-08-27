# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## What B2 is

A personal, local-first Markdown knowledge vault with an AI layer that **surfaces semantically
similar notes** for you to connect. The Markdown files stay plain and yours; B2 is the intelligence
layer over them, not a container around them. This Cargo workspace is the **index engine + its two
dumb adapters** — the `b2` CLI and the Tauri desktop app (with the `ui/` frontend).

## Where the truth lives

Read before changing behaviour. The code is a *projection of the spec*, and comments cite it
constantly (`data-model.md §2`, `index-engine.md §6`, invariant ids like `S2`, `D1`).

| Source | Role |
|---|---|
| [`docs/invariants.md`](docs/invariants.md) | The **invariant register** — the normative list of what must always be true, cited by id. **On conflict with any other doc, it wins.** |
| [`docs/data-model.md`](docs/data-model.md) | The *what*: note + connection in Markdown, the two storage tiers, the relation vocabulary. |
| [`docs/index-engine.md`](docs/index-engine.md) | The *how*: the SQLite (FTS5 + in-process vector scan) projection, table DDL, data flows. |
| [`ADRs/`](ADRs/README.md) | **Architecture Decision Records** — why each of the above reads the way it does. Key architectural choices only, terse. Add one when a decision is expensive to reverse and its *why* isn't readable off the code; do **not** add one per feature or bug. |
| [`docs/evals.md`](docs/evals.md) | The **eval suite guide** — every instrument, how to read its output, the exit gate, the verdict record, and the **process rules**; read before touching the corpus, the labels, or the metrics. |
| [GitHub Issues](https://github.com/AlteredCraft/B2/issues) | Backlog and planned work. Decision history = the issue that drove a verdict + the commit that shipped it. |

## Commands

`make` (no args) lists every target with a one-line summary, grouped setup / dev / gates / coverage /
model — that listing is the command reference; what follows is what it can't tell you.

**The two gates.** Same commands, same order, whether you run them or GitHub does (ADR-0018):

```bash
make check     # FAST (~3s warm): fmt-check → clippy -D warnings → engine suite → ui/ suite.
               # The loop you run while working. No network, no desktop build.
make ci        # COMPLETE (~18s warm): no-tokio → ui-build → fmt-check → clippy (whole workspace)
               # → cargo suite → ui/ suite → audit. Verbatim what CI runs. Run before pushing.
```

**Test suites.** `make test` = `cargo test -p b2-core`, the fast deterministic model-free engine
suite (the bulk of the weight). `make test-ui` = the `ui/` pure-logic suite (node's own runner over
`src/**/*.test.ts`, globbed recursively, running off the source with types stripped). Narrower runs:
`cargo test -p b2-core --test discover` (one file), `cargo test -p b2-core one_note_reindex` (name
filter). Whole-workspace `cargo test` compiles candle and embeds `ui/dist` — run `make ui-build` once
first.

**Out of CI, real model** (ADR-0005, ADR-0013): `make init` provisions bge-base-en-v1.5 into the XDG
cache; `make eval` / `eval-sweep` / `eval-stemmer` / `eval-metal` / `eval-chat` score quality;
`make stability` is the model-free rank probe (`stability-bless` accepts an *intended* ranking
change — it records what is, never what is better); `make calibrate VAULT=<vault>` is the real-vault
transfer check any distributional constant owes. Grounded chat needs a model server —
`ollama serve` + `ollama pull llama3.2`, or `B2_LLM=fake` for the deterministic provider.

**Coverage** needs both `cargo-llvm-cov` and `rustup component add llvm-tools-preview` (per
toolchain). Without them the recipes die on cargo's bare "no such command: `llvm-cov`", which names
neither — `make doctor` checks both and prints the fix.

**Metal** is a *build* switch, not runtime: `cargo run -p b2-cli --features metal -- reindex`, and
`make app` auto-selects it on Apple Silicon (`make app-cpu` forces CPU). Flipping device re-embeds
the vault, because the device is part of the embedding space's identity (ADR-0007).

**Desktop path quirk:** `make app` runs `cargo tauri dev` with cwd `crates/b2-desktop/`, so a
*relative* `B2_LOG_FILE` lands under that crate dir. Pass an absolute path:
`B2_LOG_FILE=$PWD/logs/desktop.jsonl B2_VAULT_PATH=~/notes make app`.

### Environment variables

- **`B2_VAULT_PATH`** — the vault root, so commands need no `-C`/`--vault` (an explicit flag wins).
  Read-only commands fall back to the current dir; **commands that write** (`reindex`/`add`/`mv`/
  `rm`/`link`) require an explicit vault and refuse otherwise (`Cli::require_vault`), so a stale
  binary or typo'd var can't silently touch the wrong dir.
- **`B2_EMBEDDER=fake`** — the deterministic fake embedder everywhere (offline/dev; what the test
  suite runs under). **`B2_LLM=fake`** is its sibling for chat.
- **`B2_DEBUG`** — print internal error detail after the generic message.
- **`B2_LLM_URL` / `B2_LLM_MODEL`** — the OpenAI-compatible endpoint + model (defaults
  `http://localhost:11434/v1` + `llama3.2`). **`B2_LLM_API_KEY`** carries a cloud endpoint's bearer
  token — never a flag, because a key in a flag is a key in `ps`. `ask`/`chat`'s `--llm-url` /
  `--llm-model` beat the env, which beats the default; resolution lives once in
  `b2_llm::LlmConfig::from_env`, and the desktop layers its Settings over the same base. The **key**
  layers the other way: the desktop stores it in the **macOS Keychain**, never in `chat.json`, and
  `B2_LLM_API_KEY` overrides whatever is stored (`b2_llm::ApiKeySource` — `none`/`environment`/
  `stored`/`session` — is what the Settings copy reads; the key never crosses back to the webview).
  Chat config is adapter-level, never vault or index state, so a chat-model swap costs no reindex.
- **`B2_LOG`** — structured debug logging as **JSON Lines** (stdout stays pure data), one flat object
  per event, to stderr or to **`B2_LOG_FILE=<path>`** (append mode, so runs accumulate into one
  reportable dataset). The value is a tracing filter (`debug`, `b2::sqlite=debug`, …); `B2_DEBUG` or
  `B2_LOG_FILE` alone implies **`b2=debug`** — the kernel's own targets only, so Tauri/wry/`ureq`
  records stay out of the dataset. Emitted: per-statement SQLite timings from SQLite's own profiler
  (`b2::sqlite` — SQL template, `duration_us`, `vm_steps`/`fullscan_steps`; at/over
  `B2_SLOW_QUERY_MS`, default 100, logged at WARN with `slow=true`), a span per `Vault` op
  (`b2::vault`), and flow milestones (`b2::ingest`, `b2::search`). The core only *emits* — the
  subscriber and its clock live in the adapter (`init_logging` in both), so `b2-core` stays
  wall-clock-free and the instrumentation is inert unless an adapter opts in.

## Architecture

**`index = a pure projection of (the vault directory)`** (ADR-0002; invariants S1–S5). Two tiers: the
vault directory is the source of truth, `<vault>/.b2/b2.sqlite` is a disposable cache — FTS5 +
plain-table vectors + the typed `edges` graph. A note's identity is its **vault-relative path**
(ADR-0003). **B2 makes no unbidden write at all** (ADR-0004): reading, walking and reindexing write
nothing; every write is the mechanics of a command the human invoked.

### Workspace crates

- **`b2-core`** — the whole index engine and the typed `Vault` façade. Deliberately **model-free** (no
  candle) so its suite stays fast and deterministic. Deps: rusqlite (bundled SQLite + FTS5), blake3,
  ulid, yaml-rust2. No vector extension — vectors are plain BLOB tables scored in-process (ADR-0006).
- **`b2-embed`** — the real candle-backed embedder (bge-base-en-v1.5, 768-dim). **All heavy ML deps
  live only here.** `provision` (`b2 init`) downloads + verifies into a shared XDG cache;
  `LocalEmbedder::load` fails fast with "run `b2 init`" if absent. `hf-hub` is taken
  `default-features = false` (ADR-0011).
- **`b2-llm`** — the real chat provider: a hand-rolled **sync** OpenAI-compatible SSE client over
  `ureq` behind the `LlmProvider` seam. One wire shape (`POST {base}/chat/completions`,
  `stream: true`, frames until `[DONE]`), so the quirks it owns are its tests: keep-alive comments,
  multi-line `data:`, CRLF, mid-stream error frames, a stream that stops without `[DONE]` (a
  *truncated* answer, honestly marked cancelled — not an error), and a garbled frame (an error, so a
  protocol mismatch can't pass as a short answer). `probe` is `b2 init`'s posture for chat — one
  `GET /models` before a human waits — and it believes only what it can check: an HTTP refusal is a
  refusal (`LlmError::Refused`), and tolerance is bounded to a 2xx whose body isn't a model list.
  `setup` is the same question asked for a *card*: it never fails, and it is the one deliberately
  **Ollama-native** corner (`GET /api/tags` + a memory-sized pull suggestion) in an otherwise generic
  `/v1` crate. That asymmetry is by design — guided setup is a per-runtime feature — so it must not
  be "generalized" against an abstraction that cannot serve it.
- **`b2-cli`** — the `b2` binary. A *dumb* adapter (ADR-0012): parse args, inject the embedder and
  chat provider, call `Vault`, print (human-readable, streamed, or `--json` for agents).
- **`b2-desktop`** — the Tauri host, the GUI sibling of `b2-cli`. Each `#[tauri::command]` is
  deserialize → one `Vault` call → serialize, reusing the CLI's `--json` view types as the IPC
  contract; it owns host-only infrastructure (the cancellable reindex task, the fs-watch
  `vault-changed` pulse, the OS folder dialog, the declared menu bar, the streaming cancellable
  `ask`, the Keychain). **Has its own `CLAUDE.md` with the thin-adapter rules — read it before
  touching this crate.**
- **`ui/`** (not a crate) — the frontend: Vite + vanilla TS + CodeMirror 6, talking to the host over
  Tauri IPC (`ui/src/api.ts` is the seam). Rendering is a **trust boundary** (ADR-0016). Chords live
  in one registry (ADR-0017). Fenced code is highlighted from CodeMirror's own grammar registry
  through one resolver (`ui/src/highlight.ts`), so a fence looks the same read or edited; icons are
  one vendored Bootstrap Icons subset generated into `icons.gen.ts` (`make icons`; `npm test` runs
  the generator in `--check` mode first, so a stale generated file fails the gate). Layout, theme,
  pane widths and rebound chords persist in `localStorage` — a viewing choice, never vault state —
  and the loader re-judges what it reads.

### The `Vault` façade (`b2-core/src/vault.rs`)

The **one typed API**; the CLI and the desktop host are its only clients (every other `b2-core`
module is called directly only by integration tests). Surface: lifecycle + indexing (`open` /
`open_with_embedder` / `reindex` / `reindex_with_progress` / `plan_reindex` / `project` / `embed`),
reads (`read` / `list_notes` / `list_resources` / `list_dirs` / `neighbors` / `explain` /
`explain_resource` / `search` / `search_evidence` / `similar` / `ask`), writes (`add_note` /
`create_note` / `create_dir` / `import_file` / `import_path` / `move_note` / `move_resource` /
`move_dir` / `link` / `write` / `write_frontmatter` / `delete_note` / `delete_resource` /
`delete_dir`). **Add operations when a command needs them; do not pre-build a broad surface.** The
embedder is injected here: `open` defaults to the fake, `open_with_embedder` wires the real model.

### Data flows

- **Flow ① ingest/reindex** (`ingest.rs`) — parse → project notes, chunks (+FTS), embeddings and the
  typed `edges` graph, **writing nothing to the vault**. Two-phase, so link resolution is independent
  of file order, and **two separately-invokable passes**: model-free `project_vault`
  (notes/chunks/FTS/edges) and `embed_vault` (fills the DB-derived missing-vector set). `reindex`
  composes them; `search` falls back to BM25-only on a projected-but-unembedded vault. The
  whole-vault pass owns every *reconciliation* — pruning rows for files the walk no longer met,
  collecting vectors no chunk references — and single-note paths never prune.
- **Flow ② hybrid search** (`search.rs`) — BM25 (`chunks_fts`) ⊕ vector KNN fused with RRF, resolved
  from chunks up to notes (ADR-0008). Raw NL queries are sanitized into a safe FTS5 `MATCH`
  expression (punctuation is FTS5 syntax and would otherwise crash the parse). `hybrid_search`
  returns a `Retrieval` carrying the per-hit and per-query evidence signals RRF discards; a query the
  vault holds no evidence for answers **"no matches"** (ADR-0015).
- **Flow ③ connection discovery** — `discover::candidates` ranks the nearest *unlinked* notes in two
  stages (centroid shortlist → exact max-sim over the shortlist's chunk vectors, minus the anchor's
  1-hop neighbours), and always serves the ranked top-N (ADR-0009, ADR-0014). `b2 link` appends a
  typed `b2_relations:` entry (`note::add_relation` — frontmatter, **never the body**) and re-projects
  it. The GUI's other authoring gesture is `ui/src/droplink.ts`: dragging a Similar card onto a line
  types a `[[wikilink]]` there, landing in the editor's buffer exactly as `[[` completion does.
- **Flow ④ grounded chat** (`chat.rs` + `Vault::ask`) — condense (multi-turn only; degrades to the
  raw question on failure, so that step can never break chat) → retrieve (`search_chunks` at
  `chat::ASK_PASSAGES`) → assemble (grounded system prompt + numbered passages — prompt assembly is
  core logic) → stream (tokens through the caller's callback; `Break` cancels at token granularity)
  → cite (`[n]` markers resolve to `(path, excerpt)` in `AnswerView`; a hallucinated marker resolves
  to nothing, and the answer text is **never rewritten**). Chat is a **reader**: nothing model-derived
  is stored, history is session-only, model output is untrusted content. Every surface streams, which
  is why `--json` is a JSONL *event* stream. Both adapters cancel the same way (Ctrl-C / Esc) and
  render what already arrived — `Completion` marks a cut stream rather than failing it.
- **`graph_filtered_search`** (`search.rs`) — the vector⨝graph join: nearest chunks whose note is
  within *k* typed hops of an anchor. `discover::candidates` is its *complement* (nearest notes *not*
  already connected).

### The typed graph

`edges` carries `origin` (`inline`/`frontmatter`) and a deterministic id from `(src, dst, type,
occurrence)`. **No `status` column** — every edge is authored and active. Body links are always
untyped `references`; frontmatter `b2_relations:` is the sole typed home; overlap dedups
frontmatter-wins. The vocabulary is a closed three-verb stance core (`references` / `supports` /
`contradicts`) plus a tolerated verbatim tail. Edges are directed and stored once; inverse labels are
display-only. Details and rationale: ADR-0010.

### Embedding-space discipline

`embeddings(text_hash, vector)` + `note_centroids(note_path, centroid)`, created at **embed time** so
their existence is the "this vault has an embedding space" signal the fallbacks key on;
content-addressed by blake3 of the chunk text (ADR-0006). `meta.(embed_model_id, embed_dim)` is the
one place a model swap is detectable, device included — a swap drops both tables and re-embeds on
`reindex`, `search` fails fast rather than mixing spaces, and `open` never mutates the vector space
(ADR-0007).

## Conventions

- **Determinism is a hard requirement of the core.** No wall-clock and no randomness inside
  `b2-core`: timestamps are passed in (the `created` param on write ops), and nothing is minted at
  all — a note's identity is its path, so the `IdGen` seam is gone. Tests assert against golden-vault
  *paths* in `tests/common/mod.rs`.
- **Keep `cargo test` fast, deterministic, and model-free.** Real-model work belongs out of CI —
  behind `b2 init`, an `--example`, or a manual run. Never add candle/tokenizers deps to `b2-core`.
- **Never `#[ignore]` a test, and a hard-to-write test is a signal, not a chore.** `#[ignore]` hides a
  test while leaving it looking present — a silent gap. If a test is difficult to write, keep
  faithful, or make pass, *stop and reflect*: is the test valuable; are we testing the right thing; is
  the fault in the test or in the system? A test that fights you is usually coupled to an
  implementation detail rather than a real invariant — re-anchor it, or fix the system. When the
  resolution isn't obvious, **open a conversation with the user**; do not reach for `#[ignore]`, a
  brittle fixture, or a weakened assertion. A check that genuinely needs the real model belongs in the
  eval harness, where it actually runs (the batch ≡ single embedding check is the worked example).
- **A test's name is part of its contract.** If the name claims more than the body asserts, the suite
  reads as covering ground it doesn't. `make coverage` finds the other half of that problem: a line
  the suite never executes.
- **Shared test scaffolding lives in `crates/b2-core/tests/common/mod.rs`** — fixture setup
  (`reindexed_vault` / `opened_vault` / `ingest_golden`) and the read-back shims (`index_conn`,
  `count`). A helper wanted by more than one file goes there rather than being copied; what only one
  file needs stays in that file. The one deliberate exception is the tracing `Capture` writer
  duplicated by `tests/logging.rs` and `tests/discover_query_count.rs`: hoisting it would make every
  test binary link `tracing-subscriber` to serve two, and those two already need their own binaries
  (tracing's global callsite-interest cache races across parallel test threads).
- **User-facing errors are generic and actionable, never leaking internals** (sqlite/io/serde). The
  CLI funnels everything through `user_message`; `B2_DEBUG` opts into detail.
- Integration tests copy `fixtures/golden-vault/` into a tempdir first, so no suite can mutate the
  repo fixtures (CI's `git diff --exit-code` is the backstop). `fixtures/test-vault/` is a *separate*,
  larger synthetic fixture (~200 notes) for **out-of-CI throughput/quality experiments** — see
  `fixtures/README.md`.

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
