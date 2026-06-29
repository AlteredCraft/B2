---
title: "B2 — Tasks"
type: note
tags: [b2, tasks, planning]
created: 2026-06-28
updated: 2026-06-29
status: active
---

# B2 — Tasks

Working task queue for B2. Context lives in [notes.md](notes.md) (motivations, principles,
headless-first approach) and [vision-and-scope.md](vision-and-scope.md) (vision, capability areas,
v1 scope, locked decisions).

## Done

- [x] **Motivations & problem** — [notes.md](notes.md).
- [x] **Vision & scope** — [vision-and-scope.md](vision-and-scope.md), including v1 scope and the
  three locked decisions (2026-06-28: semantic is engine-gated, full CRUD in CLI, v1 discovery =
  links only).

## Next up — Data model sketch

**Goal:** define *what a note is* and *what a connection is*, as the plain-Markdown source of truth.
Engine-independent — this is the yardstick the index-engine evaluation will use right after.

**Deliverable:** `data-model.md`.

**Decided so far (2026-06-29) — link format & identity:**
- **Identity = durable frontmatter `id`** (ULID-style); the typed graph keys every edge by `id`, not
  by path or title — so an agent can reorganize / split / merge without breaking backlinks.
- **Authored links = `[[path|title]]`** — vault-relative `path` target + `title` display alias; an
  ordinary Obsidian wikilink, clickable and portable with no B2 (principle #1). Chosen over
  `[[id|title]]` because an id target isn't clickable in vanilla Obsidian during the deferred-UI era.
- **The inline `path` is a repairable convenience copy**, not identity: the kernel keeps
  `title ↔ id ↔ path` in sync and rewrites inbound `path` text on move. People see `[[path|title]]`;
  the graph sees an `id → id` edge.
- Full rationale + scenarios: [user-stories.md](user-stories.md) ("Link format & identity"); mirrored
  in [vision-and-scope.md](vision-and-scope.md) ("Decisions locked, 2026-06-29").

**Still to resolve:**
- **Frontmatter schema** — `id`, `type` (required; OKF-compatible), `title`, `description`, `tags`,
  `created` / `updated`, `provenance`. Tolerate unknown keys.
- **Typed relations in Markdown** — the *reference encoding* is now settled (`[[path|title]]` + `id`,
  above); what remains is how a relation's **type** is expressed so it round-trips losslessly: inline
  (`- contradicts [[path|title]]`, Basic-Memory style) vs. a frontmatter `relations:` block vs. a
  hybrid. *This is the remaining central question.*
- **Connection / edge model** — `src`, `dst`, `type`, `status`, `provenance`, `explanation`,
  `origin` (inline | frontmatter | suggested).
- **Provenance & trust** — `by` (human | agent:&lt;model&gt;), `source`, `confidence`; and the
  **suggestion lifecycle** (suggested → accepted / rejected) that keeps agent proposals *inert until
  accepted* (see "Review & trust" in [vision-and-scope.md](vision-and-scope.md)).
- **OKF-compatibility checks** — keep `type`, `resource` URIs, and an `index.md` so "export to OKF"
  stays a no-op (build *like* OKF — see "Inspiration" in [notes.md](notes.md)).

**Ties to scope:** anchors capability areas 1–3 and 5–6 in
[vision-and-scope.md](vision-and-scope.md) (vault, CRUD, typed links, connection discovery, review
& trust).

## Then — Index-engine evaluation

Gated by the data model. Evaluate `qmd` vs. SQLite (FTS + `sqlite-vec`) vs. alternatives against the
note / edge / provenance shape above. **Decides whether semantic search is in v1 or a fast follow**
(see "Decisions locked" in [vision-and-scope.md](vision-and-scope.md)).

## Backlog (later, not now)

- Core API surface — the typed contract every adapter calls.
- CLI command surface — `b2 add / search / link / suggest / neighbors / reindex / explain`.
- Connection-discovery pipeline — candidate generation → typed, explained suggestions → review loop.
- Tech-stack / language decision — constrained by the single-binary goal.
- Test harness — golden vaults, property tests, deterministic AI seams.
