---
title: "B2 — Read me / map"
type: note
tags: [b2, readme, overview, map]
created: 2026-06-29
status: draft
---

# B2 — "second brain"

A personal, **local-first** knowledge-management vault — plain Markdown you fully own — with an
AI agent that discovers **typed, explained connections** between your notes that you'd never find
by hand.

> **Status:** the design is **locked**, the **index engine is built** (`crates/b2-core`: steps 0→5 of
> the [build spec](planning/specs/index-engine-build.md) + the suggestion **accept** operation), the
> **`b2` CLI over a typed core API** is live (`crates/b2-cli`): point B2 at a folder and `reindex` /
> `neighbors` / `search` it from the terminal, with `--json` for agents — and **semantic search is now
> real** (`crates/b2-embed`: a candle-backed local embedder behind the seam; `b2 init` downloads the
> model into a shared cache; the fake stays the CI default). All green (73 tests). **Next up:** the
> connection-discovery pipeline ([tasks.md](planning/tasks.md)). A tour of how the system is
> constructed — grounded in the test suite: [docs/architecture.html](docs/architecture.html).

## What B2 is (the north star)

Point B2 at a folder of Markdown notes and it becomes a second brain that actively thinks alongside
you: it reads everything, builds a *typed* graph, and keeps **discovering and explaining the
connections** between notes — so the structure of your knowledge grows on its own instead of rotting.
The files stay plain Markdown on your disk, yours forever; B2 is the **intelligence layer over them,
not a container around them**. Humans and AI agents are both first-class users.

Full motivation, scope, and locked decisions: **[vision-and-scope.md](planning/vision-and-scope.md)**.

## How we build it

Two architectural tenets shape every decision (full text:
[vision-and-scope.md → Design philosophy](planning/vision-and-scope.md#design-philosophy)):

- **A volatile vault over a disposable index.** Refactor fearlessly — move, split, merge, compress,
  trim orphans. The index is a pure projection of your Markdown (drop it, rebuild it identical);
  the only durable thing your notes can't reconstruct is a thin event log. Idempotency is the
  mechanism; a vault you can rewrite without fear is the point.
- **Build for tomorrow's model (the Bitter Lesson).** Every AI part sits behind a swappable seam;
  we orchestrate the minimum today's model needs and no more — so a more capable model is a drop-in,
  not a redesign.

…in service of five product non-negotiables — plain-Markdown source of truth · local-first · zero
lock-in · AI-native (not bolted-on) · single binary
([vision-and-scope.md → Principles](planning/vision-and-scope.md#principles--non-negotiables)).

## The docs

### HTML guides — [alteredcraft.github.io/B2](https://alteredcraft.github.io/B2/)

New here? Start with the **[Quick start](https://alteredcraft.github.io/B2/quickstart.html)** — set up
and work with a vault in about ten minutes. Then go deeper:
[system architecture](https://alteredcraft.github.io/B2/architecture.html) ·
[indexing pipeline](https://alteredcraft.github.io/B2/indexing.html) ·
[connection discovery](https://alteredcraft.github.io/B2/discovery.html).

| Doc | What it owns |
|---|---|
| [vision-and-scope.md](planning/vision-and-scope.md) | Why B2 exists · principles · **design philosophy** · v1 scope · locked decisions. The canonical *why*. |
| [data-model.md](planning/data-model.md) | What a **note** and a **connection** are, in plain Markdown · the three storage tiers · the relation vocabulary · the invariant *definitions*. The canonical *what*. |
| [index-engine.md](planning/index-engine.md) | How the derived index is *built* — SQLite (FTS5 + `sqlite-vec`) as a disposable projection. The canonical *how*. |
| [specs/index-engine-build.md](planning/specs/index-engine-build.md) | The build **spec** — precise table DDL, relations, data flows, and the step 0→5 build order. The buildable contract. |
| [user-stories.md](planning/user-stories.md) | Kernel behavior as testable scenarios (rename/move, link delete) · link-identity mechanics. |
| [tasks.md](planning/tasks.md) | The working queue — what's done, what's next. |


## Build and run

```bash
cargo build --release -p b2-cli
alias b2=./target/release/b2
b2 --help
```
