# Architecture

How B2 is built: the crates, the index, and the flows over it. Read this to find your way
around the code. What a vault holds is [data-model.md](data-model.md), the rules are
[invariants.md](invariants.md), and the reasons are [decisions.md](decisions.md).

## The idea

```
index = projection of (the vault directory)
```

The vault is the only durable thing. `<vault>/.b2/b2.sqlite` is a cache derived from it:
delete it, reindex, and it comes back the same (S2). Everything below exists to keep that
true.

## Crates

| Crate | Role |
|---|---|
| `b2-core` | The whole engine and its one typed API, the `Vault` façade (`vault.rs`). Model-free and deterministic: bundled SQLite with FTS5, blake3, YAML. No ML, no HTTP, no clock. |
| `b2-embed` | The real embedder: candle running a bge model in-process (bge-base, 768-dim, by default; bge-small supported). Also home of the eval harness. |
| `b2-llm` | The real chat provider: a small sync OpenAI-compatible streaming client over `ureq`. Ollama by default. |
| `b2-cli` | The `b2` binary. Parse args, inject the embedder and chat provider, make one `Vault` call, print text or `--json`. |
| `b2-desktop` | The Tauri host. Each command is deserialize, one `Vault` call, serialize, reusing the CLI's `--json` view types as the IPC contract. The frontend is `ui/` (Vite, vanilla TypeScript, CodeMirror 6). |

Dependencies point one way, from the adapters to the core (E3). Engine modules other than
`vault.rs` are called directly only by tests. The models reach the core through two traits
it defines, `Embedder` and `LlmProvider`, so the engine and its tests run on deterministic
fakes (M1).

## The index

```
b2.sqlite
├── meta(key, value)                       schema_version, embed_model_id, embed_dim
├── notes(path PK, title, created, body_hash, mtime, indexed_at)
├── resources(path PK, class, size, mtime, content_hash, indexed_at)
├── chunks(id, note_path, seq, char_start, char_end, token_count,
│          heading_path, text, text_hash)
├── chunks_fts                             FTS5 over chunk text, `porter unicode61`
├── embeddings(text_hash PK, vector)       created at embed time
├── note_centroids(note_path PK, centroid) created at embed time
└── edges(id, src_path, dst_path, dst_resource_path, dst_path_raw,
          type, origin, explanation, caption, embed, occurrence_index)
```

The DDL is in `db.rs`. What matters about the shape:

- **Everything keys on the note's path** (L1). `chunks`, `note_centroids` and
  `edges.src_path` reference `notes(path)` with cascading update and delete, so a move is
  one `UPDATE`. `edges.dst_path` is deliberately not a foreign key: it has to be NULL for a
  dangling link (G5).
- **Vectors are keyed by the hash of the chunk text** (M4), which is exactly what was
  embedded. Identical text shares one vector, a moved note re-embeds nothing, and a hash no
  chunk references is garbage the whole-vault pass collects.
- **The vector tables exist only once something has been embedded.** Their absence is the
  signal for keyword-only search.
- **Vectors are plain BLOBs scanned in-process**, with no vector extension. At
  personal-vault scale (100k chunks) an exact scan takes milliseconds, it measured faster
  than a `vec0` virtual table (GH #38), and an extension is one more binary to ship.
- **`edges` is a cache of what parsing would say** (G6). It exists because backlinks, typed
  traversal and discovery's "already linked" exclusion can't be answered from one file.
- **A schema change bumps `schema_version` and rebuilds** (S5). There are no migrations.

## Reindex

`ingest.rs`. `b2 reindex` is two passes that can also run separately:

1. **Project** (model-free): walk the vault, upsert notes and resources, chunk each body
   into `chunks` and FTS, then derive edges. Edges come in a second phase, after every note
   is known, so link resolution never depends on file order.
2. **Embed:** fill in a vector for every chunk that lacks one, in batches, then refresh the
   centroids.

A projected but unembedded vault is already keyword-searchable. The desktop relies on that:
it projects on every vault open, paints, embeds in the background, and watches the folder
for changes made anywhere.

- **Incremental by default.** A note whose body hash is unchanged and whose chunks all have
  vectors is skipped. Edges are always re-derived, because a link's target may have moved.
  `--force` re-projects everything and still embeds only text it has no vector for.
- **Only the whole-vault pass prunes** (S3): rows for files that are gone, vectors nothing
  references. Single-note paths (`add`, `write`, `link`, `mv`) touch their own note.
- **One unreadable file is skipped and reported.** It never fails the index.
- **Cancellable and resumable.** A cancel lands between embed batches, and the next run
  embeds whatever still lacks a vector.
- **It writes nothing to the vault** (W1).

**Chunking** (`chunk.rs`) follows [qmd](https://github.com/tobi/qmd)'s heuristic: chunks of
about 450 tokens with about 15% overlap, cut at headings first, then paragraph breaks, and
never inside a fenced code block or a table. Size is estimated as characters ÷ 4, so the
core needs no tokenizer. Each chunk records its heading path and its character span in the
body.

## Search

`search.rs`, entered through `Vault::search_evidence`.

1. **Check the model.** If the recorded embedding model differs from the active one, fail
   with `ModelMismatch` ("run `b2 reindex`") rather than compare vectors from two spaces
   (M2).
2. **Keyword leg.** The query is sanitized into a safe FTS5 expression (each word quoted,
   OR-joined) and ranked by BM25 over the stemmed index.
3. **Semantic leg.** The query is embedded and every chunk vector is scored by exact
   distance. A vault with no vectors skips this leg and searches by keyword only.
4. **Fuse.** The two ranked lists merge by Reciprocal Rank Fusion
   (`score = Σ 1/(60 + rank + 1)`). It uses positions because the two score scales aren't
   comparable. Ties break toward the semantic rank.
5. **Resolve.** Chunks collapse onto their note's best chunk, each with a snippet, up to
   `limit`.
6. **Verdict** (D2). Beside the fused order, search reads the two absolute signals that
   fusion throws away: how much of the query's IDF weight the vault's text covers, and the
   best cosine. `vouched` is true when either clears the bar for the active embedding model,
   false when neither does, and null when that model has no calibrated bar.

Surfaces act on the verdict. `false` shows "No matches" and none of the rows, though
`--json` still returns them beside the verdict so an agent can be honest about them. `null`
serves as always.

Each leg pulls a pool wider than `limit` before fusing. Pool width changes the answers, not
only the speed, so it moves only on eval evidence ([evals.md](evals.md)). Chat's retrieval
(`search_chunks`) is the same pipeline at passage level.

## Discovery

`discover.rs`, entered through `Vault::similar`. It answers "what belongs next to this note
that I haven't linked?" from stored vectors alone: no model call, no network.

1. Load the anchor's stored chunk vectors.
2. Exclude the anchor and everything one link away.
3. **Shortlist by centroid.** Scan every note's centroid and keep the nearest
   (`SHORTLIST_PER_RESULT` per requested result, at least `SHORTLIST_MIN`). This only bounds
   the work. It is never a quality gate.
4. **Score by best passage.** Each shortlisted note scores by its single best chunk pair
   against the anchor. The best pair decides, not the average, so one strong section inside
   a sprawling note still counts, and that passage is returned as the evidence.
5. Rank, attach a z-score when the list can be graded (at least `STATS_MIN_POPULATION`
   candidates, a real model, non-zero spread), and cut to `limit`.

The list is always served (D1). The z only paints the strength band, and an ungraded list
says so. `explain_similar` (`b2 similar --explain`, the app's **Explain**) reads one
candidate's place in the same computation, so it can't disagree with the list.

## Chat

`chat.rs` and `vault/chat.rs`. Chat reads the index and stores nothing: no transcript, no
cache, session-only history. Swapping chat models is a config change.

**`Vault::ask`** runs five steps: condense a follow-up into a standalone query (falling
back to the raw question if that fails), retrieve the top `ASK_PASSAGES` passages with the
search pipeline, assemble the grounded prompt, stream tokens through a callback whose return
value cancels, then resolve each `[n]` to a path and excerpt. A marker that names no passage
resolves to nothing, and the answer text is never rewritten.

**`Vault::why_similar`** (`b2 why`, the app's **Why?**) is the one tool-using turn. The
model is offered four read-only tools, each a single façade read: `b2_passage_pairs`,
`b2_similar`, `b2_neighbors` and `b2_read`.

- The loop lives in the core, so both adapters behave the same, and it is bounded
  (`MAX_TOOL_ROUNDS`, `MAX_CALLS_PER_ROUND`). The last round offers no tools.
- A tool call is model output. A malformed or unknown one gets an `error:` result, not a
  failed turn.
- If the model skips the pair lookup, the core adds it, marked `seeded`. If the model or
  provider can't use tools, the core makes the lookup itself and sends one plain grounded
  request.
- Every passage a tool returns is numbered on one ledger for the turn, and the answer lists
  which tools ran.
- A reply carrying more tool calls than `B2_LLM_MAX_TOOL_CALLS` fails loudly. That is a
  broken server, not a model without tool support.

## Moves

`mv.rs`. Identity is the path, so `b2 mv` has to rewrite every inbound link. The `edges`
table names exactly which files and links to touch.

- Every inbound file is read and its rewrite computed before anything is written. An
  occupied destination is refused.
- The rewrites and the rename run as a unit. If a step fails, the earlier ones are undone
  and the vault is byte-identical to before. If an undo fails, the error names the files
  still holding a rewrite (`MoveIncomplete`).
- Then the index is re-keyed with one cascading `UPDATE` and the inbound notes are
  re-projected. If that fails, the Markdown is already correct and `b2 reindex` catches the
  index up.
- Nothing re-embeds, because vectors belong to the text.

A move made outside B2 is a delete plus a create. Inbound links dangle visibly (G5) and
heal if the target reappears. B2 doesn't guess a repair (W4).

## Concurrency

`db.rs`. The app, a `b2` command and a backgrounded `b2 reindex &` may all hold one index
(C1).

- Opening a current, complete index costs two reads and no write lock, so a reader is never
  refused because a reindex is running.
- The two drop-and-rebuild paths (the schema, the vector tables) each run as one
  `BEGIN IMMEDIATE` transaction that re-checks inside the lock. SQLite's own lock does the
  serializing. There is no lock file, because a lock file stops meaning anything on a
  synced folder.
- The switch to WAL mode is retried by hand: it is the one statement `busy_timeout` doesn't
  cover.
- Waiting is bounded. Past the budget, a stuck writer is reported.
- Writers stay single in flight: the CLI's `reindex` lock, the desktop's one embed slot.

Any new drop-and-rebuild in `db.rs` must take the same shape.

## The embedding model

- `b2 init` downloads and verifies the model, and nothing else ever downloads. Without it,
  anything that embeds fails fast with "run `b2 init`".
- Changing the model or the compute device is a swap (M2): the next `reindex` drops the
  vectors and re-embeds everything. Metal is chosen at build time (`make app` on Apple
  Silicon).
- `B2_EMBEDDER=fake` hashes text into a vector: deterministic, not semantic. It is what the
  tests run on.

## Tests

The suite is the executable spec, and it is mostly integration: open a real SQLite index
over a real vault on disk and assert on the projection.

- `crates/b2-core/tests/`: one file per property (`roundtrip`, `graph`, `mv`,
  `discover_surfacing`), over a temp copy of `fixtures/golden-vault/`.
- `crates/b2-cli/tests/cli.rs`: every command through the real binary, text and `--json`,
  exit codes included.
- `b2-desktop`, `b2-embed` and `b2-llm` test their own wiring. `ui/` tests its pure-logic
  modules under node.

Everything runs on the fakes. Whether the results are any good is measured by the eval
harness, out of CI ([evals.md](evals.md)).
