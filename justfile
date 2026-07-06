# B2 task runner. Install `just` with `brew install just` (or `cargo install just`).
# Recipes just wrap the cargo commands documented in CLAUDE.md → "Commands";
# `just` is the single place to name the multi-step ones.

# List available recipes
default:
    @just --list

# Install the `b2` binary to ~/.cargo/bin (on PATH; no alias, works from any dir).
# Re-run to update after code changes (--force reinstalls the same 0.1.0 version).
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

# Download + verify bge-base-en-v1.5 into the shared XDG cache (needed for the real embedder)
init:
    cargo run -p b2-cli -- init

# Semantic-retrieval quality eval (real model; never part of `cargo test`)
eval:
    cargo run -p b2-embed --example eval

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

# Run the desktop app in dev: Vite HMR + a live Tauri window (beforeDevCommand starts
# Vite). Point it at a vault with B2_VAULT_PATH, e.g. `B2_VAULT_PATH=~/notes just app`.
app:
    cd crates/b2-desktop && cargo tauri dev

# Bundle the desktop app (per-platform); builds the frontend first (beforeBuildCommand).
app-build:
    cd crates/b2-desktop && cargo tauri build

# Lint + type-check the desktop crate (needs ui/dist, so build the frontend first).
check-app: ui-build
    cargo clippy -p b2-desktop
