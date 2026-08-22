---
title: "B2 — Index Engine"
type: note
tags: [b2, index-engine, sqlite, fts5, vectors, search, discovery, chat, architecture]
created: 2026-06-29
status: active
---

# B2 — Index Engine

> **The engine design — the *how*.** Specifies the disposable SQLite index (FTS5 + an in-process
> vector scan + the typed graph) and the flows over it, and records why B2 rebuilt
> [tobi/qmd](https://github.com/tobi/qmd)'s pipeline rather than depending on it. Companion docs:
> [invariants.md](invariants.md) (the *why*, cited by id) and [data-model.md](data-model.md) (the
> *what*).

## TL;DR

**Build our own SQLite-backed index engine; take qmd as a design reference, not a dependency.**

- qmd is an excellent *blueprint* for hybrid retrieval (BM25 + vector + RRF + LLM rerank) and proves
  the pipeline runs locally. But it is a **search engine**, and B2 is a **typed graph with hybrid
  retrieval over it**. qmd has no notion of typed edges or backlinks, which are the reasons B2 exists.
- SQLite gives us **one embedded store for every *queryable* concern at once** — full-text (FTS5),
  vectors (plain tables scored in-process), and the typed graph — with transactional consistency
  across them, so `b2 similar` candidate generation joins all three in a single query. That
  single-store property is worth more than anything we'd inherit from qmd. The index is a pure
  disposable cache (S1, S2).
- Because the engine **does** provide vector search, the engine-gated decision resolves in favour of
  **semantic search in v1** (§4).
- The **reranker is a clean fast-follow** — a swappable seam after RRF fusion (§5). Retrieval quality
  is good without it.
- The one genuinely hard part is **not the engine** — it's **producing embeddings inside a single
  binary**, and it is orthogonal to choosing SQLite (§6).

---

## 1. qmd, the reference

A local CLI search engine for Markdown, all on-device: SQLite + FTS5 + `sqlite-vec`; ~900-token chunks
with ~15% overlap and Markdown-aware break-point scoring; three search modes (BM25, vector, hybrid);
a hybrid pipeline of LLM query expansion → parallel retrieval → **RRF fusion** (`Σ 1/(k+rank+1)`,
k=60) → LLM cross-encoder rerank → position-aware blend; local GGUF models via `node-llama-cpp`
(~2–3 GB VRAM with all three loaded); a rich CLI plus an MCP server. TypeScript on Node 22+/Bun, MIT.

It's a clean, well-thought-out design. The disagreement is **scope**, not quality.

**What we borrow wholesale:** the chunking heuristic, the RRF formula and k, the position-aware blend,
the asymmetric query/document prompt discipline (each model brings its own prefix — B2 ships bge's,
§6), the JSON/`--explain` agent-output discipline, and the MCP surface idea.

**What we discard:** the npm/Node packaging and the "DB is the product" framing.

**Chunking as adapted** (`chunk.rs`, [GH #19](https://github.com/AlteredCraft/B2/issues/19)) — four
model-free changes to qmd's heuristic: a **450**-token target (headroom under bge's 512-token
truncation), a `chars/4` proxy for token sizing (the core stays tokenizer-free, E1), an unconditional
stored `heading_path` breadcrumb, and every lever on a `ChunkConfig` (overlap 0.15, backscan 200, the
H1=100 … list=5 break weights). A forced cut is pushed past a fenced code block or Markdown table
rather than bisecting it ([GH #41](https://github.com/AlteredCraft/B2/issues/41)). Tree-sitter AST
chunking for code stays deferred ([GH #104](https://github.com/AlteredCraft/B2/issues/104)).

## 2. Why we rebuild instead of depend on qmd

| Concern | qmd | B2's need |
|---|---|---|
| Full-text search | ✅ FTS5/BM25 | ✅ same |
| Semantic search | ✅ `sqlite-vec` | ✅ in-process scan |
| Rerank | ✅ cross-encoder | ✅ fast-follow |
| **Typed graph** (path→path edges with a relation type) | ❌ none | ⭐ core (G1–G6) |
| **Backlinks** (who points at X, typed, vault-wide) | ❌ none | ⭐ core (G6) |
| **Move-safe links** (a B2-performed move repairs every backlink) | ❌ nothing rewrites links | ⭐ core (L1) |
| **Markdown as source of truth** (index rebuildable/derived) | ~ index *is* the artifact | ⭐ non-negotiable (S1, S2) |
| Distribution | npm package, Node runtime | ⭐ single binary |

The decisive point: B2's index is a **derived projection of the vault** that holds the **typed graph**
*next to* the search indexes, so retrieval and connection discovery share one transactional store.
qmd models none of the graph layer — wrapping it would mean maintaining a second store for everything
that makes B2 *B2*, and reconciling two sources of truth. Rebuilding the ~300 lines of retrieval glue
we actually want is cheaper than that integration tax, and qmd's MIT license + public design make the
rebuild low-risk.

## 3. The storage architecture (one disposable SQLite index)

One artifact, per S1/S2: a **disposable** SQLite index holding every queryable concern
transactionally, rebuildable from the vault at any time (S3). The vault is the single source of truth,
with Markdown its sole authored subset; the index is a cache of it, with no durable B2-derived state
outside your notes (S4).

> The precise DDL and the build order are realized in `crates/b2-core/src/db.rs` (schema) and
> `ingest.rs` (flows). The sketch below is the orientation; the code is the buildable contract.

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
embedding space" signal the BM25-only fallbacks key on (M4).

*(The projection runs as two separately-invokable passes — model-free `project` (notes/resources/
chunks/FTS/edges) then `embed` (vectors), with `reindex` their composition — so keyword search and the
graph are usable before embedding completes
([GH #15](https://github.com/AlteredCraft/B2/issues/15)). S2 is untouched: a
projected-but-unembedded index is a smaller projection, never a wrong one.)*

Why this shape fits B2 specifically:

- **Everything keys on the vault-relative path** (L1). `notes.path` is the primary key;
  `chunks.note_path`, `note_aliases.note_path`, `note_centroids.note_path` and `edges.src_path` are
  `REFERENCES notes(path) ON DELETE CASCADE ON UPDATE CASCADE`, which makes a B2-performed move a
  **path re-key rather than a rebuild**: `UPDATE notes SET path = …` cascades through every child in
  one transaction, alongside the inbound link-text rewrite and a re-projection of the inbound sources.
  `edges.dst_path` is deliberately *not* an FK — it must be allowed to be NULL, the dangling case
  (G5). "Rename keeps every backlink resolving" is therefore a property of the **move operation**, not
  of the key; the price is that a move made *outside* B2 is a delete plus a create (§8).
- **Vectors are content-addressed, and that is what makes the price small** (M4). `embeddings` is
  keyed by `text_hash` — blake3 of the chunk text, which *is* the embed input, verbatim — so a note
  that moves (in band or out) re-embeds nothing: its chunks hash identically and find their vectors
  already stored. The store needs no invalidation rule beyond "a hash no chunk references is garbage",
  pruned by the whole-vault pass on the same derived-data lifecycle as centroids. Identical text
  anywhere in the vault shares one vector, which is a correctness statement before it is a saving.
- **Every `edges` row derives from Markdown** (G1, G2), deduped frontmatter-wins. There is **no
  `status` column and no suggestion queue**. Committing with `b2 link` appends a typed-link string to
  the source note's frontmatter and re-projects that note — a projection of an authored line, not an
  in-place index write.
- **Hybrid retrieval and graph queries compose in one query** — "semantic-nearest chunks whose note is
  within 2 typed hops of note X" is a join across `embeddings`, `chunks`, and `edges`. This is the
  substrate `b2 similar` runs on, and the thing a qmd-as-dependency design could never give us cleanly.
- **Deterministic seams for tests** — a fake embedder writes to `embeddings`, so the whole pipeline is
  assertable with no live model (E2).

**Resources widen the projection without disturbing any statement above** — they are path-keyed peers,
so `index = projection of (the vault directory)`. `resources` is a **separate** table from `notes`,
not a `kind` column on it (two tables, two contracts, zero "unless it's a resource" clauses), and
`edges.dst_resource_path` lets a body `![[photo.png]]` or `[[papers/x.pdf]]` resolve against it while
`src` stays a note (G6). What is built is inventory + graph; making resource *content* searchable
(extraction, chunks, `resource_centroids`) is designed and tracked, not shipped — see
[data-model.md](data-model.md) §10 and [GH #108](https://github.com/AlteredCraft/B2/issues/108). Either
way there is **no migration**: a schema change is a `schema_version` bump + rebuild (S5), the
disposable-index tenet paying rent.

### Opening the index concurrently — many readers, one builder

Nothing in B2 opens the index exclusively, and several things open it at once: `b2 reindex &` racing a
`b2 status`, the desktop app launching while a CLI reindex runs, the desktop host's own threads. The
locked stance is **C1**, and `db::open` is where it is enforced, in three layers that each answer a
different failure:

- **The `WAL` flip is retried** ([GH #111](https://github.com/AlteredCraft/B2/issues/111)). Setting
  `journal_mode = WAL` is the one statement in `open` that takes a write lock, and the one
  `busy_timeout` cannot cover (SQLite skips the busy handler for a write lock upgraded from an
  already-open read transaction), so a second opener took an immediate `SQLITE_BUSY`. The wait is ours
  to do.
- **The schema migration is one `BEGIN IMMEDIATE` transaction, entered only when there is work**
  ([GH #114](https://github.com/AlteredCraft/B2/issues/114)). It is a read-then-decide-then-write
  sequence whose rebuild is ~30 DDL statements; unserialized, two openers that both read a stale
  `schema_version` interleave — one's `DROP TABLE resources` landing after the other's `CREATE`, and
  the current version stamped over a half-demolished schema. `busy_timeout` was irrelevant: nothing
  contended for a lock, every statement succeeded, in the wrong order. Serializing on SQLite's own
  write lock makes the loser wait and then find the work done; wrapping it makes a rebuild
  all-or-nothing, which is what lets the stamp be trusted. The check that decides whether to enter is
  a **read**, so the common open against a current schema takes no write lock and can never be refused
  by a writer.
- **Completeness is checked, not assumed.** A stamp is believed only alongside the tables it vouches
  for; a current stamp over missing tables is treated as stale and rebuilt from empty. Recreating just
  the missing tables would be worse than useless: an incremental reindex skips notes whose `body_hash`
  matches, so the recreated tables would stay empty and S3 would quietly fail.

The vector tables are the same drop-and-rebuild shape and get the same treatment — two embed passes
can genuinely overlap, since the `reindex` advisory lock
([GH #55](https://github.com/AlteredCraft/B2/issues/55)) is taken by `b2-cli` alone and never by the
desktop host or by readers.

**Why not an advisory lock file for this**, given B2 already has one for `reindex`? It would be a
*third* concurrency mechanism guarding state the database already knows how to guard, and the weaker
one where it counts: a vault on a network share or a synced folder — a plausible home for a personal
vault — is exactly where `flock` quietly stops meaning anything. The `reindex` lock answers a question
SQLite cannot (*is another **process** already doing this expensive work?*); schema atomicity is not
that question.

### Why materialize the graph at all — vs. resolving links at runtime

A note's *outbound* links (and their type + explanation) are parseable from that one file on demand,
so it's fair to ask why the index carries an `edges` table. **Edge metadata is not the reason** — a
runtime parse yields the verb and explanation just as well, *for that note's outbound edges*.
**Inversion and composition are the reason.** Materializing edges turns three things from full-vault
scans (or impossibilities) into indexed lookups:

- **Backlinks / inversion.** "Who points at X" cannot be read from X — only from every *other* note.
  The runtime answer is O(vault) per query; the table makes it one lookup. This is also what services
  L1: the edges name the exact N inbound files to rewrite on a move instead of scanning the vault to
  find them (§8).
- **Typed multi-hop traversal.** "notes within 2 hops of X via `supports`/`contradicts`" is a scan
  *per hop* at runtime; over `edges` it is one SQL traversal.
- **The graph⨝vector join.** "semantic-nearest chunks whose note is within k typed hops of X" is a
  single join `embeddings ⨝ chunks ⨝ edges`, not expressible as a per-note parse. It is a
  **scoped-traversal** primitive (search *within* an already-related neighborhood).
  **`b2 similar`'s candidate generation is its *complement*:** notes semantically near an anchor but
  *not* within 1 hop — the links you *haven't* made — where the materialized graph supplies the
  "∖ already-connected" exclusion.

The reframe that keeps this cheap is **G6**: runtime outbound-parsing is the correctness *definition*;
the `edges` table is its *cache*. It is not a third subsystem beside FTS5 and the vector tables — it is
one more **disposable** table in the same store, populated by the **same parse pass** that already
walks each body for chunking. Strip it and B2 is vector + keyword search over Markdown — i.e. qmd (§2).
The standing cost of carrying it is the move-repair write-amplification budgeted in §8.

### Discovery surfacing serves the ranked list — `limit` is a cap, not a promise

The standing rule is **invariants.md D1**, ruled by
[GH #197](https://github.com/AlteredCraft/B2/issues/197) (2026-08-18) on
[GH #196](https://github.com/AlteredCraft/B2/issues/196)'s measurement. Discovery's question is
**relative** — *what in my vault belongs next to this note?* — and the ranked best-passage order
answers it. `similar` serves the ranked top-N whenever candidates exist; `limit` under-fills only for
want of scorable notes, and **no statistical bar truncates the list**.

The per-candidate z survives as a *statistic* — computed after stage 2 on the best-passage distances,
non-increasing down the row order (strictly monotonic in the *score*; tied scores share a z and order
by the path tie-break), painted as the within-list strength band — but it **gates nothing**. Because
the judged z is affine in the squared best-pair distance the score negates the root of, **score order,
z order, and band are one number**: a card can never show a weaker band above a stronger one. Below
`STATS_MIN_POPULATION` (12), in a space with no spread, or under the fake embedder, **no z exists at
all**, and a surface must *say* the list is ungraded — silence there reads as "all judged, all scored
low", the opposite of what happened. Candidate *generation* is unchanged and stays recall-oriented:
the two-stage scan (§4) over-produces, nothing auto-links, and the human commits every edge (W4).

**How this was earned.** Seven issues of measurement, in order — kept as citations rather than
narrative, per the repo's rule that the decision history is the issue plus the commit:

| Issue | What it measured | Verdict |
|---|---|---|
| [#150](https://github.com/AlteredCraft/B2/issues/150) | absolute cosine floors vs. drop-off-from-top-1, against labelled score piles | both fail; shipped a per-anchor **z-score floor** (leader gate + member bar) instead |
| [#183](https://github.com/AlteredCraft/B2/issues/183) | a **multi-topic note family** + a **per-mate** metric (the per-anchor one saturates at an anchor's easiest mate) | the centroid-z order both **demoted** and **suppressed** labelled mates — 3 of 14 never served |
| [#187](https://github.com/AlteredCraft/B2/issues/187) | dumped every candidate's ungated z and re-derived both admissible windows each run | the **member bar has no window**: mates from +0.80, strangers to +1.62 — no constant separates an inversion |
| [#189](https://github.com/AlteredCraft/B2/issues/189) | the journal/daily-note archetype (N≥6 unrelated sections) | averaging disagreeing chunk vectors collapses the centroid toward the corpus mean — the note tops *loner* anchors while its own gem is suppressed; both failures at once |
| [#192](https://github.com/AlteredCraft/B2/issues/192) | judging **after stage 2, on the best-passage z** — the same number that orders the list | the unit separates what the centroid could not; stage 1 is recall only again; 14/15 mates, 0 strangers |
| [#182](https://github.com/AlteredCraft/B2/issues/182) | the desktop band, still calibrated on the *centroid* z's landmarks after the reorder | bands re-read in the judged unit; **a change to the judged statistic is a change to every surface that paints it** — the standing rule |
| [#196](https://github.com/AlteredCraft/B2/issues/196)/[#197](https://github.com/AlteredCraft/B2/issues/197) | the first real vault dogfooded: single-domain, 17 notes | **16 of 17 panes dark on correct rankings** — the gate retired |

The closing finding is the one that generalizes. A z-score treats the anchor's population mean as a
*noise floor*, valid only when related notes are rare outliers in a dominant unrelated tail; the
threshold literature models relevant and non-relevant scores as **two** distributions and thresholds
at the crossover. A vault whose notes all sit in one domain has no unrelated tail — the mean *is*
"moderately related" — so a single-population outlier test reads *everything is related* as *nothing
is*. "One diffuse cloud" and "a coherent single-subject vault" are the same geometry from opposite
ends. Three amplifiers were measured on that vault: the leader sits inside the population it is judged
against; **cluster self-dilution** (link-worthy siblings lift the mean for each other, so investing in
a topic makes its notes progressively harder to surface, and the 1-hop exclusion only relieves it
*after* the pane has done a job it cannot do); and a population-size cliff crossed by adding four
notes. Large vaults fail the same way eventually — above `SHORTLIST_MIN` the judged population is the
anchor's centroid-nearest slice, pre-selected related, so every big vault reproduces the single-domain
geometry inside its own shortlist.

The review also relocated two pieces of standing evidence. The eval corpus is engineered *orthogonal*
(its token audit deliberately minimizes shared vocabulary), which makes it a good instrument for
**ranks** and a structurally incapable one for **distributions** — its strongest labelled pair sits
below a real vault's background. And #150's rejection of absolute floors ("kept 99–100% of a
228-essay single-author vault's candidates") was the same finding read through the assumption it
should have been testing: keeping nearly everything of a single-author vault was plausibly *correct*.

What #197 left standing: the z as the band's ungated input; two instruments —
**`just calibrate <vault>`** (per-anchor pool distributions on any built vault; the real-vault transfer
check every distributional constant now owes) and the **dense single-domain fixture**
(`evals/corpus-dense/`, whose assertions are a per-mate MRR floor and **zero empty panes**); a
suppression assertion kept as a **structural-zero tripwire** that re-arms if a gate ever returns; and
a Phase-2 **evidence-gated bake-off** for any replacement existence signal — the leading candidate
being mutual-kNN/reciprocal-rank (rank-based, therefore constant-free), with "no gate at all" an
admissible winner and continuity in population size an entry requirement. The **pair-scorer
escalation** stays the named long-term seam: an anchor-local rule cannot catch a *pair-level*
miscalibration (a single stranger the model scores like a cluster-mate — the standing
`encryption ↔ phishing` residue), and a discovery-side pair-scorer would be a second model seam,
sibling of §5's reranker but distinct from it (that seam needs query text and `similar` has none). It
would still only filter what is surfaced, never author a link. Under always-serve that residue is
**ordering quality, not existence**.

**Phase 2 is open, and its question is disclosure, not existence** (D1 as redrafted, 2026-08-22;
the bake-off is [GH #200](https://github.com/AlteredCraft/B2/issues/200), search's sibling
[GH #201](https://github.com/AlteredCraft/B2/issues/201), the surfaces and exit-gate moves
[GH #202](https://github.com/AlteredCraft/B2/issues/202)).
Real-vault dogfooding measured always-serve's cost from the other side: a pane that fills to `limit`
regardless of quality trains distrust of every card — count read as a claim when it was only layout.
The redrafted D1 splits the axes the retired gate conflated: the *ranked list* stays fully reachable
(the #196 guarantee, unweakened), while a quality signal may set the *default disclosure boundary* —
a prefix fold, the remainder collapsed one gesture away, so a misjudged fold costs a keystroke where
the gate cost the feature. The bake-off defined above now has its opening evidence and two entry
requirements beside continuity: prefix form (a signal that would admit rank 5 while folding rank 2
cannot ship — row order, band, and fold must never visibly disagree), and on the dense fixture a
non-empty default view is absolute. If a fold ships, the retired negatives assertion returns on this
axis: a loner anchor's correct default view is empty-above-the-fold — the labelled "nothing relates"
made assertable again without re-darkening the vault where everything does.

## 4. Retrieval — semantic search, fusion, and discovery

The engine-gated decision was *"if the index engine provides vector/semantic search, it's in v1."* It
does. Therefore **semantic search is in v1** — exact, in-process, no vector extension, no ANN.

- **Storage & scoring.** Vectors live in plain tables — `embeddings(text_hash, vector)` plus per-note
  `note_centroids` — read with one statement and scored in-process (`embed::l2_sq`).
  Content-addressing (M4) costs the scan one indexed join back to `chunks` to recover which chunk each
  vector ranks for; it buys a vault-wide "identical text, one vector" store where a move re-embeds
  nothing. A `vec0`-style virtual table charges a per-row shadow-table probe on every scan, which
  dominates at real-vault scale; the plain-table scan does not
  ([GH #38](https://github.com/AlteredCraft/B2/issues/38)).
- **Flow ② hybrid search** (`search.rs`). BM25 over `chunks_fts` ⊕ vector KNN, fused with Reciprocal
  Rank Fusion (`Σ 1/(k+rank+1)`, `RRF_K = 60`), resolved from chunks up to notes. Raw NL queries are
  sanitized into a safe FTS5 `MATCH` expression — punctuation is FTS5 syntax and would otherwise crash
  the parse. On a projected-but-unembedded vault the vector half is simply absent and the same fusion
  runs over the single BM25 list, so scores stay on one scale.
  One honesty debt is on record (invariants.md **D2**, 2026-08-22;
  [GH #201](https://github.com/AlteredCraft/B2/issues/201)): KNN always has k nearest and RRF
  keeps only ranks, so this flow cannot yet answer *zero* — a nonsense query serves `limit`
  confident-looking results. The fix owes the harness first: labelled negative queries, a per-model
  evidence bar for the vector half (a distributional constant, so process rule 5's transfer check),
  and hit provenance carried through fusion so a fold can judge what RRF currently discards —
  the negative queries and the evidence dump are landed (Phase A); the bar and the fold are #201's.
- **Flow ③ discovery is two-stage** (`discover.rs`). An O(notes) coarse scan over centroids shortlists
  candidates (`SHORTLIST_PER_RESULT = 20` per asked result, floored at `SHORTLIST_MIN = 200`), then an
  exact max-sim rescore over only the shortlist's chunk vectors, minus the anchor's 1-hop graph
  neighbors. No model call at surface time — `b2 similar` surfaces, `b2 link` commits, and the human
  is the precision gate.
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

**The reranker is a fast follow.** Slot it exactly where qmd puts it: **after RRF fusion, before final
ranking**, behind a swappable seam. v1 is retrieve → fuse → return top-N, which is already a strong
hybrid baseline; the fast follow inserts a cross-encoder rerank over the top ~30 candidates plus a
position-aware blend. It is a pure function `(query, candidates) → scores`, so it changes *ordering*,
not the store, the schema, or the candidate set — "eventually add a reranker" is a one-stage
insertion, not a redesign. It is also **store-agnostic**: a model-side seam above the index, so no
vector-store choice simplifies or blocks it. Tracked in
[GH #28](https://github.com/AlteredCraft/B2/issues/28).

**Scope — this reranks `b2 search`, not `b2 similar`.** The signature is the tell: it needs *query
text*, and `b2 similar` has none — it is passage↔passage KNN, "near ∖ connected" (§3). The
discovery-side levers are distance-weighting ([GH #20](https://github.com/AlteredCraft/B2/issues/20))
and the pair-scorer seam (§3), not this; and the discovery-side *precision* stance is D1's — the
ranked list stays reachable, any fold of the default view must win Phase 2's evidence-gated
bake-off, and the human is the precision gate. #20 reorders candidates; it cannot decide an empty
default view.

**Gate the decision on the eval, not intuition.** RRF is a strong baseline; the reranker buys
**top-k precision**, whose value *grows with vault size* (semantic near-misses crowd the top past ~1k
notes) and is *highest when an agent consumes top-1/top-3 with no human eye* (the `serve` adapter,
[GH #24](https://github.com/AlteredCraft/B2/issues/24)). Vault size changes whether the precision is
*worth* it, not the reranker's cost — that is fixed at the top-N it rescores.

**…and check the instrument can see the change you are gating.** Retrieval reaches at least
`chunk_candidate_pool(10) = 60` candidates per signal (150 for the note view). While a corpus has no
more chunks than that, **neither signal is truncated** and a candidate-width change prints
bit-identical numbers while genuinely reordering a real vault
([GH #141](https://github.com/AlteredCraft/B2/issues/141)). So: score *relevance* on the labelled
corpus, but measure a width change with the rank-stability probe over a vault big enough for the pool
to bind (`just stability` on `fixtures/test-vault`); `just eval` prints its own blindness
(`pool_blind`) when the corpus fits inside the narrower pool. That gate has already ruled once —
[#140](https://github.com/AlteredCraft/B2/pull/140) widened the passage view to 3× as plumbing, the
eval printed bit-identical numbers, the probe found 10 of 10 top-4 passage lists changed, and with no
labelled evidence that the wider set ranked *better*,
[#142](https://github.com/AlteredCraft/B2/issues/142) returned it to the constant. **The instrument
that can say "different" is not the one that can say "better"** — shipping a width change needs both.
The blindness is to *candidates*, not to fusion: `RRF_K` re-weights the same two lists, so it reorders
results on a corpus of any size and is gated on the labelled eval like any other quality change.

**Query expansion** (qmd's third model) is **optional and lowest priority** — the heaviest model for
the smallest, most variable win. A later, off-by-default flag
([GH #105](https://github.com/AlteredCraft/B2/issues/105) covers the adjacent fusion tuning).

The harness itself — corpora, labels, metrics, exit gates, and its process rules — is
[docs/evals/README.md](../evals/README.md); read it before touching any of them.

## 6. The AI seams — the embedder in a single binary, and grounded chat

Two seams, both enumerated by M1 and both injected by the adapters.

### `Embedder` — and the real hard part, embeddings in a single binary

This is the only place the architecture meets real friction, and it is **independent of the SQLite
decision**: any engine that does semantic search needs vectors from somewhere. qmd's answer
(`node-llama-cpp` + auto-downloaded GGUF + a JS runtime) tensions B2's single-binary,
no-install-ritual goal directly. The resolution, in three steps:

**Decided (2026-06-30).** Runtime = **`candle` + `hf-hub`** — pure-Rust inference compiled into the
binary, no external ONNX Runtime to ship, with `hf-hub` as the download seam. **Not bundled** — an
explicit **`b2 init`** downloads and verifies the model into a shared XDG cache, never a surprise
mid-command download; `reindex`/`search` fail fast with "run `b2 init`" if it is absent. **The model
source is configurable** (default an HF repo id; overridable to a mirror, another repo, or a local
path for offline installs) via a global TOML at `$XDG_CONFIG_HOME/b2/config.toml`.

**Built (2026-07-01),** in the `b2-embed` crate: `LocalEmbedder` behind the `b2-core` `Embedder` seam;
CLS-pool + L2-normalize; asymmetric query prefix. **Model default `BAAI/bge-base-en-v1.5`**
(BERT-family, **768-dim**, ungated). EmbeddingGemma-300M was the first choice and lost on friction —
it is *gated* on Hugging Face (HTTP 401 without a token + license acceptance), which defeats a
friction-free `b2 init`; it remains selectable via config for anyone who provides a token. The dim is
read authoritatively from the model's own `config.json` (`hidden_size`), so config can't lie about it.

**Consequences that are invariants.** The embedding space has exactly one recorded identity —
`meta.(embed_model_id, embed_dim)` — and the compute **device folds into it**: a `--features metal`
GPU build tags the id `@metal`, because a device/precision change that alters vectors *is* a model
swap ([GH #40](https://github.com/AlteredCraft/B2/issues/40)). A swap drops both vector tables and
re-embeds on `reindex`; `search` **fails fast** rather than mixing spaces; `open` never mutates the
vector space, so changing the configured model cannot wipe vectors on the next command (M2). The fake
embedder stays the CI default, so model quality never enters the fast suite (E2).

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
loop, so **no B2 crate starts an async runtime** — a cut stream is marked cancelled and what already
arrived is rendered, never discarded, because a truncated answer is not an error. And model output is
untrusted content like any note (E5), enforced at the render surface rather than in the core.

## 7. Tech-stack implications — resolved: Rust

SQLite + FTS5 are language-agnostic, so the engine didn't pick the
language; the **single-binary goal** did. B2 is a Rust Cargo workspace (`rusqlite` with bundled
SQLite; `candle` for the embedder). qmd's TypeScript/Node path is the least aligned with that goal,
which is another reason not to inherit its runtime by depending on it.
## 8. Risks & operational burden

**Chunk vs. note granularity for the graph.** Search is chunk-level; the typed graph is note-level.
`chunks.note_path` is the join, and search hits resolve up to notes for graph operations (§3).

### The bill for a path-keyed graph under `[[path|title]]` links

Keying the graph by the vault-relative path (L1) makes the stored key *the same thing the human
authored*, which removes a whole class of state the old stamped-id design had to reconcile — and
leaves one standing cost in its place. These are *the trade working as designed*, not defects; they
must be budgeted, tested, and watched.

- **Write amplification on move.** The link *is* the path, so moving one note rewrites the inbound
  link text in **every** file that points at it — an N-file write. It is bounded and mechanical (the
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
  re-embeds nothing. Two repairs were considered and left as future investigation in GH #170: a
  *proposed* hash-match repair (`notes.body_hash` and the resource `content_hash` are already stored,
  but a proposal must stay the human's to accept, W4), and a `b2 watch` daemon observing renames live,
  rejected as a lifecycle and coordination surface that shrinks the gap rather than closing it.
- **Path ownership follows the filesystem, so there is nothing to reconcile.** A path names at most
  one file — the filesystem's guarantee, not B2's — so the states the old design had to arbitrate do
  not arise: a note deleted and recreated at the same path is that path's note, a Finder-duplicated
  note is two notes at two paths, and `db::upsert_note`'s `ON CONFLICT(path)` is the whole of the
  reconciliation. A note file *deleted with no replacement* is reconciled by the whole-vault pass
  ([GH #31](https://github.com/AlteredCraft/B2/issues/31)): `project_vault` prunes every `notes` row
  whose path the walk did not see this run (aliases/chunks/FTS/centroid/outgoing edges cascade;
  inbound links re-dangle when phase 2 re-derives edges against the pruned resolver), **except** rows
  whose file was skipped as unreadable — the walk *saw* that file, so evicting it would lie.
  Single-note ingest (`add`/`mv`/`write`) touches one note and never prunes. Orphaned vectors — hashes
  no chunk references after that pruning — are collected by the same pass, the only bookkeeping
  content-addressing adds. Resources churn more than notes and their inventory pass has always pruned
  this way.
- **What this section no longer has to budget.** The stamped `b2id` made "two files, one identity"
  representable, and a collision subsystem, a shadowed-copy review panel, restamp notices, a
  malformed-YAML re-stamp guard, a frontmatter write guard, and a carve-out on S3 were the bill for
  it. All of it is deleted with the stamp (GH #170), and `reindex --dry-run` correspondingly shrinks
  to what a read-only preview can honestly say: which notes would (re)embed. Nothing anomaly-shaped
  was ever *stored*, so nothing had to be migrated away — each notice was re-derived from vault +
  index every pass (S2), and the ones that no longer exist simply stop being derived.
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

## 9. Recommendation

1. **SQLite is the B2 index engine** (FTS5 + plain vector tables) per the §3 schema — one disposable
   index, `index = projection of (the vault directory)`. qmd is a design reference under its MIT
   license, **not** a dependency.
2. **The engine-gated outcome:** semantic search is **in v1** (exact in-process KNN; quantization
   reserved for scale).
3. **Reranker = explicit fast-follow** behind a post-fusion seam; query expansion = later/optional.
4. **The seams are the AI surface** (M1): the store, indexes, and typed graph are built and tested
   against the **deterministic fake embedder**, with the golden-vault fixtures as the yardstick
   ([data-model.md](data-model.md) §8).

> Net: qmd answers "can a great hybrid search engine run locally on Markdown?" — yes, and here's how.
> B2's question is one layer up: "can that retrieval live inside a typed, traversable, agent-operated
> graph I fully own, in a single binary?" SQLite is the substrate that makes every queryable concern
> one disposable store, a pure projection of your Markdown. We take qmd's pipeline and build the graph
> it was never trying to be.
