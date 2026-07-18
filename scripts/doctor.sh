#!/usr/bin/env bash
# Environment sanity check for local development — "stop 0" before `just app` works.
#
# Walks the same order a fresh clone hits in the README's Build-and-run: Rust (needed for
# every recipe) -> Node/npm + the Tauri CLI (desktop app only) -> the platform's native
# webview toolchain -> optional extras (the embedding model; B2_VAULT_PATH is purely FYI —
# `just app` works with or without it via the in-app vault switcher). Each check
# prints pass/fail/warn with the fix inline, so a broken environment becomes a checklist
# instead of a scavenger hunt — this script exists because `just app` failing with
# "error: no such command: `tauri`" gives no hint that the fix is a separate `cargo install`.
#
# Usage: `just doctor` (or run directly: scripts/doctor.sh). Exits 0 if every hard
# requirement passes; warnings (optional extras) don't fail the run.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

FAILS=0
WARNS=0

# ANSI colors, no 3rd-party dep (just raw escapes) — off when stdout isn't a terminal
# (piped/redirected output) or NO_COLOR is set (https://no-color.org), so logs/files stay plain.
if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
  C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'
  C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'; C_RED=$'\033[31m'
  C_RESET=$'\033[0m'
else
  C_BOLD=''; C_DIM=''; C_GREEN=''; C_YELLOW=''; C_RED=''; C_RESET=''
fi

pass() { printf '  %s[ok]%s   %s\n' "$C_GREEN" "$C_RESET" "$1"; }
fail() { printf '  %s[FAIL]%s %s\n' "$C_RED" "$C_RESET" "$1"; FAILS=$((FAILS + 1)); }
warn() { printf '  %s[warn]%s %s\n' "$C_YELLOW" "$C_RESET" "$1"; WARNS=$((WARNS + 1)); }
info() { printf '  %s[--]%s   %s\n' "$C_DIM" "$C_RESET" "$1"; }
section() { printf '\n%s%s%s\n' "$C_BOLD" "$1" "$C_RESET"; }

# --- Rust toolchain (every recipe needs this) -----------------------------------------------
section "Rust toolchain"
if ! command -v cargo >/dev/null 2>&1 && ! command -v rustup >/dev/null 2>&1; then
  fail "no cargo/rustup found — install via https://rustup.rs:  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
else
  if command -v rustup >/dev/null 2>&1; then
    pass "rustup found ($(rustup --version 2>/dev/null | head -1))"
  else
    warn "cargo found but not via rustup — rust-toolchain.toml (pins 1.96) won't auto-apply; if builds complain about MSRV, install rustup instead"
  fi
  # Running cargo here resolves (and, via rustup, auto-installs) the toolchain pinned in
  # rust-toolchain.toml — the same thing `just build`/`just app` would do on first run, so
  # doing it here just surfaces that cost up front instead of mid-build.
  if VERSION_OUT="$(cargo --version 2>&1)"; then
    pass "cargo resolves: $VERSION_OUT"
  else
    fail "cargo failed to resolve a toolchain for this repo: $VERSION_OUT"
  fi
fi

# --- just itself (informational — you're running this via `just doctor` most likely) --------
section "just"
if command -v just >/dev/null 2>&1; then
  pass "just found ($(just --version))"
else
  warn "just not found — install with: brew install just (or: cargo install just). cargo/cargo run still work without it."
fi

# --- Node + npm (needed for ui/, the desktop frontend) ---------------------------------------
# Version floor comes from vite (ui/package-lock.json pins vite@6, engines.node =
# "^18.0.0 || ^20.0.0 || >=22.0.0") — the strictest of the frontend's deps. Note that range
# excludes the odd-numbered releases (19, 21, ...): those are Node's short-lived non-LTS
# lines, not vite-incompatible per se, but unsupported here — install an LTS line instead.
section "Node.js + npm (desktop app frontend)"
nvm_available() {
  [[ -n "${NVM_DIR:-}" && -s "${NVM_DIR}/nvm.sh" ]] || [[ -s "$HOME/.nvm/nvm.sh" ]]
}
node_install_hint() {
  if nvm_available; then
    echo "you have nvm — run: nvm install --lts && nvm use --lts"
  else
    echo "install via nvm (recommended: https://github.com/nvm-sh/nvm, then: nvm install --lts), https://nodejs.org, or: brew install node"
  fi
}
if command -v node >/dev/null 2>&1; then
  NODE_VERSION_STR="$(node --version)"
  NODE_MAJOR="${NODE_VERSION_STR#v}"
  NODE_MAJOR="${NODE_MAJOR%%.*}"
  if [[ "$NODE_MAJOR" =~ ^[0-9]+$ ]]; then
    if (( NODE_MAJOR == 18 || NODE_MAJOR == 20 || NODE_MAJOR >= 22 )); then
      pass "node found ($NODE_VERSION_STR)"
    elif (( NODE_MAJOR < 18 )); then
      fail "node $NODE_VERSION_STR is too old — vite 6 needs Node 18, 20, or 22+; $(node_install_hint)"
    else
      warn "node $NODE_VERSION_STR is an odd-numbered (non-LTS) release — vite 6 requires Node 18, 20, or 22+ and may misbehave on $NODE_MAJOR; $(node_install_hint)"
    fi
  else
    pass "node found ($NODE_VERSION_STR)"
  fi
else
  fail "node not found — $(node_install_hint)"
fi
if command -v npm >/dev/null 2>&1; then
  pass "npm found ($(npm --version))"
else
  fail "npm not found — comes with Node; $(node_install_hint)"
fi
if [[ -d "ui/node_modules" ]] && [[ -n "$(ls -A ui/node_modules 2>/dev/null)" ]]; then
  pass "ui/node_modules present"
else
  fail "ui/ dependencies not installed — run: just ui-install"
fi

# --- Tauri CLI (the one that broke: `just app` needs `cargo tauri`) -------------------------
section "Tauri CLI"
if TAURI_VERSION_OUT="$(cargo tauri --version 2>&1)"; then
  pass "cargo-tauri found ($TAURI_VERSION_OUT)"
  if [[ "$TAURI_VERSION_OUT" != *" 2."* ]]; then
    warn "expected a 2.x Tauri CLI (this project's tauri.conf.json is schema v2); found: $TAURI_VERSION_OUT — if \`just app\` fails with a config error, run: cargo install tauri-cli --locked --force"
  fi
else
  fail "cargo-tauri not found — run: cargo install tauri-cli --locked"
fi

# --- Platform native build toolchain (needed to compile the Tauri host + candle) ------------
section "Platform build toolchain"
case "$(uname -s)" in
  Darwin)
    if xcode-select -p >/dev/null 2>&1; then
      pass "Xcode Command Line Tools installed ($(xcode-select -p))"
    else
      fail "Xcode Command Line Tools not found — run: xcode-select --install"
    fi
    ;;
  Linux)
    if command -v pkg-config >/dev/null 2>&1 && { pkg-config --exists webkit2gtk-4.1 2>/dev/null || pkg-config --exists webkit2gtk-4.0 2>/dev/null; }; then
      pass "webkit2gtk found via pkg-config"
    else
      warn "webkit2gtk (+ friends) not detected — Tauri's Linux prerequisites (build-essential, webkit2gtk-4.1-dev, libssl-dev, libayatana-appindicator3-dev, librsvg2-dev) may be missing; see https://v2.tauri.app/start/prerequisites/"
    fi
    ;;
  *)
    warn "unrecognized platform ($(uname -s)) — this repo's desktop-app path is developed on macOS; native build prerequisites are unverified here"
    ;;
esac

# --- Optional: the real embedder's model cache (skip entirely with B2_EMBEDDER=fake) --------
section "Embedding model (optional — skip if you use B2_EMBEDDER=fake)"
case "$(uname -s)" in
  Darwin) MODEL_CACHE="$HOME/Library/Application Support/b2/models/BAAI_bge-base-en-v1.5" ;;
  Linux) MODEL_CACHE="${XDG_DATA_HOME:-$HOME/.local/share}/b2/models/BAAI_bge-base-en-v1.5" ;;
  *) MODEL_CACHE="" ;;
esac
if [[ -n "$MODEL_CACHE" && -f "$MODEL_CACHE/model.safetensors" ]]; then
  pass "default model (bge-base-en-v1.5) found in cache"
else
  warn "default model not found at the default cache path — this only checks the default location (a custom cache_dir in config.toml won't show up here); run: just init (idempotent — a no-op if already installed). Not needed if you run with B2_EMBEDDER=fake."
fi

# --- Informational: B2_VAULT_PATH --------------------------------------------------------
# Not required either way: `just app` opens the in-app vault switcher when unset. This is
# purely FYI, so it never counts as a warning.
section "Vault (informational)"
if [[ -n "${B2_VAULT_PATH:-}" ]]; then
  info "B2_VAULT_PATH set: $B2_VAULT_PATH — just app will open this vault directly"
else
  info "B2_VAULT_PATH not set — just app will open the in-app vault switcher (or set it to jump straight to a vault)"
fi

# --- Summary ---------------------------------------------------------------------------------
section "Summary"
if [[ "$FAILS" -eq 0 ]]; then
  printf '  %sAll required checks passed%s (%d warning(s)). Try: just app\n' "$C_GREEN" "$C_RESET" "$WARNS"
  exit 0
else
  printf '  %s%d check(s) failed%s, %d warning(s). Fix the [FAIL] items above, then re-run: just doctor\n' "$C_RED" "$FAILS" "$C_RESET" "$WARNS"
  exit 1
fi
