# Index engine

How B2 turns a vault into a searchable index and serves reads over it, for anyone changing
the engine. This page specifies the disposable SQLite index (FTS5, an in-process vector scan,
and the typed graph) and the four flows over it.

Related pages: [invariants.md](invariants.md) is the normative register, cited by id.
[data-model.md](data-model.md) defines what the index projects. The *why* behind each choice
is an [ADR](../ADRs/README.md): why SQLite and not qmd (ADR-0019), why plain vector tables
(ADR-0006), why the graph is materialized (ADR-0010), why discovery always serves (ADR-0014),
why a served result claims evidence (ADR-0015), why candle (ADR-0020). Each section below
names the record that decided it.

Words used on this page: a **chunk** is a slice of a note, roughly 450 tokens, the unit
everything compares. An **embedding** (or vector) is the list of numbers a model produces for
a chunk: its position on a map of meaning. **BM25** is classic keyword ranking (rare words
count more). **RRF** (Reciprocal Rank Fusion) merges two ranked lists by position.

## 1. qmd, the reference, and the chunker

[qmd](https://github.com/tobi/qmd) is a local CLI search engine for Markdown, all on-device:
SQLite + FTS5 + `sqlite-vec`; ~900-token chunks with ~15% overlap and Markdown-aware
break-point scoring; three search modes; a pipeline of LLM query expansion → parallel
retrieval → RRF fusion (`Σ 1/(k+rank+1)`, k=60) → cross-encoder rerank → position-aware
blend; local GGUF models; TypeScript on Node, MIT.

We rebuilt rather than depended (ADR-0019). Taken wholesale: the chunking heuristic, the RRF
formula and `RRF_K = 60`, the position-aware blend, the asymmetric query/document prompt
discipline (each model brings its own prefix; B2 ships bge's, §6), and the JSON/`--explain`
agent-output discipline. Discarded: the npm/Node packaging and the "DB is the product"
framing.

**The chunker as adapted** (`chunk.rs`, [GH #19](https://github.com/AlteredCraft/B2/issues/19)).
Four model-free changes to qmd's heuristic:

- A **450**-token target: headroom under bge's hard 512-token truncation.
- A `chars/4` proxy for token sizing, so the core stays tokenizer-free (E1).
- An unconditional stored `heading_path` breadcrumb on every chunk.
- Every lever on a `ChunkConfig`: overlap 0.15, backscan 200, and the break weights (H1=100
  down to list=5).

The chunker prefers to cut at headings, then paragraph breaks. A forced cut is pushed past a
fenced code block or a Markdown table rather than bisecting it
([GH #41](https://github.com/AlteredCraft/B2/issues/41)). Consecutive chunks overlap by about
15%, so an idea straddling a boundary is not sliced in half. Every chunk records
`char_start..char_end` for the exact body slice that produced it, so it stays addressable for
highlight and explain. The config is eval-validated: GH #44 ran a seven-variant sweep
(`make eval-sweep`) and kept `ChunkConfig::default()`; the `prepend_heading_path` knob
measured rank-neutral twice and ships off. Tree-sitter AST chunking for code stays deferred
([GH #104](https://github.com/AlteredCraft/B2/issues/104)).

## 2. Why we rebuilt instead of depending on qmd

Recorded in full as
[ADR-0019](../ADRs/0019-build-our-own-sqlite-index-engine.md). In one line: qmd is a search
engine, and B2 is a typed graph with hybrid retrieval over it. qmd models no typed edges, no
backlinks, and nothing that repairs links on a move (G1 to G6, L1), and SQLite holds all three
queryable concerns in one transactional store.

## 3. The storage architecture

One artifact, per S1/S2 and ADR-0002: a disposable SQLite index holding every queryable
concern transactionally, rebuildable from the vault at any time (S3).

The precise DDL and build order live in `crates/b2-core/src/db.rs` (schema) and `ingest.rs`
(flows). The sketch below is the orientation; the code is the buildable contract.

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

Every table is derived from the vault; there is no third home. The two vector tables are
created at *embed* time rather than in the base migration: their existence *is* the "this
vault has an embedding space" signal the BM25-only fallbacks key on (M4, ADR-0006).

Why this shape fits B2:

- **Everything keys on the vault-relative path** (L1, ADR-0003). `notes.path` is the primary
  key; `chunks.note_path`, `note_aliases.note_path`, `note_centroids.note_path`, and
  `edges.src_path` are `REFERENCES notes(path) ON DELETE CASCADE ON UPDATE CASCADE`. That
  makes a B2-performed move a path re-key rather than a rebuild: `UPDATE notes SET path = …`
  cascades through every child in one transaction, alongside the inbound link-text rewrite
  and a re-projection of the inbound sources. `edges.dst_path` is deliberately *not* a
  foreign key: it must be allowed to be NULL, the dangling case (G5). "Rename keeps every
  backlink resolving" is therefore a property of the move operation, not of the key; the
  price is that a move made outside B2 is a delete plus a create (§8).
- **Vectors are content-addressed** (M4, ADR-0006), keyed by blake3 of the chunk text, which
  *is* the embed input, verbatim. A note that moves re-embeds nothing, identical text
  anywhere shares one vector, and the only invalidation rule is "a hash no chunk references
  is garbage", pruned by the whole-vault pass on the same derived-data lifecycle as
  centroids.
- **Every `edges` row derives from Markdown** (G1, G2, ADR-0010), deduped frontmatter-wins.
  There is no `status` column and no suggestion queue: `b2 link` appends a typed-link string
  to the source note's frontmatter and re-projects that note. Committing is the projection of
  an authored line, not an in-place index write.
- **Hybrid retrieval and graph queries compose in one query.** "Semantic-nearest chunks whose
  note is within 2 typed hops of note X" is a join across `embeddings`, `chunks`, and
  `edges`. This is the substrate `b2 similar` runs on.
- **Deterministic seams for tests.** A fake embedder writes to `embeddings`, so the whole
  pipeline is assertable with no live model (E2).

**Resources widen the projection without disturbing any statement above.** Path-keyed peers,
so `index = projection of (the vault directory)` still holds. `resources` is a separate table
from `notes`, not a `kind` column on it (two tables, two contracts, zero "unless it's a
resource" clauses), and `edges.dst_resource_path` lets a body `![[photo.png]]` or
`[[papers/x.pdf]]` resolve against it while `src` stays a note (G6). What is built is
inventory plus graph; resource *content* search is designed, not shipped
([data-model.md](data-model.md) §10). Either way there is no migration: a schema change is a
`schema_version` bump plus a rebuild (S5).

### How a reindex runs

Ingest (`ingest.rs`) is the write path that realizes `index = projection of (the vault
directory)`. A reindex composes two separately invokable passes
([GH #15](https://github.com/AlteredCraft/B2/issues/15)):

1. **Project** (`project_vault`, model-free): notes, resources, chunks, FTS, edges.
2. **Embed** (`embed_vault`): fill the DB-derived set of chunks still missing a vector.

Projection itself runs in two phases so link resolution never depends on file order: phase 1
projects every note and its chunks (filling the resolver, the set of known note paths, for
the whole vault); phase 2 derives edges against that now-complete resolver. Chunk text lands
in `chunks_fts` at projection time, so a projected-but-unembedded vault is already
keyword-searchable (search degrades to BM25-only). The desktop uses exactly this: project,
paint the tree, embed in the background. S2 is untouched: a projected-but-unembedded index is
a smaller projection, never a wrong one.

**The incremental skip: embed only what changed.** A full re-embed is the one genuinely slow
step (a real transformer, on CPU), so a routine reindex must not redo finished work. Per
note: if the body is unchanged and its chunks already all have vectors, reuse them and embed
nothing. Otherwise re-chunk and re-embed. Two signals, one per pass:

| Signal | Source | Why it is needed |
|---|---|---|
| body unchanged | `db::note_body_hash`, the stored hash read before the upsert overwrites it | same body ⇒ `chunk_body` yields identical chunks, deterministically |
| fully embedded | `db::note_fully_embedded`, every chunk has an `embeddings` row | catches the model-swap case: body unchanged, but the vector space was just emptied |

`--force` bypasses the skip; a model swap forces it implicitly (§6). Edges are the asymmetry:
they are *always* re-derived in phase 2 (cheap), because a link's target may have moved even
when this note's body didn't. A frontmatter-only edit (a `b2_relations:` entry written by
`b2 link`) therefore re-projects the note and its edges but embeds nothing, which is what
makes `b2 link` cheap.

The skip only reuses vectors a fresh embed would reproduce byte-for-byte (the embedder is a
pure function of chunk text), so incremental ≡ full rebuild holds (S3). Both paths replace
rather than accumulate: re-projecting a note deletes its old chunks (FTS triggers fire) and
all its edges, then re-derives everything from the current Markdown. The whole-vault pass
owns every *reconciliation*: pruning rows for files the walk no longer met, collecting
vectors no chunk references. Single-note paths never prune. That division of labor is S3's
stated scope: a single-note path converges for the note it touches, and deletions reconcile
on the next whole-vault pass. An interrupted embed heals on the
next reindex, because the embed pass fills whatever chunks lack vectors, whyever they lack
them.

**The operator view.** The embed phase reports `ReindexProgress` after every batch; the CLI
renders a live line on an interactive stderr only, so `--json` and piped output stay pure
data. The count tracks notes that actually embed, not position in the full list. `reindex`
reports what it did: `indexed`, `embedded` (the count that re-ran), `resources`, plus any
unreadable files it skipped. `b2 reindex --dry-run` previews the same decision, writing
nothing. `b2 reindex &` backgrounds through the shell; `b2 status` reports coverage plus the
running pid; `b2 reindex --cancel` signals that pid onto the same cooperative-cancel path
Ctrl-C uses. Readers keep working throughout (C1).

### Opening the index concurrently: many readers, one builder

Several things open one index at once: `b2 reindex &` racing a `b2 status`, the desktop app
launching while a CLI reindex runs, the desktop host's own threads. The locked stance is C1,
enforced in `db::open` in three layers, each answering a different failure:

- **The WAL flip is retried** ([GH #111](https://github.com/AlteredCraft/B2/issues/111)).
  Setting `journal_mode = WAL` is the one statement in `open` that takes a write lock and the
  one `busy_timeout` cannot cover: SQLite skips the busy handler for a write lock upgraded
  from an already-open read transaction, so a second opener took an immediate `SQLITE_BUSY`.
  The wait is ours.
- **The schema migration is one `BEGIN IMMEDIATE` transaction, entered only when there is
  work** ([GH #114](https://github.com/AlteredCraft/B2/issues/114)). Unserialized, two openers
  that both read a stale `schema_version` interleave: one's `DROP TABLE` lands after the
  other's `CREATE`, and the current version gets stamped over a half-demolished schema.
  `busy_timeout` was irrelevant: nothing contended; every statement succeeded, in the wrong
  order. The check that decides whether to enter is a *read*, so the common open against a
  current schema takes no write lock and can never be refused.
- **Completeness is checked, not assumed.** A stamp is believed only alongside the tables it
  vouches for; a current stamp over missing tables is stale and rebuilt from empty.
  Recreating just the missing tables would be worse than useless: an incremental reindex
  skips notes whose `body_hash` matches, so they would stay empty and S3 would quietly fail.

The vector tables are the same drop-and-rebuild shape and get the same treatment. Not an
advisory lock file: that would be a third concurrency mechanism guarding state the database
already guards, and the weaker one where it counts (on a network share or synced folder,
`flock` quietly stops meaning anything). The `reindex` lock
([GH #55](https://github.com/AlteredCraft/B2/issues/55)) answers a question SQLite cannot
(is another *process* already doing this expensive work?). It is taken by `b2-cli` alone,
never by the desktop host or by readers, so it cannot cover schema atomicity.

### Why materialize the graph at all

A note's *outbound* links are parseable from that one file on demand, so edge metadata is not
the reason. Inversion and composition are. Materializing turns three things from full-vault
scans (or impossibilities) into indexed lookups:

- **Backlinks.** "Who points at X" cannot be read from X, only from every *other* note:
  O(vault) per query at runtime, one lookup here. This also services L1: the edges name the
  exact files to rewrite on a move instead of scanning the vault to find them (§8).
- **Typed multi-hop traversal.** "Notes within 2 hops of X via `supports`" is a scan per hop
  at runtime; over `edges` it is one SQL traversal.
- **The graph⨝vector join.** "Semantic-nearest chunks whose note is within k typed hops of X"
  is a single join `embeddings ⨝ chunks ⨝ edges`, not expressible as a per-note parse.
  `b2 similar`'s candidate generation is its complement: notes semantically near an anchor
  but *not* within 1 hop, where the materialized graph supplies the exclusion.

G6 keeps this cheap: runtime outbound-parsing is the correctness *definition*, and the
`edges` table is its *cache*. One more disposable table in the same store, populated by the
same parse pass that already walks each body for chunking. The standing cost is the
move-repair amplification in §8.

### Discovery serves the ranked list: `limit` is a cap, not a promise

The standing rule is invariants.md D1; the decision, the seven issues of measurement behind
it, and the retired z-gate are
[ADR-0014](../ADRs/0014-discovery-always-serves-the-ranked-prefix.md). Mechanically, in this
engine:

- `similar` serves the ranked top-N whenever candidates exist. `limit` under-fills only for
  want of scorable notes, and no statistical bar truncates the list.
- The per-candidate **z survives as a statistic that gates nothing**: computed after stage 2
  on the best-passage distances, non-increasing down the row order (strictly monotonic in the
  score; tied scores share a z and order by the path tie-break), painted as the within-list
  strength band. Because the judged z is affine in the squared best-pair distance the score
  negates the root of, score order, z order, and band are one number: a card can never show a
  weaker band above a stronger one.
- Below `STATS_MIN_POPULATION` (12), in a space with no spread, or under the fake embedder,
  no z exists at all, and a surface must *say* the list is ungraded. Silence there would read
  as "all judged, all scored low", the opposite of what happened.
- Candidate *generation* is unchanged and stays recall-oriented: the two-stage scan (§4)
  over-produces, nothing auto-links, and the human commits every edge (W4).

The **pair-scorer escalation** stays the named long-term seam. An anchor-local rule cannot
catch a *pair-level* miscalibration (a single stranger the model scores like a cluster-mate,
the standing `encryption ↔ phishing` residue). A discovery-side pair-scorer would be a second
model seam, sibling of §5's reranker but distinct from it (that seam needs query text, and
`similar` has none). It would still only filter what is surfaced, never author a link. Under
always-serve that residue is ordering quality, not existence.

## 4. Retrieval: search, fusion, and discovery

Semantic search is in v1 (ADR-0019): exact, in-process, no vector extension, no approximate
nearest-neighbor index.

**Storage and scoring.** Vectors live in plain tables (`embeddings(text_hash, vector)` plus
per-note `note_centroids`), read with one statement and scored in-process (`embed::l2_sq`).
Content-addressing costs the scan one indexed join back to `chunks` to recover which chunk
each vector ranks for. A `vec0`-style virtual table charges a per-row shadow-table probe on
every scan, which dominates at real-vault scale; the plain-table scan does not
([GH #38](https://github.com/AlteredCraft/B2/issues/38), ADR-0006).

### Flow ②: search, from query to served rows

`search.rs`; the entry point is `Vault::search_evidence` (the CLI's `cmd_search` adds
`--exclude` via `search_evidence_excluding`; the desktop's `search` command is the same
read). The stages, in order:

1. **Fail fast on a model swap.** `ensure_query_space_matches` returns
   `Error::ModelMismatch` ("run `b2 reindex`") rather than fusing incomparable vectors (M2).
2. **Dispatch.** If the `embeddings` table does not exist, run `keyword_only_search`: the
   BM25 leg alone, same fusion, so scores stay on one scale. BM25-only is a smaller
   projection, never an error, and `best_cos` is honestly `None`.
3. **The keyword leg.** `fts5_query` sanitizes raw natural language into a safe FTS5 `MATCH`
   expression: each alphanumeric run double-quoted so nothing reads as an operator, OR-joined
   for recall. Punctuation is FTS5 syntax and would otherwise crash the parse. The index it
   matches is stemmed (`porter unicode61`, below).
4. **The semantic leg.** `embedder.embed_query` (bge's asymmetric query prefix), then
   `db::vector_search`: a full in-process scan, `embed::l2_sq` per chunk vector, nearest
   first.
5. **Provenance.** `provenance_of` records, per chunk, its BM25 rank, its vector rank, and
   its own distance, plus the query's best cosine: the absolute signals RRF is about to
   discard (D2).
6. **Fusion.** `rrf_fuse`: `score = Σ 1/(60 + rank + 1)` over both lists (`RRF_K = 60`). A
   fused-score tie breaks on the dense list's rank, and that is a policy, not walk order:
   RRF over integer ranks lands every fused score on a lattice, so mirrored rank pairs tie
   structurally, and the eval corpus produced one where the semantic half named the labelled
   answer and BM25 named the wrong one ([GH #156](https://github.com/AlteredCraft/B2/issues/156)).
   Absent ranks sort below present ones; id is last, purely for determinism.
7. **Resolve to notes.** `resolve_hits` / `resolve_note_hits`: dedup chunks onto each note's
   best one, cut a query-windowed snippet, stop at `limit`.
8. **The evidence verdict.** `lexical_evidence` reads IDF-weighted term coverage;
   `EvidenceBar::for_model` supplies the calibrated bar (or none);
   `vouched = coverage clears it OR best_cos clears it`. The result is a
   `SearchEvidenceView`: the rows, whole and in fused order (never reordered), plus
   `{vouched, chunk_total, terms, best_cos}`.

**The three verdict states are three behaviors** (D2, ADR-0015,
[GH #202](https://github.com/AlteredCraft/B2/issues/202)):

| Verdict | CLI human mode | CLI `--json` | Desktop |
|---|---|---|---|
| `vouched: true` (evidence found) | the rows | the object: rows + verdict | the rows |
| `vouched: false` (no evidence) | "No matches." and none of the rows. Strict: no reveal, no `--all` | the rows are served, beside `vouched: false`; an agent handed an explicit verdict can be honest about them | no rows kept (one boundary: `doSearch`, `ui/src/main.ts`) |
| `vouched: null` (no calibrated bar for the active model: the fake, or any unmeasured model) | serve as always; no verdict is offered rather than one guessed | same | same |

The verdict rule is *lexical OR semantic*: two independent signals, because one test cannot
tell "nothing here matches" from "everything here matches". Details and history: D2. The
lexical anchor is IDF-weighted term coverage (`min_term_coverage`): each term weighs
`ln((chunks+1)/(df+1))`, and the anchor is the share of the query's own weight the vault
carries. A stopword is a measurement, never a shipped word list. The cosine bar (`min_cos`)
is the backstop, judged only on queries the lexical half leaves undecided, and thin by
construction: ask the lexical half for more, and the window collapses, because the queries it
must then rescue have the weakest semantic evidence too. The two constants are placed inside
a joint band, never tuned one at a time. They are distributional, keyed to `embed_model_id`
(M2; a swap invalidates them; the device suffix shares the reading, re-checked by
`make eval-metal`). They live in `search::BGE_BASE_EVIDENCE_BAR`, and their justification is
re-derived on every `make eval` run, never frozen into a comment (ADR-0013).

**Width is a quality knob, not plumbing**
([GH #142](https://github.com/AlteredCraft/B2/issues/142)). Retrieval widens twice: each
façade read asks for a hit pool over its `limit`, and each signal (`search::pool_size`) pulls
5× *that* (minimum 30) before RRF fuses the two lists. Note-level `Vault::search` keeps 3×:
dedup collapses every chunk sharing a note onto that note's best one, so a pool of exactly
`limit` would under-fill `limit` distinct notes. Passage-level `Vault::search_chunks` (chat's
retrieve step) has no dedup and keeps `limit + 2`: enough to backfill the one hit a C1 torn
read can drop, and no more. Giving it 3× too is not a free tidy-up: the 5× multiplies every
hit of headroom into candidates, and RRF over a wider candidate set returns *different*
answers. At k = 60, a chunk ranked ~60th in both lists outscores one ranked first in a single
list (`2/121 > 1/61`). Width moves only on measured relevance (§5).

**`--exclude` subtracts rows, never evidence.** `search_evidence_excluding` is the same read
minus a caller-named set of notes: the follow-up-search form an agent loop passes its
already-inspected paths to. The verdict and its signals still read the whole vault, the
remaining rows keep their fused order, and the pool is unchanged, so a heavily excluded query
may honestly under-fill.

**The FTS tokenizer is `porter unicode61`, stemmed, and that is a measured verdict, not a
default.** Unstemmed BM25 matched surface forms only: `pedalling` found nothing in a note
that says "pedals", leaving the lexical half at rank 41 to 46 on queries the dense half
ranked first, which RRF's consensus bias turned into hybrid demotions of correct dense hits.
The A/B that settled it ([GH #157](https://github.com/AlteredCraft/B2/issues/157)): porter
improved 7 BM25-only and 3 hybrid note ranks, worsened none, and dissolved every standing
fusion demotion, while the precision probes built to vote *against* stemming (the
`universe`/`university` collision, the code-literal queries) did not move. Stemming remains a
real trade (Porter is English-only, and a vault holds code, identifiers, and proper nouns),
so the retired arm stays measurable: `Vault::rebuild_fts` swaps the tokenizer over identical
chunk rows and vectors, and `make eval-stemmer` scores the unstemmed ablation beside every
default run.

**The tail.** Folding where a *real* query's per-hit evidence runs out is measured and stays
unshipped ([GH #206](https://github.com/AlteredCraft/B2/issues/206)): with `tail_relevant`
labels in place, the four-family prefix-cut bake-off found every admissible rule near-vacuous.
The fused order is not an evidence order, so admissible folds reach 2 to 23 of the 367 rows an
oracle fold would cut. The ruling and its numbers live in [evals.md](evals.md); the bake-off
re-arms every run.

### Flow ③: similar, the two-stage discovery scan

`discover.rs` (`candidates`); the entry point is `Vault::similar`. Discovery makes zero model
calls and touches no network: it is a pure read over stored vectors, so the CLI can open the
vault with the fake embedder and still serve real-model rankings. The anchor is represented
by its *stored* chunk vectors, never an `embed_query` of its text (bge's asymmetric query
prefix is the wrong side of the space). The stages:

1. **Honest refusals.** A resource anchor is `ResourceUnsupported`; an unknown ref is
   `NoteNotFound`. The grade flag is read from the recorded model id
   (`db::recorded_embedder`): a fake-embedded space serves ungraded.
2. **Load the anchor.** Its stored chunk vectors (`db::note_chunk_vectors`), centroid
   computed in-process. Nothing is re-embedded.
3. **Exclude the already-connected.** `graph::reachable_within(anchor, 1)`: the anchor and
   its 1-hop neighbors are excluded *up front*, so they never occupy a shortlist slot. A
   2-hop note (the triadic-closure candidate: you linked A–B and B–E but never A–E) survives,
   ranked purely by semantic score.
4. **Stage 1: centroid shortlist, O(notes).** Stream every note centroid
   (`for_each_note_centroid`), keep `SHORTLIST_PER_RESULT = 20` per asked result, floored at
   `SHORTLIST_MIN = 200`. The shortlist is a recall device, never a quality gate: on any
   vault at or below 200 candidate notes, the two-stage result equals the whole-space scan
   ([GH #38](https://github.com/AlteredCraft/B2/issues/38)/[#192](https://github.com/AlteredCraft/B2/issues/192)).
5. **Stage 2: exact max-sim over the shortlist only.** Every shortlisted note scores by its
   single best chunk pair against the anchor (smallest L2²), and that winning chunk rides
   along as the evidence passage the surface shows. Max-sim, not the centroid, decides the
   score, so one strong section inside a messy note still counts (the buried gem;
   [GH #192](https://github.com/AlteredCraft/B2/issues/192)).
6. **Rank, grade, cap.** Ties break by path. The z is computed only when graded (pool ≥ 12,
   spread > 0, real model). `take(limit)`. The result is `CandidateNote`: path, title,
   `score = −√(best-pair d²)`, evidence, optional z. The z gates nothing; it paints the band
   (§3).

`b2 similar` surfaces, `b2 link` commits, and you are the precision gate (ADR-0009). An empty
list at a nonzero `limit` means only "nothing to compare": no unlinked note has stored
vectors yet, or the space is not semantic. The CLI's two empty states say exactly that.

**`graph_filtered_search`** is the near-neighbor of both flows that is neither: the
vector⨝graph scoped-traversal primitive, "nearest chunks whose note is within k typed hops of
an anchor" (near ∩ connected). Discovery is its complement (near ∖ connected).
`vector_only_search` is the eval harness's ablation instrument, never an adapter surface.

### Does brute force scale to B2?

Comfortably. A personal vault of 10k notes is roughly 50k to 100k chunks; brute-force cosine
over ~100k × 768-dim float32 vectors is single-digit to low-tens of milliseconds. We are
nowhere near the regime where an approximate index matters. If a vault ever is, int8/binary
quantization and ANN hold a standby order behind the centroid stage
([GH #106](https://github.com/AlteredCraft/B2/issues/106)).

### The constants in one place

Structural constants are design choices, quoted here. Distributional constants are
measurements keyed to the embedding model: named, never quoted. Their values live in the code
and their justification is re-derived on every `make eval` run (ADR-0013).

| Constant | Value | Governs | Source |
|---|---|---|---|
| `RRF_K` | 60 | fusion weighting (qmd heritage) | `search.rs` |
| `pool_size` | 5 × hit pool, min 30 | per-signal candidate depth | `search.rs` |
| note hit pool | 3 × limit | note-view headroom (dedup + torn reads) | `vault.rs` |
| chunk hit pool | limit + 2 | passage-view headroom (torn reads only) | `vault.rs` |
| `ASK_PASSAGES` | 10 | chat's retrieve depth (flow ④ reads this flow) | `chat.rs` |
| `SHORTLIST_PER_RESULT` | 20 | discovery stage-1 width per asked result | `discover.rs` |
| `SHORTLIST_MIN` | 200 | discovery stage-1 floor | `discover.rs` |
| `EXCLUDE_HOPS` | 1 | discovery's already-connected radius | `discover.rs` |
| `STATS_MIN_POPULATION` | 12 | smallest population a z is claimed over | `discover.rs` |
| `BGE_BASE_EVIDENCE_BAR` | named, not quoted | the lexical anchor + the semantic backstop, per model | `search.rs` |

## 5. Deferred model machinery: the reranker and query expansion

**The reranker is a fast follow.** Slot it where qmd puts it: after RRF fusion, before final
ranking, behind a swappable seam. v1 is retrieve → fuse → return top-N, already a strong
hybrid baseline; the fast follow inserts a cross-encoder rerank over the top ~30 candidates
plus a position-aware blend. It is a pure function `(query, candidates) → scores`, so it
changes *ordering*, not the store, the schema, or the candidate set. It is store-agnostic, a
model-side seam above the index. Tracked in
[GH #28](https://github.com/AlteredCraft/B2/issues/28).

**Scope: this reranks `b2 search`, not `b2 similar`.** The signature is the tell: it needs
query text, and `b2 similar` has none (it is passage↔passage KNN, "near ∖ connected", §3).
The discovery-side levers are distance weighting
([GH #20](https://github.com/AlteredCraft/B2/issues/20)) and the pair-scorer seam (§3); the
discovery-side precision stance is D1's (ADR-0014).

**Gate the decision on the eval, not intuition** (ADR-0013). RRF is a strong baseline; the
reranker buys top-k precision, whose value grows with vault size and is highest when an agent
consumes top-1/top-3 with no human eye
([GH #24](https://github.com/AlteredCraft/B2/issues/24)). Vault size changes whether the
precision is worth it, not the reranker's cost.

**And check the instrument can see the change you are gating.** Retrieval reaches at least
`chunk_candidate_pool(10) = 60` candidates per signal (150 for the note view). While a corpus
has no more chunks than that, neither signal is truncated, and a candidate-width change
prints bit-identical numbers while genuinely reordering a real vault
([GH #141](https://github.com/AlteredCraft/B2/issues/141)). Score *relevance* on the labelled
corpus, but measure a width change with `make stability` over a vault big enough for the pool
to bind. That gate has already ruled once:
[#140](https://github.com/AlteredCraft/B2/pull/140) widened the passage view as plumbing, the
eval printed bit-identical numbers, the probe found 10 of 10 top-4 lists changed, and
[#142](https://github.com/AlteredCraft/B2/issues/142) returned it. The instrument that can
say "different" is not the one that can say "better". The blindness is to *candidates*, not
to fusion: `RRF_K` re-weights the same two lists and reorders results at any corpus size.

**Query expansion** (qmd's third model) is optional and lowest priority: the heaviest model
for the smallest, most variable win
([GH #105](https://github.com/AlteredCraft/B2/issues/105)).

The harness itself (corpora, labels, metrics, exit gates, process rules) is
[evals.md](evals.md); read it before touching any of them.

## 6. The AI seams: the embedder, and grounded chat

Two seams, both enumerated by M1 (ADR-0005) and both injected by the adapters.

### `Embedder`

Runtime, provisioning, and the model choice are
[ADR-0020](../ADRs/0020-embeddings-inside-the-single-binary.md): `candle` + `hf-hub` compiled
into the binary, an explicit `b2 init` into a shared XDG cache, default
`BAAI/bge-base-en-v1.5` (768-dim, CLS-pooled, L2-normalized, bge's asymmetric query prefix),
the dimension read from the model's own `config.json`. Loading fails fast if the files are
absent ("run `b2 init`"), never a surprise mid-command download.

The trait is five methods: `model_id`, `dim`, `embed` (one passage), `embed_query`
(asymmetric-ready, defaults to symmetric), and `embed_batch`. The fake (`FakeEmbedder`, the
CI default) blake3-hashes text into a vector: identical text, identical vector,
deterministically. Not semantic; a stand-in so the fast suite stays model-free.

**Batching.** Embedding is the reindex hot path, so the write side hands whole batches of
chunks to `embed_batch` (up to `EMBED_BATCH = 16`). The real model turns a batch into one
padded forward pass instead of N single ones (a large CPU win; on macOS candle's matmuls run
on Apple's Accelerate BLAS). Right-padding plus the attention mask make each row's CLS vector
identical to embedding that text alone, so batching is a pure speedup, never a change in
result. That claim is pinned where it can actually run: `check_batch_matches_single` (cosine
> 0.9999, row for row) needs the provisioned model, so it lives in the eval harness and runs
on every `make eval` instead of sitting behind an `#[ignore]` nobody passes `--ignored` to.

**The model swap.** The engine-side consequences are invariants (M2, ADR-0007,
[GH #40](https://github.com/AlteredCraft/B2/issues/40)): the embedding space has exactly one
recorded identity, `meta.(embed_model_id, embed_dim)`, and the compute device folds into it
(a Metal build tags the id `@metal`). On a swap, `ensure_embedding_space` drops and recreates
the vector tables empty; every note is then "not fully embedded", so the incremental skip
(§3) re-embeds the whole vault on that reindex, automatically, no `--force` needed. Opening a
vault never mutates the vector space, so changing the configured model cannot silently wipe
vectors on a read command. A stale read fails fast: `search` compares the recorded model to
the active one and returns `ModelMismatch` ("run `b2 reindex`") rather than fusing
incomparable vectors. The fake stays the CI default, so model quality never enters the fast
suite (E2).

### `LlmProvider`: flow ④, grounded chat

Chat is a *reader* of this index and adds nothing to it (M1): no table, no cached response,
no `meta` row, session-only history (S4). That is the deliberate contrast with the embedder:
swapping chat models never touches the index, so a provider swap is a URL or config change.

`Vault::ask` (`chat.rs`) is five steps:

1. **Condense** (multi-turn only). A provider call rewrites the follow-up into a standalone
   query. On failure it degrades to the raw question, so this step can never break chat.
2. **Retrieve.** `search_chunks` at `ASK_PASSAGES = 10`: the §4 pipeline unchanged, BM25-only
   fallback included.
3. **Assemble.** The grounded system prompt plus numbered passages. Prompt assembly is core
   logic, not an adapter's.
4. **Stream.** Tokens flow up through the caller's callback, whose return value cancels at
   token granularity. Sync, no runtime.
5. **Cite.** `[n]` markers resolve to `(path, excerpt)` in the returned `AnswerView`. A
   hallucinated marker resolves to nothing, and the answer text is never rewritten.

Two properties follow from the seam's shape. Cancellation is returning early from a blocking
read loop, so no B2 crate starts an async runtime (ADR-0011): a cut stream is marked
cancelled, and what already arrived is rendered, never discarded, because a truncated answer
is not an error. And model output is untrusted content like any note (E5, ADR-0016),
enforced at the render surface rather than in the core.

## 7. Tech stack: Rust

The single-binary goal picked the language, not the engine; SQLite and FTS5 are
language-agnostic. See ADR-0019.

## 8. Risks and operational burden

**Chunk vs. note granularity.** Search is chunk-level; the typed graph is note-level.
`chunks.note_path` is the join, and search hits resolve up to notes for graph operations
(§3).

### The bill for a path-keyed graph under `[[path|title]]` links

Keying the graph by the vault-relative path (L1, ADR-0003) makes the stored key the same
thing you authored. The items below are the trade working as designed, not defects. They must
be budgeted, tested, and watched.

- **Write amplification on move.** The link *is* the path, so moving one note rewrites the
  inbound link text in every file that points at it: an N-file write. It is bounded and
  mechanical (the materialized edges name exactly which files and links to touch, Markdown
  first, then the index), but moving a heavily linked note is proportional to its backlink
  count, not O(1). The rewrite is transactional, so a partial move never half-updates the
  vault. The index side is one cascading `UPDATE` plus re-projection of the inbound sources,
  bounded by the same count. Only the exact files the graph names are touched: a
  prefix-sharing `[[foo-bar]]` is never rewritten when moving `foo`.
- **Out-of-band moves are identified, not repaired, and that is the scope decision.** A
  `git mv` or Finder move is, to a path-keyed index, a delete plus a create: the old path's
  rows prune, the new path projects fresh, and every inbound `[[oldpath]]` becomes a surfaced
  dangling edge (G5): authored text kept, `dst` NULL, healing by itself on the next pass if
  the target comes back. Nothing is silently dropped, and nothing is guessed at.
  Content-addressed vectors (M4) make it cheap as well as honest: the re-created note's chunk
  text hashes identically, so the delete-plus-create re-embeds nothing. Two repairs are
  future investigation in GH #170: a *proposed* hash-match repair (`notes.body_hash` and the
  resource `content_hash` are already stored, but a proposal must stay yours to accept, W4),
  and a `b2 watch` daemon observing renames live, rejected as a lifecycle and coordination
  surface that shrinks the gap rather than closing it.
- **Path ownership follows the filesystem, so there is nothing to reconcile.** A path names
  at most one file (the filesystem's guarantee), so a note deleted and recreated at the same
  path is that path's note, a Finder-duplicated note is two notes at two paths, and
  `db::upsert_note`'s `ON CONFLICT(path)` is the whole of the reconciliation. A note deleted
  with no replacement is reconciled by the whole-vault pass
  ([GH #31](https://github.com/AlteredCraft/B2/issues/31)): `project_vault` prunes every
  `notes` row whose path the walk did not see this run (aliases, chunks, FTS, centroid, and
  outgoing edges cascade; inbound links re-dangle when phase 2 re-derives edges against the
  pruned resolver), *except* rows whose file was skipped as unreadable: the walk *saw* that
  file, so evicting it would lie. Single-note ingest (`add`/`mv`/`write`) touches one note
  and never prunes. Orphaned vectors (hashes no chunk references after that pruning) are
  collected by the same pass: the only bookkeeping content-addressing adds.
- **A single unreadable file never fails the whole index.** A real vault holds the odd
  non-UTF-8 or permission-denied `.md`. Projection skips it (reported as a `skipped` entry
  with a short, file-level reason, surfaced by the CLI and the desktop) and indexes
  everything else, rather than aborting on one file it cannot read.
- **Derived-index consistency is a permanent invariant, not a one-time build.** W5, S3, and
  L1 are the tripwires. Every edit path (`b2 mv`, link delete, out-of-band reindex) has to
  preserve all three, or the graph silently diverges from the source of truth.
- **Committed edges are only ever authored, never inferred** (G1). Editing the vault can
  strand a connection (deleting an authored A→B link), but B2 only ever *surfaces* the
  consequence. It never silently rewrites an inbound file or an edge (W4).

(The stamped `b2id` made "two files, one identity" representable, and cost a collision
subsystem, a shadowed-copy panel, restamp notices, and a carve-out on S3. All of it went with
the stamp, ADR-0003. Nothing anomaly-shaped was ever *stored*, so nothing had to be migrated
away.)
