# B2 task runner. Install `just` with `brew install just` (or `cargo install just`).
# Recipes just wrap the cargo commands documented in CLAUDE.md → "Commands";
# `just` is the single place to name the multi-step ones.

# List available recipes
default:
    @just --list

# Sanity-check your local setup — run this first on a fresh clone. Checks Rust, Node/npm,
# the Tauri CLI, the platform build toolchain, and a couple of optional extras, printing
# the fix for anything missing (this is "stop 0" before `just app` works — see README).
doctor:
    -@scripts/doctor.sh

# Install the `b2` binary to ~/.cargo/bin (on PATH; no alias, works from any dir).
# The recipe always passes --force itself (cargo would otherwise refuse a same-version
# reinstall, since this stays 0.1.0) — just re-run `just install` to update after code changes.
install:
    cargo install --path crates/b2-cli --locked --force

# Remove the installed `b2` binary
uninstall:
    cargo uninstall b2-cli

# Build the whole workspace
build:
    cargo build

# Fast, deterministic, model-free engine suite — what CI runs
test:
    cargo test -p b2-core

# Whole-workspace tests (compiles candle in b2-embed — slower)
test-all:
    cargo test

# Auto-format the workspace
fmt:
    cargo fmt

# Format-check + lint + fast tests — the pre-commit gate. Excludes b2-desktop from
# clippy: linting it embeds ui/dist (needs a frontend build), so it's a separate,
# heavier job (`just check-app`), out of the fast gate — like b2-embed's candle build.
check:
    cargo fmt --check
    cargo clippy --workspace --exclude b2-desktop
    cargo test -p b2-core

# --- Coverage (cargo-llvm-cov; `cargo install cargo-llvm-cov` + `rustup component add
# llvm-tools-preview`) ------------------------------------------------------------
#
# Source-based coverage over the same model-free suite CI runs — the numbers answer
# "which engine lines does the deterministic suite actually execute", so an untested
# branch shows up as a gap rather than as a hole nobody named. Real-model paths
# (b2-embed's candle code) are deliberately out: they are exercised by `just eval`,
# not by `cargo test`, so instrumenting them would report a permanent, meaningless 0%.

# Engine coverage — the daily number. Mirrors `just test` (b2-core only), so it is as
# fast as the suite itself and pulls in no ML deps.
coverage:
    cargo llvm-cov -p b2-core --summary-only

# Same, as a browsable HTML report (per-file, line-by-line) under target/llvm-cov/html.
coverage-html:
    cargo llvm-cov -p b2-core --html
    @echo "report: target/llvm-cov/html/index.html"

# Engine + the CLI adapter. The CLI suite spawns the real `b2` binary, which is
# instrumented too, so its process-level runs count. Heavier on a cold cache: b2-cli
# depends on b2-embed, so this compiles candle once (excluded crates are still built
# when a covered crate depends on them).
coverage-all:
    cargo llvm-cov --workspace --exclude b2-desktop --exclude b2-embed --summary-only

# Coverage for the desktop host's own unit tests. Separate and heavier for the same
# reason `check-app` is: it embeds ui/dist (so the frontend builds first) and needs
# the platform webview toolchain, exactly like `just app`. Expect a low number by
# design — b2-desktop is a dumb adapter, and the behaviour behind its commands is
# covered by the façade suite (crates/b2-desktop/CLAUDE.md, "inherited tests").
coverage-app: ui-build
    cargo llvm-cov -p b2-desktop --summary-only

# lcov.info for editor gutters (VS Code Coverage Gutters, etc.) or a CI upload.
coverage-lcov:
    cargo llvm-cov -p b2-core --lcov --output-path target/llvm-cov/lcov.info
    @echo "lcov: target/llvm-cov/lcov.info"

# Download + verify bge-base-en-v1.5 into the shared XDG cache (needed for the real embedder)
init:
    cargo run -p b2-cli -- init

# Semantic-retrieval + discovery quality eval (real model; never part of `cargo test`).
# Scores BM25-only vs hybrid (the semantic lift), passage-level ranks, and `similar`;
# appends every run to crates/b2-embed/evals/results.jsonl (gitignored).
eval:
    cargo run -p b2-embed --example eval

# `just eval` plus the in-process chunker A/B (ChunkConfig sweep) — the GH #44 gate.
eval-sweep:
    cargo run -p b2-embed --example eval -- --sweep

# Same eval, but embedding on the Metal GPU (GH #40, macOS-only). Compare its retrieval
# quality against `just eval` (CPU) — a device switch is a model swap (`@metal` id tag).
eval-metal:
    cargo run -p b2-embed --example eval --features metal

# CPU-vs-Metal embed throughput A/B on a vault (default fixtures/test-vault; GH #40,
# macOS-only). Reindexes an isolated copy on each device and reports chunks/s + speedup.
# Never mutates the committed fixture; artifacts are gitignored + cleaned up.
compare-device vault="fixtures/test-vault":
    scripts/compare-embed-device.sh {{vault}}

# --- Desktop app (crates/b2-desktop + ui/) — heavier; needs Node + the Tauri CLI ---
# One-time frontend prerequisites: `npm i -D @tauri-apps/cli` is *not* needed if the
# Tauri CLI is installed via cargo: `cargo install tauri-cli --locked`.

# Install the frontend's npm dependencies (run once, or after package.json changes).
ui-install:
    npm --prefix ui install

# Vite dev server on :5173 (usually started automatically by `just app`).
ui-dev:
    npm --prefix ui run dev

# Type-check + build the frontend bundle into ui/dist (what the Tauri host embeds).
ui-build:
    npm --prefix ui run build

# Embed on the Metal GPU by default on Apple Silicon (GH #40); CPU everywhere else. The `metal`
# feature is a compile-time switch, so this selects it for the dev build — the runtime still
# falls back to CPU if the GPU can't initialize. `just app-cpu` forces CPU.
metal_feature := if os() == "macos" { if arch() == "aarch64" { "--features metal" } else { "" } } else { "" }

# Run the desktop app in dev (Vite HMR + a live Tauri window). Point it at a vault with
# B2_VAULT_PATH, e.g. `B2_VAULT_PATH=~/notes just app`. Settings (⌘,) shows a CPU/Metal badge;
# switching device re-embeds the vault (a `@metal` model swap).
# Auto-selects Metal on Apple Silicon; `just app-cpu` forces CPU.
app:
    cd crates/b2-desktop && cargo tauri dev {{metal_feature}}

# Force the CPU embedder regardless of platform — the A/B counterpart to the default `just app`.
app-cpu:
    cd crates/b2-desktop && cargo tauri dev

# Bundle the desktop app (per-platform); builds the frontend first (beforeBuildCommand).
app-build:
    cd crates/b2-desktop && cargo tauri build

# Lint + type-check the desktop crate (needs ui/dist, so build the frontend first).
check-app: ui-build
    cargo clippy -p b2-desktop
