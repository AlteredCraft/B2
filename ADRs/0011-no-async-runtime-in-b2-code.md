# ADR-0011 — No B2 code is async; no B2 crate starts a runtime

- **Status:** Accepted · 2026-08-14
- **Refs:** GH #154, #174 · `just no-tokio`

## Context

B2 is a local single-binary tool whose I/O is a file walk, a SQLite handle, and one HTTP stream at a
time. An async runtime buys nothing here and costs a large dependency tree — and the claim "no tokio
in the workspace" was once made of the lockfile and was **false** there: `hf-hub`'s default features
pulled reqwest → hyper → tokio into the `b2` binary to serve a path nothing called.

## Decision

- Every B2 crate is **sync end-to-end**. Cancellation is returning early from a blocking read loop,
  not a future being dropped.
- `b2-llm` is a hand-rolled sync OpenAI-compatible SSE client over `ureq`; `hf-hub` is taken
  `default-features = false` so B2 uses its sync API (TLS is rustls + webpki-roots, the same stack
  `b2-llm` uses).
- **`just no-tokio` is a gate** — it reads the lockfile, compiles nothing, and leads `just ci`
  because a dependency added with default features on can restore the whole async HTTP stack while
  every other stage stays green.
- Do not introduce `async` (or generics, traits, macros) without a concrete need.

## Consequences

- The one runtime in the tree is **Tauri's**, in the desktop host, which is what a GUI host is.
- Streaming cancellation is a callback return value (`Break`) read at token granularity, in both
  adapters (Ctrl-C in the CLI, Esc in the GUI).
