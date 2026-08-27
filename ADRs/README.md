# Architecture Decision Records

One file per **key architectural choice** — the decisions that would be expensive to reverse and
whose *why* is not readable off the code. Not a changelog: features, bug fixes and tuning do not get
an ADR. Feature history lives in [GitHub Issues](https://github.com/AlteredCraft/B2/issues) and git.

**Keep them terse** — Context / Decision / Consequences, a page at most. When a decision is
overturned, mark the old ADR `Superseded by ADR-NNNN` and write a new one; never edit history.

Normative detail lives in `docs/invariants.md` (the register — it wins on conflict); these
records say *why* an entry reads the way it does.

| # | Decision |
|---|---|
| [0001](0001-design-docs-are-normative.md) | Design docs are normative; the code is a projection of them |
| [0002](0002-vault-is-truth-index-is-a-disposable-projection.md) | The vault is the source of truth; the index is a disposable projection |
| [0003](0003-note-identity-is-the-vault-relative-path.md) | A note's identity is its vault-relative path |
| [0004](0004-markdown-is-the-only-surface-b2-writes.md) | Markdown is the only surface B2 writes, and B2 makes no unbidden writes |
| [0005](0005-ai-behind-enumerated-seams.md) | AI lives behind two enumerated seams; the core is model-free |
| [0006](0006-vectors-in-plain-tables-content-addressed.md) | Vectors live in plain SQLite tables, scored in-process, content-addressed |
| [0007](0007-embedding-space-identity-includes-the-device.md) | The embedding space has one recorded identity, and the device is part of it |
| [0008](0008-hybrid-retrieval-bm25-plus-vector-fused-by-rrf.md) | Retrieval is BM25 ⊕ vector KNN, fused by RRF |
| [0009](0009-discovery-is-model-free-at-surface-time.md) | Discovery surfaces candidates; the human commits the link |
| [0010](0010-typed-graph-two-homes-closed-vocabulary.md) | The typed graph: two authored homes, frontmatter-wins, a closed verb core |
| [0011](0011-no-async-runtime-in-b2-code.md) | No B2 code is async; no B2 crate starts a runtime |
| [0012](0012-one-typed-facade-two-dumb-adapters.md) | One typed façade, two dumb adapters |
| [0013](0013-model-quality-is-measured-out-of-ci.md) | Model quality is measured out of CI, by a labelled harness |
| [0014](0014-discovery-always-serves-the-ranked-prefix.md) | Discovery always serves the ranked top-N; no anchor-local existence gate |
| [0015](0015-a-served-search-result-is-a-claim-of-evidence.md) | A served search result is a claim of evidence |
| [0016](0016-rendering-note-content-is-a-trust-boundary.md) | Rendering note content is a trust boundary |
| [0017](0017-the-gui-is-keyboard-complete-with-one-chord-registry.md) | The GUI is keyboard-complete, and every chord lives in one registry |
| [0018](0018-ci-runs-make-ci-and-nothing-else.md) | CI runs `make ci` and nothing else |
| [0019](0019-build-our-own-sqlite-index-engine.md) | Build our own SQLite index engine; qmd is a reference, not a dependency |
| [0020](0020-embeddings-inside-the-single-binary.md) | Embeddings inside the single binary: candle + hf-hub, provisioned by `b2 init` |
| [0021](0021-concurrency-is-serialized-on-sqlites-own-locks.md) | Many readers, one writer, serialized on SQLite's own locks |
