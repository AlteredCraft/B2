# Test fixtures

Two committed vaults for two different jobs. This file sits outside both, because B2 treats
every `.md` under a vault root as a note.

## `golden-vault/`

The small hand-written vault the deterministic integration tests run against: two linked
notes and a few resources ([docs/data-model.md](../docs/data-model.md), "The golden vault").
Tests copy it into a temp dir first (`crates/b2-core/tests/common/mod.rs`), so no suite can
change it. Change it only together with the tests.

## `test-vault/`

A synthetic, generated vault: about 200 notes and 790 chunks across 10 topics, cross-linked
by about 2,300 body links and a few typed ones. It exists for out-of-CI work that needs
volume:

- **Rank stability** (`make stability`). The labelled eval corpus is too small for the
  candidate pools to bind; this vault is big enough. **Editing it invalidates the blessed
  baseline**, so run `make stability-bless` in the same commit.
- **Embed throughput** (`make compare-device`): CPU against the Metal GPU.
- **Eyeballing** `b2 search` and `b2 similar` on real clusters.

The prose is templated and not meant to be read, and this is not the labelled eval corpus
(that lives in `crates/b2-embed/evals/`). Both instruments work on a throwaway copy, and
indexing never writes to a vault anyway (W1).
