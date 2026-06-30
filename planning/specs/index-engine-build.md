---
title: "B2 — Index Engine: Build Plan & Schema"
type: note
tags: [b2, index-engine, sqlite, fts5, sqlite-vec, schema, build-plan, spec]
created: 2026-06-29
status: draft
---

# B2 — Index Engine: Build Plan & Schema

> **The build spec for the SQLite index engine.** Where [index-engine.md](../index-engine.md) decides
> *why* we build our own SQLite store (and takes qmd as a reference, not a dependency), and
> [data-model.md](../data-model.md) defines *what* a note and an edge are in plain Markdown, this doc is
> the precise *how*: exact table definitions, their relations, the data flows that must hold the locked
> invariants, and the build order. The schema here is a **derived projection** of
> [data-model.md](../data-model.md) — it must satisfy that model, never the reverse. Everything below is
> **language-agnostic** (SQLite + FTS5 + `sqlite-vec`), which is exactly why it can be locked *before*
> the Rust/Go stack gate ([index-engine.md](../index-engine.md) §7).

## 0. Scope & ground rules

**This doc owns:** the precise DDL, the relations between tables, the read/write data flows, and the
build sequence. **It does not own:** the *why* of SQLite ([index-engine.md](../index-engine.md)), the
note/edge/provenance model it projects ([data-model.md](../data-model.md)), or the language/packaging
choice ([index-engine.md](../index-engine.md) §6–§7).

Recap of the three tiers it sits in ([data-model.md](../data-model.md)): **Markdown** is the source of
truth for *knowledge*; **`b2.sqlite`** (this doc) is a **disposable cache** of every queryable concern;
the **`.b2/log/`** event log is the durable source of truth for *history* + review state. The law that
binds them, and the yardstick every flow below is measured against:

> **`index = projection of (Markdown ∪ log)`.** Drop `b2.sqlite`, re-scan the vault, replay the log →
> a byte-identical index (the locked `full-reindex ≡ incremental-update` invariant).

**Conventions used throughout the DDL:**

- Identities (`b2id`, edge `id`) are **`TEXT`**; timestamps are **ISO-8601 `TEXT`** (`created`,
  `updated`, `indexed_at`, …). `mtime` is an integer epoch for fast change detection only.
- The DB is opened with `PRAGMA journal_mode = WAL` and `PRAGMA foreign_keys = ON`. `sqlite-vec` is
  statically linked (no runtime `load_extension`, removing the macOS friction noted in
  [index-engine.md](../index-engine.md) §8).
- Every table is rebuildable from `(Markdown ∪ log)`; nothing here is a source of truth.

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
--   embed_model_id   the embedder behind chunks_vec (e.g. 'embeddinggemma-300m')
--   embed_dim        vector dimension — MUST equal the FLOAT[N] literal in chunks_vec (§1.2)
--   log_seq          last .b2/log sequence applied → enables incremental replay (Flow ⑤)
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
-- Chunking heuristic is borrowed wholesale from qmd (index-engine.md §1): ~900-token chunks, ~15%
-- overlap, Markdown-aware boundary scoring. Chunk ids are NOT stable across a re-index (a note's chunks
-- are deleted + reinserted); that is fine because its FTS and vec rows are deleted with them.
--
-- BUILD NOTE (step 2 ships a MINIMAL chunker): the first cut is a paragraph splitter (maximal runs of
-- non-blank lines), token_count = whitespace word count, heading_path left NULL. It populates this exact
-- schema and exercises the FTS triggers; the qmd heuristic above is folded in at STEP 5, when hybrid
-- retrieval quality is first measured and a better chunker can actually be scored. Swapping it is a pure
-- re-projection (drop & rebuild) — no schema or invariant change — so deferring it costs nothing.

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
  embedding FLOAT[768]                 -- 768 = EmbeddingGemma-300M / Qwen3-Embedding-0.6B default
);
-- Scale levers if brute force ever stops being instant (index-engine.md §4): int8/binary quantization,
-- or a partition/auxiliary column on note_b2id for filtered KNN — both before reaching for ANN.
```

### 1.3 Derived: typed graph — every edge keyed by `b2id`, never path

```sql
CREATE TABLE edges (
  id            TEXT PRIMARY KEY,      -- suggested: ULID carried IN the log event (replay-stable).
                                       -- authored: derived from (src_id, dst_id|dst_path_raw, type, occ).
  src_id        TEXT NOT NULL REFERENCES notes(b2id) ON DELETE CASCADE,  -- the authoring note; always exists
  dst_id        TEXT,                  -- resolved b2id, or NULL when the link is dangling (soft ref by design)
  dst_path_raw  TEXT NOT NULL,         -- the literal [[path]] as written — the anchor for move-rewrite + repair
  type          TEXT NOT NULL,         -- relation verb (data-model §2); 'references' for a bare wikilink
  origin        TEXT NOT NULL CHECK (origin IN ('inline','frontmatter','suggested')),
  status        TEXT NOT NULL CHECK (status IN ('active','suggested','rejected')),
  explanation   TEXT,                  -- trailing text after — / : , or an agent's rationale
  occurrence_index INTEGER NOT NULL DEFAULT 0,  -- completes data-model §2's authored-identity tuple;
                                                -- lets one note link the same target+verb twice. =0 for suggestions.

  -- origin/status coupling (data-model §3, refined): authored edges are always active; a suggested edge
  -- is only ever suggested or rejected. There is NO 'suggested + active' row — acceptance re-projects the
  -- edge as origin='inline' from the Markdown (see Flow ③). This CHECK makes that a hard guarantee, so a
  -- bug can never leak an un-accepted edge into the live (status='active') graph.
  CHECK ( (origin = 'suggested'                AND status IN ('suggested','rejected'))
       OR (origin IN ('inline','frontmatter')  AND status = 'active') ),

  UNIQUE (src_id, dst_id, type, occurrence_index)   -- the authored-identity tuple as a constraint
);
CREATE INDEX edges_src_idx      ON edges(src_id);
CREATE INDEX edges_dst_type_idx ON edges(dst_id, type);                       -- b2 neighbors [--type]
CREATE INDEX edges_status_idx   ON edges(status);                            -- the live review queue
CREATE INDEX edges_dangling_idx ON edges(dst_path_raw) WHERE dst_id IS NULL; -- the repair sweep
-- dst_id is a SOFT reference (nullable, no enforced FK): a link may be dangling, and deleting a target
-- must be allowed. When a target is deleted, its inbound edges re-project to dst_id = NULL (dangling) —
-- not auto-removed. src_id IS a hard FK with CASCADE: deleting a note removes its outbound edges
-- (including any suggestions about it, which are then moot).
--
-- DEFERRED column — dst_alias_raw TEXT: the literal |title as written. Only powers the cosmetic
-- alias-refresh on a title rename (user-stories Story 1, display-only). A move preserves the existing
-- alias verbatim by re-parsing the source link, so it is not needed for correctness. Add it the day we
-- ship alias-refresh.
```

### 1.4 Derived from the log: the review queue — replayed, never authored here

```sql
CREATE TABLE edge_provenance (
  edge_id    TEXT PRIMARY KEY REFERENCES edges(id) ON DELETE CASCADE,
  by         TEXT NOT NULL,            -- 'human' | 'agent:<model-id>'   (data-model §4)
  source     TEXT,                     -- the evidence signal (free text) — fuel for the accept/reject call
  confidence REAL,                     -- 0.0–1.0, for triaging the queue
  created    TEXT NOT NULL,            -- when the suggestion was generated
  decided    TEXT                      -- when accepted/rejected; NULL while pending
);
-- Exists ONLY for edges with status IN ('suggested','rejected'). An accepted edge is inline + pristine
-- in Markdown and carries NO row here — its history (who proposed it, confidence, when) lives in the log
-- alone (data-model §4). This table is rebuilt purely by replaying .b2/log; it is never written directly.
```

### 1.5 Caches — purely disposable

```sql
CREATE TABLE llm_cache (
  key     TEXT PRIMARY KEY,            -- hash(model_id ∥ task ∥ input)
  value   TEXT NOT NULL,               -- cached query-expansion variants / rerank scores
  created TEXT NOT NULL
);
```

### 1.6 Deliberately deferred (out of v1, no invariant depends on them)

- **`dst_alias_raw`** (edges column, §1.3) — cosmetic alias-refresh only.
- **`note_bodies(note_b2id, content)`** — a cache of each file's text; re-readable from disk anytime.
- **`notes.frontmatter_json`** — a blob of all frontmatter; round-trip losslessness is owned by the file
  parser, so the DB only needs the queryable columns above.
- **`note_tags(note_b2id, tag)`** — fast structured `--tag` filtering; FTS already covers tag *text*.

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
                   └────┬─────┘  │ id PK                                            │
              ┌─────────┴────────┐└───────────────────────┬──────────────────────────┘
       (rowid)│                  │(chunk_id)         1:1   │ (suggested / rejected only)
        ┌──────────┐       ┌─────────────┐                ▼
        │chunks_fts│ ext-  │  chunks_vec │          ┌────────────────┐
        │  FTS5    │content│ vec0 f32[768]│         │ edge_provenance│
        └──────────┘       └─────────────┘          └────────────────┘

   meta(key,value) · llm_cache(key,value)  — standalone bookkeeping / cache
```

- **`notes`** is the hub; `note_aliases`, `chunks`, and `edges.src_id` cascade-delete with it.
- **`chunks` ⇄ `chunks_fts`/`chunks_vec`** join on the chunk rowid; the FTS triggers keep all three
  consistent under incremental edits.
- **`edges.dst_id`** is intentionally *not* a hard FK — that is what lets a target be deleted and a link
  dangle (re-projected to `NULL`) instead of blocking the delete.
- **`edge_provenance`** hangs off only the suggested/rejected edges; accepted (inline) edges have none.

---

## 3. Data flow (the five paths that have to hold the invariants)

### ① Ingest / re-index one note — asserts *round-trip losslessness* + *incremental ≡ full*

```
file.md → parse → (frontmatter, body, links)
  ├─ b2id missing? → stamp → WRITE file (the one always-allowed write) → log: b2id.stamped
  ├─ upsert notes + replace note_aliases        [dirty-check: mtime then body_hash; unchanged ⇒ skip]
  ├─ body → md-aware chunk → chunks(seq, offsets, text)        (FTS auto-synced by triggers)
  │                              └→ embedder SEAM → chunks_vec[chunk_id]   (deterministic fake in tests)
  └─ links → resolve path→b2id → upsert edges(origin=inline|frontmatter, status=active)
                       └─ unresolved → dst_id = NULL, keep dst_path_raw   (flagged for repair)
```

Re-projecting one note refreshes **only its `origin IN ('inline','frontmatter')` edges** (delete those
for `src_id`, re-derive from the current Markdown) and its chunks. **`suggested`/`rejected` rows are
log-derived and are left untouched by a Markdown re-parse** — that separation is what makes
"incremental ≡ full" hold without a re-parse clobbering the review queue.

### ② Hybrid retrieval — reranker is a clean post-fusion seam (fast-follow)

```
query ─ (opt) expansion SEAM ─┐ llm_cache
   ┌─ BM25 over chunks_fts ────┼─► RRF fuse (Σ 1/(k+rank), k=60) ─► top-N
   └─ KNN  over chunks_vec ────┘                     │
        ▲ embed(query) via embedder SEAM             ▼ (fast-follow) rerank SEAM → position-aware blend
   (opt) graph filter: JOIN edges  ◄── "nearest chunks within k typed hops of note X"
        → resolve chunk_id → note_b2id → results (+ --explain)
```

RRF formula/`k`, the position-aware blend, and the EmbeddingGemma prompt format are borrowed from qmd
([index-engine.md](../index-engine.md) §1, §5). The reranker and query expansion are deferred behind
seams — they change *ordering*, not the store or the candidate set.

### ③ Accept a suggestion — makes *inert-until-accepted* literal (the #5 refinement)

```
b2 link --accept <edge_id>
  1. WRITE "- <type> [[path|title]] — <explanation>" into the SRC note body     (Markdown FIRST)
  2. log: suggestion.accepted {edge_id, by, confidence, evidence}
  3. reconcile index: re-project the SRC note (Flow ①)
        → the suggested row LEAVES the queue; the edge re-materializes as origin='inline'/status='active'
          straight from the Markdown just written — NOT an in-place status flip.
        → its edge_provenance row is gone; the accepted edge is pristine, history lives only in the log.
```

This is why an `active` edge always traces to a body link, never to a mutated queue row — keeping
`index = projection of (Markdown ∪ log)` exact (the CHECK in §1.3 enforces it).

### ④ Move / rename — *rename keeps every backlink resolving*

```
b2 mv old/path.md → new/path.md            (b2id unchanged ⇒ ZERO edge rows change identity)
  1. update notes.path (+ the resolver)
  2. inbound = SELECT src_id, id FROM edges WHERE dst_id = <moved b2id>
  3. for each inbound src: in that file, find the link whose literal path = dst_path_raw and
        WRITE [[oldpath|title]] → [[newpath|title]]     (Markdown FIRST; the |title alias is preserved verbatim)
  4. log: note.moved + link.rewritten_on_move (old→new, per file)
  5. reconcile index (re-project touched files → dst_path_raw refreshed; no edge identity changes)
```

A **title-only** rename leaves `path` resolving, so every backlink still works with **no rewrite**;
refreshing the stale `|title` alias is the deferred cosmetic path (needs `dst_alias_raw`, §1.3). An
**out-of-band** move (Finder/`git mv`) is repaired on the next reindex via the frontmatter `b2id`, with
the cold-no-prior-state caveat from [user-stories.md](../user-stories.md) Story 1 (dangling links are
*flagged*, not dropped).

### ⑤ Drop & rebuild — the disposability proof; `index = projection of (Markdown ∪ log)`

```
rm b2.sqlite
  ├─ scan vault: each .md → Flow ①              (rebuilds notes/aliases/chunks/fts/vec + inline edges)
  └─ replay .b2/log from seq 0:
        suggestion.generated → edges(origin=suggested, status=suggested) + edge_provenance
        suggestion.rejected  → edges(status=rejected)        (tombstone: same src,dst,type never re-proposed)
        suggestion.accepted  → NO-OP for the queue (its inline edge already came from Markdown in Flow ①)
        b2id.stamped / note.* / link.rewritten_on_move → history only; not replayed into queryable state
  ⇒ a byte-for-byte identical index.
```

Replaying an **accepted** suggestion adds no live queue row, so it is never double-counted as both a
queue row and an `active` edge — the reason Flow ③ removes the row rather than flipping it.

---

## 4. Build plan (expands [tasks.md](../tasks.md)'s 5 steps; each ends on a golden-vault assertion)

The **language gate** (Rust vs Go, [index-engine.md](../index-engine.md) §7) precedes writing engine
code, but the schema and flows above are language-neutral and can be locked now.

| # | Step | Tables it lands | Seam | Green-scenario assertion ([data-model.md](../data-model.md) §8) |
|---|------|-----------------|------|------------------|
| **0** | DB skeleton & migrations | `meta` | — | open→reopen stable; WAL + `foreign_keys=ON`; `sqlite-vec` links; `schema_version` gate seeded |
| **1** | Vault parse/serialize + resolver | `notes`, `note_aliases` | — | golden vault round-trips byte-identical; a missing `b2id` is stamped + logged; resolver maps `memory ⇄ path` both ways |
| **2** | Markdown-derived tables (**minimal paragraph chunker**, §1.2 build note) | `chunks` (+`chunks_fts`), `edges` | — | golden graph: `references` + `elaborates` spaced-rep→memory (inline/active); a bare link ⇒ `references`; `neighbors memory` = {referenced-by, elaborated-by}; one-note re-index ≡ full |
| **3** | sqlite-vec + embedder seam | `chunks_vec` | **embedder** (fake→real) | deterministic fake vectors → reproducible KNN; `embed_model_id`/`embed_dim` recorded; real local model deferred behind the seam (§6 of index-engine.md) |
| **4** | `.b2/` event log + replay | `edges` (suggested/rejected), `edge_provenance` | log sink | the inert suggestion shows in `b2 suggest`, absent from every file on disk; drop→replay reproduces the queue (Flow ⑤); a rejection tombstone blocks re-proposal |
| **5** | Hybrid retrieval **+ upgrade chunker to the qmd heuristic** (§1.2) | reads fts+vec; `llm_cache` | **reranker** (fast-follow), expansion (off) | hybrid beats either alone on a fixture query set; RRF `k=60`; the graph-filtered retrieval join works |

Steps 1–2 establish the Markdown-derived tiers; step 4 layers the log-derived review queue on top; the
two together are what `index = projection of (Markdown ∪ log)` asserts. The embedder (step 3) and
reranker (step 5) are the two AI seams — both exercised with deterministic fakes so the plumbing is
provable with no live model ([vision-and-scope.md](../vision-and-scope.md), testability stack).

---

## 5. Decisions baked into this schema (resolved 2026-06-29)

- **Acceptance is a re-projection, not an in-place flip (#5).** A `suggested` row leaves the queue and
  the edge re-materializes as `origin='inline'`/`status='active'` from the Markdown (Flow ③); the
  origin/status `CHECK` (§1.3) forbids a `suggested + active` row. Mirrored into
  [data-model.md](../data-model.md) §3–§4 and [index-engine.md](../index-engine.md) §3.
- **Schema additions over [index-engine.md](../index-engine.md) §3's sketch (#2), adopted:** `meta`
  (model/dim/replay/version), `note_aliases` (resolution + cold repair), `occurrence_index` (the
  data-model §2 identity tuple), `dst_path_raw` (the move-rewrite + dangling-repair anchor), and the
  origin/status `CHECK`. **Deferred:** `dst_alias_raw` and the `note_bodies` / `frontmatter_json` /
  `note_tags` caches (§1.6) — no invariant depends on them.
- **`dst_id` is a soft, nullable reference; `src_id` is a hard FK with CASCADE (#4).** Targets can be
  deleted (inbound edges re-project to dangling) and links can dangle, while deleting an authoring note
  cleanly removes its outbound edges and suggestions.
- **Edge `id` (#3):** suggestions carry a replay-stable ULID minted at suggestion time (stored in the
  log event); authored edges derive their `id` deterministically from `(src_id, dst_id | dst_path_raw,
  type, occurrence_index)`. Only suggested/rejected edges are referenced by `edge_provenance`.

> Next ([tasks.md](../tasks.md)): pick the language (the immediate gate), then build steps 0→5 against
> the golden-vault fixtures. The schema and flows in this doc are the language-agnostic contract that
> work implements.
