# ADR-0006 — Vectors live in plain SQLite tables, scored in-process, content-addressed

- **Status:** Accepted · 2026-07-12 (content-addressing 2026-08-13)
- **Refs:** invariants M4 · `index-engine.md` §3–§4 · GH #38, #170

## Context

A vector extension (sqlite-vec, FAISS) is another binary to ship, version, and fail to load — for a
personal vault whose vectors number in the thousands, not the millions.

## Decision

- `embeddings(text_hash, vector)` and `note_centroids(note_path, centroid)` are **plain BLOB
  tables**. Every distance is computed **in-process** in one scan (`embed::l2_sq`).
- The tables are created at **embed time**, not in the base migration: their existence *is* the
  "this vault has an embedding space" signal that the fallbacks key on (BM25-only search on a
  projected-but-unembedded vault).
- The store is **content-addressed**: the key is blake3 of the chunk text, which is exactly the embed
  input. Identical text has one vector.
- Centroids are derived data on the vectors' lifecycle: refreshed by the embed pass, dropped on
  re-chunk. No separate invalidation exists.

## Consequences

- A moved or renamed note re-embeds nothing — which is what makes path identity (ADR-0003) cheap.
- A vector **outlives** the chunk that addressed it (it may be shared), so `replace_chunks` cannot
  cascade it away; the whole-vault pass collects hashes no chunk references. Every read joins through
  `chunks`, so an uncollected orphan is invisible to search, discovery, and coverage alike.
- `--force` re-*chunks* but re-embeds only what genuinely differs.
