# ADR-0010 — The typed graph: two authored homes, frontmatter-wins, a closed verb core

- **Status:** Accepted · 2026-07
- **Refs:** invariants G1–G6 · `data-model.md` §0–§3

## Context

Typed relations need somewhere to live that survives B2 being deleted, without turning note prose
into a markup dialect. The verb set has two consumers — the human typing a connection, and queries
(`b2 neighbors --type supports`) — and both want the core small, orthogonal, and stable, so the same
relationship always gets the same verb. The core encodes the one thing embedding similarity cannot
infer: **stance**. The model already surfaces "these are related"; whether the notes *agree* is what
only the human at the typing moment knows.

## Decision

- The edge set is the union of exactly two homes: **body links** (`origin=inline`, always untyped
  `references` — the body carries no B2 syntax, so no verb or explanation is ever parsed from prose)
  and **frontmatter `b2_relations:`** (`origin=frontmatter`, the sole home of a verb + explanation).
- Same `(target, type)` in both homes → **frontmatter wins** (it alone can carry the explanation); a
  *different* verb over a body-linked target coexists (the augment case).
- The vocabulary is a **closed three-verb stance core** — `references` / `supports` / `contradicts` —
  plus a tolerated tail stored verbatim.
- Edges are **directed and stored once**, with a deterministic id from `(src, dst, type, occurrence)`.
  Inverse labels are display-only; B2 never writes a reciprocal link into the target file.
- There is **no `status` column**: every edge is authored and active. An unresolvable target projects
  as a surfaced dangling edge, never a dropped one, and heals on the next reindex.

## Consequences

- The `edges` table is a **cache**; runtime parsing is the correctness definition. It exists for what
  parsing cannot serve — backlinks, typed traversal, the discovery exclusion — and is rebuilt from
  scratch every reindex.
- Nothing is ever copied between homes or auto-removed from a file.
