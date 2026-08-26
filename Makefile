# B2 task runner. Ported from `just` to GNU Make — `make` needs no extra install on this repo's
# platforms (it ships with the Xcode Command Line Tools on macOS and build-essential on Linux,
# both of which `scripts/doctor.sh` already checks for the native build toolchain). Recipes wrap
# the cargo/npm commands documented in CLAUDE.md → "Commands"; this Makefile is the single place
# the multi-step ones are named. `.github/workflows/ci.yml` runs `make ci` rather than
# re-specifying its stages in YAML, so the gate can't drift between local and CI: what Actions
# enforces is exactly what you can run yourself.
#
# `make` (no args, or `make help`) prints this listing grouped setup / dev / gates / coverage /
# model, built from the `##@ <group>` section markers and the `## <summary>` trailing each target
# line below — the Make analogue of the `[group(…)]` + `[doc(…)]` recipe attributes the justfile
# used to carry.

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
# Required by `calibrate` (deliberately no default — a wrong-vault default would silently
# calibrate against the wrong corpus); optional for `compare-device`, which defaults below.
VAULT ?=

##@ Setup

# what a fresh clone needs before anything else works

doctor: ## Sanity-check the local toolchain and print the fix for anything missing — run this first.
	-@scripts/doctor.sh

# The recipe always passes --force itself (cargo would otherwise refuse a same-version
# reinstall, since this stays 0.1.0) — just re-run `make install` to update after code changes.
install: ## Install the `b2` binary to ~/.cargo/bin (on PATH; no alias, works from any dir).
	cargo install --path crates/b2-cli --locked --force

uninstall: ## Remove the installed `b2` binary.
	cargo uninstall b2-cli

# Every recipe that needs node_modules depends on this, so you rarely run it by hand. A no-op
# `npm install` costs ~0.3s against an already satisfied lockfile — nothing next to the Tauri
# build it precedes — and that price buys the invariant that a pull adding a frontend dep
# can't leave you at a Vite "failed to resolve import" with node_modules a commit behind.
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

# Re-vendor the Bootstrap Icons subset into ui/src/icons.gen.ts (the names live in
# ui/scripts/gen-icons.ts). Run it after adding an icon to that manifest, or after bumping
# the bootstrap-icons devDependency — `make test-ui` runs the same generator in --check mode
# first, so a stale generated file fails the gate rather than shipping quietly.
icons: ui-install ## Regenerate ui/src/icons.gen.ts from the bootstrap-icons package.
	npm --prefix ui run icons

# Embed on the Metal GPU by default on Apple Silicon (GH #40); CPU everywhere else. The `metal`
# feature is a compile-time switch, so this selects it for the dev build — the runtime still
# falls back to CPU if the GPU can't initialize. `make app-cpu` forces CPU.
UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
METAL_FEATURE :=
ifeq ($(UNAME_S),Darwin)
ifeq ($(UNAME_M),arm64)
METAL_FEATURE := --features metal
endif
endif

# Point it at a vault with B2_VAULT_PATH, e.g. `B2_VAULT_PATH=~/notes make app`. Settings (⌘,)
# shows a CPU/Metal badge; switching device re-embeds the vault (a `@metal` model swap).
# Depends on ui-install because Tauri's beforeDevCommand shells out to `npm run dev` itself,
# outside `make` — so the deps have to be there before cargo hands off.
app: ui-install ## Run the desktop app in dev — auto-selects Metal on Apple Silicon; `make app-cpu` forces CPU.
	cd crates/b2-desktop && cargo tauri dev $(METAL_FEATURE)

app-cpu: ui-install ## Force the CPU embedder regardless of platform — the A/B counterpart to the default `make app`.
	cd crates/b2-desktop && cargo tauri dev

app-build: ui-install ## Bundle the desktop app (per-platform); builds the frontend first (beforeBuildCommand).
	cd crates/b2-desktop && cargo tauri build

##@ Gates

# Two tiers, plus the pieces they compose from.
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
# test-ui runs last, via a nested `$(MAKE)` call, rather than as a prerequisite: prerequisites
# run *before* the recipe body, and the cargo stages are the cheaper failures (0.1s fmt, 0.2s
# clippy) and the bulk of the code, so they should be what a broken commit trips on first.
# Nothing enforces this locally: there is no git hook, and none is wanted (.git/hooks isn't
# cloneable) — CI is the enforcement, and this is the fast subset you run before you get there.
check: ## Fast gate (~3s) — fmt-check, lint, engine + frontend tests. The one you run while working.
	cargo fmt --check
	cargo clippy --workspace --exclude b2-desktop -- -D warnings
	cargo test -p b2-core
	$(MAKE) test-ui

# The union of `check` + `check-app` + every test in the repo, with the overlap removed: run
# back-to-back those re-ran the engine suite, `ui-build`, and the frontend suite twice over.
# Note the absent `--exclude b2-desktop` — the `ui-build` prerequisite has already satisfied the
# desktop crate's `ui/dist` embed, so one clippy pass covers the whole workspace.
# Stage order is failure order: the cheap mechanical checks first (`no-tokio`, then fmt, then
# lint), the bulk of the code next, then the frontend suite, and `audit` last — it is the only
# stage that *queries a remote service to do its job*, asking the npm advisory database on every
# run, so a slow or unreachable registry can't delay a real failure above it. That is a narrower
# claim than "the only stage that touches the network", which was never quite true and is less
# so now: `ui-install` (upstream of `ui-build`) and `no-tokio` both reach for a registry when
# their lockfile is out of date. The difference is that against a satisfied lockfile they are
# offline no-ops, where `audit` goes out every time.
# `no-tokio` leads because it is a manifest-level check: it reads the lockfile and never compiles
# anything, so it costs a fraction of a second and its failure is structural.
ci: no-tokio ui-build ## Complete gate (~18s warm) — every mechanical check in one pass; exactly what CI runs.
	cargo fmt --check
	cargo clippy --workspace -- -D warnings
	cargo test
	$(MAKE) test-ui
	$(MAKE) audit

# GH #174. `hf-hub` shipped its async download backend (reqwest → hyper → h2 → tokio) under
# default features, so the whole async HTTP stack compiled into `b2` — at opt-3, per the
# workspace's dependency profile — to serve an API B2 never calls. Trimming it is what made
# "nothing in B2 is async" true of the *tree* and not just of the code. This is the guard on
# that, and it is the `-D warnings` lesson again in a new place: a dependency added with its
# defaults left on can put the stack back and nothing else in the gate would notice, because
# the result still builds and still passes — it just costs minutes of every cold build.
# Scoped to `b2-cli` because that is the shipped binary; `b2-desktop` legitimately links tokio,
# since **Tauri** does, and that is the host's runtime rather than one of ours.
# The obvious spelling of this check — `if cargo tree -i tokio; then fail; fi` — is wrong, and
# instructively so. `cargo tree -i <spec>` exits non-zero when the package is *absent*, so the
# passing case is the command failing, and inverting an exit code silently promotes every OTHER
# failure to a pass. That is not hypothetical: `-i` resolves a package *specification*, which
# also errors on an AMBIGUOUS one ("specification `windows-sys` is ambiguous", exit 101) — so the
# day some crate drags in a tokio 0.x beside the 1.x, the check reporting on it would go green.
# So detection doesn't use `-i` at all: line 1 resolves the tree and fails loudly if it can't,
# and line 2 asks the same, already-proven command for a flat package list and greps it. The
# formatting flags can't introduce a failure mode of their own, so nothing here can fail open.
# `^tokio v` is anchored because this is now a text match — `tokio-util` must not trip it.
# `--locked` is what keeps the recipe side-effect-free: `cargo tree` would otherwise rewrite
# Cargo.lock to serve the query, and this is the first stage of `ci`, whose last CI step asserts
# no stage edited a tracked file. Drift now fails here, naming itself, instead of surfacing later
# as an unexplained dirty worktree.
# Edges are cargo's default (normal + build + dev), deliberately WIDER than the shipped binary:
# a dev-dependency dragging the stack back in wouldn't ship, but it would still be compiled by
# `cargo test`, which is the cold-build cost this exists to protect. It costs nothing to be
# strict here — the tree is clean under every edge kind *and* under `--target all`.
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

# Defined here so the npm half of the repo is reachable from `make` like the cargo half.
test-ui: ui-install ## The frontend's pure-logic suite (node's own test runner over ui/src/**/*.test.ts).
	npm --prefix ui test

check-app: ui-build ## Lint the desktop crate alone (needs ui/dist, so the frontend builds first) — the heavy half of `check`.
	cargo clippy -p b2-desktop -- -D warnings

# The generalized version of the `-D warnings` lesson (GH #87 §3): a tool that reports a problem
# and exits 0 passes the gate green. `npm install` prints "1 high severity vulnerability" and
# exits 0 — which is how a high-severity postcss advisory (GHSA-r28c-9q8g-f849, path traversal
# via source-map auto-loading) rode along under vite through every recipe until it was noticed
# by hand. `npm audit` itself exits non-zero when the tree is vulnerable, so it gates directly.
# Deliberately NOT a prerequisite of `ui-install`: that is now upstream of `make app`, and a
# newly-published advisory in a transitive dep must not stop the app from launching. Equally not
# in `check` — this needs the network, and `check` is the 3s loop that has to work on a plane.
# `--audit-level=high` is an explicit threshold: low/moderate findings inform without blocking.
audit: ## Fail on a high-or-worse advisory in the frontend dep tree (needs network; runs as part of `make ci`).
	npm --prefix ui audit --audit-level=high

##@ Coverage

# cargo-llvm-cov (`cargo install cargo-llvm-cov` + `rustup component add llvm-tools-preview` —
# both, and the component is per-toolchain; `make doctor` checks for them)
#
# Source-based coverage over the same model-free suite CI runs — the numbers answer
# "which engine lines does the deterministic suite actually execute", so an untested
# branch shows up as a gap rather than as a hole nobody named. Real-model paths
# (b2-embed's candle code) are deliberately out: they are exercised by `make eval`,
# not by `cargo test`, so instrumenting them would report a permanent, meaningless 0%.

# Mirrors `make test` (b2-core only), so it is as fast as the suite itself and pulls in no ML deps.
coverage: ## Engine line/region coverage — the daily number.
	cargo llvm-cov -p b2-core

coverage-html: ## The same run as a browsable per-line HTML report under target/llvm-cov/html.
	cargo llvm-cov -p b2-core --html
	@echo "report: target/llvm-cov/html/index.html"

coverage-lcov: ## lcov.info for editor gutters (VS Code Coverage Gutters, etc.) or a CI upload.
	cargo llvm-cov -p b2-core --lcov --output-path target/llvm-cov/lcov.info
	@echo "lcov: target/llvm-cov/lcov.info"

# The CLI suite spawns the real `b2` binary, which is instrumented too, so its process-level
# runs count. Heavier on a cold cache: b2-cli depends on b2-embed, so this compiles candle once
# (excluded crates are still built when a covered crate depends on them).
coverage-all: ## Engine + the CLI adapter — its tests spawn the instrumented binary, so those runs count.
	cargo llvm-cov --workspace --exclude b2-desktop --exclude b2-embed

# Separate and heavier for the same reason `check-app` is: it embeds ui/dist (so the frontend
# builds first) and needs the platform webview toolchain, exactly like `make app`. Expect a low
# number by design — b2-desktop is a dumb adapter, and the behaviour behind its commands is
# covered by the façade suite (crates/b2-desktop/CLAUDE.md, "inherited tests").
coverage-app: ui-build ## Coverage for the desktop host's own unit tests.
	cargo llvm-cov -p b2-desktop

##@ Model

# The eval suite. Never part of `cargo test` or CI — the scored runs need a provisioned
# model, append to gitignored results logs, and their numbers are a property of the machine's
# model/device/build (reproducible per machine, never across machines or devices; `eval-chat`
# is the genuinely non-deterministic one, since LLM output varies run to run). Run these on
# demand. `stability` is the exception that proves the group: it measures retrieval
# *sensitivity* rather than model quality, so it is machine-independent (fake embedder) and
# needs no model — but it is the other half of the same harness (GH #141).
#
# **crates/b2-embed/evals/README.md is the guide**: what each instrument measures, how to
# read every block it prints, the exit gate, and the process rules that bind any edit to the
# corpora, the labels, or the metrics. The recipe comments here stay operational only.

init: ## Download + verify bge-base-en-v1.5 into the shared XDG cache (needed for the real embedder).
	cargo run -p b2-cli -- init

# Both labelled corpora through the real pipeline; appends rows to
# crates/b2-embed/evals/results.jsonl (gitignored); exits non-zero on an exit-gate regression.
eval: ## Semantic-retrieval + discovery quality eval (real model).
	cargo run -p b2-embed --example eval

eval-sweep: ## `make eval` plus the in-process chunker A/B (ChunkConfig sweep) — the GH #44 gate.
	cargo run -p b2-embed --example eval -- --sweep

# One lever, isolated: chunks_fts rebuilt under each tokenizer over identical chunks and
# vectors, so every rank move is the tokenizer's alone.
eval-stemmer: ## `make eval` plus the FTS tokenizer ablation (shipped porter vs unstemmed unicode61) — the GH #157 instrument.
	cargo run -p b2-embed --example eval -- --stemmer

# What `make eval` mostly cannot see (GH #141): candidate width. Deterministic (fake
# embedder), so it needs no `make init`. Flags via ARGS: --verbose (show diverging rankings),
# --model (real bge magnitude, no baseline), --vault <path> (any vault).
stability: ## Rank-stability probe on fixtures/test-vault: pool sensitivity + drift vs the blessed baseline (GH #141). Usage: make stability [ARGS="--verbose"]
	cargo run -p b2-embed --example stability -- $(ARGS)

# Only after a ranking change is the intended one — the snapshot is unlabelled, so it records
# what the ranking IS, never that it got better.
stability-bless: ## Accept the current ranking as the committed rank-stability baseline.
	cargo run -p b2-embed --example stability -- --bless

# Process rule 5's instrument: a constant derived from a corpus's score *distribution* is
# invalid until transfer-checked on a real vault, and this is the check — a pure read over
# stored vectors, no labels needed. Flags via ARGS: --limit N, --leader-z/--member-z,
# --mutual-k N, --json; ARGS=--search adds the search-side transfer check (GH #201/#206),
# the one part that loads the real model (to embed the probe queries).
calibrate: ## Calibration on any built vault: discovery pools/bands (GH #197) and, with ARGS=--search, the evidence bar (GH #201). Usage: make calibrate VAULT=<path> [ARGS=--search]
	@if [ -z "$(VAULT)" ]; then echo "error: VAULT is required, e.g. make calibrate VAULT=fixtures/test-vault"; exit 1; fi
	cargo run -p b2-embed --example calibrate -- "$(VAULT)" $(ARGS)

# Compare its retrieval quality against `make eval` (CPU) — a device switch is a model swap
# (the recorded model id gains an `@metal` tag), so the vault re-embeds.
eval-metal: ## The same eval, embedding on the Metal GPU (GH #40, macOS-only).
	cargo run -p b2-embed --example eval --features metal

# Reindexes an isolated copy on each device and reports chunks/s + speedup. Never mutates the
# committed fixture; artifacts are gitignored + cleaned up.
compare-device: ## CPU-vs-Metal embed throughput A/B on a vault (default fixtures/test-vault; GH #40, macOS-only). Usage: make compare-device [VAULT=<path>]
	scripts/compare-embed-device.sh "$(if $(VAULT),$(VAULT),fixtures/test-vault)"

# The chat seam's out-of-CI half (GH #154): the labelled questions in
# crates/b2-llm/evals/questions.json through the real `Vault::ask`. Needs a model server
# (`ollama serve`, or point B2_LLM_URL at any OpenAI-compatible one; B2_LLM_MODEL picks the
# model). Appends rows to crates/b2-llm/evals/results.jsonl (gitignored).
eval-chat: ## Grounded-chat quality eval: citation accuracy + refusal over the eval corpus (real model + model server).
	cargo run -p b2-llm --example groundedness
