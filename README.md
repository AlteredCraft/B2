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
  notes it isn't linked to yet. Click **Why?** and a model explains what the two have in
  common, citing the passages.
- **Lets you say how ideas relate.** Link two notes and type the link: `references`,
  `supports` or `contradicts`. Similarity can tell that two notes are about the same thing;
  only you know whether one backs the other up or argues against it.
- **Searches honestly.** Keyword and semantic search together. When the vault holds no
  evidence for a query, B2 says "no matches" instead of showing its nearest guesses.
- **Answers from your notes, with sources.** Ask a question and get a streamed answer
  grounded only in your notes, each `[n]` pointing back to the passage it came from.
- **Works with agents.** Every CLI command has a `--json` form, and the vault is plain
  Markdown on disk, so an agent can search, read and walk the graph the way you do.
- **Is a real editor.** Live-preview Markdown, `[[wikilink]]` completion, images, a graph
  view, a file tree over your real folders, and full keyboard control with rebindable keys.

## What it promises

- **Your notes stay yours.** The vault is a folder of Markdown that reads fine in Obsidian
  or any editor. B2's index lives in `.b2/` and can be deleted and rebuilt at any time.
- **No surprise writes.** Opening, reading and indexing a vault change nothing. B2 writes
  only when you ask it to (a link, a move, a save), and never into the body of a note on its
  own.
- **Local first.** Embeddings run on your machine. Chat uses a local model through Ollama
  by default; a cloud model is something you choose.
- **Refactor without fear.** Move or rename a note through B2 and every link to it is
  rewritten to match.

## Get started

```bash
make doctor                        # checks Rust, Node, the Tauri CLI and the build toolchain
make init                          # one time: download the embedding model
B2_VAULT_PATH=~/notes make app     # open the desktop app on a folder of Markdown
```

Point it at an existing Obsidian vault or any folder of `.md` files. For chat, install
[Ollama](https://ollama.com) and run `ollama pull gemma4:e2b`.

The same engine runs from the terminal:

```bash
make install                       # puts `b2` on your PATH
b2 -C ~/notes search "spaced repetition"
b2 -C ~/notes similar notes/memory
b2 -C ~/notes ask "what have I written about sleep and memory?"
```

The [quick start](docs/quickstart.md) has the walkthrough, every command and every setting.
B2 runs from source on macOS today; an installable build is
[#23](https://github.com/AlteredCraft/B2/issues/23).

## How it's built

A Rust workspace. `b2-core` is the whole engine: it turns a folder of Markdown into a SQLite
index (full-text search, vectors and a typed link graph) that is a pure projection of the
files. The `b2` CLI and the Tauri desktop app are two thin front ends over the same typed
API. The real models sit behind small seams (`b2-embed` for embeddings, `b2-llm` for chat)
and the engine is tested without them.

Two ideas shape every decision:

- **The vault is the truth; the index is disposable.** Drop the index, rebuild it, get the
  same thing back.
- **Build for tomorrow's model.** Do the least today's model needs, behind a seam, so the
  next one is an upgrade and not a rewrite.

## Docs

| Doc | What it covers |
|---|---|
| [Quick start](docs/quickstart.md) | Using B2: the walkthrough, commands, config, environment variables. |
| [Data model](docs/data-model.md) | What a vault holds: notes, links, the verbs, resources. |
| [Architecture](docs/architecture.md) | How it's built: the crates, the index, the flows. |
| [Invariants](docs/invariants.md) | What must always be true, cited by id. Wins any conflict. |
| [Decisions](docs/decisions.md) | The few key decisions, and why. |
| [Evals](docs/evals.md) | How search, discovery and chat quality are measured. |

## Develop

```bash
make check     # the fast loop: format, lint, engine and frontend tests
make ci        # exactly what GitHub Actions runs; run it before pushing
make           # every target, grouped
```

The build-out is done, so new work starts from daily use: an
[`observed` issue](https://github.com/AlteredCraft/B2/issues?q=label%3Aobserved) saying what
I was doing, what I expected and what happened. Ground rules for contributors and agents
are in [CLAUDE.md](CLAUDE.md), with the desktop crate's own in
[crates/b2-desktop/CLAUDE.md](crates/b2-desktop/CLAUDE.md).
