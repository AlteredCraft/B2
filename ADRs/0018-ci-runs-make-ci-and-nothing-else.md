# ADR-0018 — CI runs `make ci` and nothing else

- **Status:** Accepted · 2026-07-25 · migrated `just` → `make` 2026-08-25
- **Refs:** GH #87 · `Makefile`, `.github/workflows/ci.yml`

## Context

A check re-specified in workflow YAML is a check that can drift from the one you run locally.

## Decision

- **Every check lives in a `make` target.** The workflow supplies only the environment
  (toolchain, Node, the Rust cache) and runs `make ci`. Two gates: `make check` (fast, the
  working loop) and `make ci` (complete, what CI runs — run it before pushing).
- CI runs on **macos-latest**: B2 ships on macOS only, and the Xcode CLT the WebView links
  against — and `make` itself — are preinstalled, so no separate task-runner install step exists.
- CI is model-free by construction (`b2-core` never links candle; CLI tests spawn the binary under
  `B2_EMBEDDER=fake`).
- One CI-only step follows the gate: **`git diff --exit-code`**, asserting no stage edited a tracked
  file — so the lockfile cannot drift from `package.json` and no suite can mutate a committed fixture.
- **Advisory-but-exit-0 output is a hole in a gate.** Clippy warns and exits 0 (hence `-D warnings`
  everywhere); `npm install` reports vulnerabilities and exits 0 (hence a separate `make audit`).
  When wiring a new tool into a recipe, check its exit code on a *failing* input.

## Consequences

- Targets carry a trailing `## <summary>` comment and a `##@ <group>` section marker: `make help`
  (the no-args default) parses these into a grouped command reference — the Make analogue of the
  `just` recipe attributes this ADR originally described, which `just` itself rendered the same
  way from `[group('…')]` + `[doc("…")]`.
- `make audit` is deliberately **not** a prerequisite of `ui-install` or `check` — a fresh advisory
  must not stop the app launching.
- **2026-08-25: migrated the task runner from `just` to `make`**, removing the `just` dependency.
  The decision this ADR records — one enforcement point, `check`/`ci` as the two gates, no
  advisory-but-exit-0 holes — is unchanged; only the tool executing it is. `justfile` → `Makefile`,
  `extractions/setup-just@v4` dropped from CI (nothing left to install), and every `just <recipe>`
  reference across the docs became `make <target>`.
