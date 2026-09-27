# B2 task runner. CI runs `make ci` rather than re-specifying its stages in YAML, so the gate
# can't drift between local and CI.
#
# `make` (or `make help`) lists targets, built from the `##@ <group>` markers and the
# `## <summary>` after each target.

.DEFAULT_GOAL := help

.PHONY: help doctor install uninstall ui-install build fmt ui-build ui-dev icons \
	app app-cpu app-build check ci no-tokio test test-ui check-app audit \
	coverage coverage-html coverage-lcov coverage-all coverage-app \
	init eval eval-sweep eval-stemmer stability stability-bless calibrate eval-metal \
	compare-device eval-chat

help:
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} \
	/^[a-zA-Z0-9_-]+:.*##/ { printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2 } \
	/^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)

# Passed through to `stability` / `calibrate` / `compare-device`, e.g. `make stability ARGS=--verbose`.
ARGS ?=
# Required by `calibrate` (no default: a wrong vault would silently calibrate the wrong
# corpus); optional for `compare-device`.
VAULT ?=

##@ Setup

# what a fresh clone needs before anything else works

doctor: ## Sanity-check the local toolchain and print the fix for anything missing — run this first.
	-@scripts/doctor.sh

# --force because the version stays 0.1.0; re-run to update after code changes.
install: ## Install the `b2` binary to ~/.cargo/bin (on PATH; no alias, works from any dir).
	cargo install --path crates/b2-cli --locked --force

uninstall: ## Remove the installed `b2` binary.
	cargo uninstall b2-cli

# A prerequisite of every recipe that needs node_modules (~0.3s when satisfied), so a pull
# that adds a frontend dep can't leave node_modules a commit behind.
ui-install: ## Install the frontend's npm dependencies (a prerequisite of every recipe that needs them).
	npm --prefix ui install

##@ Dev

# build and run

build: ## Build the whole workspace.
	cargo build

fmt: ## Auto-format the workspace.
	cargo fmt

ui-build: ui-install ## Type-check + build the frontend bundle into ui/dist (what the Tauri host embeds).
	npm --prefix ui run build

ui-dev: ui-install ## Vite dev server on :5173 (usually started automatically by `make app`).
	npm --prefix ui run dev

# Run after adding an icon to ui/scripts/gen-icons.ts or bumping bootstrap-icons;
# `make test-ui` fails on a stale generated file.
icons: ui-install ## Regenerate ui/src/icons.gen.ts from the bootstrap-icons package.
	npm --prefix ui run icons

# Metal on Apple Silicon (GH #40), CPU elsewhere. A compile-time switch; the runtime still
# falls back to CPU if the GPU can't initialize.
UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
METAL_FEATURE :=
ifeq ($(UNAME_S),Darwin)
ifeq ($(UNAME_M),arm64)
METAL_FEATURE := --features metal
endif
endif

# e.g. `B2_VAULT_PATH=~/notes make app`. Switching device re-embeds the vault. Needs
# ui-install because Tauri's beforeDevCommand runs `npm run dev` outside `make`.
app: ui-install ## Run the desktop app in dev — auto-selects Metal on Apple Silicon; `make app-cpu` forces CPU.
	cd crates/b2-desktop && cargo tauri dev $(METAL_FEATURE)

app-cpu: ui-install ## Force the CPU embedder regardless of platform — the A/B counterpart to the default `make app`.
	cd crates/b2-desktop && cargo tauri dev

app-build: ui-install ## Bundle the desktop app (per-platform); builds the frontend first (beforeBuildCommand).
	cd crates/b2-desktop && cargo tauri build

##@ Gates

# `check` is the constant loop; `ci` is the complete pass CI runs. The rest are building
# blocks. `-D warnings` is what makes clippy a gate: it exits 0 on warnings.

# Excludes b2-desktop from clippy: linting it embeds ui/dist, so `ci` covers it. test-ui runs
# last via `$(MAKE)`, not as a prerequisite, so the cheaper cargo stages fail first.
check: ## Fast gate (~3s) — fmt-check, lint, engine + frontend tests. The one you run while working.
	cargo fmt --check
	cargo clippy --workspace --exclude b2-desktop -- -D warnings
	cargo test -p b2-core
	$(MAKE) test-ui

# `check` + `check-app` + every test, without the overlap. `ui-build` satisfies the desktop
# crate's ui/dist embed, so one clippy pass covers the workspace. Stage order is failure
# order, cheapest first; `audit` is last because it queries the npm advisory service every
# run, so a slow registry can't delay a real failure above it.
ci: no-tokio ui-build ## Complete gate (~18s warm) — every mechanical check in one pass; exactly what CI runs.
	cargo fmt --check
	cargo clippy --workspace -- -D warnings
	cargo test
	$(MAKE) test-ui
	$(MAKE) audit

# GH #174: a dependency's default features once pulled a whole async HTTP stack into `b2`.
# Scoped to `b2-cli`; `b2-desktop` links tokio legitimately, via Tauri.
# Not `if cargo tree -i tokio`: `-i` exits non-zero both when tokio is absent and on other
# errors (e.g. an ambiguous spec), so inverting it would fail open. Line 1 fails loudly if
# the tree can't resolve; line 2 greps a flat list (`^tokio v`, so `tokio-util` won't match).
# `--locked` stops `cargo tree` rewriting Cargo.lock. Default edges include dev-deps, since
# `cargo test` compiles them too.
no-tokio: ## Fail if tokio is back in the `b2` binary's dependency tree (GH #174).
	@cargo tree -p b2-cli --locked > /dev/null
	@if cargo tree -p b2-cli --locked --prefix none --format '{p}' | grep -Eq '^tokio v'; then \
		echo "error: tokio is in b2-cli's dependency tree again (GH #174) —"; \
		echo "       a dependency is probably pulling an async HTTP stack via default features:"; \
		cargo tree -p b2-cli --locked --invert tokio || true; \
		exit 1; \
	fi

test: ## Fast, deterministic, model-free engine suite (b2-core only) — the bulk of the test weight.
	cargo test -p b2-core

test-ui: ui-install ## The frontend's pure-logic suite (node's own test runner over ui/src/**/*.test.ts).
	npm --prefix ui test

check-app: ui-build ## Lint the desktop crate alone (needs ui/dist, so the frontend builds first) — the heavy half of `check`.
	cargo clippy -p b2-desktop -- -D warnings

# `npm install` reports vulnerabilities but exits 0; `npm audit` gates (GH #87 §3). Not in
# `ui-install` (a new advisory must not stop the app launching) nor `check` (needs network).
audit: ## Fail on a high-or-worse advisory in the frontend dep tree (needs network; runs as part of `make ci`).
	npm --prefix ui audit --audit-level=high

##@ Coverage

# Needs cargo-llvm-cov and the llvm-tools-preview component (`make doctor` checks both).
# Covers the model-free suite; b2-embed's candle code is exercised by `make eval`, not
# `cargo test`, so it is left out.

coverage: ## Engine line/region coverage — the daily number.
	cargo llvm-cov -p b2-core

coverage-html: ## The same run as a browsable per-line HTML report under target/llvm-cov/html.
	cargo llvm-cov -p b2-core --html
	@echo "report: target/llvm-cov/html/index.html"

coverage-lcov: ## lcov.info for editor gutters (VS Code Coverage Gutters, etc.) or a CI upload.
	cargo llvm-cov -p b2-core --lcov --output-path target/llvm-cov/lcov.info
	@echo "lcov: target/llvm-cov/lcov.info"

# Heavier on a cold cache: b2-cli depends on b2-embed, so candle still compiles.
coverage-all: ## Engine + the CLI adapter — its tests spawn the instrumented binary, so those runs count.
	cargo llvm-cov --workspace --exclude b2-desktop --exclude b2-embed

# Expect a low number: b2-desktop is a dumb adapter, and its behaviour is covered by the
# façade suite.
coverage-app: ui-build ## Coverage for the desktop host's own unit tests.
	cargo llvm-cov -p b2-desktop

##@ Model

# The eval suite, never part of `cargo test` or CI: the scored runs need a real model and
# their numbers are per machine. `stability` uses the fake embedder and needs no model.
# docs/evals.md is the guide.

init: ## Download + verify bge-base-en-v1.5 into the shared XDG cache (needed for the real embedder).
	cargo run -p b2-cli -- init

# Appends to crates/b2-embed/evals/results.jsonl; exits non-zero on an exit-gate regression.
eval: ## Semantic-retrieval + discovery quality eval (real model).
	cargo run -p b2-embed --example eval

eval-sweep: ## `make eval` plus the in-process chunker A/B (ChunkConfig sweep) — the GH #44 gate.
	cargo run -p b2-embed --example eval -- --sweep

# chunks_fts rebuilt under each tokenizer over identical chunks and vectors.
eval-stemmer: ## `make eval` plus the FTS tokenizer ablation (shipped porter vs unstemmed unicode61) — the GH #157 instrument.
	cargo run -p b2-embed --example eval -- --stemmer

# Deterministic, no `make init` needed. ARGS: --verbose (diverging rankings), --model (real
# bge, no baseline), --vault <path>.
stability: ## Rank-stability probe on fixtures/test-vault: pool sensitivity + drift vs the blessed baseline (GH #141). Usage: make stability [ARGS="--verbose"]
	cargo run -p b2-embed --example stability -- $(ARGS)

# Only after an intended ranking change: the snapshot records what the ranking is, not that
# it got better.
stability-bless: ## Accept the current ranking as the committed rank-stability baseline.
	cargo run -p b2-embed --example stability -- --bless

# Process rule 5's transfer check (docs/evals.md): a read over stored vectors, no labels.
# ARGS: --limit N, --leader-z/--member-z, --mutual-k N, --json; --search adds the search-side
# check (GH #201/#206), which loads the real model.
calibrate: ## Calibration on any built vault: discovery pools/bands (GH #197) and, with ARGS=--search, the evidence bar (GH #201). Usage: make calibrate VAULT=<path> [ARGS=--search]
	@if [ -z "$(VAULT)" ]; then echo "error: VAULT is required, e.g. make calibrate VAULT=fixtures/test-vault"; exit 1; fi
	cargo run -p b2-embed --example calibrate -- "$(VAULT)" $(ARGS)

# Compare against `make eval` (CPU); a device switch is a model swap (`@metal`).
eval-metal: ## The same eval, embedding on the Metal GPU (GH #40, macOS-only).
	cargo run -p b2-embed --example eval --features metal

# Reindexes a temp copy on each device; never mutates the fixture.
compare-device: ## CPU-vs-Metal embed throughput A/B on a vault (default fixtures/test-vault; GH #40, macOS-only). Usage: make compare-device [VAULT=<path>]
	scripts/compare-embed-device.sh "$(if $(VAULT),$(VAULT),fixtures/test-vault)"

# GH #154: crates/b2-llm/evals/questions.json through `Vault::ask`. Needs `ollama serve` or
# B2_LLM_URL; appends to crates/b2-llm/evals/results.jsonl.
eval-chat: ## Grounded-chat quality eval: citation accuracy + refusal over the eval corpus (real model + model server).
	cargo run -p b2-llm --example groundedness
