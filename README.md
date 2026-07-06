---
b2id: 01KWSRHXGB0G6ZEHKYGDEQFW40
title: "B2 — Read me / map"
type: note
tags: [b2, readme, overview, map]
created: 2026-06-29
status: draft
---

# B2 — "second brain"

A personal, **local-first** knowledge vault — plain Markdown you fully own — with an AI layer that
**surfaces the semantically similar notes you haven't linked yet**, so you can commit the typed,
explained connections between them yourself.

> **Status:** the design is **locked** and the **index engine is built** (`crates/b2-core`: steps 0→5
> of the [build spec](planning/specs/index-engine-build.md)). The **`b2` CLI over a typed core API** is
> live (`crates/b2-cli`): point B2 at a folder and `reindex` / `search` / `neighbors` / `explain` it
> from the terminal, with `--json` for agents. **Semantic search is real** (`crates/b2-embed`: a
> candle-backed local embedder behind the one seam; `b2 init` downloads the model into a shared cache;
> the fake stays the CI default). **Connection discovery** ships as **`b2 similar`** (surface the
> nearest *unlinked* notes — local, free, no model call) **+ `b2 link`** (you commit a typed relation
> to frontmatter). The LLM relator was tried and **cut 2026-07-04** — its per-pair cost didn't scale;
> the human is the precision gate ([tasks.md](planning/tasks.md)). All green (129 tests). A tour
> grounded in the test suite: [docs/architecture.html](docs/architecture.html).

## What B2 is (the north star)

Point B2 at a folder of Markdown notes and it becomes a second brain that thinks alongside you: it
reads everything, builds a *typed* graph, and keeps **surfacing the similar notes you haven't
connected yet** — so the structure of your knowledge grows as you link them, instead of rotting.
The files stay plain Markdown on your disk, yours forever; B2 is the **intelligence layer over them,
not a container around them**. Humans and AI agents are both first-class users.

Full motivation, scope, and locked decisions: **[vision-and-scope.md](planning/vision-and-scope.md)**.

## How we build it

Two architectural tenets shape every decision (full text:
[vision-and-scope.md → Design philosophy](planning/vision-and-scope.md#design-philosophy)):

- **A volatile vault over a disposable index.** Refactor fearlessly — move, split, merge, compress,
  trim orphans. The index is a pure projection of your Markdown (drop it, rebuild it identical);
  **nothing durable lives outside your notes** (`index = projection of (Markdown)`). Idempotency is the
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
| [data-model.md](planning/data-model.md) | What a **note** and a **connection** are, in plain Markdown · the two storage tiers · the relation vocabulary · the invariant *definitions*. The canonical *what*. |
| [index-engine.md](planning/index-engine.md) | How the derived index is *built* — SQLite (FTS5 + `sqlite-vec`) as a disposable projection. The canonical *how*. |
| [specs/index-engine-build.md](planning/specs/index-engine-build.md) | The build **spec** — precise table DDL, relations, data flows, and the step 0→5 build order. The buildable contract. |
| [user-stories.md](planning/user-stories.md) | Kernel behavior as testable scenarios (rename/move, link delete) · link-identity mechanics. |
| [tasks.md](planning/tasks.md) | The working queue — what's done, what's next. |


## Build and run

```bash
cargo install --path crates/b2-cli --locked   # installs `b2` to ~/.cargo/bin (on PATH)
b2 --help
```

This puts a real `b2` on your PATH. Re-run it (add `--force`) or `just install` to update after code changes.

For engine iteration where you don't want to reinstall each time, `cargo run -p b2-cli -- …` runs in place.
[`just`](https://github.com/casey/just) recipes wrap this and the other common commands:

```bash
just install    # build + install `b2` onto your PATH (~/.cargo/bin)
just test       # fast, deterministic, model-free engine suite (what CI runs)
just check      # fmt-check + clippy + tests — the pre-commit gate
just init       # download + verify the embedding model into the shared cache
just eval       # semantic-retrieval quality eval (real model)
just            # list every recipe
```

Point B2 at a vault with `-C <path>` (a.k.a. `--vault`) on any command, or set `B2_VAULT_PATH` once so
every command finds it without the flag (an explicit `-C` wins). Read-only commands (`search`,
`neighbors`, …) fall back to the current dir; commands that write (`reindex`, `add`, `mv`, `link`) require an
explicit vault and refuse otherwise, so they can't silently touch the wrong place. Full walkthrough:
**[Quick start](https://alteredcraft.github.io/B2/quickstart.html)**.
