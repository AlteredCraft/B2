# B2 task runner. Install `just` with `brew install just` (or `cargo install just`).
# Recipes wrap the cargo/npm commands documented in CLAUDE.md → "Commands"; `just` is the
# single place the multi-step ones are named. `.github/workflows/ci.yml` runs `just ci`
# rather than re-specifying its stages in YAML, so the gate can't drift between local and CI:
# what Actions enforces is exactly what you can run yourself.
#
# Authoring convention (GH #87): every recipe carries `[group(…)]` + `[doc("…")]`. `just`
# otherwise renders a recipe's *last comment line* as its `--list` summary, so multi-line
# rationale surfaces as a nonsense fragment — `install` used to list as
# "# reinstall, since this stays 0.1.0) — just re-run …". The doc attribute states the summary
# explicitly, which frees the comments above a recipe to be as long as the reasoning needs.

[private]
default:
    @just --list --unsorted

# --- setup: what a fresh clone needs before anything else works -----------------------------

# Sanity-check your local setup — run this first on a fresh clone. Checks Rust, Node/npm,
# the Tauri CLI, the platform build toolchain, and a couple of optional extras, printing
# the fix for anything missing (this is "stop 0" before `just app` works — see README).
[group('setup')]
[doc('Sanity-check the local toolchain and print the fix for anything missing — run this first.')]
doctor:
    -@scripts/doctor.sh

# The recipe always passes --force itself (cargo would otherwise refuse a same-version
# reinstall, since this stays 0.1.0) — just re-run `just install` to update after code changes.
[group('setup')]
[doc('Install the `b2` binary to ~/.cargo/bin (on PATH; no alias, works from any dir).')]
install:
    cargo install --path crates/b2-cli --locked --force

[group('setup')]
[doc('Remove the installed `b2` binary.')]
uninstall:
    cargo uninstall b2-cli

# Every recipe that needs node_modules depends on this, so you rarely run it by hand. A no-op
# `npm install` costs ~0.3s against an already satisfied lockfile — nothing next to the Tauri
# build it precedes — and that price buys the invariant that a pull adding a frontend dep
# can't leave you at a Vite "failed to resolve import" with node_modules a commit behind.
[group('setup')]
[doc("Install the frontend's npm dependencies (a prerequisite of every recipe that needs them).")]
ui-install:
    npm --prefix ui install

# --- dev: build and run ----------------------------------------------------------------------

[group('dev')]
[doc('Build the whole workspace.')]
build:
    cargo build

[group('dev')]
[doc('Auto-format the workspace.')]
fmt:
    cargo fmt

[group('dev')]
[doc('Type-check + build the frontend bundle into ui/dist (what the Tauri host embeds).')]
ui-build: ui-install
    npm --prefix ui run build

[group('dev')]
[doc('Vite dev server on :5173 (usually started automatically by `just app`).')]
ui-dev: ui-install
    npm --prefix ui run dev

# Re-vendor the Bootstrap Icons subset into ui/src/icons.gen.ts (the names live in
# ui/scripts/gen-icons.ts). Run it after adding an icon to that manifest, or after bumping
# the bootstrap-icons devDependency — `just test-ui` runs the same generator in --check mode
# first, so a stale generated file fails the gate rather than shipping quietly.
[group('dev')]
[doc('Regenerate ui/src/icons.gen.ts from the bootstrap-icons package.')]
icons: ui-install
    npm --prefix ui run icons

# Embed on the Metal GPU by default on Apple Silicon (GH #40); CPU everywhere else. The `metal`
# feature is a compile-time switch, so this selects it for the dev build — the runtime still
# falls back to CPU if the GPU can't initialize. `just app-cpu` forces CPU.
metal_feature := if os() == "macos" { if arch() == "aarch64" { "--features metal" } else { "" } } else { "" }

# Point it at a vault with B2_VAULT_PATH, e.g. `B2_VAULT_PATH=~/notes just app`. Settings (⌘,)
# shows a CPU/Metal badge; switching device re-embeds the vault (a `@metal` model swap).
# Depends on ui-install because Tauri's beforeDevCommand shells out to `npm run dev` itself,
# outside `just` — so the deps have to be there before cargo hands off.
[group('dev')]
[doc('Run the desktop app in dev — auto-selects Metal on Apple Silicon; `just app-cpu` forces CPU.')]
app: ui-install
    cd crates/b2-desktop && cargo tauri dev {{metal_feature}}

[group('dev')]
[doc('Force the CPU embedder regardless of platform — the A/B counterpart to the default `just app`.')]
app-cpu: ui-install
    cd crates/b2-desktop && cargo tauri dev

[group('dev')]
[doc('Bundle the desktop app (per-platform); builds the frontend first (beforeBuildCommand).')]
app-build: ui-install
    cd crates/b2-desktop && cargo tauri build

# --- gates: two tiers, plus the pieces they compose from --------------------------------------
#
# `check` is the loop you run constantly; `ci` is the complete pass, and is literally what
# .github/workflows/ci.yml invokes. Everything below them is a building block — useful when
# you're working inside one crate and want that crate's stage alone.
#
# `-D warnings` on every clippy stage is what makes linting a gate at all: clippy exits 0 on
# warnings, so without it the stage caught only lints severe enough to be errors, and
# everything clippy actually exists to say slid through green.

# Excludes b2-desktop from clippy: linting it embeds ui/dist (needs a frontend build), so it's
# a separate, heavier job here — `ci` picks it up, since `ui-build` runs there anyway.
# test-ui is a `&&` (subsequent) dependency so it runs *after* the body rather than before:
# the cargo stages are the cheaper failures (0.1s fmt, 0.2s clippy) and the bulk of the code,
# so they should be what a broken commit trips on first.
# Nothing enforces this locally: there is no git hook, and none is wanted (.git/hooks isn't
# cloneable) — CI is the enforcement, and this is the fast subset you run before you get there.
[group('gates')]
[doc('Fast gate (~3s) — fmt-check, lint, engine + frontend tests. The one you run while working.')]
check: && test-ui
    cargo fmt --check
    cargo clippy --workspace --exclude b2-desktop -- -D warnings
    cargo test -p b2-core

# The union of `check` + `check-app` + every test in the repo, with the overlap removed: run
# back-to-back those re-ran the engine suite, `ui-build`, and the frontend suite twice over.
# Note the absent `--exclude b2-desktop` — the `ui-build` prerequisite has already satisfied the
# desktop crate's `ui/dist` embed, so one clippy pass covers the whole workspace.
# Stage order is failure order: the cheap mechanical checks first (fmt, then lint), the bulk of
# the code next, then the frontend suite, and `audit` last — it is the only stage that touches
# the network, so a slow or unreachable registry can't delay a real failure above it.
[group('gates')]
[doc('Complete gate (~18s warm) — every mechanical check in one pass; exactly what CI runs.')]
ci: ui-build && test-ui audit
    cargo fmt --check
    cargo clippy --workspace -- -D warnings
    cargo test

[group('gates')]
[doc('Fast, deterministic, model-free engine suite (b2-core only) — the bulk of the test weight.')]
test:
    cargo test -p b2-core

# Defined here so the npm half of the repo is reachable from `just` like the cargo half.
[group('gates')]
[doc("The frontend's pure-logic suite (node's own test runner over ui/src/**/*.test.ts).")]
test-ui: ui-install
    npm --prefix ui test

[group('gates')]
[doc('Lint the desktop crate alone (needs ui/dist, so the frontend builds first) — the heavy half of `check`.')]
check-app: ui-build
    cargo clippy -p b2-desktop -- -D warnings

# The generalized version of the `-D warnings` lesson (GH #87 §3): a tool that reports a problem
# and exits 0 passes the gate green. `npm install` prints "1 high severity vulnerability" and
# exits 0 — which is how a high-severity postcss advisory (GHSA-r28c-9q8g-f849, path traversal
# via source-map auto-loading) rode along under vite through every recipe until it was noticed
# by hand. `npm audit` itself exits non-zero when the tree is vulnerable, so it gates directly.
# Deliberately NOT a prerequisite of `ui-install`: that is now upstream of `just app`, and a
# newly-published advisory in a transitive dep must not stop the app from launching. Equally not
# in `check` — this needs the network, and `check` is the 3s loop that has to work on a plane.
# `--audit-level=high` is an explicit threshold: low/moderate findings inform without blocking.
[group('gates')]
[doc('Fail on a high-or-worse advisory in the frontend dep tree (needs network; runs as part of `just ci`).')]
audit:
    npm --prefix ui audit --audit-level=high

# --- coverage (cargo-llvm-cov; `cargo install cargo-llvm-cov` + `rustup component add
# llvm-tools-preview` — both, and the component is per-toolchain; `just doctor` checks for them)
#
# Source-based coverage over the same model-free suite CI runs — the numbers answer
# "which engine lines does the deterministic suite actually execute", so an untested
# branch shows up as a gap rather than as a hole nobody named. Real-model paths
# (b2-embed's candle code) are deliberately out: they are exercised by `just eval`,
# not by `cargo test`, so instrumenting them would report a permanent, meaningless 0%.
#
# No `--summary-only` anywhere here: cargo-llvm-cov documents it as valid only alongside
# --json/--lcov/--cobertura. 0.8.7 doesn't enforce that (it accepts and ignores the flag in
# text mode), but the default text report already *is* the per-file summary, so the flag buys
# nothing and would break if a later release starts rejecting it.

# Mirrors `just test` (b2-core only), so it is as fast as the suite itself and pulls in no ML deps.
[group('coverage')]
[doc('Engine line/region coverage — the daily number.')]
coverage:
    cargo llvm-cov -p b2-core

[group('coverage')]
[doc('The same run as a browsable per-line HTML report under target/llvm-cov/html.')]
coverage-html:
    cargo llvm-cov -p b2-core --html
    @echo "report: target/llvm-cov/html/index.html"

[group('coverage')]
[doc('lcov.info for editor gutters (VS Code Coverage Gutters, etc.) or a CI upload.')]
coverage-lcov:
    cargo llvm-cov -p b2-core --lcov --output-path target/llvm-cov/lcov.info
    @echo "lcov: target/llvm-cov/lcov.info"

# The CLI suite spawns the real `b2` binary, which is instrumented too, so its process-level
# runs count. Heavier on a cold cache: b2-cli depends on b2-embed, so this compiles candle once
# (excluded crates are still built when a covered crate depends on them).
[group('coverage')]
[doc('Engine + the CLI adapter — its tests spawn the instrumented binary, so those runs count.')]
coverage-all:
    cargo llvm-cov --workspace --exclude b2-desktop --exclude b2-embed

# Separate and heavier for the same reason `check-app` is: it embeds ui/dist (so the frontend
# builds first) and needs the platform webview toolchain, exactly like `just app`. Expect a low
# number by design — b2-desktop is a dumb adapter, and the behaviour behind its commands is
# covered by the façade suite (crates/b2-desktop/CLAUDE.md, "inherited tests").
[group('coverage')]
[doc("Coverage for the desktop host's own unit tests.")]
coverage-app: ui-build
    cargo llvm-cov -p b2-desktop

# --- model: the eval harness. Never part of `cargo test` or CI — the scored runs are
# non-deterministic, need a provisioned model, and append to a gitignored results log. Run
# these on demand. `stability` is the exception that proves the group: it measures retrieval
# *sensitivity* rather than model quality, so it is deterministic and needs no model — but it
# is the other half of the same harness, and you reach for it from the same place (GH #141).

[group('model')]
[doc('Download + verify bge-base-en-v1.5 into the shared XDG cache (needed for the real embedder).')]
init:
    cargo run -p b2-cli -- init

# Scores BM25-only vs hybrid (the semantic lift), passage-level ranks, and `similar`;
# appends every run to crates/b2-embed/evals/results.jsonl (gitignored).
[group('model')]
[doc('Semantic-retrieval + discovery quality eval (real model).')]
eval:
    cargo run -p b2-embed --example eval

[group('model')]
[doc('`just eval` plus the in-process chunker A/B (ChunkConfig sweep) — the GH #44 gate.')]
eval-sweep:
    cargo run -p b2-embed --example eval -- --sweep

# What `just eval` structurally cannot see (GH #141): its 29-chunk corpus is smaller than the
# 150-candidate pool retrieval reaches, so neither signal is truncated and a candidate-width
# change prints bit-identical scores. This probe asks the same queries at widening pools on a
# vault big enough for the pool to bind, and diffs the shipped top-10 against a blessed
# snapshot. Deterministic (fake embedder), so it needs no `just init`. Takes the example's
# flags: `just stability --verbose` (show the diverging rankings), `--model` (real bge
# magnitude), `--vault <path>` (any vault; `crates/b2-embed/evals/corpus` reproduces the gap).
[group('model')]
[doc('Rank-stability probe on fixtures/test-vault: pool sensitivity + drift vs the blessed baseline (GH #141).')]
stability *args:
    cargo run -p b2-embed --example stability -- {{args}}

# Only after a ranking change is the intended one — the snapshot is unlabelled, so it records
# what the ranking IS, never that it got better.
[group('model')]
[doc('Accept the current ranking as the committed rank-stability baseline.')]
stability-bless:
    cargo run -p b2-embed --example stability -- --bless

# Compare its retrieval quality against `just eval` (CPU) — a device switch is a model swap
# (the recorded model id gains an `@metal` tag), so the vault re-embeds.
[group('model')]
[doc('The same eval, embedding on the Metal GPU (GH #40, macOS-only).')]
eval-metal:
    cargo run -p b2-embed --example eval --features metal

# Reindexes an isolated copy on each device and reports chunks/s + speedup. Never mutates the
# committed fixture; artifacts are gitignored + cleaned up.
[group('model')]
[doc('CPU-vs-Metal embed throughput A/B on a vault (default fixtures/test-vault; GH #40, macOS-only).')]
compare-device vault="fixtures/test-vault":
    scripts/compare-embed-device.sh {{vault}}
