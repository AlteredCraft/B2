# Architecture

How the B2 system is built, for anyone working on the code. Read this to orient yourself:
what the pieces are, how data flows through them, and where each rule is enforced. Then
follow the links into the two specs for the details.

B2 is a local-first Markdown vault with an AI layer that surfaces semantically similar,
not-yet-linked notes for you to connect. This repo is the index engine plus its two adapters:
the `b2` CLI and a Tauri desktop app. A SQLite store is treated as a disposable projection of
your Markdown, real local AI sits behind two enumerated seams, and both adapters drive the
same typed `Vault` API. The *why* behind each choice is an [ADR](../ADRs/README.md); the
normative spec is [invariants.md](invariants.md), [data-model.md](data-model.md), and
[index-engine.md](index-engine.md).

## The governing equation

One equation shapes every decision (ADR-0002; invariants S1 to S5):

```
index = projection of (the vault directory)
```

Delete `b2.sqlite`, rebuild it from the Markdown, and it must come back identical. Exactly
one thing is durable and un-derivable: your vault, the source of truth for both knowledge and
every committed connection. There is no state anywhere outside your notes.

- **Tier 1, the vault: source of truth.** Notes plus every committed edge, plain `.md`, fully
  usable in Obsidian with no B2. Plus resource peers (PDFs, images, clippings) and the folder
  tree itself. Stays pristine: B2's only writes are surgical and in frontmatter; the body is
  100% yours.
- **Tier 2, `b2.sqlite`: disposable cache.** FTS5, plain vector tables scored in-process, and
  the typed graph. Holds nothing that can't be rebuilt from the vault.

A whole vault is one portable folder: the index lives under `<root>/.b2/`, a dot-folder both
Obsidian and B2's own vault scanner ignore. Point B2 at a folder of Markdown; that is the
entire setup.

## The five crates

The engine owes nothing to the two adapters above it or the two model crates beside it. The
AI-heavy crates sit behind traits defined in `b2-core` (the `Embedder` and `LlmProvider`
seams), so the engine and its fast suite never import a tensor or an HTTP client, and adding
the desktop UI added one adapter, not new architecture (ADR-0012).

| Crate | Role |
|---|---|
| `b2-core` | The engine: turns a folder of Markdown into a queryable index and keeps it a pure function of disk. Model-free (no candle, no network) so its suite is fast and deterministic. rusqlite (bundled SQLite + FTS5), blake3, yaml. No vector extension: vectors are plain tables scored in-process (ADR-0006) |
| `b2-embed` | The real embedder: `LocalEmbedder`, candle-backed BERT (bge-base 768-dim default, bge-small 384-dim supported), pure-Rust inference. `b2 init` downloads + verifies into a shared XDG cache. All heavy ML deps live only here (ADR-0020) |
| `b2-llm` | The real chat provider: a hand-rolled sync OpenAI-compatible SSE client over `ureq` behind the `LlmProvider` seam. Ollama by default, any compatible endpoint by config. Owns the stream's quirks as tests. No async runtime anywhere (ADR-0011) |
| `b2-cli` | The `b2` binary. Holds no engine logic: parse args, inject the embedder and chat provider, call the `Vault` façade, print (human, streamed, or `--json` for agents). Funnels every error through `user_message` so nothing internal leaks |
| `b2-desktop` | The Tauri host, the GUI sibling of `b2-cli`, equally logic-free: each `#[tauri::command]` is deserialize, one `Vault` call, serialize, reusing the CLI's `--json` view types as the IPC contract. The `ui/` frontend (Vite + vanilla TS + CodeMirror 6) renders the read → discover → link → edit → chat loop |

Ground truth: the layering is enforced by where the dependencies live. `b2-core`'s
`Cargo.toml` has no candle and no HTTP client; the whole engine suite runs on deterministic
fakes. The real models are reached only through `b2 init`, a configured chat endpoint, and
the out-of-CI eval harness.

## The `Vault` façade

Every engine module is called directly only by the integration tests. The `Vault` façade
(`crates/b2-core/src/vault.rs`) is the single typed entry point, and its only clients are the
two adapters. It owns the connection and the injected seams, and returns display-ready view
structs the CLI prints and the desktop reuses as its IPC contract. Façade operations are
added when a command needs them, never pre-built. The full command surface, with what each
command needs, is the [quickstart's command reference](quickstart.md#command-reference).

## The four flows

Each flow is specified in [index-engine.md](index-engine.md); this is the shape of each, and
where it lives.

**Flow ①: ingest** (`ingest.rs`). `b2 reindex` composes two separately invokable passes:
model-free `project_vault` (notes, resources, chunks, FTS, edges), then `embed_vault` (fill
the missing vectors). Projection runs in two phases so link resolution never depends on file
order. Ingest writes nothing to the vault, is incremental by default, and converges on
exactly what a from-scratch rebuild would produce. A projected-but-unembedded vault is
already keyword-searchable. Details: [index-engine.md §3](index-engine.md).

**Flow ②: search** (`search.rs`). BM25 keyword search and brute-force vector KNN run over the
same index and fuse with Reciprocal Rank Fusion, resolved from chunks up to notes (ADR-0008).
A served result is a claim of evidence (D2, ADR-0015): a query the vault holds no lexical or
semantic evidence for answers "no matches", and `--json` is an object (rows plus the
`vouched` verdict). On an unembedded vault, results are BM25-only: degraded honestly, never
an error. Details: [index-engine.md §4](index-engine.md).

**Flow ③: similar, then link** (`discover.rs`, `note.rs`). `b2 similar <note>` surfaces the
nearest notes you haven't linked yet: a two-stage scan over stored vectors, no model call, no
network. The machine finds candidates; you supply the judgment and the type (ADR-0009). The
ranked list is always served; `limit` is a cap, not a promise (D1, ADR-0014). Committing is
`b2 link`, which appends one typed-link line to the source note's frontmatter and re-projects
it, or a `[[link]]` you write in the body yourself. Details:
[index-engine.md §3 and §4](index-engine.md).

**Flow ④: grounded chat** (`chat.rs`, `Vault::ask`). `b2 ask`, `b2 chat`, and the desktop's
chat pane stream an answer grounded in your notes: condense, retrieve (flow ② at 10
passages), assemble the grounded prompt, stream (cancellable at token granularity), cite
(`[n]` markers resolve to path + excerpt). Chat is a reader: nothing model-derived is stored,
and swapping chat models is a config change, never a reindex. Details:
[index-engine.md §6](index-engine.md).

## The write discipline

The vault stays yours. A parsed note keeps its raw text verbatim and records only the byte
spans of the frontmatter block; serialization returns those raw bytes. So
`parse → serialize → parse` is byte-identical: unknown keys, comments, odd whitespace, a
missing final newline all survive (W5). Against that backdrop, every byte B2 writes is the
mechanics of a command you invoked (W1, ADR-0004). The complete list of on-command writes is
[invariants.md W3](invariants.md).

`b2 mv` never breaks the graph. A note's identity is its path, so a move is a re-key, not a
rebuild: children reference `notes(path)` with `ON UPDATE CASCADE`, so one statement carries
chunks, vectors, and edges across, and inbound link text is repaired in exactly the files the
materialized graph names. Move a note outside B2 and the bargain is the honest one: the links
dangle, visibly, until you repair them, and content-addressed vectors make the
delete-plus-create re-embed nothing. Details: [index-engine.md §8](index-engine.md).

## The AI seams

Every AI part sits behind a swappable trait defined in `b2-core` (ADR-0005; M1). The engine
is built and tested against deterministic fakes; a real model drops in through the identical
seam with no schema or flow change. There are exactly two seams, and they differ on the one
axis that matters: what they are allowed to store.

- **`Embedder` carries the index's identity.** `meta` records
  `(embed_model_id, embed_dim)`, device tag included; on a change, the vector tables are
  dropped and a full re-embed follows on `reindex`. Opening a vault never mutates the vector
  space; a stale `search` fails fast with `ModelMismatch` (M2, ADR-0007).
- **`LlmProvider` deliberately carries none.** Chat stores nothing, so swapping chat models
  never touches the index. Chat config is adapter-level (flags/env on the CLI, Settings on
  the desktop, resolved once in `b2_llm::LlmConfig`), never vault or index state.

The relation vocabulary rides beside them: a closed three-verb stance core (`references`,
`supports`, `contradicts`) is your typing palette on `b2 link --type`, each verb with a
display-only inverse label. Three, not thirty, because stance is the one thing embedding
similarity cannot infer: the vectors already tell you two notes are about the same thing;
whether one backs or fights the other is what only you know. A tail verb you write by hand is
kept verbatim (ADR-0010; [data-model.md §2](data-model.md)).

## Grounded in the tests

Nothing here is aspirational. The suite is the executable specification, integration-first:
most tests open a real SQLite database, ingest a real vault on disk, and assert on the
resulting projection. Most share the golden vault fixture (`fixtures/golden-vault/`), copied
into a temp dir before every run; the copy plus CI's tree-clean assertion is what stands
behind "no unbidden writes".

- The engine suite (`cargo test -p b2-core`, the bulk of the weight): one integration file
  per property, named for it (`roundtrip`, `graph`, `mv`, `discover_surfacing`, …).
- The CLI end to end (`cli.rs`): every command through the spawned real binary, human and
  `--json`, exit codes included.
- The adapters stay dumb: `b2-desktop`'s suite pins commands, errors, fs-watch, and the menu;
  `b2-embed` pins config and provisioning; `b2-llm` pins the SSE wire shape; the `ui/`
  pure-logic suite runs under node's own test runner.

The two gates are `make check` (fast, the working loop) and `make ci` (verbatim what CI runs,
ADR-0018). Nothing is `#[ignore]`d: a check that genuinely needs the real model lives in the
eval harness, which runs on demand and therefore actually runs. Model quality never flakes CI
(ADR-0013); how it *is* measured is [evals.md](evals.md).

## Not yet built

What remains is tuning, scale, and packaging, tracked in
[GitHub Issues](https://github.com/AlteredCraft/B2/issues):

- Semantic quality in CI: never. The engine suite proves plumbing on the fake embedder; the
  real model is measured by the out-of-CI harness ([evals.md](evals.md)).
- A cross-encoder reranker is the likely next seam: post-fusion, it changes ordering, not the
  store, gated on the eval like everything else. Query expansion sits behind it in priority.
- Resource content search: resources are inventoried and are graph targets today; chunking
  and embedding them is designed, not shipped ([data-model.md §10](data-model.md)).
- Packaging and distribution: B2 ships as source today.

Source of truth for every claim: `crates/*/src/` and the test suite under each crate's
`tests/`. The spec is this folder; the *why* is [ADRs/](../ADRs/README.md); the backlog is
GitHub Issues.
