---
title: "B2 — Index Engine"
type: note
tags: [b2, index-engine, sqlite, fts5, vectors, search, discovery, chat, architecture]
created: 2026-06-29
status: active
---

# B2 — Index Engine

> **The engine design — the *how*.** Specifies the disposable SQLite index (FTS5 + an in-process
> vector scan + the typed graph) and the flows over it. Companion docs: [invariants.md](invariants.md)
> (the normative *why*, cited by id) and [data-model.md](data-model.md) (the *what*).
>
> **Rationale lives in [`ADRs/`](../../ADRs/README.md)** — why SQLite and not qmd (ADR-0019), why
> plain vector tables (ADR-0006), why the graph is materialized (ADR-0010), why discovery always
> serves (ADR-0014), why a served result claims evidence (ADR-0015), why candle (ADR-0020). This
> doc specifies what the engine *is*; each section names the record that decided it.

## 1. qmd, the reference — and what we took

[qmd](https://github.com/tobi/qmd) is a local CLI search engine for Markdown, all on-device: SQLite +
FTS5 + `sqlite-vec`; ~900-token chunks with ~15% overlap and Markdown-aware break-point scoring;
three search modes; a pipeline of LLM query expansion → parallel retrieval → **RRF fusion**
(`Σ 1/(k+rank+1)`, k=60) → cross-encoder rerank → position-aware blend; local GGUF models via
`node-llama-cpp`; TypeScript on Node, MIT.

**We rebuilt rather than depended — ADR-0019.** Taken wholesale: the chunking heuristic, **the RRF
formula and `RRF_K = 60`**, the position-aware blend, the asymmetric query/document prompt discipline
(each model brings its own prefix — B2 ships bge's, §6), and the JSON/`--explain` agent-output
discipline. Discarded: the npm/Node packaging and the "DB is the product" framing.

**Chunking as adapted** (`chunk.rs`, [GH #19](https://github.com/AlteredCraft/B2/issues/19)) — four
model-free changes to qmd's heuristic: a **450**-token target (headroom under bge's 512-token
truncation), a `chars/4` proxy for token sizing (the core stays tokenizer-free, E1), an unconditional
stored `heading_path` breadcrumb, and every lever on a `ChunkConfig` (overlap 0.15, backscan 200, the
H1=100 … list=5 break weights). A forced cut is pushed past a fenced code block or Markdown table
rather than bisecting it ([GH #41](https://github.com/AlteredCraft/B2/issues/41)). Tree-sitter AST
chunking for code stays deferred ([GH #104](https://github.com/AlteredCraft/B2/issues/104)).

## 2. Why we rebuild instead of depend on qmd

Recorded in full as **[ADR-0019](../../ADRs/0019-build-our-own-sqlite-index-engine.md)**. In one
line: qmd is a search engine and B2 is a typed graph with hybrid retrieval over it — qmd models no
typed edges, no backlinks, and nothing that repairs links on a move (G1–G6, L1), and SQLite holds all
three queryable concerns in one transactional store.

## 3. The storage architecture (one disposable SQLite index)

One artifact, per S1/S2 and ADR-0002: a **disposable** SQLite index holding every queryable concern
transactionally, rebuildable from the vault at any time (S3).

> The precise DDL and build order are realized in `crates/b2-core/src/db.rs` (schema) and `ingest.rs`
> (flows). The sketch below is the orientation; the code is the buildable contract.

```
b2.sqlite — DISPOSABLE CACHE  (= projection of the vault directory; drop & rebuild any time)
├── MIRROR OF THE VAULT (lets us diff vs. disk)
│   ├── meta(key, value)                          -- schema_version, embed_model_id, embed_dim
│   ├── notes(path PK, type, title, description,  -- the path IS the identity (L1)
│   │         created, updated, body_hash, mtime, indexed_at)
│   ├── note_aliases(note_path, alias)            -- frontmatter `aliases:`
│   └── resources(path PK, class, size, mtime,    -- non-.md peers; class by extension (§10 dm)
│                 content_hash, indexed_at)
│
├── DERIVED: SEARCH
│   ├── chunks(id, note_path, seq, char_start, char_end, token_count, heading_path, text, text_hash)
│   ├── chunks_fts                                -- FTS5 over chunk text, `porter unicode61` (BM25)
│   ├── embeddings(text_hash PK, vector)          -- CONTENT-ADDRESSED plain BLOB vectors (768-dim)
│   └── note_centroids(note_path, centroid)       -- per-note centroid (discovery's coarse stage)
│
└── DERIVED: TYPED GRAPH
    └── edges(id PK, src_path, dst_path, dst_resource_path, dst_path_raw,
              type, origin, explanation, caption, embed, occurrence_index)
```

Every table is derived from the vault; there is no third home. The two **vector** tables are created
at *embed* time rather than in the base migration — their existence *is* the "this vault has an
embedding space" signal the BM25-only fallbacks key on (M4, ADR-0006).

*(The projection runs as two separately-invokable passes — model-free `project` (notes/resources/
chunks/FTS/edges) then `embed` (vectors), with `reindex` their composition — so keyword search and the
graph are usable before embedding completes
([GH #15](https://github.com/AlteredCraft/B2/issues/15)). S2 is untouched: a projected-but-unembedded
index is a smaller projection, never a wrong one.)*

Why this shape fits B2 specifically:

- **Everything keys on the vault-relative path** (L1, ADR-0003). `notes.path` is the primary key;
  `chunks.note_path`, `note_aliases.note_path`, `note_centroids.note_path` and `edges.src_path` are
  `REFERENCES notes(path) ON DELETE CASCADE ON UPDATE CASCADE`, which makes a B2-performed move a
  **path re-key rather than a rebuild**: `UPDATE notes SET path = …` cascades through every child in
  one transaction, alongside the inbound link-text rewrite and a re-projection of the inbound sources.
  `edges.dst_path` is deliberately *not* an FK — it must be allowed to be NULL, the dangling case
  (G5). "Rename keeps every backlink resolving" is therefore a property of the **move operation**, not
  of the key; the price is that a move made *outside* B2 is a delete plus a create (§8).
- **Vectors are content-addressed** (M4, ADR-0006), keyed by blake3 of the chunk text — which *is* the
  embed input, verbatim — so a note that moves re-embeds nothing, identical text anywhere shares one
  vector, and the only invalidation rule is "a hash no chunk references is garbage", pruned by the
  whole-vault pass on the same derived-data lifecycle as centroids.
- **Every `edges` row derives from Markdown** (G1, G2, ADR-0010), deduped frontmatter-wins. There is
  **no `status` column and no suggestion queue**: `b2 link` appends a typed-link string to the source
  note's frontmatter and re-projects that note — the projection of an authored line, not an in-place
  index write.
- **Hybrid retrieval and graph queries compose in one query** — "semantic-nearest chunks whose note is
  within 2 typed hops of note X" is a join across `embeddings`, `chunks`, and `edges`. This is the
  substrate `b2 similar` runs on.
- **Deterministic seams for tests** — a fake embedder writes to `embeddings`, so the whole pipeline is
  assertable with no live model (E2).

**Resources widen the projection without disturbing any statement above** — path-keyed peers, so
`index = projection of (the vault directory)`. `resources` is a **separate** table from `notes`, not a
`kind` column on it (two tables, two contracts, zero "unless it's a resource" clauses), and
`edges.dst_resource_path` lets a body `![[photo.png]]` or `[[papers/x.pdf]]` resolve against it while
`src` stays a note (G6). What is built is inventory + graph; resource *content* search is designed,
not shipped — [data-model.md](data-model.md) §10. Either way there is **no migration**: a schema
change is a `schema_version` bump + rebuild (S5).

### Opening the index concurrently — many readers, one builder

Several things open one index at once: `b2 reindex &` racing a `b2 status`, the desktop app launching
while a CLI reindex runs, the desktop host's own threads. The locked stance is **C1**, enforced in
`db::open` in three layers that each answer a different failure:

- **The `WAL` flip is retried** ([GH #111](https://github.com/AlteredCraft/B2/issues/111)). Setting
  `journal_mode = WAL` is the one statement in `open` that takes a write lock and the one
  `busy_timeout` cannot cover — SQLite skips the busy handler for a write lock upgraded from an
  already-open read transaction — so a second opener took an immediate `SQLITE_BUSY`. The wait is ours.
- **The schema migration is one `BEGIN IMMEDIATE` transaction, entered only when there is work**
  ([GH #114](https://github.com/AlteredCraft/B2/issues/114)). Unserialized, two openers that both read
  a stale `schema_version` interleave — one's `DROP TABLE` landing after the other's `CREATE`, and the
  current version stamped over a half-demolished schema. `busy_timeout` was irrelevant: nothing
  contended, every statement succeeded, in the wrong order. The check that decides whether to enter is
  a **read**, so the common open against a current schema takes no write lock and can never be refused.
- **Completeness is checked, not assumed.** A stamp is believed only alongside the tables it vouches
  for; a current stamp over missing tables is stale and rebuilt from empty. Recreating just the missing
  tables would be worse than useless — an incremental reindex skips notes whose `body_hash` matches, so
  they would stay empty and S3 would quietly fail.

The vector tables are the same drop-and-rebuild shape and get the same treatment. **Not an advisory
lock file:** that would be a third concurrency mechanism guarding state the database already guards,
and the weaker one where it counts — on a network share or synced folder, `flock` quietly stops
meaning anything. The `reindex` lock ([GH #55](https://github.com/AlteredCraft/B2/issues/55)) answers
a question SQLite cannot (*is another **process** already doing this expensive work?*); it is taken by
`b2-cli` alone, never by the desktop host or by readers, and so cannot cover schema atomicity.

### Why materialize the graph at all — vs. resolving links at runtime

A note's *outbound* links are parseable from that one file on demand, so edge metadata is not the
reason. **Inversion and composition are** — materializing turns three things from full-vault scans
(or impossibilities) into indexed lookups:

- **Backlinks / inversion.** "Who points at X" cannot be read from X, only from every *other* note:
  O(vault) per query at runtime, one lookup here. This is also what services L1 — the edges name the
  exact N inbound files to rewrite on a move instead of scanning the vault to find them (§8).
- **Typed multi-hop traversal.** "notes within 2 hops of X via `supports`" is a scan *per hop* at
  runtime; over `edges` it is one SQL traversal.
- **The graph⨝vector join.** "semantic-nearest chunks whose note is within k typed hops of X" is a
  single join `embeddings ⨝ chunks ⨝ edges`, not expressible as a per-note parse — a **scoped
  traversal** primitive. **`b2 similar`'s candidate generation is its complement**: notes semantically
  near an anchor but *not* within 1 hop, where the materialized graph supplies the exclusion.

**G6** keeps this cheap: runtime outbound-parsing is the correctness *definition*, the `edges` table
its *cache* — one more disposable table in the same store, populated by the same parse pass that
already walks each body for chunking. The standing cost is the move-repair amplification in §8.

### Discovery surfacing serves the ranked list — `limit` is a cap, not a promise

The standing rule is **invariants.md D1**; the decision, the seven issues of measurement behind it,
and the retired z-gate are **[ADR-0014](../../ADRs/0014-discovery-always-serves-the-ranked-prefix.md)**.
Mechanically, in this engine:

- `similar` serves the ranked top-N whenever candidates exist. `limit` under-fills only for want of
  scorable notes, and **no statistical bar truncates the list**.
- The per-candidate **z survives as a statistic that gates nothing** — computed after stage 2 on the
  best-passage distances, non-increasing down the row order (strictly monotonic in the *score*; tied
  scores share a z and order by the path tie-break), painted as the within-list strength band. Because
  the judged z is affine in the squared best-pair distance the score negates the root of, **score
  order, z order, and band are one number**: a card can never show a weaker band above a stronger one.
- Below `STATS_MIN_POPULATION` (12), in a space with no spread, or under the fake embedder, **no z
  exists at all**, and a surface must *say* the list is ungraded — silence there reads as "all judged,
  all scored low", the opposite of what happened.
- Candidate *generation* is unchanged and stays recall-oriented: the two-stage scan (§4) over-produces,
  nothing auto-links, and the human commits every edge (W4).

The **pair-scorer escalation** stays the named long-term seam: an anchor-local rule cannot catch a
*pair-level* miscalibration (a single stranger the model scores like a cluster-mate — the standing
`encryption ↔ phishing` residue), and a discovery-side pair-scorer would be a second model seam,
sibling of §5's reranker but distinct from it (that seam needs query text and `similar` has none). It
would still only filter what is surfaced, never author a link. Under always-serve that residue is
**ordering quality, not existence**.

## 4. Retrieval — semantic search, fusion, and discovery

Semantic search is **in v1** (ADR-0019) — exact, in-process, no vector extension, no ANN.

- **Storage & scoring.** Vectors live in plain tables — `embeddings(text_hash, vector)` plus per-note
  `note_centroids` — read with one statement and scored in-process (`embed::l2_sq`).
  Content-addressing costs the scan one indexed join back to `chunks` to recover which chunk each
  vector ranks for. A `vec0`-style virtual table charges a per-row shadow-table probe on every scan,
  which dominates at real-vault scale; the plain-table scan does not
  ([GH #38](https://github.com/AlteredCraft/B2/issues/38), ADR-0006).
- **Flow ② hybrid search** (`search.rs`). BM25 over `chunks_fts` ⊕ vector KNN, fused with Reciprocal
  Rank Fusion (`Σ 1/(k+rank+1)`, `RRF_K = 60`), resolved from chunks up to notes. Raw NL queries are
  sanitized into a safe FTS5 `MATCH` expression — punctuation is FTS5 syntax and would otherwise crash
  the parse. On a projected-but-unembedded vault the vector half is simply absent and the same fusion
  runs over the single BM25 list, so scores stay on one scale.
- **The flow can answer zero, and the evidence rides beside the order** (invariants.md **D2**;
  decision and history in
  **[ADR-0015](../../ADRs/0015-a-served-search-result-is-a-claim-of-evidence.md)**). `hybrid_search`
  returns a `Retrieval`: the same fused order (untouched — provenance is carried, never folded in),
  each hit naming the lists that ranked it and its own distance, plus a query-level `QueryEvidence`.
  The rule over it is *lexical OR semantic*:
  - **The lexical anchor is IDF-weighted coverage, not presence.** `fts5_query` ORs every term, and
    the labelled phrase negatives match 68 of 70 chunks through `a`/`to`/`my` alone — one reading a
    *better* best-BM25 than several positives off nothing but function words. So neither a hit count
    nor a raw BM25 score is a lexical test. What is: each term weighs `ln((chunks+1)/(df+1))`, and the
    anchor is the share of the query's total weight the vault carries (`min_term_coverage`). Stopwords
    are a *measurement*, never a shipped word-list.
  - **The cosine bar is the backstop** (`min_cos`), judged only on queries the lexical half leaves
    undecided, and thin by construction: ask the lexical half for more and the window collapses,
    because the queries it then has to rescue are the ones with the weakest semantic evidence too. The
    two constants are placed inside a **joint** band, never tuned one at a time.
  - The constants are **distributional**, so they are keyed to `embed_model_id` (M2 — a swap
    invalidates them; the device suffix shares the reading and `just eval-metal` is where that
    assumption is re-checked). They live in `search::BGE_BASE_EVIDENCE_BAR` and their *justification*
    is re-derived on every `just eval` run — never frozen into a comment (ADR-0013).
  - The verdict reaches an adapter through `Vault::search_evidence`, which serves **exactly** the rows
    `search` does, in the same order — three-state, and the three states are three behaviours
    (ADR-0015). **The tail** — folding where a *real* query's per-hit evidence runs out — is unshipped:
    it needs per-hit labels the corpus does not carry, so provenance is measured and reported
    (`dense_only`) and no rule is drawn from it yet.
- **Flow ③ discovery is two-stage** (`discover.rs`). An O(notes) coarse scan over centroids shortlists
  candidates (`SHORTLIST_PER_RESULT = 20` per asked result, floored at `SHORTLIST_MIN = 200`), then an
  exact max-sim rescore over only the shortlist's chunk vectors, minus the anchor's 1-hop graph
  neighbors. No model call at surface time — `b2 similar` surfaces, `b2 link` commits, and the human
  is the precision gate (ADR-0009).
- **Candidate width is per view, and it is a quality knob, not plumbing.** Retrieval widens twice:
  each façade read asks for a hit pool over its `limit`, and each signal (`search::pool_size`) pulls
  5× *that* (minimum 30) before RRF fuses the two lists. The two reads need headroom for different
  reasons, so they get different pools
  ([GH #142](https://github.com/AlteredCraft/B2/issues/142)). Note-level `Vault::search` keeps **3×**:
  dedup collapses every chunk sharing a note onto that note's best one, so a pool of exactly `limit`
  would under-fill `limit` distinct notes. Passage-level `Vault::search_chunks` has no dedup and keeps
  a **small constant** (`limit + 2`) — enough to backfill the one hit it can drop to a C1 torn read,
  and no more. Giving it the 3× too is not a free tidy-up: the 5× multiplies every hit of headroom
  into candidates (150 per signal against 60, at a 10-result ask), and RRF over a wider candidate set
  returns *different* answers — at k = 60 a chunk ranked ~60th in **both** lists outscores one ranked
  first in a single list (`2/121 > 1/61`). Width moves only on measured relevance (§5).
- **A fused-score tie is broken by the dense signal, and that is a policy.** RRF over integer ranks
  lands every fused score on a discrete lattice, so bit-identical ties between mirrored rank pairs —
  (1, 3) vs (3, 1) — are structural, and the eval corpus produced one
  ([GH #156](https://github.com/AlteredCraft/B2/issues/156): the semantic half named the labelled
  answer, BM25 named the wrong one). The secondary sort key in `rrf_fuse` is therefore the candidate's
  rank in the **vector list** (absent ranks below present), with id last purely for determinism — a
  photo finish is decided on the signal measured to be right there, never on projection walk order.
- **The FTS tokenizer is `porter unicode61` — stemmed, and that is a *measured verdict*, not a
  default.** Unstemmed BM25 matched surface forms only — `pedalling` found nothing in a note that says
  "pedals", leaving the lexical half at rank 41–46 on queries the dense half ranked first, which RRF's
  consensus bias turned into hybrid demotions of correct dense hits. The A/B that settled it
  ([GH #157](https://github.com/AlteredCraft/B2/issues/157), 2026-08-11): porter improved 7 BM25-only
  and 3 hybrid note ranks, worsened none, and dissolved every standing fusion demotion — while the
  precision probes built to vote *against* stemming (the `universe`/`university` Porter collision, the
  code-literal queries) did not move. Stemming remains a real trade — Porter is English-only, and a
  vault holds code, identifiers, and proper nouns — so the retired arm stays measurable:
  `Vault::rebuild_fts` swaps the tokenizer over identical chunk rows and vectors (nothing re-chunks or
  re-embeds), and `just eval-stemmer` scores the unstemmed ablation beside every default run.
- **Does brute force scale to B2?** Comfortably. A personal vault of 10k notes → ~50–100k chunks;
  brute-force cosine over ~100k × 768-dim float32 vectors is single-digit to low-tens of milliseconds.
  We are nowhere near the regime where ANN matters; if a vault ever is, int8/binary quantization and
  ANN hold a standby order behind the centroid stage
  ([GH #106](https://github.com/AlteredCraft/B2/issues/106)).

## 5. Deferred model machinery — the reranker & query expansion

**The reranker is a fast follow.** Slot it where qmd puts it: **after RRF fusion, before final
ranking**, behind a swappable seam. v1 is retrieve → fuse → return top-N, already a strong hybrid
baseline; the fast follow inserts a cross-encoder rerank over the top ~30 candidates plus a
position-aware blend. It is a pure function `(query, candidates) → scores`, so it changes *ordering*,
not the store, the schema, or the candidate set — and it is **store-agnostic**, a model-side seam
above the index. Tracked in [GH #28](https://github.com/AlteredCraft/B2/issues/28).

**Scope — this reranks `b2 search`, not `b2 similar`.** The signature is the tell: it needs *query
text*, and `b2 similar` has none — it is passage↔passage KNN, "near ∖ connected" (§3). The
discovery-side levers are distance-weighting ([GH #20](https://github.com/AlteredCraft/B2/issues/20))
and the pair-scorer seam (§3); the discovery-side *precision* stance is D1's (ADR-0014).

**Gate the decision on the eval, not intuition** (ADR-0013). RRF is a strong baseline; the reranker
buys **top-k precision**, whose value *grows with vault size* and is *highest when an agent consumes
top-1/top-3 with no human eye* ([GH #24](https://github.com/AlteredCraft/B2/issues/24)). Vault size
changes whether the precision is worth it, not the reranker's cost.

**…and check the instrument can see the change you are gating.** Retrieval reaches at least
`chunk_candidate_pool(10) = 60` candidates per signal (150 for the note view); while a corpus has no
more chunks than that, neither signal is truncated and a candidate-width change prints bit-identical
numbers while genuinely reordering a real vault
([GH #141](https://github.com/AlteredCraft/B2/issues/141)). Score *relevance* on the labelled corpus,
but measure a width change with `just stability` over a vault big enough for the pool to bind. That
gate has already ruled once: [#140](https://github.com/AlteredCraft/B2/pull/140) widened the passage
view as plumbing, the eval printed bit-identical numbers, the probe found 10 of 10 top-4 lists
changed, and [#142](https://github.com/AlteredCraft/B2/issues/142) returned it — **the instrument that
can say "different" is not the one that can say "better"**. The blindness is to *candidates*, not to
fusion: `RRF_K` re-weights the same two lists and reorders results at any corpus size.

**Query expansion** (qmd's third model) is **optional and lowest priority** — the heaviest model for
the smallest, most variable win ([GH #105](https://github.com/AlteredCraft/B2/issues/105)).

The harness itself — corpora, labels, metrics, exit gates, process rules — is
[docs/evals/README.md](../evals/README.md); read it before touching any of them.

## 6. The AI seams — the embedder in a single binary, and grounded chat

Two seams, both enumerated by M1 (ADR-0005) and both injected by the adapters.

### `Embedder`

Runtime, provisioning, and the model choice are
**[ADR-0020](../../ADRs/0020-embeddings-inside-the-single-binary.md)**: `candle` + `hf-hub` compiled
into the binary, an explicit `b2 init` into a shared XDG cache, default `BAAI/bge-base-en-v1.5`
(768-dim, CLS-pooled, L2-normalized, bge's asymmetric query prefix), the dimension read from the
model's own `config.json`.

The engine-side consequences are invariants: the embedding space has exactly one recorded identity —
`meta.(embed_model_id, embed_dim)` — and the compute **device folds into it** (ADR-0007, [GH #40](https://github.com/AlteredCraft/B2/issues/40)). A
swap drops both vector tables and re-embeds on `reindex`; `search` **fails fast** rather than mixing
spaces; `open` never mutates the vector space (M2). The fake embedder stays the CI default, so model
quality never enters the fast suite (E2).

### `LlmProvider` — flow ④, grounded chat

Chat is a **reader** of this index and adds nothing to it (M1): no table, no cached response, no
`meta` row, session-only history (S4). That is the deliberate contrast with the embedder — swapping
chat models never touches the index, so a provider swap is a URL/config change.

`Vault::ask` (`chat.rs`) is five steps: **condense** (multi-turn only — a provider call rewrites the
follow-up into a standalone query, degrading to the raw question on failure, so that step can never
break chat) → **retrieve** (`search_chunks` at `ASK_PASSAGES = 10`, the §4 pipeline unchanged,
BM25-only fallback included) → **assemble** (the grounded system prompt + numbered passages — prompt
assembly is core logic, not an adapter's) → **stream** (tokens up through the caller's callback, whose
return value cancels at token granularity — sync, no runtime) → **cite** (`[n]` markers resolve to
`(path, excerpt)` in the returned `AnswerView`; a hallucinated marker resolves to nothing and the
answer text is **never rewritten**).

Two properties follow from the seam's shape. Cancellation is returning early from a blocking read
loop, so **no B2 crate starts an async runtime** (ADR-0011) — a cut stream is marked cancelled and
what already arrived is rendered, never discarded, because a truncated answer is not an error. And
model output is untrusted content like any note (E5, ADR-0016), enforced at the render surface rather
than in the core.

## 7. Tech-stack implications — resolved: Rust

The **single-binary goal** picked the language, not the engine; SQLite and FTS5 are
language-agnostic. See ADR-0019.

## 8. Risks & operational burden

**Chunk vs. note granularity for the graph.** Search is chunk-level; the typed graph is note-level.
`chunks.note_path` is the join, and search hits resolve up to notes for graph operations (§3).

### The bill for a path-keyed graph under `[[path|title]]` links

Keying the graph by the vault-relative path (L1, ADR-0003) makes the stored key *the same thing the
human authored*. These are *the trade working as designed*, not defects; they must be budgeted,
tested, and watched.

- **Write amplification on move.** The link *is* the path, so moving one note rewrites the inbound link
  text in **every** file that points at it — an N-file write. It is bounded and mechanical (the
  materialized edges name exactly which files and links to touch, Markdown-first then index), but
  moving a heavily-linked note is proportional to its backlink count, not O(1). Keep the rewrite
  transactional so a partial move never half-updates the vault. The index side is one cascading
  `UPDATE` plus the re-projection of the inbound sources, bounded by the same count.
- **Out-of-band moves are identified, not repaired — and that is the scope decision.** A `git mv` or
  Finder move is, to a path-keyed index, a delete plus a create: the old path's rows prune, the new
  path projects fresh, and every inbound `[[oldpath]]` becomes a **surfaced dangling edge** (G5) —
  authored text kept, `dst` NULL, healing by itself on the next pass if the target comes back. Nothing
  is silently dropped and nothing is guessed at. **Content-addressed vectors (M4) make it cheap as
  well as honest**: the re-created note's chunk text hashes identically, so the delete-plus-create
  re-embeds nothing. Two repairs are future investigation in GH #170 — a *proposed* hash-match repair
  (`notes.body_hash` and the resource `content_hash` are already stored, but a proposal must stay the
  human's to accept, W4), and a `b2 watch` daemon observing renames live, rejected as a lifecycle and
  coordination surface that shrinks the gap rather than closing it.
- **Path ownership follows the filesystem, so there is nothing to reconcile.** A path names at most
  one file — the filesystem's guarantee, not B2's — so a note deleted and recreated at the same path is
  that path's note, a Finder-duplicated note is two notes at two paths, and `db::upsert_note`'s
  `ON CONFLICT(path)` is the whole of the reconciliation. A note file *deleted with no replacement* is
  reconciled by the whole-vault pass ([GH #31](https://github.com/AlteredCraft/B2/issues/31)):
  `project_vault` prunes every `notes` row whose path the walk did not see this run (aliases, chunks,
  FTS, centroid, outgoing edges cascade; inbound links re-dangle when phase 2 re-derives edges against
  the pruned resolver), **except** rows whose file was skipped as unreadable — the walk *saw* that
  file, so evicting it would lie. Single-note ingest (`add`/`mv`/`write`) touches one note and never
  prunes. Orphaned vectors — hashes no chunk references after that pruning — are collected by the same
  pass, the only bookkeeping content-addressing adds.
- **A single unreadable file never fails the whole index.** A real vault holds the odd non-UTF-8 or
  permission-denied `.md`; projection **skips** it (reported as a `skipped` entry carrying a short,
  file-level reason, surfaced by the CLI and the desktop) and indexes everything else, rather than
  aborting on one file it cannot read.
- **Derived-index consistency is a permanent invariant, not a one-time build.** W5, S3, and L1 are the
  tripwires; every edit path (`b2 mv`, link delete, out-of-band reindex) has to preserve all three or
  the graph silently diverges from the source of truth.
- **Committed edges are only ever authored, never inferred** (G1). Editing the vault can strand a
  connection — deleting an authored `A→B` link — but B2 only ever *surfaces* the consequence, never
  silently rewrites an inbound file or an edge (W4).

*(The stamped `b2id` made "two files, one identity" representable and cost a collision subsystem, a
shadowed-copy panel, restamp notices, and a carve-out on S3. All of it went with the stamp —
ADR-0003. Nothing anomaly-shaped was ever *stored*, so nothing had to be migrated away.)*
