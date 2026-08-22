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
[group('gates')]
[doc('Complete gate (~18s warm) — every mechanical check in one pass; exactly what CI runs.')]
ci: no-tokio ui-build && test-ui audit
    cargo fmt --check
    cargo clippy --workspace -- -D warnings
    cargo test

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
[group('gates')]
[doc("Fail if tokio is back in the `b2` binary's dependency tree (GH #174).")]
no-tokio:
    @cargo tree -p b2-cli --locked > /dev/null
    @if cargo tree -p b2-cli --locked --prefix none --format '{p}' | grep -Eq '^tokio v'; then \
        echo "error: tokio is in b2-cli's dependency tree again (GH #174) —"; \
        echo "       a dependency is probably pulling an async HTTP stack via default features:"; \
        cargo tree -p b2-cli --locked --invert tokio || true; \
        exit 1; \
    fi

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
# appends every run to crates/b2-embed/evals/results.jsonl (gitignored). It also prints the
# three calibration blocks that gate nothing and exist to price the next rule: the discovery z
# dump (GH #187), the search evidence dump (GH #201), and the fold bake-off (GH #200) — which
# re-derives a disclosure rule's admissible window from the run rather than quoting a constant.
[group('model')]
[doc('Semantic-retrieval + discovery quality eval (real model).')]
eval:
    cargo run -p b2-embed --example eval

[group('model')]
[doc('`just eval` plus the in-process chunker A/B (ChunkConfig sweep) — the GH #44 gate.')]
eval-sweep:
    cargo run -p b2-embed --example eval -- --sweep

# One lever, isolated: `Vault::rebuild_fts` swaps chunks_fts between the shipped
# `porter unicode61` (schema v5 — the GH #157 verdict) and the unstemmed `unicode61` it
# retired, over the identical chunk rows and vectors — nothing re-chunks or re-embeds, so
# every rank move is the tokenizer's alone. BM25-only is scored under both tokenizers (the
# honest lexical ablation), hybrid under both; the dense column doubles as an instrument
# check (FTS cannot reach it, so movement there means the harness broke). The paired
# per-query win/loss list is the readout; the standing precision probes
# (universe/university, the git code-literal queries) guard the stemmed side of the trade.
[group('model')]
[doc('`just eval` plus the FTS tokenizer ablation (shipped porter vs unstemmed unicode61) — the GH #157 instrument.')]
eval-stemmer:
    cargo run -p b2-embed --example eval -- --stemmer

# What `just eval` mostly cannot see (GH #141): candidate-width. Since GH #183 its corpus
# (63 chunks) does truncate the 60-chunk passage view by a hair, but the 150-candidate note
# view is never cut there. This probe asks the same queries at widening pools on a
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

# GH #196's hand arithmetic, promoted into a command (GH #197, Phase 0a): run it against ANY
# built vault — no labels needed, a pure read over stored vectors — and it prints per-anchor
# pool distributions (cosine min/med/max, leader cosine + z), what a z existence gate would
# serve vs what always-serve does, and the strength bands the pane would paint, plus a
# vault-level summary. This is process rule 5's instrument: a constant derived from a corpus's
# score *distribution* is invalid until transfer-checked on a real vault, and this is the
# check. Flags: `--limit N` (pane depth), `--leader-z/--member-z` (replay a candidate gate;
# defaults are the retired GH #192 constants, so GH #196's dark-vault reading reproduces),
# `--json` (the whole reading as one object).
[group('model')]
[doc('Discovery calibration on any built vault: pool cosines, leader z, gate-vs-always-serve, bands (GH #197).')]
calibrate vault *args:
    cargo run -p b2-embed --example calibrate -- "{{vault}}" {{args}}

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

# The chat seam's out-of-CI half (GH #154), and the second AI seam's answer to `just eval`:
# it asks the labelled questions in crates/b2-llm/evals/questions.json through the real
# `Vault::ask` — real embedder to retrieve, real chat model to answer — and scores
# groundedness, citation accuracy, and refusal on the deliberate negatives. Needs a model
# server running (`ollama serve`, or point B2_LLM_URL at any OpenAI-compatible one);
# B2_LLM_MODEL picks the model. Appends one row to crates/b2-llm/evals/results.jsonl
# (gitignored). Retrieval reach is reported beside the chat numbers, because the model can
# only cite what retrieval handed it — a miss there is `just eval`'s result, not this one's.
[group('model')]
[doc('Grounded-chat quality eval: citation accuracy + refusal over the eval corpus (real model + model server).')]
eval-chat:
    cargo run -p b2-llm --example groundedness
