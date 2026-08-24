# ADR-0021 — Many readers, one writer, serialized on SQLite's own locks

- **Status:** Accepted · 2026-08-23
- **Refs:** invariants C1 · `index-engine.md` §3 · GH #55, #111, #114

## Context

A personal vault is opened by more than one process at a time — the desktop app, a `b2` command, a
backgrounded `b2 reindex &` — and the index is a disposable projection (ADR-0002), so a reader must
never be refused because a writer is rebuilding one. Three defects made that untrue, each measured:

| Issue | The race | Measured |
|---|---|---|
| [#111](https://github.com/AlteredCraft/B2/issues/111) | `journal_mode = WAL` is the one statement in `open` that takes a write lock, and `busy_timeout` does not cover it — SQLite consults the busy handler only for a write lock taken from no transaction at all | ~4% of racing opens took an immediate `SQLITE_BUSY`; the documented `b2 reindex & ; b2 status` flow failed ~40% of the time, usually killing the reindex |
| [#114](https://github.com/AlteredCraft/B2/issues/114) | `migrate` was read-then-decide-then-write over ~30 separately-committed DDL statements, so two openers both rebuilt and interleaved | over 320 racing opens: 10 failed outright, and 8 missing-table observations were left behind by rounds where *every* opener returned `Ok` |
| #114 (again) | `ensure_embedding_space` is the same drop-and-rebuild, and the `#55` advisory lock is the CLI's alone — a desktop reindex and a `b2 reindex` genuinely overlap | 70 of 80 workers errored and **every** round lost vectors |

`busy_timeout` never applied to the second and third: nothing contended for a lock, the statements
all succeeded in the wrong order.

## Decision

- **Serialization is SQLite's own write lock** (`BEGIN IMMEDIATE` + a bounded retry), not a second
  mechanism layered over the database. Both drop-and-rebuilds — the schema migration and the vector
  tables — check, then re-check *inside* the lock, then rebuild once, in **one transaction** (DDL is
  transactional in SQLite), so nothing observes a half-built shape.
- **An advisory lock file was rejected.** It would be a third concurrency mechanism guarding state
  the database already guards, and the weaker guard exactly where it matters: a vault on a network
  share or synced folder is where `flock` stops meaning anything.
- **The fast path takes no write lock at all.** An index already at the current `schema_version`
  costs two reads, which is what makes C1's "a reader is never refused" true rather than hoped for.
- **"Complete" is checked, not assumed:** a current stamp over missing tables is rebuilt from empty,
  because surviving rows would look up-to-date to an incremental reindex and break S3.
- **The WAL flip retries in `b2-core`**, since it is the one statement `busy_timeout` cannot cover;
  its budget is therefore large where the DDL rebuilds' is small (theirs sits *on top of* the
  timeout). The flip's result is **read back**, because a filesystem with no shared-memory support
  declines it with `SQLITE_OK` and the old mode — a decline is said out loud, never retried.
- **Concurrent writers stay single-in-flight** by the `reindex` advisory lock (the CLI) and the
  single-in-flight embed slot (the desktop). Readers never take either, which is precisely why
  neither can cover the rebuilds above.

## Consequences

- Waiting is bounded, deliberately: past the budget a stuck writer is *reported* rather than hung on.
- The races are covered by probes, not proofs, and the tests say so — `tests/substrate.rs` holds the
  contended lock outright for the deterministic gate and keeps the thread-race version beside it,
  labelled as the smoke test it is (green ~88% of the time against the unfixed `open`).
- Every new drop-and-rebuild in `db.rs` inherits this shape. Adding one without it reintroduces the
  same defect, which is the quiet kind: an index left incomplete while every opener returned `Ok`.
