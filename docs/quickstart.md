# Quick start

Build B2, point it at a folder of Markdown, then search it, connect notes, and ask it
questions. About ten minutes, most of it a one-time model download.

Your notes stay plain Markdown. B2 keeps its index in a hidden `.b2/` folder you can delete
at any time, and writes to a note only when you ask it to.

## Set up

You need a Rust toolchain ([rustup.rs](https://rustup.rs)). The desktop app also needs Node
(18, 20 or 22+) and the Tauri CLI (`cargo install tauri-cli --locked`). macOS is the tested
platform. `make doctor` checks all of this and prints the fix for anything missing.

```console
$ git clone https://github.com/AlteredCraft/B2 && cd B2
$ make install        # puts `b2` on your PATH
$ b2 init             # one time: download the embedding model (~440 MB)
```

`b2 init` is the only download B2 ever makes. Indexing, search and discovery run locally,
with no account and no key. To skip the model, prefix commands with `B2_EMBEDDER=fake`:
search then works by keyword only, and says so.

## Index a vault

A vault is any folder of `.md` files. An Obsidian vault works as it is; nothing is imported
or converted.

```console
$ cd ~/notes
$ b2 reindex .
Indexed 2 notes (2 embedded) and 0 resources
```

Indexing reads your notes and never writes to them. It is incremental, so run it again
after editing. `--dry-run` previews the work, `--force` re-projects everything, and
`b2 status` shows embedding coverage and whether a reindex is running. If the vault is a
git repo, add `.b2/` to `.gitignore`.

Commands find the vault from `-C <path>`, from `B2_VAULT_PATH`, or from the current
directory. Commands that write (`reindex`, `add`, `write`, `mv`, `rm`, `link`) need it
stated explicitly, so a typo can't touch the wrong folder. Every command takes `--json`.

## Search

```console
$ b2 search "how does forgetting work"
0.0328 spaced-repetition (notes/spaced-repetition.md)
   Spaced repetition exploits the retrieval curve of [[concepts/memory]].
```

Search combines keyword and semantic matching, so it finds notes by wording and by meaning.
When the vault holds no evidence for a query, it says so instead of showing its nearest
guesses:

```console
$ b2 search "Fasdfadsf"
No matches. Nothing in the vault matches "Fasdfadsf".
```

`--limit` is a cap, not a quota. With `--json` the answer is an object: `results` plus a
`vouched` verdict.

## Find and make connections

`b2 similar` lists the notes most like this one that it isn't linked to yet. It reads
stored vectors, so there is no model call and no network.

```console
$ b2 similar notes/spaced-repetition
0.7132 cramming (notes/cramming.md)
   Massed practice yields only short-term recall.
```

The list is always the ranked nearest. Similarity is relative to your vault, so B2 ranks
and leaves the judgment to you; an empty list means only that there was nothing to compare.
`--explain <other>` shows the passage pairs behind one candidate, and
`b2 why <note> <candidate>` asks the chat model to explain it with citations.

A connection exists once you write it. There are two kinds:

- **A `[[link]]` in the body**, written in any editor. Always a plain `references` link.
- **A typed link**, written into the source note's frontmatter by `b2 link`:

```console
$ b2 link notes/spaced-repetition notes/cramming --type contradicts --explanation "massed vs. spaced practice"
```

```yaml
b2_relations:
  - "contradicts [[notes/cramming.md]] — massed vs. spaced practice"
```

`--type` is `references` (the default), `supports` or `contradicts`. To undo a link, delete
its line. `b2 neighbors <note>` lists a note's links in both directions. `b2 explain <note>`
adds where each came from and its why, and flags orphans and links that resolve to nothing.

## Ask

`b2 ask` answers from your notes and cites them. It needs a model server:
[Ollama](https://ollama.com) by default (`ollama serve`, then `ollama pull gemma4:e2b`), or
any OpenAI-compatible endpoint through `--llm-url` and `--llm-model`.

```console
$ b2 ask "why does spacing out reviews beat cramming?"
Spaced repetition exploits the retrieval curve [1], where massed practice yields only
short-term recall [2].

Sources:
  [1] notes/spaced-repetition.md
  [2] notes/cramming.md
```

`b2 chat` is the interactive form: follow-ups remember the conversation, Ctrl-C stops an
answer, `/exit` leaves. Chat stores nothing. The default endpoint is local, and a cloud
endpoint is used only if you configure one. Its key comes from `B2_LLM_API_KEY` (or the
macOS Keychain in the app), never from a flag or a file.

## Move and rename

```console
$ b2 mv concepts/memory concepts/human-memory
Moved concepts/memory.md → concepts/human-memory.md
Rewrote 1 inbound link(s) across 1 file(s).
```

A move through B2 rewrites every link that pointed at the old path. Folders move whole.
Rename a note outside B2 and its inbound links break, as they would anywhere. `b2 explain`
lists each as unresolved, and it heals when a note exists at that path again.

## The desktop app

```console
$ B2_VAULT_PATH=~/notes make app
```

The same engine in a window. Without `B2_VAULT_PATH` the app asks for a folder and
remembers it. What it adds:

- **Automatic indexing.** It scans the vault on open and watches the folder, so edits made
  anywhere are picked up. Keyword search works at once, and vectors fill in behind.
- **Related notes beside the note you're reading**, with strength dots, a one-click typed
  **Link**, **Explain** (the evidence, no model) and **Why?** (the chat model's account,
  cited).
- **An editor:** live-preview Markdown, autosave, `[[wikilink]]` completion, a frontmatter
  drawer, and a conflict bar if the file changed underneath you.
- **Chat** (⌘J), a **graph** of the open note's connections, a **file tree** over the real
  folders (drag files in from Finder to import them), and **Settings** (⌘,) for the
  embedding model, the chat endpoint and every keyboard shortcut.

On Apple Silicon `make app` embeds on the Metal GPU, and `make app-cpu` forces CPU. The two
produce different vectors, so switching re-embeds the vault once.

### Reading the strength dots

The dots grade a candidate against this note's other candidates. They are not a percentage,
and they never decide what appears.

- ●●● is a strong match, ●●○ a clear one, ●○○ a near one.
- The same pair of notes can show different dots from each side, because each side has
  different competition.
- In a vault about one subject everything is close, and the dots can all read modest. The
  order is still right.
- A list with fewer than a dozen candidates, or one indexed with the fake embedder, is
  ungraded and says so.

## Commands

| Command | What it does |
|---|---|
| `b2 init` | Download and verify the embedding model. Once per machine. |
| `b2 reindex` | Bring the index up to date. `--dry-run`, `--force`, `--cancel`. |
| `b2 status` | Embedding coverage, and whether a reindex is running. |
| `b2 search <query>` | Hybrid search. `--limit N`; `--exclude <path>` (repeatable) drops notes already seen, for agent loops. |
| `b2 similar <note>` | The nearest notes not yet linked. `--limit N`, `--explain <other>`. |
| `b2 why <note> <candidate>` | The chat model's cited explanation of one suggestion. |
| `b2 link <src> <dst>` | Write a typed link into the source's frontmatter. `--type`, `--explanation`. |
| `b2 neighbors <note>` | A note's links, in and out. |
| `b2 explain <note>` | Every connection with its origin and why. Flags orphans and unresolved links. |
| `b2 add <path>` | Create a note (`--content`, `--title`) and index it. |
| `b2 write <note>` | Replace a note's body from stdin. Frontmatter is left alone. |
| `b2 mv <from> <to>` | Move a note, file or folder and repair its inbound links. |
| `b2 rm <target>` | Delete a note, file or folder (`-r`). Inbound links dangle and are surfaced. |
| `b2 ask <question>` | One grounded, cited answer. `--json` is a JSONL event stream. |
| `b2 chat` | Interactive grounded chat. |

Notes are addressed by vault-relative path, with or without `.md`. `b2 --help` includes the
retrieval loop for agents.

## Config and environment

Nothing needs configuring. One optional file sets the embedder, and the CLI and the app
both read it: `~/Library/Application Support/b2/config.toml` on macOS,
`~/.config/b2/config.toml` on Linux.

```toml
[embedder]
model = "BAAI/bge-base-en-v1.5"      # the default: 768-dim, ~440 MB
# model = "BAAI/bge-small-en-v1.5"   # 384-dim, ~130 MB: faster, a little weaker
# source = "https://hf-mirror.com"   # a mirror URL, or a local folder of model files
# cache_dir = "/path/to/models"      # default: <data dir>/b2/models
# query_prefix = "..."               # the instruction prepended to queries; "" for none
```

Changing the model is a swap: run `b2 init`, then `b2 reindex` to rebuild the vectors.
Until then `search` refuses rather than mix two models. The app's Settings does the same
swap and downloads the model for you.

| Variable | Effect |
|---|---|
| `B2_VAULT_PATH` | The vault root. `-C`/`--vault` overrides it. |
| `B2_EMBEDDER=fake` | A deterministic, non-semantic embedder. Search runs keyword-only. |
| `HF_ENDPOINT` | The Hugging Face mirror `b2 init` downloads from, when the config's `source` names none. |
| `B2_LLM_URL`, `B2_LLM_MODEL` | The OpenAI-compatible chat endpoint and model. Defaults: `http://localhost:11434/v1` and `gemma4:e2b`. A flag beats the variable, which beats the default. |
| `B2_LLM_API_KEY` | Bearer token for a cloud chat endpoint. A variable and never a flag, because a key in a flag is a key in `ps`. |
| `B2_LLM=fake` | A deterministic chat provider, for tests and offline use. |
| `B2_LLM_MAX_TOOL_CALLS` | The most tool calls one model reply may carry in `b2 why` (default 64, allowed 1 to 4096). A reply over the cap fails with its own message. The app's Settings → Chat overrides it. |
| `B2_DEBUG` | Print internal error detail after the generic message. |
| `B2_LOG` | Structured logging as JSON Lines on stderr. Takes a tracing filter (`debug`, `b2::sqlite=debug`, `warn`). |
| `B2_LOG_FILE` | Append the structured log to this file instead of stderr. Use an absolute path with `make app`. |
| `B2_SLOW_QUERY_MS` | The slow-query threshold in milliseconds (default 100). Statements at or over it log at WARN. |
