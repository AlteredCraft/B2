# Quick start

Set up B2 and work with a vault, for anyone new to it. You will build the `b2` CLI, point it
at a folder of Markdown, then search it, surface similar notes, and link them. It takes about
ten minutes, most of it a one-time model download. The last section opens the same vault in
the desktop app.

Your `.md` files stay plain and yours the whole way. B2's whole write surface is a hidden
`.b2/` index folder plus one line of frontmatter (a `b2_relations:` entry, and only when you
commit a typed link). Indexing writes nothing to your notes. Your prose is never rewritten.

## Before you start

You need a Rust toolchain ([rustup.rs](https://rustup.rs)) to build the `b2` binary. B2 ships
as source today, as one static binary. macOS and Linux are the tested platforms. For the
desktop app (its own section near the end), also install Node + npm and the Tauri CLI. Run
`make doctor` in the checkout: it checks all of this and prints the fix for anything missing.

One thing is optional, and it is local. B2 makes no network calls and needs no account or API
key at any point:

- **The embedding model.** A one-time download (`b2 init`) of a local model that powers
  semantic search and connection discovery. Two are supported, and you can switch later.
  Pick **BGE Base** (768-dim, ~440 MB), the default and the better ranker. Pick **BGE Small**
  (384-dim, ~130 MB) if you want faster downloads and embedding for a modest quality drop,
  worth it on a big vault. Or skip the model entirely and run keyword-only (shown below).

## Set up

### 1. Build the b2 CLI

Clone the repo, build the release binary, and alias it for this shell:

```console
$ git clone https://github.com/AlteredCraft/B2 && cd B2
$ cargo build --release -p b2-cli
$ alias b2=./target/release/b2
$ b2 --help
```

You should see the command list, starting with "B2 — explore a Markdown vault's typed graph
and search from the terminal".

Tip: every command takes a global `-C <path>` (or `--vault`) to point at your vault. Or set
`B2_VAULT_PATH` once so every command finds it without the flag. Add `--json` for
machine-readable output (for agents and scripts). The examples below run from inside the
vault folder, so read-only commands default to the current directory. Commands that write
(`reindex`, `add`, `mv`, `link`) need an explicit vault (a path, `-C`, or `B2_VAULT_PATH`),
so they can't silently touch the wrong directory.

### 2. Install the embedding model (one time)

Semantic search and connection discovery run on a local embedding model. No account, no API
key, no network at query time. `b2 init` downloads and verifies the model into a shared cache
once. It is not re-downloaded per vault, and there is never a surprise download mid-command
later.

```console
$ b2 init
```

You should see the download steps, then: `Installed 'BAAI/bge-base-en-v1.5' (768 dims). Run
\`b2 reindex\` to embed your vault.`

`b2 init` provisions whichever model is configured: the 768-dim base model unless you said
otherwise. To take the smaller, faster one instead, set it once before running `init` (see
[Config and environment](#config-and-environment)), or pick it in the desktop app's
Settings, which offers the same two and downloads on the spot:

```toml
# <config dir>/b2/config.toml
[embedder]
model = "BAAI/bge-small-en-v1.5"   # 384-dim, ~130 MB
```

Switching later: both front ends read that one config, so they always agree on the model.
Changing it is a model swap. The vectors are rebuilt on the next `reindex`, and until then
`search` refuses rather than mixing two embedding spaces. Your Markdown is untouched by any
of it.

No model? You can do everything in this guide offline by prefixing commands with
`B2_EMBEDDER=fake`. Search still works on keywords (BM25), but the semantic half is off. The
CLI tells you so, and never pretends a result is semantic when it isn't.

## Create a vault

A vault is nothing more than a folder of Markdown files. Point B2 at notes you already have,
or start fresh. There is no import step and no format to convert into.

### 3. Create some notes, or choose a folder you already have

Already have one? An Obsidian vault, a docs tree, a directory of daily notes: that's a vault.
Point B2 at it and skip to step 5. Nothing is converted, moved, or rewritten.

Starting fresh? Just write Markdown files. Frontmatter is supported, not required. B2 reads
it when it's there, preserves every key it doesn't understand, and indexes a note with none
at all just as happily. Here is one of each:

```markdown
# ~/vault/concepts/memory.md — with frontmatter
---
type: concept
created: 2026-07-03
tags: [cognition]
---
The brain encodes, stores, and retrieves information.
```

```markdown
# ~/vault/notes/spaced-repetition.md — without any
Spaced repetition exploits the retrieval curve of [[concepts/memory]].

Expanding review intervals beat massed practice for long-term recall.
```

That `[[concepts/memory]]` is an ordinary Obsidian-style wikilink, and B2 reads it as a
connection between the two notes. There is no B2 syntax for the body: nothing to learn here,
and nothing left behind to un-learn if you stop using B2. Connections that carry a stance
("this one supports that one") do exist, but they live in frontmatter, and you'll meet them
in step 8.

On titles: a note's display title is its filename (`spaced-repetition`), so
`concepts/memory.md` shows up as `memory` below. A frontmatter `title:` is inert to B2, kept
verbatim for whatever tool of yours does use it.

Prefer a command? `b2 add notes/spaced-repetition --content "…"` writes a valid note and
indexes it in one step (so you can skip the reindex in step 5 for notes you create this way).
The `.md` extension is optional; parent folders are created for you.

### 4. Know what B2 will and won't touch

The files stay ordinary Markdown that works in Obsidian, or anything else, with no B2
running. Indexing (next step) changes nothing at all in them: the second note above has no
frontmatter, and after a full reindex it still has none. A note that already has frontmatter
keeps every key exactly as you wrote it: same bytes, same order, same comments. B2 knows a
note by where it sits (its path in the vault is its identity), so there is nothing to stamp
into the file.

B2 writes to your notes in exactly one place, in frontmatter, and never touches your prose:

- `b2_relations:`, a typed connection, appended only when you commit one (step 8, or a
  click in the desktop app).

That is the whole of B2's write surface, plus the disposable `.b2/` index folder. Links you
write yourself in the body stay ordinary Markdown: B2 reads them, never edits them, and
treats each as an untyped `references` edge (labelled `inline`). A typed relation is a
frontmatter-only thing (labelled `frontmatter`).

### 5. Index the vault

`b2 reindex` reads every note and builds the searchable index. Reading only; your notes are
not written to. It's how B2 catches up to files you created or edited by hand:

```console
$ cd ~/vault
$ b2 reindex .
Indexing /Users/you/vault
  embedding 2/2 · notes/spaced-repetition.md (2 chunks)
Indexed 2 notes (2 embedded) and 0 resources
```

Run it again after editing notes. It's incremental: unchanged notes keep their vectors, so
only what actually changed is re-embedded. Two flags help:

- `b2 reindex --dry-run` previews how much work a run would be, touching neither your notes
  nor the index.
- `b2 reindex --force` re-projects everything from scratch. It still re-embeds only what
  genuinely differs: identical text always produces the identical vector, so B2 keeps the
  one it has.

`b2 status` answers "is it done?" at any point: how many notes are embedded (so semantic
ranking is live rather than keyword-only), and whether a reindex is running.

Where it lives: the index goes in a single `.b2/` folder inside your vault. It's disposable.
Delete it and the next reindex rebuilds it identical from your Markdown. Add `.b2/` to your
`.gitignore` if the vault is a git repo.

In the app: the desktop app does this step for you. It indexes the vault when you open it and
watches the folder for changes made outside the app, so there is no reindex to remember. The
CLI keeps it explicit on purpose: a command you run is a command that can't surprise you in a
script.

## Work with it

### 6. Search

One command, hybrid ranking. It fuses keyword (BM25) and semantic (vector) matches, so you
find notes by wording and by meaning:

```console
$ b2 search "how does forgetting work"
0.0328 spaced-repetition (notes/spaced-repetition.md)
   Spaced repetition exploits the retrieval curve of [[concepts/memory]].
0.0161 memory (concepts/memory.md)
   The brain encodes, stores, and retrieves information.
```

Use `--limit N` to widen or narrow the result set. It's a cap, never a quota to fill. A query
your vault holds no evidence for gets a plain `no matches` rather than its nearest guesses:

```console
$ b2 search "Fasdfadsf"
No matches. Nothing in the vault matches "Fasdfadsf".
```

B2 decides that from two independent readings: whether your words appear at all (weighted by
how rare each one is), and how near the closest passage genuinely is. That is how it can tell
"nothing here matches" from "everything here matches". The plain-language walk-through is
[search-and-similarity.md](search-and-similarity.md). Under `--json` the answer is an object,
`results` plus the `vouched` verdict, so an agent gets the rows and the honest reading of
them.

### 7. Explore the graph

Follow the connections around any note. `neighbors` is a quick list of what a note links to
and from. `explain` adds each connection's provenance, and its "why" when it has one:

```console
$ b2 neighbors notes/spaced-repetition
→ references  memory (concepts/memory.md)

$ b2 explain concepts/memory
memory (concepts/memory.md)
Connections:
  ← referenced-by  spaced-repetition (notes/spaced-repetition.md)  [inline]
```

That is the body wikilink, read back as a graph edge: untyped (`references`), sourced from
the body (`inline`), and shown from the far end under its inverse label (`referenced-by`).
The edge is stored once and directed; the inverse is display only. Step 8 adds a typed one,
and this listing grows a second row.

You address a note by its vault-relative path, with or without the `.md`: the same two forms
a link is written in. When nothing points at a note, `explain` flags it as an orphan.
Surfaced for you to notice, never auto-changed.

### 8. Discover connections

This is the point of B2. `b2 similar <note>` surfaces the notes most semantically similar to
a given one that you haven't linked yet. It is a pure, instant read over the vectors
`reindex` already stored: no model call, no network, no cost.

```console
$ b2 similar notes/spaced-repetition
0.7132 cramming (notes/cramming.md)
   Massed practice yields only short-term recall.
0.6318 forgetting-curve (concepts/forgetting-curve.md)
   Retention decays exponentially without review.
# stderr: Commit one with: b2 link notes/spaced-repetition <note> --type <verb>
```

Each row is a similarity score, the note's name and path, and the passage that made it
similar. The note itself and anything already linked to it never appear. `--limit N` caps the
list (ten by default).

Ranked, always: `--limit` is a cap, not a quota. The list is the ranked nearest, and it
under-fills only when the vault genuinely has fewer unlinked, embedded notes than you asked
for. Similarity is relative to your vault: in a single-subject vault everything is somewhat
related, and the top of the list is still the right answer, so B2 serves the ranking and
leaves the judgment to you. An empty list (at any nonzero `--limit`) means only that there
was nothing to compare, not that nothing relates.

You are the precision gate: B2 finds the candidates; you supply the judgment and the type.
There is no review queue and nothing "inert until accepted". A connection exists only once
you author it.

Pick the connections worth keeping and commit them. There are two ways, and they are not the
same connection:

- Write a `[[link]]` in your note's body, in any editor. Cheap, and it reads naturally in
  prose. But a body link is always untyped: B2 records it as a plain `references` edge, with
  no stance and no "why". The body has no syntax for those.
- Run `b2 link` (or click Link in the desktop app). This writes a typed relation, a stance
  verb and optionally a "why", into the source note's frontmatter.

```console
$ b2 link notes/spaced-repetition notes/cramming --type contradicts --explanation "massed vs. spaced practice"
Linked notes/spaced-repetition.md —contradicts→ notes/cramming.md. Wrote the relation into the source note's frontmatter.
```

What changed on disk is one line, in the source note's frontmatter. The body is untouched:

```yaml
b2_relations:
  - "contradicts [[notes/cramming.md]] — massed vs. spaced practice"
```

`--type` takes a verb from the closed stance core: `references` (neutral), `supports` (for),
`contradicts` (against). It defaults to `references`. Three, not thirty, because stance is
the one thing embedding similarity cannot infer for you; everything else about the connection
is already in the two notes. `--explanation` records the "why", and `b2 explain` reads it
back:

```console
$ b2 explain notes/cramming
cramming (notes/cramming.md)
Connections:
  ← contradicts  spaced-repetition (notes/spaced-repetition.md)  [frontmatter]
    why: massed vs. spaced practice
```

Re-run `b2 similar` afterward and a note you just linked drops off the list: it's connected
now, so the exclusion filters it out. To undo a link, delete its `b2_relations:` entry; it's
gone on the next reindex. There is nothing else to clean up, because the edge only ever lived
in your Markdown.

### 9. Ask your vault (optional, needs a local model server)

`b2 ask` streams an answer grounded in your own notes. B2 retrieves the most relevant
passages with the same hybrid search as step 6, hands the model only those, and the answer
cites them by `[n]`. It needs a model server: Ollama by default (`ollama serve`, then
`ollama pull llama3.2`); any OpenAI-compatible endpoint works via `--llm-url`/`--llm-model`.

```console
$ b2 ask "why does spacing out reviews beat cramming?"
Spaced repetition exploits the retrieval curve [1]: expanding intervals counter the
exponential decay of retention [2], where massed practice yields only short-term recall.

Sources:
  [1] notes/spaced-repetition.md
  [2] concepts/forgetting-curve.md
```

`b2 chat` is the interactive form: follow-up questions remember the conversation, Ctrl-C
stops an answer mid-stream (the partial text stands), `/exit` leaves. The desktop app has the
same thing as a chat pane (⌘J).

A reader, not a writer: chat stores nothing. No transcript, no cache, session-only history.
Nothing about a chat is ever written to your notes or the index, and swapping chat models is
a config change, never a reindex. The default endpoint is local; a cloud endpoint exists only
if you configure one. Its key rides `B2_LLM_API_KEY`, or the macOS Keychain in the desktop
app. Never a flag, never a config file.

## Day to day

Edit notes in whatever you like. B2 is a layer over your files, not a place you have to live
in. When you rename or reorganize, let B2 keep the graph intact:

```console
$ b2 mv concepts/memory concepts/human-memory
Moved concepts/memory.md → concepts/human-memory.md
Rewrote 1 inbound link(s) across 1 file(s).
```

A move made through B2 never breaks a backlink. It rewrites the now-stale path inside every
note that pointed at the old one (so the Markdown still reads correctly in any other editor)
and re-keys the index in the same breath. Folders move as a whole. Refactor fearlessly: move,
split, merge, rename. After a batch of hand-edits, a quick `b2 reindex` catches everything
up. Or nothing at all, if you're in the desktop app, which is watching the folder anyway.

Rename a note outside B2 (in Finder, or with `git mv`) and the links pointing at it break,
exactly as they would with Obsidian closed. The difference is that B2 tells you: `b2 explain`
lists each one as unresolved, with the path it was written to, and it heals by itself the
moment a note exists there again.

## The same vault, in a window

Everything above has a desktop app counterpart. Not a viewer bolted onto the CLI, but the
other front end over the same engine: same index, same search, same discovery, same rules
about what may be written to your notes. Open the vault you just built and it's all there.

```console
$ make doctor                       # checks Node, npm, the Tauri CLI, the platform toolchain
$ B2_VAULT_PATH=~/vault make app
```

On first launch with nothing remembered, the window opens with no vault selected. Click the
vault switcher and pick a folder. After that it reopens whatever you had open last, and
`B2_VAULT_PATH` is just a way to skip that first pick.

What the window adds over the terminal:

- **You never run reindex.** It indexes the vault on open, and a native fs-watch keeps up
  with edits made outside the app (an external editor, a `git pull`), re-projecting and
  re-embedding what changed. Projection and embedding are decoupled, so a cold vault is
  browsable and keyword-searchable in seconds while the vectors stream in behind. A manual
  reindex is still there in Settings → Index, with live progress and a Cancel.
- **Discovery sits next to what you're reading.** The similar-but-unlinked notes are a pane,
  not a command you remember to run: always the ranked nearest, with a strength band grading
  each candidate within the list where a statistic exists (a vault under a dozen candidates
  is served ungraded, and says so), and a one-click typed Link that writes the same
  `b2_relations:` line `b2 link` does. There is also a graph view of the connections around
  the open note.
- **Grounded chat is a pane (⌘J):** the same cited, streamed answers as `b2 ask`, with Esc to
  stop a stream. The model endpoint and its Keychain-held key live in Settings.
- **An editor.** CodeMirror 6 with live-preview Markdown, autosave, syntax-highlighted code
  fences, `[[wikilink]]` completion, and a conflict bar if the file changed under you. A
  frontmatter drawer edits the block on its own, body untouched.
- **A file tree over the real folders:** create, rename, move, delete, and drag files in from
  Finder to import them (a dropped `.md` keeps its own frontmatter; a dropped PDF keeps its
  bytes).
- **Settings (⌘,):** pick the embedding model (base or small, downloaded on the spot), see
  whether embedding runs on CPU or the Metal GPU, and browse the whole keyboard map, where
  every chord is rebindable.

Why both? The desktop app is a second dumb adapter: each of its actions is deserialize, one
call into the same typed `Vault` façade the CLI calls, render. That is what keeps them
honest. The GUI can't grow its own idea of what "linked" means, a fix in the engine fixes
both, and the app inherits the engine's test suite instead of re-implementing search or
discovery a second time. Use the CLI for scripts, pipes, agents, or SSH; use the app to read
and write. Neither is the "real" one.

The mental model in one line: `index = projection of (Markdown)`. Your Markdown is the only
source of truth. The index is a disposable cache you can drop and rebuild identically from
your notes. Nothing locks you in.

## Command reference

| Command | What it does | Model? |
|---|---|---|
| `b2 init` | Download + verify the configured embedding model (one time, per machine) | downloads it |
| `b2 reindex` | Re-project the vault; incremental. `--dry-run`, `--force`, `--cancel` (stop a run backgrounded with `&`; `b2 status` names its pid) | embeds |
| `b2 status` | Embedding coverage (is semantic ranking live?) and whether a reindex is running | no |
| `b2 search <query>` | Hybrid keyword + semantic search. `--limit N`; `--exclude <path>` subtracts already-inspected notes (for agent loops) | embeds query |
| `b2 similar <note>` | Surface the semantically nearest notes you haven't linked yet: the ranked list, always. `--limit N` | reads vectors |
| `b2 link <src> <dst>` | Commit a typed relation into the source note's frontmatter. `--type` (references/supports/contradicts), `--explanation` | re-embeds note |
| `b2 neighbors <note>` | List a note's links, in and out; flags ones that resolve to nothing | no |
| `b2 explain <note>` | Every connection with its provenance and "why"; flags orphans | no |
| `b2 add <path>` | Create a note (`--content`, and `--title` for a frontmatter `title:`, inert to B2) and index it | embeds |
| `b2 write <note>` | Replace a note's body from stdin: the scripting/agent editing surface. Frontmatter is left alone | no |
| `b2 mv <from> <to>` | Move/rename a note, file, or folder and repair every inbound link | re-embeds touched |
| `b2 rm <target>` | Delete a note, file, or folder from the vault and disk (`-r` for a folder). Inbound links dangle and are surfaced, never rewritten | no |
| `b2 ask <question>` | One grounded, cited, streamed answer from your notes. `--llm-url`, `--llm-model`; `--json` is a JSONL event stream | embeds query + model server |
| `b2 chat` | Interactive grounded chat; session-only history, nothing stored. Ctrl-C stops an answer; `/exit` leaves | embeds query + model server |

## Config and environment

Zero-config is the happy path: everything above works with no config file. When you want to
tune, a single optional TOML in your platform's config dir configures the embedder:
`~/.config/b2/config.toml` on Linux, `~/Library/Application Support/b2/config.toml` on macOS.
Both front ends read this one file, so the CLI and the desktop app can never disagree about
which model built your vectors:

```toml
# <config dir>/b2/config.toml (all optional)
[embedder]
model = "BAAI/bge-base-en-v1.5"    # default — 768-dim, ~440 MB, the better ranker
# model = "BAAI/bge-small-en-v1.5" # 384-dim, ~130 MB, faster to download and embed
```

Those two are the supported models, the same pair the desktop app's Settings → Embedding
picker offers (where switching also downloads the new one for you). After a switch, run
`b2 init` (if you changed it by hand) and `b2 reindex`. The vectors are rebuilt, and until
they are, `search` refuses rather than mixing embedding spaces.

| Environment variable | Effect |
|---|---|
| `B2_VAULT_PATH` | Vault root, so commands find it without `-C`. An explicit `-C`/`--vault` overrides it. Read-only commands fall back to the current dir; commands that write (`reindex`/`add`/`mv`/`link`) require it explicitly |
| `B2_EMBEDDER=fake` | Offline mode: deterministic non-semantic embedder. Search runs keyword-only |
| `B2_LLM_URL` / `B2_LLM_MODEL` | The OpenAI-compatible chat endpoint + model for `ask`/`chat` (defaults: `http://localhost:11434/v1`, Ollama's, and `llama3.2`). The `--llm-url`/`--llm-model` flags beat the env, which beats the default |
| `B2_LLM_API_KEY` | Bearer token for a cloud chat endpoint. An env var, never a flag, because a key in a flag is a key in `ps`. The desktop stores its key in the macOS Keychain instead |
| `B2_LLM=fake` | The deterministic chat provider: `B2_EMBEDDER=fake`'s sibling for `ask`/`chat` |
| `B2_DEBUG` | Print internal error detail after the generic user-facing message |
| `B2_LOG` | Structured debug logging: JSON Lines on stderr (stdout stays pure data), ready for jq/DuckDB/pandas. Takes a tracing filter (`debug`, `b2::sqlite=debug`, `warn`). Includes per-statement SQLite timings; `B2_DEBUG` or `B2_LOG_FILE` alone implies `B2_LOG=debug` |
| `B2_LOG_FILE` | Write the structured log to this file instead of stderr (append mode, so runs accumulate into one dataset) |
| `B2_SLOW_QUERY_MS` | Slow-query threshold in milliseconds (default 100): statements at or over it log at WARN with `slow=true` |

Honest about limits: search snippets and scores come from the index, so reindex first.
`similar` reads the vectors a prior reindex stored, so index before you discover. And under
`B2_EMBEDDER=fake` the semantic half of search and similarity is off: great for offline
exploring, not for real recall.
