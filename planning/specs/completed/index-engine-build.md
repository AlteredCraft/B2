---
b2id: 01KWSRM7W17H7WJ3NAHBBZ0ACB
title: "B2 — Index Engine: Build Plan & Schema"
type: note
tags: [b2, index-engine, sqlite, fts5, sqlite-vec, schema, build-plan, spec]
created: 2026-06-29
status: implemented
---

# B2 — Index Engine: Build Plan & Schema

> **The build spec for the SQLite index engine.** Where [index-engine.md](../../index-engine.md) decides
> *why* we build our own SQLite store (and takes qmd as a reference, not a dependency), and
> [data-model.md](../../data-model.md) defines *what* a note and an edge are in plain Markdown, this doc is
> the precise *how*: exact table definitions, their relations, the data flows that must hold the locked
> invariants, and the build order. The schema here is a **derived projection** of
> [data-model.md](../../data-model.md) — it must satisfy that model, never the reverse. Everything below is
> **language-agnostic** (SQLite + FTS5 + `sqlite-vec`), which is exactly why it can be locked *before*
> the Rust/Go stack gate ([index-engine.md](../../index-engine.md) §7).

## 0. Scope & ground rules

**This doc owns:** the precise DDL, the relations between tables, the read/write data flows, and the
build sequence. **It does not own:** the *why* of SQLite ([index-engine.md](../../index-engine.md)), the
note/edge model it projects ([data-model.md](../../data-model.md)), or the language/packaging choice
([index-engine.md](../../index-engine.md) §6–§7).

Recap of the two tiers it sits in ([data-model.md](../../data-model.md)): **Markdown** is the single source
of truth; **`b2.sqlite`** (this doc) is a **disposable cache** of every queryable concern, with no
durable state outside the notes. The law that binds them, and the yardstick every flow below is measured
against:

> **`index = projection of (Markdown)`.** Drop `b2.sqlite`, re-scan the vault → a byte-identical index
> (the locked `full-reindex ≡ incremental-update` invariant). *(Through 2026-06-30 a durable `.b2/log/`
> event log was a second source of truth for review state; the LLM-relator cut removed it — see
> [data-model.md](../../data-model.md) §4.)*

**Conventions used throughout the DDL:**

- Identities (`b2id`, edge `id`) are **`TEXT`**; timestamps are **ISO-8601 `TEXT`** (`created`,
  `updated`, `indexed_at`, …). `mtime` is an integer epoch for fast change detection only.
- The DB is opened with `PRAGMA journal_mode = WAL` and `PRAGMA foreign_keys = ON`. `sqlite-vec` is
  statically linked (no runtime `load_extension`, removing the macOS friction noted in
  [index-engine.md](../../index-engine.md) §8).
- Every table is rebuildable from `Markdown`; nothing here is a source of truth.

---

## 1. The schema (precise DDL)

### 1.0 Engine meta — authoritative bookkeeping for the index itself

```sql
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- Canonical keys:
--   schema_version   migration gate — which B2 schema built this index
--   b2_version       build that wrote it (debug)
--   embed_model_id   the embedder behind chunks_vec (e.g. 'BAAI/bge-base-en-v1.5')
--   embed_dim        vector dimension — MUST equal the FLOAT[N] literal in chunks_vec (§1.2)
--   created_at       when the index was first built
-- A change to embed_model_id/embed_dim is a model swap → drop chunks_vec + full re-embed
-- (index-engine.md §8). meta is the only place that can detect it, so vectors never go silently stale.
```

### 1.1 Mirror of Markdown — projection of frontmatter (files remain the source of truth)

```sql
CREATE TABLE notes (
  b2id        TEXT PRIMARY KEY,        -- ULID from frontmatter; identity, never the path (data-model §1)
  path        TEXT NOT NULL UNIQUE,    -- vault-relative; the b2id⇄path resolver is this column + the PK
  type        TEXT NOT NULL,           -- OKF entity discriminator (data-model §1, §5)
  title       TEXT,                    -- frontmatter title / natural alias; NULL ⇒ derived at read, not stored
  description TEXT,
  created     TEXT,                    -- ISO-8601
  updated     TEXT,
  body_hash   TEXT NOT NULL,           -- content hash → the incremental-reindex dirty check (Flow ①)
  mtime       INTEGER,                 -- fs mtime → cheap pre-filter before hashing
  indexed_at  TEXT NOT NULL            -- when this row was last (re)projected
);
CREATE INDEX notes_type_idx ON notes(type);

CREATE TABLE note_aliases (            -- load-bearing: link-by-alias resolution + cold dangling-repair
  note_b2id TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,
  alias     TEXT NOT NULL,
  PRIMARY KEY (note_b2id, alias)
);
CREATE INDEX note_aliases_alias_idx ON note_aliases(alias);
-- Populated from frontmatter `aliases:` (data-model §1). The `title` is matched via notes.title; both
-- feed (a) resolving a link written against an alias and (b) the heuristic cold-reindex repair of a
-- dangling [[oldpath]] when there is no prior index continuity (user-stories Story 1; index-engine §8).
```

**The `path ↔ b2id` resolver is not a table** — it is `notes(b2id PK, path UNIQUE)` plus the alias
index. `path → b2id` = `SELECT b2id FROM notes WHERE path = ?`; `b2id → path` is the reverse; the
heuristic fallback for a dangling target scans `notes.title` / `note_aliases.alias`.

### 1.2 Derived: search — chunks + FTS5 (BM25) + sqlite-vec (KNN)

```sql
CREATE TABLE chunks (
  id           INTEGER PRIMARY KEY,    -- rowid; the join key shared by chunks_fts and chunks_vec
  note_b2id    TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL,       -- 0-based order within the note
  char_start   INTEGER NOT NULL,       -- offsets into the body (explain / highlight / future reuse check)
  char_end     INTEGER NOT NULL,
  token_count  INTEGER NOT NULL,
  heading_path TEXT,                   -- e.g. 'Relations > Evidence' — breadcrumb for explain (optional)
  text         TEXT NOT NULL,
  UNIQUE (note_b2id, seq)
);
CREATE INDEX chunks_note_idx ON chunks(note_b2id);
-- Chunking heuristic is borrowed wholesale from qmd (index-engine.md §1): size-targeted, ~15%
-- overlap, Markdown-aware boundary scoring. Chunk ids are NOT stable across a re-index (a note's chunks
-- are deleted + reinserted); that is fine because its FTS and vec rows are deleted with them.
--
-- BUILD NOTE (the qmd heuristic SHIPPED, #19 / specs/qmd-chunker.md, 2026-07-13): chunk.rs now sizes
-- chunks toward a ~450-token target (under bge's 512 truncation), cuts at the best-scoring Markdown break
-- within a backward scan, carries ~15% overlap, and stamps token_count (a chars/4 estimate) + heading_path
-- (the H1 › H2 › H3 breadcrumb, this column). It replaced the step-2 MINIMAL paragraph splitter, which was
-- deferred past step 5 because scoring paragraph-vs-qmd needs a real embedder + eval; every lever lives on
-- a `ChunkConfig` (target/overlap/proxy/backscan/weights/prepend). Swapping chunkers was a pure
-- re-projection (drop & rebuild) — no schema or invariant change, exactly as planned.

-- FTS5 over chunk text. external-content (content='chunks') stores the text once; BM25 ranking is built in.
CREATE VIRTUAL TABLE chunks_fts USING fts5(
  text,
  content      = 'chunks',
  content_rowid = 'id',
  tokenize     = 'unicode61'
);
-- Triggers keep the FTS index in lockstep with chunks so an incremental edit ≡ a full rebuild
-- (the locked invariant). external-content FTS requires the 'delete' sentinel on remove/update.
CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
  INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
  INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN
  INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;

-- sqlite-vec: brute-force KNN over float vectors (index-engine.md §4 — comfortable at vault scale).
-- The dimension is a DDL literal, so it is pinned to meta.embed_dim; a model change recreates this table.
CREATE VIRTUAL TABLE chunks_vec USING vec0(
  chunk_id  INTEGER PRIMARY KEY,       -- = chunks.id
  embedding FLOAT[768]                 -- 768 = bge-base-en-v1.5 (default) / EmbeddingGemma-300M
);
-- Scale levers if brute force ever stops being instant (index-engine.md §4): int8/binary quantization,
-- or a partition/auxiliary column on note_b2id for filtered KNN — both before reaching for ANN.
```

### 1.3 Derived: typed graph — every edge keyed by `b2id`, never path

```sql
CREATE TABLE edges (
  id            TEXT PRIMARY KEY,      -- derived deterministically from (src_id, dst_id|dst_path_raw, type, occ).
  src_id        TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,  -- the authoring note; always exists
  dst_id        TEXT,                  -- resolved b2id, or NULL when the link is dangling (soft ref by design)
  dst_path_raw  TEXT NOT NULL,         -- the literal [[path]] as written — the anchor for move-rewrite + repair
  type          TEXT NOT NULL,         -- relation verb (data-model §2); 'references' for a bare wikilink
  origin        TEXT NOT NULL CHECK (origin IN ('inline','frontmatter')),
  explanation   TEXT,                  -- trailing text after — / :
  occurrence_index INTEGER NOT NULL DEFAULT 0,  -- completes data-model §2's authored-identity tuple;
                                                -- lets one note link the same target+verb twice.

  -- Every edge is authored and active — there is no lifecycle and no `status` column (data-model §3, §4).
  -- origin: 'inline' = body link/typed-line (human); 'frontmatter' = relations: entry (committed via
  -- `b2 link`, or human/importer-authored). An edge exists iff it is written in the Markdown.
  UNIQUE (src_id, dst_id, type, occurrence_index)   -- the authored-identity tuple as a constraint
);
CREATE INDEX edges_src_idx      ON edges(src_id);
CREATE INDEX edges_dst_type_idx ON edges(dst_id, type);                       -- b2 neighbors [--type] / backlinks
CREATE INDEX edges_dangling_idx ON edges(dst_path_raw) WHERE dst_id IS NULL; -- the repair sweep
-- dst_id is a SOFT reference (nullable, no enforced FK): a link may be dangling, and deleting a target
-- must be allowed. When a target is deleted, its inbound edges re-project to dst_id = NULL (dangling) —
-- not auto-removed. src_id IS a hard FK with CASCADE: deleting a note removes its outbound edges.
--
-- DEFERRED column — dst_alias_raw TEXT: the literal |title as written. Only powers the cosmetic
-- alias-refresh on a title rename (user-stories Story 1, display-only). A move preserves the existing
-- alias verbatim by re-parsing the source link, so it is not needed for correctness. Add it the day we
-- ship alias-refresh.
```

### 1.4 Caches — purely disposable

```sql
CREATE TABLE llm_cache (
  key     TEXT PRIMARY KEY,            -- hash(model_id ∥ task ∥ input)
  value   TEXT NOT NULL,               -- cached rerank scores (reserved for the reranker fast-follow, §5)
  created TEXT NOT NULL
);
```

> **Removed 2026-07-04:** the `edge_provenance` table (the log-derived review queue holding a pending
> suggestion's `by`/`source`/`confidence`). With the LLM relator and its suggestion lifecycle cut, there
> is no review queue and no edge provenance — a committed edge is a pristine authored line
> ([data-model.md](../../data-model.md) §4).

### 1.5 Deliberately deferred (out of v1, no invariant depends on them)

- **`dst_alias_raw`** (edges column, §1.3) — cosmetic alias-refresh only. **Tracked:
  [#29](https://github.com/AlteredCraft/B2/issues/29).**
- **`note_bodies(note_b2id, content)`** — a cache of each file's text; re-readable from disk anytime.
- **`notes.frontmatter_json`** — a blob of all frontmatter; round-trip losslessness is owned by the file
  parser, so the DB only needs the queryable columns above.
- **`note_tags(note_b2id, tag)`** — fast structured `--tag` filtering; FTS already covers tag *text*.

*(The other deferred work this doc names is also issue-tracked: the qmd chunker upgrade — §1.2 build
note — is [#19](https://github.com/AlteredCraft/B2/issues/19); the reranker + query-expansion seam —
Flow ② / §1.4's reserved `llm_cache` — is [#28](https://github.com/AlteredCraft/B2/issues/28). The
`note_bodies`/`frontmatter_json`/`note_tags` caches and §1.2's KNN scale levers are contingencies, not
planned work — add them the day a feature or a measurement demands them.)*

---

## 2. Relations (ERD)

```
                         ┌────────────────────────┐
                         │         notes          │   resolver = b2id ⇄ path (UNIQUE)
                         │  b2id PK · path UNIQUE  │
                         └───────────┬────────────┘
         ┌──────────────┬───────────┼───────────────┬───────────────────────┐
         │1:N           │1:N        │ 1:N (src, hard FK CASCADE)             │1:N (dst, soft / nullable)
         ▼              ▼           ▼                                        ▼
 ┌───────────────┐ ┌──────────┐  ┌──────────────────────────────────────────────────┐
 │ note_aliases  │ │  chunks  │  │                      edges                        │
 └───────────────┘ │  id PK   │  │ src_id → notes.b2id (FK) · dst_id ~→ b2id (null)  │
                   └────┬─────┘  │ id PK · origin ∈ {inline, frontmatter}           │
              ┌─────────┴────────┐└──────────────────────────────────────────────────┘
       (rowid)│                  │(chunk_id)
        ┌──────────┐       ┌─────────────┐
        │chunks_fts│ ext-  │  chunks_vec │
        │  FTS5    │content│ vec0 f32[768]│
        └──────────┘       └─────────────┘

   meta(key,value) · llm_cache(key,value)  — standalone bookkeeping / cache
```

- **`notes`** is the hub; `note_aliases`, `chunks`, and `edges.src_id` cascade-delete with it.
- **`chunks` ⇄ `chunks_fts`/`chunks_vec`** join on the chunk rowid; the FTS triggers keep all three
  consistent under incremental edits.
- **`edges.dst_id`** is intentionally *not* a hard FK — that is what lets a target be deleted and a link
  dangle (re-projected to `NULL`) instead of blocking the delete.
- Every `edges` row derives from Markdown (`origin ∈ {inline, frontmatter}`); there is no review-queue
  side table.

---

## 3. Data flow (the five paths that have to hold the invariants)

### ① Ingest / re-index one note — asserts *round-trip losslessness* + *incremental ≡ full*

```
file.md → parse → (frontmatter, body)
  ├─ b2id missing? → stamp → WRITE file (the one always-allowed write) → log: b2id.stamped
  ├─ upsert notes + replace note_aliases        [dirty-check: mtime then body_hash; unchanged ⇒ skip]
  ├─ body → md-aware chunk → chunks(seq, offsets, text)        (FTS auto-synced by triggers)
  │                              └→ embedder SEAM → chunks_vec[chunk_id]   (deterministic fake in tests)
  └─ authored edges → resolve path→b2id → upsert edges:
        ├─ BODY links/typed-lines      → origin=inline
        └─ FRONTMATTER relations:       → origin=frontmatter
        unresolved → dst_id = NULL, keep dst_path_raw   (flagged for repair)
        dedup: same (src,dst,type) in both homes → inline wins, drop the FM dup (data-model §0, §3)
```

Re-projecting one note refreshes **all its edges** (delete those for `src_id`, re-derive from the current
Markdown — both body and `relations:`) and its chunks. Every edge is Markdown-derived, so a one-note
re-parse is exactly a one-note slice of a full rebuild — which is what makes "incremental ≡ full" hold.

> **Split into two passes (2026-07-07,
> [projection-embedding-split.md](projection-embedding-split.md)):** over the whole vault, the model-free
> steps above (notes + chunks + FTS + edges) and the embed step are now **separately invokable** —
> `Vault::project` and `Vault::embed` — with the fused `reindex` remaining their composition. The embed
> pass derives its pending set from the DB (chunks with no `chunks_vec` row) rather than an in-memory
> hand-off. No change to the invariant or the two-phase link resolution; a projected-but-unembedded index
> is a *smaller* projection (keyword + graph complete), never a wrong one.

### ② Hybrid retrieval — reranker is a clean post-fusion seam (fast-follow)

```
query ─ (opt) expansion SEAM ─┐ llm_cache
   ┌─ BM25 over chunks_fts ────┼─► RRF fuse (Σ 1/(k+rank), k=60) ─► top-N
   └─ KNN  over chunks_vec ────┘                     │
        ▲ embed(query) via embedder SEAM             ▼ (fast-follow) rerank SEAM → position-aware blend
   (opt) graph filter: JOIN edges  ◄── "nearest chunks within k typed hops of note X"
        → resolve chunk_id → note_b2id → results (+ --explain)
```

RRF formula/`k`, the position-aware blend, and the asymmetric query/document prompt discipline (the
concrete prefix is the model's own — B2 ships bge's) are borrowed from qmd
([index-engine.md](../../index-engine.md) §1, §5). The reranker and query expansion are deferred behind
seams — they change *ordering*, not the store or the candidate set.

### ③ Commit a connection (`b2 link`) — B2 never authors the body

```
b2 link <src> <dst> [--type <verb>=references] [--explanation …]
  1. resolve <src> + <dst> (path or b2id) → src b2id + dst's CURRENT path + title → [[path|title]]
  2. APPEND  - "<verb> [[path|title]] — <explanation>"  to the SRC note's
     frontmatter relations:   (Markdown FIRST; NEVER the body — data-model §0)
  3. re-project the SRC note (Flow ①); the edge materializes as origin='frontmatter'
     straight from the relations: entry just written — a projection of an authored line.
```

This is why every edge always traces to an authored line in the file (body or frontmatter), never to a
bespoke index row — keeping `index = projection of (Markdown)` exact. There is no queue, no status flip,
and no provenance: a committed edge is pristine ([data-model.md](../../data-model.md) §4). `--type` defaults
to `references`; its palette is the core vocabulary (data-model §2).

### ④ Move / rename — *rename keeps every backlink resolving*

```
b2 mv old/path.md → new/path.md            (b2id unchanged ⇒ ZERO edge rows change identity)
  1. update notes.path (+ the resolver)
  2. inbound = SELECT src_id, id FROM edges WHERE dst_id = <moved b2id>
  3. for each inbound src: in that file, find the link whose literal path = dst_path_raw and
        WRITE [[oldpath|title]] → [[newpath|title]]     (Markdown FIRST; the |title alias is preserved verbatim)
  4. reconcile index (re-project touched files → dst_path_raw refreshed; no edge identity changes)
```

A **title-only** rename leaves `path` resolving, so every backlink still works with **no rewrite**;
refreshing the stale `|title` alias is the deferred cosmetic path (needs `dst_alias_raw`, §1.3). An
**out-of-band** move (Finder/`git mv`) is repaired on the next reindex via the frontmatter `b2id`, with
the cold-no-prior-state caveat from [user-stories.md](../../user-stories.md) Story 1 (dangling links are
*flagged*, not dropped).

### ⑤ Drop & rebuild — the disposability proof; `index = projection of (Markdown)`

```
rm b2.sqlite
  └─ scan vault: each .md → Flow ①         (rebuilds notes/aliases/chunks/fts/vec + every edge:
                                            inline from body, frontmatter from relations:)
  ⇒ a byte-for-byte identical index.
```

There is nothing else to replay: every queryable row is a projection of the Markdown, so re-scanning the
vault reproduces the index exactly. This is the strongest form of the disposable-index tenet — no durable
state lives anywhere but your notes.

---

## 4. Build plan (expands [tasks.md](../../tasks.md)'s 5 steps; each ends on a golden-vault assertion)

The **language gate** (Rust vs Go, [index-engine.md](../../index-engine.md) §7) precedes writing engine
code, but the schema and flows above are language-neutral and can be locked now.

| # | Step | Tables it lands | Seam | Green-scenario assertion ([data-model.md](../../data-model.md) §8) |
|---|------|-----------------|------|------------------|
| **0** | DB skeleton & migrations | `meta` | — | open→reopen stable; WAL + `foreign_keys=ON`; `sqlite-vec` links; `schema_version` gate seeded |
| **1** | Vault parse/serialize + resolver | `notes`, `note_aliases` | — | golden vault round-trips byte-identical; a missing `b2id` is stamped + logged; resolver maps `memory ⇄ path` both ways |
| **2** | Markdown-derived tables (**minimal paragraph chunker**, §1.2 build note) | `chunks` (+`chunks_fts`), `edges` | — | golden graph: `references` + `elaborates` spaced-rep→memory (inline/active); a bare link ⇒ `references`; `neighbors memory` = {referenced-by, elaborated-by}; one-note re-index ≡ full |
| **3** | sqlite-vec + embedder seam | `chunks_vec` | **embedder** (fake→real) | deterministic fake vectors → reproducible KNN; `embed_model_id`/`embed_dim` recorded; real local model deferred behind the seam (§6 of index-engine.md) |
| ~~**4**~~ | ~~`.b2/` event log + replay~~ **— built, then removed 2026-07-04** | — | — | the log tier + replay were cut with the LLM relator; there is no suggestion queue ([data-model.md](../../data-model.md) §4) |
| **5** | Hybrid retrieval (on the **minimal** chunker; qmd-chunker upgrade **deferred** past step 5 — §1.2 build note) | reads fts+vec (no new tables; `llm_cache` defers with the reranker) | **reranker** (fast-follow), expansion (off) | hybrid beats either alone on a fixture query set; RRF `k=60`; the graph-filtered retrieval join works |

Steps 1–2 establish the Markdown-derived tables; there is no log-derived tier (step 4 was removed
2026-07-04), so `index = projection of (Markdown)` asserts on the Markdown alone. The embedder (step 3)
and reranker (step 5) are the two model seams — both exercised with deterministic fakes so the plumbing
is provable with no live model ([vision-and-scope.md](../../vision-and-scope.md), testability stack).

---

## 5. Decisions baked into this schema (resolved 2026-06-29; acceptance home revised 2026-06-30; review layer removed 2026-07-04)

- **Committing writes frontmatter — not the body (Decision 1, 2026-06-30) — and is a re-projection, not
  a bespoke index write.** B2 never authors the body; `b2 link` appends the typed-link string to the
  source note's frontmatter `relations:`, and the edge materializes as `origin='frontmatter'` from that
  Markdown (Flow ③). The graph is the **union** of body (`inline`) + frontmatter relations
  (`frontmatter`), deduped **inline-wins** on overlap (Flow ①). Mirrored into
  [data-model.md](../../data-model.md) §0, §2–§4, §7 and [index-engine.md](../../index-engine.md) §3.
- **Schema additions over [index-engine.md](../../index-engine.md) §3's sketch (#2), adopted:** `meta`
  (model/dim/version), `note_aliases` (resolution + cold repair), `occurrence_index` (the data-model §2
  identity tuple), and `dst_path_raw` (the move-rewrite + dangling-repair anchor). **Deferred:**
  `dst_alias_raw` and the `note_bodies` / `frontmatter_json` / `note_tags` caches (§1.5) — no invariant
  depends on them.
- **`dst_id` is a soft, nullable reference; `src_id` is a hard FK with CASCADE (#4).** Targets can be
  deleted (inbound edges re-project to dangling) and links can dangle, while deleting an authoring note
  cleanly removes its outbound edges.
- **Edge `id` (#3):** every edge is authored, so its `id` derives deterministically from `(src_id,
  dst_id | dst_path_raw, type, occurrence_index)` — no minted ULIDs, no provenance side-table.
- **Review layer removed (2026-07-04).** The `edge_provenance` table, the `status` column, the
  `origin='suggested'` value, the origin/status `CHECK`, and the `.b2/log/` replay are all gone with the
  LLM relator. What remains is a pure projection of Markdown ([data-model.md](../../data-model.md) §4).

> **Built.** The language gate resolved to Rust and steps 0→5 all shipped against the golden-vault
> fixtures (step 4 was built, then removed with the 2026-07-04 relator cut); the split of Flow ① into
> `project` + `embed` (2026-07-07) is specced in
> [projection-embedding-split.md](projection-embedding-split.md). The schema and flows in this
> doc remain the contract the code implements; the deferred items are issue-tracked (§1.5).
