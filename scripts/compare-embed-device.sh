#!/usr/bin/env bash
# CPU vs Metal embedding A/B (GH #40).
#
# Reindexes a vault twice — once on CPU (default build) and once on the Metal GPU
# (`--features metal`) — and reports embed throughput side by side. The `metal` cargo
# feature is a BUILD switch, so this compiles both binaries; the recorded model id gains an
# `@metal` tag on the GPU run, which the report uses to confirm Metal was actually used
# (not a silent CPU fallback).
#
# Hygiene:
#   - The committed fixture is NEVER mutated: each run works on an isolated copy in the
#     system tempdir (same pattern the integration tests use for fixtures/golden-vault/),
#     removed on exit. So no `.b2/` index or b2id churn ever lands in the repo.
#   - The two JSONL logs go under the already-gitignored logs/ for post-hoc inspection.
#   - `.gitignore` also covers an ad-hoc `b2 reindex` run against the fixture directly.
#
# Usage:
#   scripts/compare-embed-device.sh [VAULT]     # VAULT defaults to fixtures/test-vault
#   KEEP=1 scripts/compare-embed-device.sh       # keep the tempdir (prints its path)
#
# macOS/Apple-Silicon only (Metal). Needs the bge model provisioned (`cargo run -p b2-cli -- init`).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
VAULT="${1:-fixtures/test-vault}"

# --- preflight ---------------------------------------------------------------------------
[[ "$(uname)" == "Darwin" ]] || { echo "error: Metal is macOS-only." >&2; exit 1; }
command -v python3 >/dev/null || { echo "error: python3 is required." >&2; exit 1; }
[[ -d "$VAULT" ]] || { echo "error: vault not found: $VAULT" >&2; exit 1; }

# Soft check for the default model cache; a custom cache_dir in config.toml is still fine
# (reindex fails fast with the proper "run b2 init" message if the model is truly absent).
MODELDIR="${HOME}/Library/Application Support/b2/models/BAAI_bge-base-en-v1.5"
[[ -f "${MODELDIR}/model.safetensors" ]] || \
  echo "note: bge model not found at the default cache; if reindex fails, run: cargo run -p b2-cli -- init" >&2

mkdir -p logs
CPU_LOG="${REPO}/logs/embed-compare-cpu.jsonl"
METAL_LOG="${REPO}/logs/embed-compare-metal.jsonl"
: > "$CPU_LOG"; : > "$METAL_LOG"   # clean starting state

# --- warm both builds (keep compile time out of the measured runs; fail early) -----------
echo "→ building b2-cli (CPU, then --features metal)…"
cargo build -q -p b2-cli
cargo build -q -p b2-cli --features metal

# --- isolated work copies (never touch the committed fixture) ----------------------------
RUN="$(mktemp -d)"
cleanup() {
  if [[ -n "${KEEP:-}" ]]; then echo "kept tempdir: $RUN"; else rm -rf "$RUN"; fi
}
trap cleanup EXIT
cp -R "$VAULT" "$RUN/cpu"
cp -R "$VAULT" "$RUN/metal"
rm -rf "$RUN/cpu/.b2" "$RUN/metal/.b2"   # each device does a full, from-scratch embed

# --- the two runs ------------------------------------------------------------------------
# Setting B2_LOG_FILE alone turns on the JSONL debug log (implies B2_LOG=debug); the CLI's
# stdout stays the plain "Indexed N notes" line, the file is pure JSONL.
echo "→ CPU reindex (this is the slow one)…"
B2_LOG_FILE="$CPU_LOG" cargo run -q -p b2-cli -- -C "$RUN/cpu" reindex

echo "→ Metal reindex…"
B2_LOG_FILE="$METAL_LOG" cargo run -q -p b2-cli --features metal -- -C "$RUN/metal" reindex

# --- report ------------------------------------------------------------------------------
echo
python3 - "$CPU_LOG" "$RUN/cpu/.b2/b2.sqlite" "$METAL_LOG" "$RUN/metal/.b2/b2.sqlite" <<'PY'
import sys, json, sqlite3
from datetime import datetime

def parse_log(path):
    """Last embed cycle's (start_ts, end_ts, chunks, notes) from the JSONL milestones."""
    start = end = chunks = notes = None
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            msg = e.get("message", "")
            if msg.startswith("embed pass starting"):
                start = e.get("timestamp")
            elif msg == "embed pass complete":
                end = e.get("timestamp")
                chunks = e.get("chunks_embedded")
                notes = e.get("notes_embedded")
    return start, end, chunks, notes

def seconds(a, b):
    if not a or not b:
        return None
    f = lambda t: datetime.fromisoformat(t.replace("Z", "+00:00"))
    return (f(b) - f(a)).total_seconds()

def model_id(db):
    try:
        c = sqlite3.connect(db)
        row = c.execute("SELECT value FROM meta WHERE key='embed_model_id'").fetchone()
        c.close()
        return row[0] if row else "?"
    except sqlite3.Error:
        return "?"

cpu_log, cpu_db, metal_log, metal_db = sys.argv[1:5]
runs = []
for name, log, db in [("CPU", cpu_log, cpu_db), ("Metal", metal_log, metal_db)]:
    s, e, ch, no = parse_log(log)
    runs.append({"name": name, "model": model_id(db), "chunks": ch, "notes": no,
                 "secs": seconds(s, e)})

cpu, metal = runs
notes = cpu["notes"] if cpu["notes"] is not None else "?"
chunks = cpu["chunks"] if cpu["chunks"] is not None else "?"

print("=" * 68)
print("  Embedding device comparison — GH #40")
print(f"  Workload: {notes} notes / {chunks} chunks")
print("=" * 68)
hdr = f"  {'Device':<7} {'Chunks':>7} {'Embed time':>12} {'Throughput':>13}   Model id"
print(hdr)
print("  " + "-" * (len(hdr) - 2))
for r in runs:
    secs = r["secs"]
    if secs and r["chunks"]:
        tput = f"{r['chunks'] / secs:.1f} ch/s"
        tstr = f"{secs:.1f} s"
    else:
        tput, tstr = "n/a", "n/a"
    print(f"  {r['name']:<7} {str(r['chunks']):>7} {tstr:>12} {tput:>13}   {r['model']}")
print()

# Speedup + a sanity check that Metal was genuinely used (the @metal id tag).
if cpu["secs"] and metal["secs"] and metal["secs"] > 0:
    print(f"  Metal speedup: {cpu['secs'] / metal['secs']:.2f}x faster than CPU")
if not str(metal["model"]).endswith("@metal"):
    print("  ⚠ Metal run did NOT record an '@metal' model id — it fell back to CPU.")
    print("    Rebuild check: `cargo build -p b2-cli --features metal`. Results are not a real A/B.")
if cpu["chunks"] != metal["chunks"]:
    print(f"  ⚠ Chunk counts differ (CPU {cpu['chunks']} vs Metal {metal['chunks']}) — not comparable.")
print("=" * 68)
PY
