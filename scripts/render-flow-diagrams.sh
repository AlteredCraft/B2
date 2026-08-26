#!/usr/bin/env bash
# Render the Mermaid sources in docs/design/retrieval-flows.md to the committed SVGs
# the doc embeds. GitHub renders ```mermaid fences but the GitHub Pages docs site does
# not, so the doc shows pre-rendered SVGs and keeps each fence beneath its image as the
# editable source — after editing a fence, run this and commit both.
#
# Needs npx (the ui/ toolchain already requires node); the first run fetches
# @mermaid-js/mermaid-cli. In a sandboxed environment, point PUPPETEER_CONFIG_FILE at a
# JSON file with the browser executablePath / --no-sandbox args mermaid-cli should use.
set -euo pipefail
cd "$(dirname "$0")/.."

doc=docs/design/retrieval-flows.md
out=docs/design/assets
# One name per ```mermaid fence, in document order — a new fence needs a name here.
names=(search-ranking search-verdict similar-discovery)

mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

awk -v dir="$tmp" '
  /^```mermaid$/ { in_block = 1; n++; file = sprintf("%s/%02d.mmd", dir, n); next }
  /^```$/ && in_block { in_block = 0; next }
  in_block { print > file }
' "$doc"

count=$(find "$tmp" -name '*.mmd' | wc -l | tr -d ' ')
if [ "$count" -ne "${#names[@]}" ]; then
  echo "expected ${#names[@]} mermaid blocks in $doc, found $count — update names= in $0" >&2
  exit 1
fi

extra=()
[ -n "${PUPPETEER_CONFIG_FILE:-}" ] && extra=(-p "$PUPPETEER_CONFIG_FILE")

i=1
for name in "${names[@]}"; do
  src=$(printf '%s/%02d.mmd' "$tmp" "$i")
  npx -y @mermaid-js/mermaid-cli -i "$src" -o "$out/$name.svg" -b white "${extra[@]}"
  i=$((i + 1))
done
echo "rendered ${#names[@]} diagrams into $out/"
