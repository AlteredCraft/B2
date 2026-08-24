# ADR-0004 — Markdown is the only surface B2 writes, and B2 makes no unbidden writes

- **Status:** Accepted · 2026-06-28
- **Refs:** invariants W1–W5, S2 · `data-model.md` §0–§1

## Context

"Your files stay yours" is only true if it is mechanical. A tool that tidies frontmatter or injects
its own syntax on read has already taken ownership of the vault.

## Decision

- **Markdown is the vault's sole authored subset** — the only format whose bytes B2 may write.
  Non-`.md` files are *resources*: path-keyed peers contributing derived rows only.
- **Reading writes nothing.** Walking, projecting and reindexing a vault leave a git-versioned vault
  with no diff, and run on a read-only one.
- The on-command writes are **enumerated**: `b2 link` appending one frontmatter `b2_relations:`
  entry; the move-repair of inbound link *path text*; the editor save (a byte-honest splice of the
  human's own body bytes, content-hash guarded); the frontmatter save (same guard, body untouched);
  import (handed bytes copied verbatim, then projected); and create/move/delete on explicit command.
- **B2 never authors body content and never asks the body to carry B2 syntax.** Round-trip is
  lossless: unknown frontmatter keys survive verbatim and in order; B2's one key is namespaced.

## Consequences

- Every committed connection must be expressible as ordinary Markdown a human could have typed.
- Consequences of human edits (orphans, dangling links) are surfaced, never silently repaired.
