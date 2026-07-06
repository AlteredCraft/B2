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

# Format-check + lint + fast tests — the pre-commit gate
check:
    cargo fmt --check
    cargo clippy --workspace
    cargo test -p b2-core

# Download + verify bge-base-en-v1.5 into the shared XDG cache (needed for the real embedder)
init:
    cargo run -p b2-cli -- init

# Semantic-retrieval quality eval (real model; never part of `cargo test`)
eval:
    cargo run -p b2-embed --example eval
