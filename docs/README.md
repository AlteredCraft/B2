# B2 docs

The documentation for B2: a personal, local-first Markdown vault with an AI layer that
surfaces semantically similar notes for you to connect. Start here to find the one page that
answers your question.

Every topic has one home. The specs are normative: the code is a projection of them, and code
comments cite them by section and by invariant id (`data-model.md §2`, `invariants.md D1`).
On any conflict, [invariants.md](invariants.md) wins and the other side gets fixed.

## Which page do you need?

| You want to… | Read |
|---|---|
| Set up B2 and use it (install, index, search, link, chat) | [quickstart.md](quickstart.md) |
| Understand what search and the related-notes panel do, in plain language | [search-and-similarity.md](search-and-similarity.md) |
| Orient yourself in the codebase (crates, flows, seams, tests) | [architecture.md](architecture.md) |
| Know what must always be true, cited by id (S2, D1, …) | [invariants.md](invariants.md) |
| Know what a note and a connection are (frontmatter, links, the verb core) | [data-model.md](data-model.md) |
| Know how the index is built and queried (schema, ingest, search, discovery, chat) | [index-engine.md](index-engine.md) |
| Measure retrieval and chat quality, or edit the eval corpora and labels | [evals.md](evals.md) |
| Know *why* a decision reads the way it does | [ADRs/](../ADRs/README.md) |

## Also worth knowing

- The backlog and planned work live in
  [GitHub Issues](https://github.com/AlteredCraft/B2/issues); build history lives in git.
- The command reference and every environment variable live in
  [quickstart.md](quickstart.md).
- Working in the repo? The contributor ground rules are [CLAUDE.md](../CLAUDE.md), and the
  desktop crate has its own in
  [crates/b2-desktop/CLAUDE.md](../crates/b2-desktop/CLAUDE.md).
