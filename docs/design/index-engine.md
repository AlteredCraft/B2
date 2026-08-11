---
b2id: 01KWSRKASCAYXRPQ1AP5HE6R2N
title: "B2 — Index Engine: rebuild qmd on SQLite"
type: note
tags: [b2, index-engine, sqlite, fts5, vectors, search, reranker, architecture]
created: 2026-06-29
status: draft
---

# B2 — Index Engine: rebuild qmd on SQLite

> **The engine design — the *how*.** Evaluates rebuilding [tobi/qmd](https://github.com/tobi/qmd) on
> our own SQLite store (FTS5 + an in-process vector scan, reranker as a fast follow) instead of
> adopting qmd as a dependency, and specifies the result. Companion design docs:
> [invariants.md](invariants.md) (the *why*) and [data-model.md](data-model.md) (the *what*); semantic
> search is **engine-gated**, single-binary, local-first.

## TL;DR / recommendation

**Build our own SQLite-backed index engine; take qmd as a design reference, not a dependency.**

- qmd is an excellent *blueprint* for hybrid retrieval (BM25 + vector + RRF + LLM rerank) and proves
  the whole pipeline runs locally. But it is a **search engine**, and B2 is not — B2 is a **typed graph
  with hybrid retrieval over it**. qmd has no notion of typed edges, backlinks, or `b2id`-stable identity,
  which are the reasons B2 exists ([invariants.md](invariants.md)).
- SQLite gives us **one embedded store for every *queryable* concern at once** — full-text (FTS5),
  vectors (plain tables scored in-process), and the typed graph — with transactional consistency across them, so
  `b2 similar` candidate generation joins all three in a single query. That single-store property is
  worth more to B2 than anything we'd inherit by depending on qmd. The index is a pure **disposable
  cache**: `index = projection of (the vault directory)` — drop it, reindex, get it back identical,
  with **no durable B2-derived state outside your notes** (two tiers, [data-model.md](data-model.md)).
- Because the engine **does** provide vector search, the locked **engine-gated** decision resolves in
  favour of **semantic search in v1**, not as a fast follow ([invariants.md](invariants.md)).
- The **reranker is a clean fast-follow**: a swappable seam after RRF fusion, exactly as the testability
  stack wants the AI parts isolated. Retrieval quality is good without it; it's pure upside later.
- The one genuinely hard part is **not the engine** — it's **producing embeddings inside a single
  binary** ([invariants.md](invariants.md)). qmd solves this with
  `node-llama-cpp` + GGUF + Node 22, a heavy stack that fights the single-binary goal. This is the real
  decision to make, and it is **orthogonal to choosing SQLite** (see §7).

---

## 1. What qmd actually is (the reference)

A local CLI search engine for Markdown, all on-device. The shape worth stealing:

- **Storage:** SQLite at `~/.cache/qmd/index.sqlite` with FTS5 + `sqlite-vec`. Tables: `collections`,
  `path_contexts`, `documents`, `documents_fts` (FTS5/BM25), `content_vectors` (chunk metadata),
  `vectors_vec` (`sqlite-vec` index), `llm_cache`.
- **Chunking:** ~900-token chunks, ~15% overlap, Markdown-aware break-point scoring (H1=100, H2=90,
  code-fence=80, … blank-line=20, list-item=5), with a 200-token backward scan and quadratic distance
  decay to pick the cleanest boundary. Optional tree-sitter AST chunking for code files.
  **Implemented in B2** (`chunk.rs`, #19, 2026-07-13),
  with four model-free adaptations: a **~450**-token target (headroom under bge's 512 truncation), a
  `chars/4` proxy for token sizing (the core stays tokenizer-free), an unconditional `heading_path`
  breadcrumb, and every lever on a `ChunkConfig`. Tree-sitter code chunking stays deferred (#41 / spec §8).
- **Three search modes:** `search` (BM25 only), `vsearch` (vector only), `query` (hybrid).
- **Hybrid pipeline (`query`):** LLM query expansion (1–2 variants, original weighted 2×) → parallel
  BM25 + vector retrieval per variant → **RRF fusion** (`Σ 1/(k+rank+1)`, k=60) + small top-rank
  bonuses → keep top 30 → **LLM rerank** (cross-encoder, 0–1) → **position-aware blend** (top ranks
  trust retrieval more, deep ranks trust the reranker more).
- **Models (local GGUF via `node-llama-cpp`):** EmbeddingGemma-300M (embed, ~300 MB),
  Qwen3-Reranker-0.6B (rerank, ~640 MB), a fine-tuned 1.7B (query expansion, ~1.1 GB). ~2–3 GB VRAM
  with all three loaded.
- **Surfaces:** rich CLI, JSON/CSV/files output for agents, an **MCP server** (stdio + HTTP daemon),
  and a TypeScript SDK (`createStore`).
- **Stack:** TypeScript on Node 22+/Bun; MIT licensed.

It's a clean, well-thought-out design. The disagreement is **scope**, not quality.

## 2. Why we rebuild instead of depend on qmd

| Concern | qmd | B2's need |
|---|---|---|
| Full-text search | ✅ FTS5/BM25 | ✅ same |
| Semantic search | ✅ `sqlite-vec` | ✅ in-process scan |
| Rerank | ✅ cross-encoder | ✅ fast-follow |
| **Typed graph** (`b2id→b2id` edges with a relation type) | ❌ none | ⭐ core (areas 3, 5) |
| **Backlinks** (who points at X, typed, over the whole vault) | ❌ none | ⭐ core (area 5) |
| **`b2id`-keyed identity** surviving move/rename | ❌ path-keyed, cache is disposable | ⭐ core (invariants L1) |
| **Markdown as source of truth** (index is rebuildable/derived) | ~ index *is* the artifact | ⭐ non-negotiable (principle #1) |
| Distribution | npm package, Node runtime | ⭐ single binary (principle #5) |

The decisive point: B2's index is a **derived projection of the vault** that holds the **typed graph**
*next to* the search indexes, so retrieval and connection discovery share one transactional store —
`index = projection of (the vault directory)`, drop-and-rebuild at any time ([data-model.md](data-model.md)). qmd
models none of the graph layer — wrapping it would mean maintaining a second store for everything that
makes B2 *B2*, and reconciling two sources of truth. Rebuilding the ~300 lines of retrieval glue that we
actually want is cheaper than that integration tax — and qmd's MIT license + public design make the
rebuild low-risk.

**What we borrow wholesale:** the chunking heuristic, the RRF formula and k, the position-aware blend,
the asymmetric query/document prompt discipline (each model brings its own prefix — B2 ships bge's, §6;
not EmbeddingGemma's `task:…|query:` / `title:…|text:`), the JSON/`--explain`
agent-output discipline, and the MCP surface idea. **What we discard:** the npm/Node packaging and the
"DB is the product" framing.

## 3. The storage architecture (one disposable SQLite index)

One artifact, per the two-tier model ([data-model.md](data-model.md)) and realizing the **"volatile vault
over a disposable index"** tenet ([invariants.md](invariants.md)): a
**disposable** SQLite index holding every queryable concern transactionally. The whole index is
**rebuildable from the vault** — drop `b2.sqlite`, re-scan the vault, get back an identical index (the
locked `full-reindex ≡ incremental-update` invariant). The vault is the single source of truth (with
Markdown its sole authored subset — notes + every committed edge); the index is a cache of it, with
**no durable B2-derived state outside your notes**.

> The precise DDL, the relations between these tables, the read/write data flows, and the build order
> are realized in the code (`crates/b2-core/src/db.rs` schema + `ingest.rs` flows). The sketch below is
> the orientation; the code is the buildable contract.

```
b2.sqlite — DISPOSABLE CACHE  (= projection of Markdown; drop & rebuild any time)
├── MIRROR OF MARKDOWN (source of truth for *knowledge*; lets us diff vs. disk)
│   ├── notes(b2id PK, path, title, type, frontmatter_json, body_hash, mtime, …)  -- b2id ← frontmatter
│   └── note_bodies(note_b2id, content)          -- optional cache of file text
│
├── DERIVED FROM MARKDOWN: SEARCH
│   ├── chunks(id, note_b2id, seq, char_start, char_end, token_count, text)
│   ├── chunks_fts                                -- FTS5 over chunk text (BM25)
│   ├── embeddings(chunk_id, vector)              -- plain BLOB vectors (768-dim), scored in-process
│   └── note_centroids(note_b2id, centroid)       -- per-note centroid (discovery's coarse stage)
│
├── DERIVED FROM MARKDOWN: TYPED GRAPH
│   └── edges(id, src_id, dst_id, type, origin,   -- every row ← Markdown (body links + FM b2_relations:)
│             explanation, …)                     -- origin ∈ {inline, frontmatter}; every edge active
│
└── CACHES (disposable)
    └── llm_cache(key, value, created)            -- reserved for a future reranker (fast-follow, §5)
```

Every table is derived from the vault; there is no third home.
*(The projection is built in two separately-invokable passes —
model-free `project` (notes/chunks/FTS/edges) then `embed` (vectors), with `reindex` their
composition — so keyword search + graph are usable before embedding completes;
the `project`/`embed` split ([#15](https://github.com/AlteredCraft/B2/issues/15)). The invariant is untouched:
a projected-but-unembedded index is a smaller projection, never a wrong one.)*

**Resources widen the projection.** A real vault also holds non-`.md` files, and the walk inventories
them. The locked
design ([data-model.md](data-model.md) §10, [#66](https://github.com/AlteredCraft/B2/issues/66))
adds them as **path-keyed peers** without disturbing any statement above — the source *tier* is the
whole vault directory, so **`index = projection of (the vault directory)`**:

- **A `resources` table** — `(path PK, class, size, mtime, content_hash, indexed_at)` — a **separate**
  table from `notes`, not a `kind` column on it (two tables, two contracts, zero "unless it's a resource"
  clauses). Class is by **extension only** (deterministic; misclassification degrades, never mis-executes):
  `note` · `text` · `html` · `pdf` · `image` · `media` · `binary` (the total fallback), each answering the
  same three questions — what index text, can it be a graph endpoint, how does it render.
- **`chunks` generalizes** from `note_b2id` to a **document reference** (a note `b2id` *or* a resource
  path — as one-of nullable FKs on the single table, CASCADE intact for both parents; locked,
  [#66](https://github.com/AlteredCraft/B2/issues/66)); search resolves hits up to the
  owning document and results carry a `kind`. **Centroids follow** — two-stage discovery's coarse stage
  scans only centroids (#38, §4 update), so a resource with chunks but no centroid would be searchable yet
  invisible to `b2 similar`; a sibling `resource_centroids` table (same locked call) is maintained through
  the existing lifecycle (embed-pass refresh, re-chunk drop) and the coarse stage scans both. Every
  class funnels to *text* — native, extracted (`html` strip / `pdf` text layer), or, for an `image`,
  aggregated inbound alt-text — embedded through the **existing** bge space: one embedding space in v1,
  the multimodal seam documented for later (§6 posture, [data-model.md](data-model.md) §10).
- **`edges.dst` may be a resource** — a body `![[photo.png]]` / `[[papers/x.pdf]]` resolves against
  `resources` and records a `dst_resource_path`; `src` stays a note (resources author no outbound edges in
  v1). The existing `dst_path_raw` + dangling-edge index (`db.rs`) is already half of this; the `link.rs`
  parser learns the two Markdown-native forms `![alt](path)` / `[text](path)` (relative paths only) and the
  `![[file.ext]]` embed, capturing the alt/caption text on the edge (it becomes the image's index text).
- **No migration, ever.** Because the index is disposable this is a `schema_version` bump + rebuild — the
  disposable-index tenet paying rent. The `resources` DDL lands in the **slice-1 build spec**
  ([#65](https://github.com/AlteredCraft/B2/issues/65)); the chunk/centroid generalization and the per-class extraction step land in
  slice 3's; the PDF text-extraction *dependency* (which crate, and its home) is deferred to slice 4 by
  design.

Why this shape fits B2 specifically:

- **Edges key on `b2id`, never path** — directly implements the link-identity decision
  ([data-model.md](data-model.md) §1). `notes.b2id` is the durable
  frontmatter identity (B2's one always-allowed write); `src_id`/`dst_id` and `note_b2id` all hold
  `b2id` values. A move rewrites `notes.path` and inbound `[[path|title]]` text; every row in `edges`
  is untouched because it never referenced the path. "Rename keeps every backlink resolving" becomes a
  foreign-key truth, not a fix-up pass.
- **Every `edges` row derives from Markdown** — body links (`origin=inline`, all untyped `references`) ∪
  frontmatter `b2_relations:` (`origin=frontmatter`, the sole typed home), deduped frontmatter-wins on
  same-`(target, type)` overlap. There is **no `status` column and no suggestion queue**: an edge exists
  iff it is authored in the Markdown. Committing with **`b2 link`** appends a typed-link
  string to the source note's frontmatter `b2_relations:` (Markdown first; never the body —
  [data-model.md](data-model.md) §0), then re-projects that note — a projection of an authored line, not
  an in-place index write.
- **Hybrid retrieval and graph queries compose in one query** — e.g. "semantic-nearest chunks whose
  note is within 2 typed hops of note X" is a join across `embeddings`, `chunks`, and `edges`. This is
  the substrate **`b2 similar`** (connection-discovery candidate generation) runs on, and it's the thing a
  qmd-as-dependency design could never give us cleanly.
- **Deterministic seams for tests** — a fake embedder (deterministic vectors) writes to `embeddings`, so
  the whole pipeline is assertable with no live model (testability stack, points 4–5). The embedder is the
  one AI seam.

### Opening the index concurrently — many readers, one builder

Nothing in B2 opens the index exclusively, and several things open it at once: `b2 reindex &` racing a
`b2 status`, the desktop app launching while a CLI reindex runs, the desktop host's own threads. The
locked stance is **C1** ([invariants.md](invariants.md)) — unrestricted readers, a serialized builder —
and `db::open` is where it is enforced, in three layers that each answer a different failure:

- **The `WAL` flip is retried** ([#111](https://github.com/AlteredCraft/B2/issues/111)). Setting
  `journal_mode = WAL` is the one statement in `open` that takes a write lock, and the one
  `busy_timeout` cannot cover (SQLite skips the busy handler for a write lock upgraded from an already-open
  read transaction), so a second opener took an immediate `SQLITE_BUSY`. The wait is ours to do.
- **The schema migration is one `BEGIN IMMEDIATE` transaction, entered only when there is work**
  ([#114](https://github.com/AlteredCraft/B2/issues/114)). It is a read-then-decide-then-write sequence,
  and its rebuild is ~30 DDL statements; unserialized, two openers that both read a stale
  `schema_version` interleave — one's `DROP TABLE resources` landing after the other's `CREATE`, and the
  current version stamped over a half-demolished schema. `busy_timeout` was irrelevant: nothing contended
  for a lock, every statement succeeded, in the wrong order. Serializing on SQLite's own write lock makes
  the loser wait and then find the work done; wrapping it makes a rebuild all-or-nothing, which is what
  lets the stamp be trusted. The check that decides whether to enter is a **read** — so the common open,
  against a current schema, takes no write lock and can never be refused by a writer.
- **Completeness is checked, not assumed.** A stamp is believed only alongside the tables it vouches
  for; a current stamp over missing tables (the wreckage a pre-#114 `b2` could leave) is treated as stale
  and rebuilt from empty. Recreating just the missing tables would be worse than useless: an incremental
  reindex skips notes whose `body_hash` matches, so the recreated tables would stay empty and `S3`
  (`full-reindex ≡ incremental-update`) would quietly fail.

The vector tables (created at embed time, §4/M4) are the same drop-and-rebuild shape and get the same
treatment — two embed passes can genuinely overlap, since the `reindex` advisory lock
([#55](https://github.com/AlteredCraft/B2/issues/55)) is taken by `b2-cli` alone and never by the desktop
host or by readers.

**Why not an advisory lock file for this**, given B2 already has one for `reindex`? Because it would be a
*third* concurrency mechanism guarding state the database already knows how to guard, and the weaker one
where it counts: a vault on a network share or a synced folder — a plausible home for a personal vault —
is exactly where `flock` quietly stops meaning anything. The `reindex` lock answers a question SQLite
cannot (*is another **process** already doing this expensive work?*); schema atomicity is not that
question.

### Why materialize the graph at all — vs. resolving links at runtime

A note's *outbound* links (and their type + explanation) are parseable from that one file on demand, so
it's fair to ask why the index carries an `edges` table at all rather than resolving links at read time.
The answer separates two things the question tends to bundle. **Edge metadata is *not* the reason:** a
`b2_relations:` entry `"supports [[path|title]] — because X"` yields its verb and explanation to a
runtime parse just as well, *for that note's outbound edges*. **Inversion and composition are the reason.** Materializing
edges is what turns the following from full-vault scans (or impossibilities) into indexed lookups:

- **Backlinks / inversion.** "Who points at X" cannot be read from X — only from every *other* note.
  The runtime answer is O(vault) per query; the table makes it one lookup. This is also what services
  *"rename keeps every backlink resolving"* ([invariants.md](invariants.md) L1): the edges name
  the exact N inbound files to rewrite on a move instead of scanning the vault to find them (§8).
- **Typed multi-hop traversal.** "notes within 2 hops of X via `supports`/`contradicts`" is a scan *per hop*
  at runtime; over `edges` it is one SQL traversal.
- **The graph⨝vector join.** "semantic-nearest chunks whose note is within k typed hops of X" is a single
  join `embeddings ⨝ chunks ⨝ edges`, not expressible as a per-note parse. It is a **scoped-traversal**
  primitive (search *within* an already-related neighborhood). **`b2 similar`'s candidate generation is its
  *complement*, not this join:** notes semantically near an anchor but *not* within 1 hop — the links you
  *haven't* made (resolved 2026-07-01, §3) — where the materialized graph supplies
  the "∖ already-connected" exclusion. Both stand on the same reason the graph and search indexes must live
  in **one** store (§2): area-5 discovery is the substrate this enables.

The reframe that keeps this cheap: **runtime outbound-parsing is the correctness *definition*
(`index = projection of Markdown`); the `edges` table is its *cache*, kept so the inverse and compositional
queries are fast.** It is therefore not a third subsystem beside FTS5 and the vector tables — it is one more
**disposable** table in the same store, populated by the **same parse pass** that already walks each body
for chunking. Strip it and B2 is vector + keyword search over Markdown — i.e. qmd (§2); the typed,
traversable graph is the value-add, not the search. The standing cost of carrying it is the
`b2id`-under-`[[path]]` write-amplification budgeted in §8.

### Discovery surfacing is quality-gated — `limit` is a cap, not a promise

**(Ruled 2026-08-05; the findings of [PR #145](https://github.com/AlteredCraft/B2/pull/145).)**
Candidate *generation* stays recall-oriented: the two-stage scan (§4) over-produces, nothing
auto-links, and the human commits every edge — W4 untouched, because filtering what is *surfaced*
is not authoring. But the surfaced list owes the human candidates worth judging, not fullness:
**zero candidates is a legitimate — and honest — answer** when nothing in the vault genuinely
relates. `limit` bounds the list; it never obliges discovery to fill it (on a real vault ten notes
are always *nearest*, even when nearest means nearest-of-nothing — that list is a cost, not a
feature). What enforces the stance is a **quality floor** in `discover::candidates`, and its
shape is constrained on two sides:

- **Model-relative, never a bare constant.** The vectors are L2-normalized, so the engine's score
  maps exactly to cosine (`cos = 1 − d²/2`) and a floor is well-defined — but bge-family models
  compress cosines into a narrow high band another model won't share, and the fake embedder's
  hash-derived vectors are no band at all. The number keys alongside `meta.embed_model_id` (M2's
  identity, device tag included), with the fake regime handled explicitly.
- **Eval-calibrated, never intuition.** The labelled corpus carries **negative anchors** — loner
  notes whose labelled answer is "nothing relates" — scoring *suppression* (does discovery say
  so?), and the eval records every surfaced score into two **cosine piles**: human-labelled
  related vs. everything else surfaced. If the piles separate, the gap *is* the floor, read off
  measured data (an absolute cosine floor and a relative drop-off from the top candidate are both
  judged against the piles); a later model swap re-derives the number by re-running the eval. If
  the piles overlap too heavily for any floor to hold, the escalation is a **discovery-side
  pair-scorer** — a second model seam, sibling of §5's reranker but a *new* issue, not an
  extension of #28 (that seam needs query text and `similar` has none) — which still only filters
  what is surfaced, never authors a link.

Until the floor lands, `discover::candidates` truncates to `limit` with no floor and the eval's
suppression metric is red by design — the failing target the floor is built against.

FTS5 is built into SQLite (BM25 ranking included); vectors need no extension — plain tables scored
in-process ([#38](https://github.com/AlteredCraft/B2/issues/38)). Both are
battle-tested at personal-vault scale.

## 4. Semantic search & the engine-gated decision → verdict

The locked decision: *"if the index engine provides vector/semantic search, it's in v1."*

The engine provides it. Therefore **semantic search is in v1.**

How it runs — an **exact, in-process scan**, no vector extension, no ANN:

- **Storage & scoring:** vectors live in plain tables — `embeddings(chunk_id, vector)` plus per-note
  `note_centroids` — read with one sequential statement and scored in-process (`embed::l2_sq`). A
  `vec0`-style virtual table charges a per-row shadow-table probe on every scan, which dominates at
  real-vault scale; the plain-table scan does not. Full analysis + options:
  [#38](https://github.com/AlteredCraft/B2/issues/38).
- **Discovery is two-stage:** an O(notes) coarse scan over centroids shortlists candidates, then an
  exact max-sim rescore over only the shortlist's chunk vectors.
- **Candidate width is per view, and it is a quality knob, not plumbing.** Retrieval widens twice: each
  façade read asks for a hit pool over its `limit`, and each signal (`search::pool_size`) pulls 5× *that*
  before RRF fuses the two lists. The two reads need headroom for different reasons, so they get
  different pools ([#142](https://github.com/AlteredCraft/B2/issues/142)). Note-level `Vault::search`
  keeps **3×**: dedup collapses every chunk that shares a note onto that note's best one, so a pool of
  exactly `limit` would under-fill `limit` distinct notes on an ordinary query. Passage-level
  `Vault::search_chunks` has no dedup and keeps a **small constant** (`limit + 2`) — enough to backfill
  the one hit it can drop, a lookup that missed on the C1 torn read (§3), and no more. Giving it the 3×
  too is not a free tidy-up: the 5× multiplies every hit of headroom into candidates (150 per signal
  against 60, at a 10-result ask), and RRF over a wider candidate set returns *different* answers — at
  k = 60 a chunk ranked ~60th in **both** lists outscores one ranked first in a single list
  (`2/121 > 1/61`). Width therefore moves only on measured relevance; the labelled corpus cannot measure
  it yet, so the conservative setting holds — see §5.
- **A fused-score tie is broken by the dense signal, and that is a policy, not a detail.** RRF over
  integer ranks lands every fused score on a discrete lattice, so bit-identical ties between mirrored
  rank pairs — (1, 3) vs (3, 1) — are structural, and the eval corpus produced one
  ([#156](https://github.com/AlteredCraft/B2/issues/156), the runlog's worked case: the semantic half
  named the labelled answer, BM25 named the wrong one). The secondary sort key in `rrf_fuse` is
  therefore the candidate's rank in the **vector list** (absent ranks below present), with id last
  purely for determinism — a photo finish is decided on the signal measured to be right there, never
  on projection walk order.
- **The FTS tokenizer is `porter unicode61` — stemmed, and that is a *measured verdict*, not a
  default (schema v5).** Unstemmed BM25 matched surface forms only — `pedalling` found nothing in a
  note that says "pedals", leaving the lexical half at rank 41–46 on queries the dense half ranked
  first, which RRF's consensus bias turned into hybrid demotions of correct dense hits. The A/B
  that settled it ([#157](https://github.com/AlteredCraft/B2/issues/157);
  docs/evals/runlog.md 2026-08-11): porter improved 7 BM25-only note ranks and 3 hybrid note ranks,
  worsened none, and dissolved every standing fusion demotion (hybrid rejoined vector-only at 0.98
  hit@1 / 0.988 MRR on the eval corpus) — while the precision probes built to vote *against*
  stemming (the `universe`/`university` Porter-collision pair, the code-literal `git-cheatsheet.md`
  queries) did not move. Stemming remains a real trade — Porter is English-only, and a vault holds
  code, identifiers, and proper nouns — so the retired arm stays measurable: `Vault::rebuild_fts`
  swaps `chunks_fts`'s tokenizer over identical chunk rows and vectors (nothing re-chunks or
  re-embeds), and `just eval-stemmer` scores the unstemmed ablation beside every default run. The
  fusion interaction is [#158](https://github.com/AlteredCraft/B2/issues/158), where the stemmed
  input is what emptied the demotions line.
- **Does brute force scale to B2?** Yes, comfortably. A personal vault of, say, 10k notes → ~50–100k
  chunks. Brute-force cosine over ~100k × 768-dim float32 vectors is on the order of **single-digit to
  low-tens of milliseconds** — well within an interactive budget. We're nowhere near the regime
  where ANN matters; if a vault ever is (multi-hundred-k chunks), int8/binary quantization and ANN
  hold a standby order behind the centroid stage.

So: **semantic search ships in v1**, exact KNN, 768-dim float vectors, with quantization in our
back pocket. This is the headline consequence of choosing SQLite.

## 5. The reranker as a fast follow

Slot it exactly where qmd puts it: **after RRF fusion, before final ranking**, behind a swappable seam.

- v1: retrieve (BM25 + vector) → **RRF fusion** → return top-N. This is already good; RRF alone is a
  strong hybrid baseline.
- Fast follow: insert a **cross-encoder rerank** over the top ~30 candidates → position-aware blend.
  The reranker is a pure function `(query, candidates) → scores`; the seam is the same one the
  testability stack wants for AI parts (replay recorded scores as fixtures; quality measured in a
  separate eval suite, never in CI).
- This is why the reranker is genuinely deferrable with no architectural debt: it changes *ordering*,
  not the store, the schema, or the candidate set. "Eventually add a reranker" is a one-stage insertion,
  not a redesign. It is also **store-agnostic** — a model-side seam above the index, not a property of it,
  so no vector-store choice simplifies or blocks it ([#67](https://github.com/AlteredCraft/B2/issues/67)).

**Scope — this reranks `b2 search`, not `b2 similar`.** The seam signature `(query, candidates) → scores`
is the tell: it needs *query text*, so it reorders **query search** (`b2 search`). **`b2 similar` has no
query** — it is passage↔passage KNN, "near ∖ connected" (§3) — so this reranker
does **not** apply to it; the discovery-side ranking levers are the qmd chunker upgrade
([#19](https://github.com/AlteredCraft/B2/issues/19)) and distance-weighting
([#20](https://github.com/AlteredCraft/B2/issues/20)), not this — and the discovery-side
*precision* lever is §3's quality floor: #20 reorders candidates, it cannot make a bad list
shorter.

**Gate the decision on the eval, not intuition** (the eval harness under `crates/b2-embed/evals/`). RRF
is a strong baseline; the reranker buys **top-k precision**, whose value *grows with vault size* (semantic
near-misses crowd the top past ~1k notes) and is *highest when an agent consumes top-1/top-3 without a
human eye* (the `serve` adapter, [#24](https://github.com/AlteredCraft/B2/issues/24)). Vault size changes
whether the precision is *worth* it, not the reranker's cost — that is fixed at the top-N it rescores. So
measure RRF precision@k / MRR on a representative set first and ship the reranker only on a measured gap;
this is the deferral §5 is built to allow. Tracked in [#28](https://github.com/AlteredCraft/B2/issues/28).

**…and check the instrument can see the change you are gating.** The labelled eval corpus is *no bigger
than the candidate pool retrieval reaches* — 29 chunks against `vault::chunk_candidate_pool(10) = 60` per
signal, and 150 for the note view — so neither half of the hybrid is truncated there: BM25 returns every
matching chunk, the vector scan every stored vector, and widening the pool cannot add a candidate.
Relevance scores on that corpus are therefore **invariant under candidate width**: a change to either
façade hit pool or to `search::pool_size` prints "no change" while genuinely reordering a real vault.
Score *relevance* on the labelled corpus, but measure a width change with the rank-stability probe over a
vault big enough for the pool to bind (`--example stability`, `fixtures/test-vault`); the eval prints its
own blindness when the corpus fits inside the narrower of the two pools
([#141](https://github.com/AlteredCraft/B2/issues/141)).

That gate has already ruled once. [#140](https://github.com/AlteredCraft/B2/pull/140) widened the passage
view to 3× as plumbing; the eval printed bit-identical numbers, the probe found 10 of 10 top-4 passage
lists changed, and with no labelled evidence that the wider set ranked *better*,
[#142](https://github.com/AlteredCraft/B2/issues/142) returned it to the constant (§4). The instrument
that can say "different" is not the one that can say "better" — shipping a width change needs both.

The blindness is to *candidates*, not to fusion. `RRF_K` re-weights the **same** two lists
(`Σ 1/(k+rank+1)`), so it reorders results on a corpus of any size — measured on this one: k = 60 → 10
moves note ranks across the query set. A k change is gated on the labelled eval like any other quality
change; only candidate width needs the larger vault.

Query expansion (qmd's third model, the fine-tuned 1.7B) is **optional and lowest priority** — it's the
heaviest model for the smallest, most variable win. Treat it as a later, off-by-default flag.

## 6. The real hard part: embeddings in a single binary

This is the only place the architecture meets real friction, and it's worth being honest that **it is
independent of the SQLite decision** — any engine that does semantic search needs vectors from
somewhere.

qmd's answer is `node-llama-cpp` + auto-downloaded GGUF models + Node 22/Bun, needing ~300 MB–3 GB of
model files and a JS runtime. That directly tensions B2's single-binary, no-install-ritual goal
([invariants.md](invariants.md)). Options, roughly in order of single-binary
friendliness:

1. **Bundle a small embedding model + a `llama.cpp`/GGUF runtime, statically linked.** Self-contained,
   fully offline, but the binary carries a few-hundred-MB model (or downloads it on first run — a
   one-time ritual, not a per-use one). EmbeddingGemma-300M or Qwen3-Embedding-0.6B are the candidates.
2. **`fastembed` / ONNX Runtime** with a small embedding model — mature, embeddable, good language
   bindings (Rust/Go/Python); similar size tradeoff, arguably cleaner than carrying a full LLM runtime
   just to embed.
3. **Pluggable embedder behind a seam, default local + optional remote API.** B2 already wants the
   embedder swappable (deterministic fake for tests). Ship local-by-default; allow an API embedder for
   users who opt in. Keeps the binary tiny; preserves local-first as the default.

**Recommendation:** make the **embedder a seam** (we need it for tests regardless — and a swappable
model seam *is* the **"build for tomorrow's model"** tenet in practice,
[invariants.md](invariants.md)), ship a **local model as the default**
(option 1 or 2), and decide model-download-on-first-run vs. bundled-in-binary as a packaging detail
later. Crucially, **none of this blocks the engine work**: build the SQLite store +
FTS5 + the vector tables + the typed graph now against the deterministic fake embedder; drop the real
embedder into the seam when the packaging path is chosen.

**Decided (2026-06-30).** Runtime = **`candle` + `hf-hub`** (pure-Rust inference compiled into the
binary — no external ONNX Runtime to ship; `hf-hub` is the download seam). Model =
**EmbeddingGemma-300M @ dim 768** (fallback to a known-good candle embedding model if it proves fiddly).
**Not bundled** — an explicit **`b2 init`** downloads + verifies the model into a shared **XDG cache**
(`~/.local/share/b2/models/`), never a surprise mid-command download; `reindex`/`search` fail fast with
"run `b2 init`" if it's absent. **The model source is configurable** (default = an HF repo id;
overridable to a mirror, another repo, or a local path for offline installs) via a global TOML at
`$XDG_CONFIG_HOME/b2/config.toml`. Build/execution plan tracked in [GitHub Issues](https://github.com/AlteredCraft/B2/issues).

**Built (2026-07-01).** Shipped in the **`b2-embed`** crate (`LocalEmbedder` behind the `b2-core`
`Embedder` seam; candle + `hf-hub`; CLS-pool + L2-normalize; asymmetric query prefix). **Model default
changed to `BAAI/bge-base-en-v1.5`** (BERT-family, **768-dim**, ungated): EmbeddingGemma-300M is *gated*
on Hugging Face (HTTP 401 without a token + license acceptance), which defeats a friction-free `b2 init`
— so B2 ships the pre-authorized bge fallback by default, validated in the spike (cat↔feline 0.83; NL
queries retrieve by meaning, not keyword). EmbeddingGemma remains selectable via config for anyone who
provides a token. The dim is read authoritatively from the model's own `config.json` (`hidden_size`), so
config can't lie about it. `open()` no longer shapes/drops the vector space (the mismatch fails fast on
`search`, re-embeds on `reindex`); the fake embedder stays the CI default so model quality never enters
the fast suite. Eval is a `cargo run -p b2-embed --example eval` pass (precision/MRR), out of CI.

## 7. Tech-stack implications — resolved: Rust

- **SQLite + FTS5 are language-agnostic** (strong embedded bindings for Rust, Go, Python, Node), so
  the engine didn't pick the language; the **single-binary goal** did — it favours a compiled
  language, and B2 is a **Rust Cargo workspace** (`rusqlite` with bundled SQLite; `candle` for the
  embedder).
- qmd's TypeScript/Node path is the *least* aligned with principle #5, which is another reason not to
  inherit qmd's runtime by depending on it.

## 8. Risks, open questions & operational burden

### Engine risks & open questions

- **Embedding model size vs. single binary** (§6) — **resolved:** not bundled; an explicit
  `b2 init` downloads a configurable model (candle + hf-hub) into a shared XDG cache. The binary stays small.
- **Embedding dimension & model lock-in.** Changing the embed model means re-embedding the whole vault.
  **Locked:** the embedding space is **dim 768** — the default **`BAAI/bge-base-en-v1.5`** and the
  config-selectable EmbeddingGemma-300M are both 768-dim (§6); a model/dim change is a
  full re-embed, detected via `meta` — fail fast on read, re-embed on `reindex`.
- **Chunk vs. note granularity for the graph.** Search is chunk-level; the typed graph is note-level.
  Keep `chunks.note_b2id` as the join and resolve search hits up to notes for graph operations — already
  reflected in §3.

### Operational burden — the bill for a `b2id`-keyed graph under `[[path|title]]` links

The graph buys B2 its reason to exist (typed, `b2id`-stable edges — §2), but the decision to
keep links written as human-clickable `[[path|title]]` while the graph keys on `b2id`
([data-model.md](data-model.md) §9) has standing operational costs. These are
*the trade working as designed*, not defects — but they must be budgeted, tested, and watched.

- **Write amplification on move.** The inline `path` is a repairable convenience copy, so moving one note
  rewrites the inbound link text in **every** file that points at it — an N-file write, not a one-file
  write. It's bounded and mechanical (the `b2id`-keyed edges name exactly which files/links to touch,
  Markdown-first then index), but moving a heavily-linked note is proportional to its backlink count, not
  O(1). Watch the cost on hub notes; keep the rewrite transactional so a partial move never half-updates
  the vault.
- **Out-of-band moves degrade gracefully, not perfectly.** A `git mv`/Finder move + reindex re-reads the
  frontmatter `b2id` and re-establishes `b2id → newpath`, repairing dangling inbound links — **if** there is
  prior index continuity. A **cold reindex with no prior state** can only repair a dangling `[[oldpath]]`
  heuristically (e.g. via the alias); those links are **flagged for repair, not silently dropped**. This
  is the same failure surface as moving files with Obsidian closed — acceptable, but it means the index is
  load-bearing for full repair fidelity.
- **Path ownership follows the Markdown, and a reindex never aborts on it.** `notes.path` is unique, but a
  path can change hands out of band — a note deleted then recreated at the same path, or files renamed/
  swapped outside `b2 mv` so a path now belongs to a different `b2id`. Projection reconciles to the current
  truth: `db::upsert_note` drops the **stale** row that still holds the path (its chunks/edges cascade)
  before writing the new owner, so an incremental reindex converges on the same state as a from-scratch
  rebuild (`full-reindex ≡ incremental-update`) instead of failing on a raw `UNIQUE(notes.path)` error.
  A note file *deleted with no replacement* is reconciled by the whole-vault projection pass
  ([#31](https://github.com/AlteredCraft/B2/issues/31)): `project_vault` prunes every `notes` row whose
  `b2id` it did not project this run (`db::prune_notes_except` — chunks/FTS/vectors/outgoing edges
  cascade; inbound links re-dangle when phase 2 re-derives edges against the pruned resolver), **except**
  rows whose file was skipped as unreadable — the walk saw that file, its `b2id` is merely unknowable this
  run, so evicting it would lie. Single-note ingest (`add`/`mv`/`write`) touches one note and never
  prunes. *(Resources churn more than notes — images/PDFs get added and deleted freely — and their
  inventory pass prunes the same way; [#66](https://github.com/AlteredCraft/B2/issues/66).)*
- **Duplicate `b2id`s resolve incumbent-wins and are surfaced, never guessed at**
  ([#81](https://github.com/AlteredCraft/B2/issues/81)). Two files presenting one id (a Finder
  duplicate) has no well-defined projection, and no vault-pure signal distinguishes original from
  copy — a copy preserves every byte, and `note copy.md` even *sorts before* `note.md`, so any
  walk-order or path-order rule would hand the identity to the copy. The projection pass therefore
  pre-scans claims and resolves each contested id **before any row is written**: the **incumbent**
  (the path the index already attributes the id to, when that file still claims it) keeps the
  identity — the one confident signal, and it lives in index memory, which is why this is a
  documented carve-out on S3 ([invariants.md](invariants.md)); a memory-less rebuild tie-breaks
  first-in-sorted-walk, *flagged as a tie-break, not a ruling*. Shadowed claimants stay on disk
  but get **no row** — and therefore no presence in the file tree either, which lists notes from
  the index (`list_notes`); only folders are read live off the filesystem. That is what makes the
  notice the *only* way a human learns the copy exists, and why the desktop's review panel offers
  a shadowed path for copying rather than a reveal that could not resolve
  ([#88](https://github.com/AlteredCraft/B2/issues/88)). The collision is reported (`collisions` on
  the reindex/project reports, with kept path + precedence + shadowed paths) on **every pass until
  resolved**: delete the copy; or remove its `b2id:` line (the next pass stamps a fresh identity —
  the documented fork gesture); or delete the original (the copy then inherits the identity through
  the ordinary move repointing). Single-note ingest through `ingest_file` (the `add`/`mv`/`link`
  path) **refuses** an identity steal outright (`Error::B2idCollision`) when the incumbent still
  exists and still claims the id; `project_file` (the in-app save path — `write`/`create_note`)
  deliberately does not guard, since refusing after the bytes hit disk would strand the index
  stale — the whole-vault pass owns reconciling and surfacing that state. The same pass
  also surfaces **identity restamps** (`restamped`): a file whose `b2id:` line was removed/blanked
  out-of-band reads as absent (#75), so the stamp mints a fresh id — identity churn that dangles
  every inbound edge keyed to the old id (G5 keeps surfacing those). Deliberately **not**
  auto-restored: removing the line is also the documented gesture for *requesting* a fresh identity,
  so restoring the old id would guess intent. Nothing anomaly-shaped is ever stored — every notice
  re-derives from vault + index each pass (S2), and `reindex --dry-run` previews all of it read-only
  (which notes lack a `b2id`, which stamps would churn an identity, which files collide).
- **A single unreadable file never fails the whole index.** A real vault holds the odd non-UTF-8 or
  permission-denied `.md`; projection **skips** it (reported as a `skipped` entry carrying a short,
  file-level reason, surfaced by the CLI and the desktop) and indexes everything else, rather than aborting
  the reindex on one file it cannot read.
- **Derived-index consistency is a permanent invariant, not a one-time build.** The index is a derived
  projection of `Markdown` and must never drift from it. Three locked invariants are the tripwires
  (the full register: [invariants.md](invariants.md)):
  round-trip losslessness (`parse → serialize → parse`),
  `full-reindex ≡ incremental-update`, and `rename keeps every backlink resolving`. Every edit path
  (kernel `b2 mv`, link delete, out-of-band reindex) has to preserve all three or the graph silently
  diverges from the source of truth.
- **Committed edges are only ever authored, never inferred.** B2 writes an edge only on your command
  (`b2 link`, or a body link you write) — there is no agent proposing edges and no review queue to keep
  consistent. Editing the vault can strand a connection — e.g. deleting an authored `A→B` link
  ([invariants.md](invariants.md) W4) — but B2 only ever *surfaces* the consequence (an orphan
  flag in `b2 explain`), never silently rewrites an inbound file or an edge. Files are touched only when asked.

## 9. Recommendation

1. **SQLite is the B2 index engine** (FTS5 + plain vector tables) per the §3 schema — one disposable
   index, `index = projection of (the vault directory)`. qmd is a design reference under its MIT
   license, **not** a dependency.
2. **The engine-gated outcome:** semantic search is **in v1** (exact in-process KNN; quantization
   reserved for scale).
3. **Reranker = explicit fast-follow** behind a post-fusion seam; query expansion = later/optional.
4. **The embedder is a seam**; the store + indexes + typed graph are built and tested against the
   **deterministic fake embedder**, with the golden-vault fixtures as the yardstick
   ([data-model.md](data-model.md) §8).

> Net: qmd answers "can a great hybrid search engine run locally on Markdown?" — yes, and here's how.
> B2's question is one layer up: "can that retrieval live inside a typed, `b2id`-stable, agent-operated
> graph I fully own, in a single binary?" SQLite is the substrate that makes every queryable concern one
> disposable store, a pure projection of your Markdown. We take qmd's pipeline and build the graph it was
> never trying to be.
