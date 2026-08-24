# ADR-0007 — The embedding space has one recorded identity, and the compute device is part of it

- **Status:** Accepted · 2026-07-13
- **Refs:** invariants M2 · `index-engine.md` §6 · GH #40

## Context

Mixing vectors from two models — or from two devices whose arithmetic differs — returns
silently-wrong results. Silence is the failure mode that matters: nothing looks broken.

## Decision

- `meta.(embed_model_id, embed_dim)` is the **only** place a model swap is detectable.
- The resolved compute device folds into that id: CPU keeps the bare repo id, a `--features metal`
  GPU build appends `@metal`. A device change that alters vectors **is** a model swap.
- On a mismatch: `search` **fails fast**; `reindex` drops both vector tables and re-embeds; `open`
  **never** mutates the vector space, so changing configuration cannot wipe vectors on the next
  command.

## Consequences

- Metal is a **build** switch, not a runtime one — recompile to flip, and re-embed the vault.
- Any per-model calibrated constant (ADR-0015's evidence bar) must be keyed to this id, device
  suffix included.
