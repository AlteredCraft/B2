# ADR-0003 — A note's identity is its vault-relative path

- **Status:** Accepted · 2026-08-13 — supersedes the `b2id` ULID stamp
- **Refs:** invariants L1, L3, S3, W1 · GH #170

## Context

Through 2026-08 B2 stamped a `b2id:` ULID into the frontmatter of every note it saw. It bought
re-binding after an out-of-band move, and cost a collision subsystem, a carve-out on the
"incremental ≡ full" invariant, an unbidden write into the user's files — and no link ever named it.

## Decision

- Identity is the **vault-relative path**. `notes.path` is the primary key; every derived row
  cascades off it. Both link homes already address by path, so the index holds no key the vault does
  not carry.
- Notes and resources share this identity model; the remaining asymmetry is authoring surface alone.
- A move **B2 performs** rewrites inbound link path text and re-keys the moved note's rows in one
  transaction. A move made **outside** B2 is a delete plus a create — inbound links surface as
  dangling, never silently dropped.

## Consequences

- No migration exists or is needed: a `b2id:` line left by an older B2 is an ordinary unknown
  frontmatter key — never read, never removed. `rm -rf .b2/` and reindex is the whole upgrade.
- Content-addressed vectors (ADR-0006) are what make path identity cheap: a rename re-embeds nothing.
