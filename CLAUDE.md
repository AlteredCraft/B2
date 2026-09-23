# CLAUDE.md

Guidance for Claude Code (claude.ai/code) and other agents working in this repository.
`AGENTS.md` is a symlink to this file.

## What B2 is for

B2 is my daily notes app, built to replace Obsidian. The reason to replace it is research:
Obsidian is a good place to keep notes and a poor place to think with them. B2 keeps what
Obsidian gets right (plain Markdown in a folder I own, usable by any editor, no lock-in) and
adds an intelligence layer that helps me do research across what I have already written:

- **Find the connections I haven't made.** Every note shows the related notes it isn't linked
  to yet, and **Why?** explains what the two have in common, with citations.
- **Say how ideas relate, not just that they do.** A link can be typed as `supports` or
  `contradicts`, because stance is the one thing similarity can't infer.
- **Get answers I can check.** Search says "no matches" when the vault holds no evidence, and
  chat answers only from my notes, citing each claim back to the passage it came from.
- **Let agents work the vault too.** Every command has a `--json` form, so an agent can search,
  read and walk the graph the way I do.

Two ideas shape every decision, and they are the ones to protect:

1. **The vault is the truth; the index is disposable.** `index = projection of (the vault
   directory)`. Drop `.b2/b2.sqlite`, rebuild, get the same index. So I can move, split, merge
   and rewrite notes without fear, and B2 writes nothing I didn't ask for.
2. **Build for tomorrow's model.** Every AI part sits behind a small seam, and B2 does the least
   today's model needs. A better model should drop in, not force a redesign.

The human is the judge. B2 suggests; I decide what gets linked, typed and kept.

## Where we are, and how work starts

The build-out is done: the engine, the `b2` CLI, the desktop app and grounded chat all work end
to end. The next phase is **using B2 every day and letting that use drive the work.**

- New work starts from an issue labelled **`observed`**: what I was doing, what I expected, what
  happened, how often. Prefer fixing friction I actually hit over tuning I might need.
- The backlog was reset on 2026-09-23. Closed ideas (rerankers, scaling levers, resource search,
  sync, highlights) keep their write-ups. Reopen one only when an observation calls for it.
- Measure before you tune. A change to ranking or model quality is judged by the eval harness
  ([docs/evals.md](docs/evals.md)), never by intuition.
- If a request would pull B2 away from the vision above (a proprietary format, an unbidden write,
  an AI step with no seam), say so before building it.

## Where the truth lives

The code is a projection of the spec, and comments cite it (`data-model.md §2`, invariant ids
like `S2`, `D1`). Read the relevant part before changing behaviour.

| Source | Role |
|---|---|
| [docs/invariants.md](docs/invariants.md) | What must always be true, cited by id. **On conflict with anything else, it wins.** |
| [docs/data-model.md](docs/data-model.md) | The *what*: notes, connections, the two storage tiers, the relation vocabulary. |
| [docs/index-engine.md](docs/index-engine.md) | The *how*: the SQLite projection, its tables, the four flows, the Why? tool turn. |
| [docs/architecture.md](docs/architecture.md) | The map: crates, flows, seams, how the tests hold it up. Start here in the code. |
| [docs/quickstart.md](docs/quickstart.md) | Using B2: commands, config, and **every environment variable**. |
| [ADRs/](ADRs/README.md) | Why the above reads the way it does. Add one only for a decision that is expensive to reverse and whose *why* isn't readable off the code. |
| [GitHub Issues](https://github.com/AlteredCraft/B2/issues) | The backlog. Decision history is the issue that drove a change plus the commit that shipped it. |

`crates/b2-desktop/CLAUDE.md` holds the desktop crate's own rules. Read it before touching that
crate or `ui/`.

## The shape of the system

A Cargo workspace plus a frontend. Details and the reasons live in
[docs/architecture.md](docs/architecture.md).

- **`b2-core`**: the whole engine, and the one typed API, the `Vault` façade (`vault.rs`).
  Model-free, deterministic, fast to test. Never add candle, tokenizers or an HTTP client here.
- **`b2-embed`**: the real local embedder (candle, bge). All heavy ML dependencies live here.
- **`b2-llm`**: the real chat provider, a small sync OpenAI-compatible streaming client. Ollama
  is the guided default; any compatible endpoint works.
- **`b2-cli`** and **`b2-desktop`** (Tauri) are the two **dumb adapters**: parse input, make one
  `Vault` call, render the result. The desktop reuses the CLI's `--json` view types as its IPC
  contract. The frontend is `ui/` (Vite, vanilla TypeScript, CodeMirror 6).

Engine logic belongs in `b2-core`, behind the façade, never in an adapter. Add a façade operation
when a command needs it; don't pre-build surface.

## Principles that hold everywhere

- **No unbidden writes.** Reading, walking and reindexing write nothing to the vault. Every write
  is the mechanics of a command I ran, and the body of a note is always mine (W1 to W3).
- **A note is its path.** Identity is the vault-relative path; nothing is minted or stamped into
  files.
- **Degrade honestly.** An unembedded vault still searches by keyword; a model that can't use
  tools still gets an answer; a failed condense falls back to the raw question. Say what is
  missing rather than failing or pretending.
- **Model output is untrusted.** Chat stores nothing, never rewrites its answer, and a bad tool
  call gets an error result, not a crash. Rendering note content is a trust boundary (ADR-0016).
- **Limits are configurable and loud.** A cap gets a clear error, a setting, and a place in
  Settings, not a silent trim.
- **Determinism in the core.** No wall clock and no randomness in `b2-core`: time is passed in,
  and logging is only emitted there (the adapters own the subscriber and the clock).
- **Errors shown to people are generic and actionable.** Nothing internal (sqlite, io, serde)
  leaks; `B2_DEBUG` opts into detail.
- **No async.** Nothing in B2's code is async, and the `b2` binary links no tokio (ADR-0011).

## Working in the repo

`make` with no arguments lists every target. Two gates, the same ones CI runs (ADR-0018):

```bash
make check   # fast loop: fmt, clippy -D warnings, engine suite, ui/ suite
make ci      # everything CI runs; run it before pushing
```

Things the target list won't tell you:

- **Offline by default.** `B2_EMBEDDER=fake` and `B2_LLM=fake` give deterministic stand-ins;
  the test suite runs on them. The real model needs `make init` once; real chat needs
  `ollama serve`.
- **Commands that write need an explicit vault** (`-C`, a path, or `B2_VAULT_PATH`), so a typo
  can't touch the wrong folder.
- **Metal is a build switch, not a runtime one.** `make app` picks it on Apple Silicon. The
  device is part of the embedding space's identity, so switching re-embeds the vault (ADR-0007).
- **Whole-workspace `cargo test`** embeds `ui/dist`, so run `make ui-build` once first.
- **Coverage** needs `cargo-llvm-cov` and `llvm-tools-preview`; `make doctor` checks both.

### Tests

- **Keep `cargo test` fast, deterministic and model-free.** Anything that needs the real model
  belongs in the eval harness, where it actually runs.
- **Never `#[ignore]` a test.** A test that is hard to write is a signal: is it testing a real
  invariant or an implementation detail? Is the fault in the test or the system? If the answer
  isn't clear, ask me rather than weakening the assertion.
- **A test's name is part of its contract.** It must not claim more than the body asserts.
- Integration tests copy `fixtures/golden-vault/` into a temp dir first, so no suite can change
  the repo's fixtures. Shared helpers live in `crates/b2-core/tests/common/mod.rs`; the one
  deliberate copy is the tracing `Capture` writer in `logging.rs` and
  `discover_query_count.rs`, which need their own test binaries. (`fixtures/test-vault/` is a
  larger synthetic vault for out-of-CI experiments.)

## Idiomatic Rust

### Data modeling

- Ownership forms a tree or DAG, never a cycle. One clear owner per value.
- For references between values, use keys (`slotmap`, or `Vec` indices if nothing is removed).
  `Rc<RefCell<T>>` and `Arc<Mutex<T>>` are last resorts.
- Prefer owned fields to borrowed ones. A lifetime on a struct usually wants owned data or a key.
  The exception is a short-lived, `Copy`, read-only view passed into one call (like `NoteRow` in
  `db.rs`); say so in its doc comment.
- Never silence the borrow checker with a reflexive `.clone()`. Ask who owns the value, and
  whether the relationship should be an ID instead of a pointer.
- No self-referential structs; restructure with indices.
- No `.unwrap()` or `.expect()` in production paths, even for an invariant you believe can't
  fail. Degrade gracefully instead of panicking.

### Style and structure

- Errors: `thiserror` enums wherever variants are matched, including the CLI's `CliError`.
  `anyhow` only where errors are just propagated and printed. Never hand-roll `From`/`Display`.
- Accept `&str` and `&[T]`; return owned types.
- Prefer iterator chains to index loops.
- No `async`, generics, traits or macros until there is a concrete need.
- `unsafe` needs a `// SAFETY:` comment stating why it is sound.
- Derive `Debug` on public data types (`Clone`/`PartialEq` where sensible).
- Keep modules small and named for their domain. Doc comments state intent, not mechanics.
