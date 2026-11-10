# CLAUDE.md

Guidance for Claude Code (claude.ai/code) and other agents working in this repository.
`AGENTS.md` is a symlink to this file.

## What B2 is for

B2 is my daily notes app, built to replace Obsidian. Obsidian is a good place to keep notes
and a poor place to think with them. B2 keeps what it gets right (plain Markdown in a folder
I own, usable by any editor) and adds an intelligence layer for research across what I have
already written:

- **Find the connections I haven't made.** Every note shows the related notes it isn't
  linked to yet, and **Why?** explains what the two have in common, with citations.
- **Say how ideas relate.** A link can be typed `supports` or `contradicts`, because stance
  is the one thing similarity can't infer.
- **Get answers I can check.** Search says "no matches" when the vault holds no evidence,
  and chat answers only from my notes, citing each claim.
- **Let agents work the vault too.** Every command has a `--json` form.

Two ideas shape every decision. Protect them:

1. **The vault is the truth; the index is disposable.** `index = projection of (the vault
   directory)`. Drop `.b2/b2.sqlite`, rebuild, get the same index.
2. **Build for tomorrow's model.** Every AI part sits behind a small seam and does the least
   today's model needs, so a better model drops in without a redesign.

B2 suggests; I decide what gets linked, typed and kept.

## How work starts

The build-out is done. The work now is using B2 every day and fixing what that shows.

- New work starts from an issue labelled **`observed`**: what I was doing, what I expected,
  what happened, how often. Prefer fixing friction I hit over tuning I might need.
- The backlog was reset on 2026-09-23. Closed ideas (rerankers, scaling, resource search,
  sync, highlights) stay closed until an observation calls for one.
- Measure before you tune. Ranking and model quality are judged by the eval harness
  ([docs/evals.md](docs/evals.md)), never by intuition.
- If a request would pull B2 away from the two ideas above (a proprietary format, an
  unbidden write, an AI step with no seam), say so before building it.

## The docs

| Doc | Holds |
|---|---|
| [docs/invariants.md](docs/invariants.md) | What must always be true, cited by id (`S2`, `D1`). **It wins any conflict.** Read the entries for the area you are changing. |
| [docs/decisions.md](docs/decisions.md) | The few key decisions and why, cited by id (`ADR-0014`). |
| [docs/data-model.md](docs/data-model.md) | What a vault holds: notes, links, the verbs, resources. |
| [docs/architecture.md](docs/architecture.md) | How it's built: the crates, the index, the flows. |
| [docs/evals.md](docs/evals.md) | Measuring search, discovery and chat quality. |
| [docs/quickstart.md](docs/quickstart.md) | Using B2: commands, config, every environment variable. |
| [crates/b2-desktop/CLAUDE.md](crates/b2-desktop/CLAUDE.md) | Rules for the desktop crate and `ui/`. Read it before touching either. |

Keep them short and current:

- Each fact has one home. When behaviour changes, update that home in the same change.
- Docs describe how B2 works now. History lives in GitHub Issues and git.
- Code comments cite an invariant id, a decision id, or a doc by name, never a section
  number.
- Add a decision only when it is expensive to reverse and its reason can't be read off the
  code.

## The shape of the system

- **`b2-core`**: the whole engine and the one typed API, the `Vault` façade (`vault.rs`).
  Model-free and deterministic. Never add candle, tokenizers or an HTTP client here.
- **`b2-embed`**: the real local embedder (candle, bge). All heavy ML dependencies live here.
- **`b2-llm`**: the real chat provider, a small sync OpenAI-compatible client. Ollama by
  default.
- **`b2-cli`** and **`b2-desktop`** (Tauri, frontend in `ui/`): two **dumb adapters**. Parse
  input, make one `Vault` call, render the result. The desktop reuses the CLI's `--json`
  view types as its IPC contract.

Engine logic belongs in `b2-core`, behind the façade, never in an adapter. Add a façade
operation when a command needs it; don't pre-build surface.

## Principles

- **No unbidden writes.** Reading and reindexing write nothing to the vault. Every write is
  the mechanics of a command I ran, and a note's body is always mine (W1 to W3).
- **A note is its path.** Nothing is minted or stamped into files (L1).
- **Degrade honestly.** An unembedded vault still searches by keyword; a model that can't
  use tools still gets an answer. Say what is missing instead of failing or pretending.
- **Model output is untrusted.** Chat stores nothing and never rewrites its answer, a bad
  tool call gets an error result, and rendering is a trust boundary (E5).
- **Limits are configurable and loud.** A cap gets a clear error and a setting, not a
  silent trim.
- **The core is deterministic.** No wall clock or randomness in `b2-core`. Time is passed
  in; the adapters own the clock and the log subscriber (E1).
- **Errors shown to people are generic and actionable.** Nothing internal leaks; `B2_DEBUG`
  opts into detail (E4).
- **No async.** Nothing in B2 is async, and the `b2` binary links no tokio (ADR-0011).

## Working in the repo

`make` lists every target. CI runs `make ci` and nothing else, so every check lives in a
make target.

```bash
make check   # fast loop: fmt, clippy -D warnings, engine suite, ui/ suite
make ci      # everything CI runs; run it before pushing
```

- **Offline by default.** `B2_EMBEDDER=fake` and `B2_LLM=fake` are deterministic stand-ins,
  and the suite runs on them. The real model needs `make init`; real chat needs
  `ollama serve`.
- **Commands that write need an explicit vault** (`-C`, a path, or `B2_VAULT_PATH`).
- **Metal is a build switch.** `make app` picks it on Apple Silicon, and switching
  re-embeds the vault (M2).
- **Whole-workspace `cargo test`** embeds `ui/dist`, so run `make ui-build` first.
- **Coverage** needs `cargo-llvm-cov` and `llvm-tools-preview`; `make doctor` checks both.
- **A new tool in a make target** must exit non-zero on a failing input. Check that it
  does.

### Tests

- **Keep `cargo test` fast, deterministic and model-free.** Anything that needs the real
  model belongs in the eval harness.
- **Never `#[ignore]` a test.** A test that is hard to write is a signal: is it testing an
  invariant or an implementation detail? Is the fault in the test or the system? If that
  isn't clear, ask me instead of weakening the assertion.
- **A test's name is part of its contract.** It must not claim more than the body asserts.
- Integration tests copy `fixtures/golden-vault/` into a temp dir first. Shared helpers
  live in `crates/b2-core/tests/common/mod.rs`; the one deliberate copy is the tracing
  `Capture` writer in `logging.rs` and `discover_query_count.rs`, which need their own
  binaries.

## Rust

- **Ownership is a tree, never a cycle.** Refer between values by key or index.
  `Rc<RefCell<T>>` and `Arc<Mutex<T>>` are last resorts. No self-referential structs.
- **Prefer owned fields.** A lifetime on a struct usually wants owned data or a key. The
  exception is a short-lived read-only view passed into one call (like `NoteRow` in
  `db.rs`); say so in its doc comment.
- **Never silence the borrow checker with a reflexive `.clone()`.** Ask who owns the value.
- **No `.unwrap()` or `.expect()` in production paths.** Degrade instead of panicking.
- **Errors:** `thiserror` enums wherever variants are matched, including `CliError`.
  `anyhow` only where errors are just propagated and printed. Never hand-roll `From` or
  `Display`.
- Accept `&str` and `&[T]`; return owned types. Prefer iterator chains to index loops.
- No `async`, generics, traits or macros until there is a concrete need.
- `unsafe` needs a `// SAFETY:` comment saying why it is sound.
- Derive `Debug` on public data types (`Clone` and `PartialEq` where sensible).
- Small modules named for their domain. Doc comments state intent, not mechanics.
