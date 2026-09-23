# B2

[![CI](https://github.com/AlteredCraft/B2/actions/workflows/ci.yml/badge.svg)](https://github.com/AlteredCraft/B2/actions/workflows/ci.yml)

**A notes app for research.** Plain Markdown in a folder you own, with an AI layer that finds
the connections you haven't made yet, explains them, and answers questions from your notes
with citations you can check.

B2 is my daily notes app, built to replace Obsidian. Obsidian is a good place to keep notes
and a poor place to think with them. B2 keeps what Obsidian gets right (plain files, any
editor, no lock-in) and adds the part research needs: help seeing what your notes have in
common, how they bear on each other, and what they actually say.

## What it does

- **Shows the connections you haven't made.** Beside every note is a ranked list of related
  notes it isn't linked to yet. Click **Why?** on one and a model explains what the two have
  in common, citing the passages, using B2's own read-only tools to look things up.
- **Lets you say how ideas relate.** Link two notes with one click and type the link:
  `references`, `supports` or `contradicts`. Similarity can tell that two notes are about the
  same thing; only you know whether one backs the other up or argues against it.
- **Searches honestly.** Keyword and semantic search together. When the vault holds no
  evidence for a query, B2 says "no matches" instead of showing its nearest guesses.
- **Answers from your notes, with sources.** Ask a question and get a streamed answer grounded
  only in your notes, with each `[n]` pointing back to the passage it came from. Nothing about
  a chat is stored.
- **Works with agents.** Every CLI command has a `--json` form, and the vault is plain
  Markdown on disk, so an agent can search, read and walk the graph the way you do.
- **Is a real editor.** Live-preview Markdown, `[[wikilink]]` completion, images, a graph
  view, a file tree over your real folders, and full keyboard control with rebindable keys.
  Indexing is automatic: the app watches the folder and keeps up with edits made anywhere.

## What it promises

- **Your notes stay yours.** The vault is a folder of Markdown that reads fine in Obsidian or
  any editor. B2's index lives in `.b2/` and can be deleted and rebuilt at any time.
- **No surprise writes.** Opening, reading and indexing a vault change nothing. B2 writes only
  when you ask it to (a link, a move, a save), and it never writes into the body of a note on
  its own.
- **Local first.** Embeddings run on your machine. Chat uses a local model through Ollama by
  default; a cloud model is something you choose.
- **Refactor without fear.** Move or rename a note through B2 and every `[[wikilink]]` to it is
  rewritten to match.

## Status

The build-out is done: the engine, the `b2` CLI, the desktop app and grounded chat all work end
to end. The next phase is using B2 every day and letting that use decide what comes next. New
work starts as an [`observed` issue](https://github.com/AlteredCraft/B2/issues?q=label%3Aobserved):
what I was doing, what I expected, what happened. B2 runs from source today (developed on
macOS); an installable build is [#23](https://github.com/AlteredCraft/B2/issues/23).

## Get started

```bash
make doctor                        # checks Rust, Node, the Tauri CLI and the build toolchain
make init                          # one-time: download the embedding model
B2_VAULT_PATH=~/notes make app     # open the desktop app on a folder of Markdown
```

Point it at an existing Obsidian vault or any folder of `.md` files. Without `B2_VAULT_PATH`,
the app asks you to pick a folder and remembers it. For chat, install
[Ollama](https://ollama.com) and run `ollama pull llama3.2`.

The same engine runs from the terminal:

```bash
make install                       # puts `b2` on your PATH
b2 -C ~/notes search "spaced repetition"
b2 -C ~/notes similar notes/memory
b2 -C ~/notes ask "what have I written about sleep and memory?"
```

The **[Quick start](docs/quickstart.md)** walks through the rest in about ten minutes, and has
every command, setting and environment variable.

## How it's built

A Rust workspace. `b2-core` is the whole engine: it turns a folder of Markdown into a SQLite
index (full-text search, vectors and a typed link graph) that is a pure projection of the
files. The `b2` CLI and the Tauri desktop app are two thin front ends over the same typed API.
The real models sit behind small seams (`b2-embed` for embeddings, `b2-llm` for chat), so a
better model drops in without a redesign, and the engine is tested without them.

Two ideas shape every decision:

- **The vault is the truth; the index is disposable.** `index = projection of (the vault
  directory)`. Drop the index, rebuild it, get the same thing back.
- **Build for tomorrow's model.** Do the least today's model needs, behind a seam, so the next
  one is an upgrade rather than a rewrite.

## Docs

| Doc | What it covers |
|---|---|
| [Quick start](docs/quickstart.md) | Set up and use B2: the walkthrough, commands, config, environment variables. |
| [Search and similarity](docs/search-and-similarity.md) | What search and the related-notes list do, in plain language. |
| [Architecture](docs/architecture.md) | The crates, the flows, the seams, and how the tests hold it up. |
| [Invariants](docs/invariants.md) | What must always be true, cited by id. Wins any conflict. |
| [Data model](docs/data-model.md) | What a note and a connection are, in plain Markdown. |
| [Index engine](docs/index-engine.md) | How the index is built and queried. |
| [Evals](docs/evals.md) | How search, discovery and chat quality are measured, outside CI. |
| [ADRs](ADRs/README.md) | Why each key decision reads the way it does. |

## Develop

```bash
make check     # the fast loop: format, lint, engine and frontend tests
make ci        # exactly what GitHub Actions runs; run it before pushing
make           # every target, grouped
```

There is no git hook; CI is the enforcement. Contributor and agent ground rules are in
[CLAUDE.md](CLAUDE.md), and the desktop crate has its own in
[crates/b2-desktop/CLAUDE.md](crates/b2-desktop/CLAUDE.md).
