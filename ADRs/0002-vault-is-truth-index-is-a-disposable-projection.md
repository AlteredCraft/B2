# ADR-0002 — The vault is the source of truth; the index is a disposable projection

- **Status:** Accepted · 2026-06-28
- **Refs:** invariants S1–S5 · `index-engine.md` §3, §8

## Context

An AI layer over personal notes can either own the notes (a database with an export button) or sit
beside them. Owning them is lock-in, and it makes every index bug a data-loss risk.

## Decision

- **`index = a pure projection of (the vault directory)`.** Two tiers: the vault (Markdown +
  resources + the directory tree) is authoritative; `<vault>/.b2/b2.sqlite` is a cache.
- Delete the index, `reindex`, get an identical index. **Incremental re-index ≡ full rebuild**,
  unconditionally — whole-vault passes own every reconciliation (pruning rows for vanished files,
  collecting unreferenced vectors); single-note paths never prune.
- **No durable B2-derived state outside the Markdown** — no event log, no sidecars.
- A schema change is a version bump + rebuild, never a data migration. A migration script would be
  evidence this ADR broke.
- Folders are user-authored structure and are **never projected**: the tree listing is a live fs
  walk, and `create_dir`/`move_dir`/`delete_dir` proxy the OS.

## Consequences

- Any feature needing durable state must find a home in the Markdown or do without.
- Many processes may hold one index open. Readers are unrestricted and never refused; the two
  drop-and-rebuild paths (schema migration, vector tables) serialize on SQLite's own write lock,
  each checking and re-checking inside `BEGIN IMMEDIATE` (GH #114). The CLI's `reindex` advisory
  lock is writer-only and cannot cover this.
