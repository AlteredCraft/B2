---
title: "B2 — User Stories"
type: note
tags: [b2, user-stories, links, kernel, vault]
created: 2026-06-29
status: draft
---

# B2 — User Stories

Concrete, testable stories for the B2 **kernel** (the headless core — see "headless-first" in
[vision-and-scope.md](vision-and-scope.md)). Each story is written as a scenario so it can become a
green test, not a screen. Context: [vision-and-scope.md](vision-and-scope.md) (vision, scope, locked
decisions) and [tasks.md](tasks.md) (data-model leans).

> **Terminology.** "Kernel" = the headless core API; the CLI and any future GUI are thin adapters
> over it. "Inbound link" / "backlink" = a link *from* some other vault file *to* the file in
> question.

> **Link format & identity are now decided** (2026-06-29) — see the section below; mirrored in
> [vision-and-scope.md](vision-and-scope.md) ("Decisions locked") and [tasks.md](tasks.md)
> (data-model). The broader data model ([data-model.md](data-model.md), still *Next up*) is not yet
> fully locked; where a story still depends on an open decision it is called out as **Open**.

---

## Link format & identity (decided 2026-06-29)

The decision the stories below rest on. It resolves the central "how is a link written" question
([tasks.md](tasks.md), "Typed relations in Markdown") for the *authored reference* layer.

- **Authored links are `[[path|title]]`.** The target is a **vault-relative `path`**; `title` is a
  display **alias**. This is an ordinary Obsidian wikilink: clickable, portable, and human-readable
  with **no B2 running** (honors principle #1 — "fully usable without B2"). It renders as *title* in
  any UI that supports alias wikilinks.
- **Identity is a durable frontmatter `b2id`** (ULID-style), not the path or the title. The typed
  graph keys **every edge by `b2id`**. Parsing a link resolves `path → b2id` and stores the edge by `b2id`.
- **The inline `path` is a repairable convenience copy**, not the identity. The kernel maintains a
  derived `title ↔ b2id ↔ path` resolution and rewrites inbound `path` text when a target moves. The
  *graph* never depends on the path being current — only the human-facing link text does.
- **Consequence for moves:** moving a file changes its path, so inbound `[[oldpath|title]]` text is
  now stale and the kernel **rewrites it to `[[newpath|title]]`**. This is bounded, mechanical work
  (the b2id-keyed edges name exactly which files/links to fix) and is the same model Obsidian uses —
  but here it is automated and covered by the locked invariants `rename keeps every backlink
  resolving` and `full-reindex ≡ incremental-update`.
- **Consequence for title renames:** the `path` still resolves, so the link is **never broken**;
  only the `title` alias is stale, and repairing it is *cosmetic* (optional, display-only).
- **Why `path` inline and not `b2id` inline (`[[b2id|title]]`):** a `b2id` target is opaque and **not
  clickable in vanilla Obsidian** (nothing on disk is named `<id>`), which would tax the entire
  deferred-UI period. We spend a bounded rewrite-on-move cost — already a committed, tested kernel
  capability — to keep the vault first-class in Obsidian today. Id-stability is preserved *inside*
  the graph regardless.

> **How `b2id` is incorporated, in one line:** humans and Obsidian see `[[path|title]]`; the kernel
> sees a `b2id → b2id` edge. Path is for people, `b2id` is for the graph, and the kernel keeps the two in
> sync.

---

## Story 1 — Rename a file; inbound links keep resolving

**As** someone reorganizing my vault (or an agent doing it for me),
**I want** renaming a note to leave every link that points *at* it still resolving,
**so that** I can freely rename and reorganize without hunting down and hand-fixing backlinks.

### What "rename" means here

Two distinct operations, both covered:

1. **Path rename / move** — the file moves on disk (`ideas/foo.md` → `archive/foo.md`, or
   `foo.md` → `bar.md`). The note's *content* and `b2id` are unchanged, but the **inline `path` in
   inbound links is now stale**.
2. **Title rename** — the note's human title (frontmatter `title`, hence the natural alias) changes.
   The `path` is unchanged, so inbound links still **resolve**; only their display alias is stale.

(See **Link format & identity** above for the `[[path|title]]` / `b2id` model these cases follow.)

### Behavior — how the kernel updates inbound links

- **The graph never breaks, because edges key on `b2id`.** Both a move and a title change leave the
  target's `b2id` untouched, so every b2id-keyed edge stays valid the instant the kernel learns the new
  path/title. Resolution is robust *before* any file is rewritten.
- **Preferred entry point: an explicit kernel operation.** A move/rename through the core API / CLI
  (`b2 mv`) is transactional: the kernel updates its `title ↔ b2id ↔ path` resolution and repairs the
  authored layer in one step.
- **Move → rewrite inbound `path` text.** Because the inline target is a `path`, a move makes every
  inbound `[[oldpath|title]]` stale, so the kernel **rewrites each to `[[newpath|title]]`** —
  Markdown written first (source of truth), index updated after. The b2id-keyed edges name *exactly*
  which inbound files and links to touch, so the rewrite is complete and bounded.
- **Title rename → cosmetic alias repair only.** The `path` still resolves, so links are never
  broken. The kernel may refresh the stale `title` alias in inbound links for readability, but this
  is display-only and optional — no link depends on it.
- **Out-of-band moves are tolerated.** If a file is moved outside B2 (Finder, `git mv`), a reindex
  re-reads its frontmatter `b2id` and re-establishes `b2id → newpath`; with index continuity the now-
  dangling inbound `[[oldpath|title]]` links are matched back to that `b2id` and repaired. *Caveat:* a
  cold reindex with no prior index state can only repair a dangling path heuristically (e.g. via the
  alias) — the same failure surface as moving files with Obsidian closed; such links are flagged for
  repair rather than silently dropped.
- **Provenance is respected.** Mechanical path/alias repairs are kernel-authored edits, not agent
  *suggestions*; they skip the suggested→accepted review loop and never alter a link's *meaning*
  (type/explanation untouched).

### Acceptance criteria (testable scenarios)

- **Given** a vault where files B and C both link to A, **when** A is moved through the kernel,
  **then** every backlink from B and C still resolves to A, the inline `path` in B and C is rewritten
  to A's new path, and `b2 neighbors A` / `b2 explain A` shows the same inbound set as before.
- **Given** A is moved, **then** B and C round-trip losslessly (`parse → serialize → parse`) and
  only their link `path` changed — every other byte is identical.
- **Given** A's *title* is renamed (path unchanged), **then** all backlinks still resolve with **no
  rewrite required**; any alias refresh is cosmetic and the link target is untouched.
- **Given** A is moved out-of-band and the vault is reindexed, **then** backlinks resolve again
  (directly realizing the locked invariant **"rename keeps every backlink resolving"**,
  [vision-and-scope.md](vision-and-scope.md)).
- The operation touches **only** inbound files that actually link to A; unrelated files are byte-
  identical afterward.

---

## Story 2 — Delete a link; the target's *other* inbound links are unaffected

**As** someone editing a note,
**I want** removing one link from my note to drop exactly that one connection and nothing else,
**so that** deleting a link is a local, predictable edit that never disturbs other notes' links to
the same target.

### Setup

File **A** contains a link to file **B** (`[[path/to/B|B]]`). Files **C** and **D** also link to
**B**. The user edits **A** and deletes its link to B. B is *not* renamed, moved, or deleted.

### Behavior — how the inbound links to B are updated

- **Only the `A → B` edge is removed.** The kernel reconciles A's derived edges from A's new
  content (incremental update, equivalent to a full reindex of A): A's outbound set loses `→ B`, so
  B's **inbound** set loses the edge *from A*. That is the whole update.
- **C's and D's links to B are untouched.** B still exists at the same `b2id`/path, so every *other*
  inbound link to B continues to resolve unchanged. There is **no cascade** — deleting a link is not
  a rename or a delete of B. The honest answer to "how are the inbound links to B updated" is:
  **they aren't, except for the one that was deleted.**
- **B's backlink count drops by one.** `b2 neighbors B` / `b2 explain B` now show C and D but not A.
- **The vault never deletes files on its own.** If A's link was B's *last* backlink, B becomes an
  **orphan** (zero inbound). The kernel may *surface* this (orphan report / `b2 explain B`) but does
  **not** move or delete B — files are only touched when asked
  ([vision-and-scope.md](vision-and-scope.md), capability area 1).
- **Source of truth first.** The edge disappears because A's *Markdown* changed; the index is
  derived from that edit, never the reverse.

### Interaction with typed links, suggestions, and provenance

- If the deleted link was a **human-authored typed** edge (`A —contradicts→ B`), that typed edge is
  gone; B's other typed edges are unaffected.
- If a **suggested/derived** connection used A→B as evidence, removing A→B may invalidate that
  suggestion's basis. Such suggestions are **inert until accepted** and live in the review layer, so
  re-evaluating or retracting them never silently rewrites B or any inbound file
  ([vision-and-scope.md](vision-and-scope.md), "Review & trust").

### Acceptance criteria (testable scenarios)

- **Given** A, C, D all link to B, **when** A's link to B is deleted, **then** `b2 neighbors B`
  returns exactly {C, D} and C's and D's files are byte-identical (unmodified).
- **Given** the deletion, **then** the only file changed on disk is A; the index change is exactly
  the removal of one edge (incremental update ≡ full reindex of A — the locked
  `full-reindex ≡ incremental-update` invariant, [vision-and-scope.md](vision-and-scope.md)).
- **Given** A held B's only backlink, **when** it is deleted, **then** B is reported as an orphan and
  B's file is **not** moved or deleted.
- **Given** a suggested link whose evidence included A→B, **when** A→B is deleted, **then** that
  suggestion is re-evaluated/retracted in the review layer only — no inbound file and no accepted
  edge is altered without explicit acceptance.

### Open

- **Orphan handling policy** — surface-only, vs. an opt-in agent suggestion ("B is now orphaned;
  link or archive?"). Either way it stays inert until accepted; the default is likely surface-only.
</content>
</invoke>
