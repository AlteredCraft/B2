# ADR-0018 — CI runs `just ci` and nothing else

- **Status:** Accepted · 2026-07-25
- **Refs:** GH #87 · `justfile`, `.github/workflows/ci.yml`

## Context

A check re-specified in workflow YAML is a check that can drift from the one you run locally.

## Decision

- **Every check lives in a `just` recipe.** The workflow supplies only the environment (toolchain,
  Node, `just`, the Rust cache) and runs `just ci`. Two gates: `just check` (fast, the working loop)
  and `just ci` (complete, what CI runs — run it before pushing).
- CI runs on **macos-latest**: B2 ships on macOS only, and the Xcode CLT the WebView links against
  are preinstalled.
- CI is model-free by construction (`b2-core` never links candle; CLI tests spawn the binary under
  `B2_EMBEDDER=fake`).
- One CI-only step follows the gate: **`git diff --exit-code`**, asserting no stage edited a tracked
  file — so the lockfile cannot drift from `package.json` and no suite can mutate a committed fixture.
- **Advisory-but-exit-0 output is a hole in a gate.** Clippy warns and exits 0 (hence `-D warnings`
  everywhere); `npm install` reports vulnerabilities and exits 0 (hence a separate `just audit`).
  When wiring a new tool into a recipe, check its exit code on a *failing* input.

## Consequences

- Recipes carry `[group('…')]` + `[doc("…")]`: `just` otherwise renders a recipe's last comment line
  as its summary, so rationale would surface as a nonsense fragment. `just` with no args is the
  command reference.
- `just audit` is deliberately **not** a prerequisite of `ui-install` or `check` — a fresh advisory
  must not stop the app launching.
