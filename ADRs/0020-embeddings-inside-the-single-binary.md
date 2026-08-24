# ADR-0020 — Embeddings inside the single binary: candle + hf-hub, provisioned by `b2 init`

- **Status:** Accepted · 2026-06-30 (built 2026-07-01)
- **Refs:** invariants M1, M2 · `index-engine.md` §6 · GH #40

## Context

Semantic search needs vectors from somewhere, and this is the one place the architecture meets real
friction — independent of the store (ADR-0019). qmd's answer (`node-llama-cpp` + auto-downloaded
GGUF + a JS runtime) tensions B2's single-binary, no-install-ritual goal directly.

## Decision

- **Runtime: `candle` + `hf-hub`** — pure-Rust inference compiled into the binary, no external ONNX
  Runtime to ship, with `hf-hub` as the download seam.
- **Not bundled, and never a surprise download.** An explicit **`b2 init`** downloads and verifies the
  model into a shared XDG cache; `reindex`/`search` fail fast with "run `b2 init`" if it is absent.
- **The model source is configurable** (default an HF repo id; overridable to a mirror, another repo,
  or a local path for offline installs) via `$XDG_CONFIG_HOME/b2/config.toml`.
- **Default model `BAAI/bge-base-en-v1.5`** (BERT-family, 768-dim, ungated), CLS-pooled and
  L2-normalized, with bge's asymmetric query prefix. **EmbeddingGemma-300M was the first choice and
  lost on friction**: it is *gated* on Hugging Face (401 without a token + license acceptance), which
  defeats a friction-free `b2 init`. It stays selectable for anyone who provides a token.
- The dimension is read authoritatively from the model's own `config.json` (`hidden_size`), so
  configuration cannot lie about it.

## Consequences

- All heavy ML deps stay in `b2-embed` alone; `b2-core` never links candle (ADR-0005), and the fake
  embedder is the CI default, so model quality never enters the fast suite.
- The recorded embedding-space identity and the device-swap rules that follow are ADR-0007.
