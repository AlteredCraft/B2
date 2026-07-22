---
title: "B2 — Invariants"
type: note
tags: [b2, invariants, architecture, canonical]
created: 2026-07-22
status: active
---

# B2 — Invariants

> The normative register of what must always be true of B2. Each entry is one testable/reviewable
> claim; the linked doc holds the elaboration and rationale. This page is the top of the design set —
> the *why* — with the *what* in [data-model.md](data-model.md) and the *how* in
> [index-engine.md](index-engine.md); product non-negotiables (local-first, zero lock-in,
> single-binary) are captured as invariants here.
>
> **On conflict, this page wins and the other doc gets fixed.** Changing this page is a deliberate
> decision, never a drive-by edit. Cite entries by id (S2, G2, …).

The register is the two design tenets — *a volatile vault over a disposable index* and *build for
tomorrow's model* — made mechanical.

## S — Storage: two tiers, one projection

- **S1 — Two tiers, sharply split.** The vault (Markdown + resources + the directory tree) is the
  source of truth; `.b2/b2.sqlite` is a disposable cache. Nothing in the index is authoritative.
  ([data-model.md](data-model.md) "Two storage tiers")
- **S2 — The index is a pure projection: `index = projection of (the vault directory)`.** Drop
  `b2.sqlite`, reindex, get an identical index. **Markdown is the vault's sole authored subset** —
  the only format whose bytes B2 may write; resources contribute derived rows only, and folders are
  never projected at all (read live off disk). ([data-model.md](data-model.md) §10,
  [index-engine.md](index-engine.md) §3)
- **S3 — `full-reindex ≡ incremental-update`.** Re-deriving one changed note converges on exactly the
  state a from-scratch rebuild would produce — including reconciling path ownership and pruning rows
  for deleted files on a whole-vault pass. ([index-engine.md](index-engine.md) §8)
- **S4 — No durable B2-derived state outside the Markdown.** No event log, no sidecar files, no
  index-only authored facts. Scope: *B2-derived* data — the human's own directory tree is vault
  material, for which the **filesystem is authoritative** (folders are never projected; the tree
  listing is a live fs walk). ([data-model.md](data-model.md) "Folders")
- **S5 — Schema change = version bump + rebuild, never a data migration.** Disposability is what
  makes this free; a migration script would be evidence S2 broke.
  ([index-engine.md](index-engine.md) §3)

## W — Write discipline: the vault changes only on your command

- **W1 — B2's one unbidden write is stamping a missing `b2id`** (a ULID, into frontmatter, on first
  sight of a note). Everything else B2 writes is the mechanics of an operation the human explicitly
  invoked. ([data-model.md](data-model.md) §1)
- **W2 — B2 never authors the body, and never asks it to carry B2 syntax.** The body is 100% the
  human's document. The lone body write is the mechanical move-repair: rewriting an inbound
  `[[oldpath|alias]]`'s *path text* when its target moves — fixing a link the human already wrote,
  never adding one, aliases preserved verbatim. ([data-model.md](data-model.md) §0)
- **W3 — The on-command writes are enumerated and minimal:** append one `b2_relations:` entry on
  `b2 link` (frontmatter, never the body); the move-repair of W2; the editor save (`Vault::write` — a
  byte-honest splice of the *human's own* body bytes, guarded by a content-hash revision); and
  create/move/delete of notes, resources, and folders on explicit command.
- **W4 — B2 never deletes, moves, or archives vault files of its own accord.** Consequences of human
  edits (orphans, dangling links, hash-matched move candidates) are *surfaced*, flagged, or proposed —
  never silently applied. ([index-engine.md](index-engine.md) §8)
- **W5 — Round-trip losslessness.** `parse → serialize → parse` is byte-identical outside the specific
  edit performed; unknown frontmatter keys survive verbatim, in order. B2's own keys are namespaced
  (`b2id`, `b2_relations`) so they can never collide; a generic `relations:` key is *not* read.
  ([data-model.md](data-model.md) §6, §1)

## L — Identity & links

- **L1 — The graph keys every edge by `b2id`, never by path or title.** The inline `[[path|alias]]`
  is a repairable convenience copy. Consequence, also locked: **rename keeps every backlink
  resolving** — a move rewrites path *text* and zero edge rows. ([data-model.md](data-model.md) §1)
- **L2 — A note's title is its filename.** The frontmatter `title:` key is recognized but inert —
  round-tripped, never driving display, aliases, or search. `b2 link` therefore writes a bare
  `[[path]]`, no alias. ([data-model.md](data-model.md) §1, §9)
- **L3 — Resources are path-keyed peers with no `b2id` and no sidecar files, ever.** The one
  asymmetry vs. notes is authoring surface, not status: B2 can read them, never write them.
  ([data-model.md](data-model.md) §10)
- **L4 — The body is read strictly as ordinary Markdown.** Every body link — wikilink, Markdown link,
  embed — is an untyped, **directed** `references` edge; no prose shape (list marker, leading verb)
  is ever B2 structure. ([data-model.md](data-model.md) §2)

## G — The typed graph

- **G1 — Every edge is authored and active.** An edge exists iff it is written in the Markdown; there
  is no `status` column, no suggestion queue, no lifecycle, and nothing inert. Committing is
  appending an authored line and re-projecting, never an index mutation.
  ([data-model.md](data-model.md) §3, §4)
- **G2 — The edge set is the union of exactly two homes, deduped frontmatter-wins.** Body links
  (`origin=inline`, always untyped) ∪ frontmatter `b2_relations:` (`origin=frontmatter`, the **sole**
  home of a verb + explanation). Same `(target, type)` in both homes keeps the frontmatter row (it
  alone carries the explanation); a *different* verb over a body-linked target coexists (the augment
  case). Nothing is ever copied between homes or auto-removed from a file.
  ([data-model.md](data-model.md) §0–§3)
- **G3 — The relation vocabulary is a closed three-verb stance core plus a tolerated tail.**
  `references` (neutral) / `supports` (for) / `contradicts` (against, symmetric) is the typing
  palette and what queries rely on; any other verb is stored verbatim as an opaque tail. The closed
  core is a *policy we can relax* (promotion path), never a structural assumption.
  ([data-model.md](data-model.md) §2)
- **G4 — Edges are directed and stored once.** Inverse labels are display-only, computed at read
  time; B2 never writes a reciprocal link into the target file. ([data-model.md](data-model.md) §2)
- **G5 — An unresolvable link target projects as a surfaced dangling edge, never a dropped one.**
  Broken links read as broken (`dst` NULL, authored text kept) and heal on the next reindex once the
  target exists. ([data-model.md](data-model.md) §3, GH #12)
- **G6 — The materialized graph is a cache; runtime parsing is the correctness definition.** The
  `edges` table exists for what parsing can't serve — backlinks, typed traversal, the discovery
  exclusion — and is rebuilt from scratch on every reindex. In v1 resources are edge *targets* only;
  `src` is always a note, because an edge must trace to an authored Markdown line.
  ([index-engine.md](index-engine.md) §3, [data-model.md](data-model.md) §10)

## M — The AI seam & the embedding space

- **M1 — Exactly one AI seam: `Embedder`.** `b2-core` is model-free and tested against a
  deterministic, content-addressed fake; a real model drops in through the seam with **no schema or
  flow change**. Model-compensating machinery (per-pair adjudication, query expansion, heavy
  orchestration) is deferred or off by default — the Bitter-Lesson tenet. A reranker, if it lands, is
  the next seam, not an exception. ([index-engine.md](index-engine.md) §5–§6)
- **M2 — The embedding space has one recorded identity: `meta.(embed_model_id, embed_dim)` — and the
  compute device folds into it** (a Metal build tags the id `@metal`). Any identity change is a model
  swap: `search` **fails fast** rather than mixing spaces, `reindex` drops and re-embeds, and `open`
  **never** mutates the vector space. ([CLAUDE.md](../../CLAUDE.md) "Embedding-space discipline", GH #40)
- **M3 — One embedding space in v1.** Every vault member funnels to *text* through the same model;
  multimodal spaces and describers are documented future seams, default-off.
  ([data-model.md](data-model.md) §10)
- **M4 — Vectors live in plain tables, scored in-process; their existence *is* the signal.** The
  vector tables are created at embed time, so "tables exist" = "this vault has an embedding space" —
  the fallbacks (BM25-only search on a projected-but-unembedded vault) key on it. Centroids are
  derived data sharing the vectors' lifecycle — refreshed by the embed pass, dropped on re-chunk, no
  separate invalidation. ([CLAUDE.md](../../CLAUDE.md), #38)

## E — Engineering discipline (what keeps the above true)

- **E1 — The core is deterministic.** No wall-clock and no randomness inside `b2-core`; ids and
  timestamps are injected (`IdGen`, `created` params). Clocks and log subscribers live in the
  adapters. ([CLAUDE.md](../../CLAUDE.md) Conventions)
- **E2 — `cargo test` is fast, deterministic, and model-free; model quality never enters CI.**
  Real-model work lives behind `b2 init` / the out-of-CI eval. `#[ignore]` is forbidden — a
  hard-to-write test is a signal to re-anchor on the invariant or fix the system.
  ([CLAUDE.md](../../CLAUDE.md), the eval harness under `crates/b2-embed/evals/`)
- **E3 — The `Vault` façade is the one typed API, and every adapter is dumb.** CLI and desktop
  commands are deserialize → one façade call → serialize; logic that wants to live in an adapter
  belongs behind the façade. Dependencies point one way (adapters → core, never back); façade ops are
  added on need, never pre-built. ([crates/b2-desktop/CLAUDE.md](../../crates/b2-desktop/CLAUDE.md))
- **E4 — User-facing errors are generic and actionable, never leaking internals.** Full detail goes
  to logs / `B2_DEBUG`, not to the terminal or webview. ([CLAUDE.md](../../CLAUDE.md) Conventions)
