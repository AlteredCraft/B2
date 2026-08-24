# ADR-0012 — One typed façade, two dumb adapters

- **Status:** Accepted · 2026-07
- **Refs:** invariants E3, E4 · `b2-core/src/vault.rs` · `crates/b2-desktop/CLAUDE.md`

## Context

Two front ends (a CLI and a Tauri app) over one engine will grow two subtly different products
unless the boundary is enforced.

## Decision

- **`Vault` is the one typed API.** The CLI and the desktop host are its only clients; every other
  `b2-core` module is called directly only by integration tests.
- Each adapter command is **deserialize → one façade call → serialize**. The desktop host reuses the
  CLI's `--json` view types as its IPC contract. Logic that wants to live in an adapter belongs
  behind the façade.
- Dependencies point one way (adapters → core, never back). Adapters own only what the host demands:
  clocks, log subscribers, the OS dialog, the menu bar, the cancellable background task.
- **Façade operations are added when a command needs them** — never a pre-built broad surface.
- The embedder (and chat provider) are **injected here**: `open` defaults to the fake,
  `open_with_embedder` is how an adapter wires the real model.
- User-facing errors are generic and actionable, never leaking sqlite/io/serde internals; full detail
  goes to logs or `B2_DEBUG`.

## Consequences

- `b2-core` stays deterministic and wall-clock-free (no randomness, timestamps injected).
- The desktop crate's low unit-coverage number is by design.
