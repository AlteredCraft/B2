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

> **Resting on a data-model lean, not a final decision.** The data model
> ([data-model.md](data-model.md), still *Next up* in [tasks.md](tasks.md)) is not yet locked. These
> stories assume the current lean: a **durable `id`** in frontmatter is the real identity; human
> `[[Title]]` wikilinks are the *authored* layer; the kernel keeps a derived `title ↔ id ↔ path`
> resolution and **repairs authored links on rename**. Where behavior depends on a still-open
> decision, it is called out as **Open**.

---

## Story 1 — Rename a file; inbound links keep resolving

**As** someone reorganizing my vault (or an agent doing it for me),
**I want** renaming a note to leave every link that points *at* it still resolving,
**so that** I can freely rename and reorganize without hunting down and hand-fixing backlinks.

### What "rename" means here

Two distinct operations, both covered:

1. **Path rename / move** — the file moves on disk (`ideas/foo.md` → `archive/foo.md`, or
   `foo.md` → `bar.md`). The note's *content* and `id` are unchanged.
2. **Title rename** — the note's human title (frontmatter `title`, and thus the natural `[[Title]]`
   the note is referenced by) changes.

### Behavior — how the kernel updates inbound links

- **Identity is the `id`, not the path or the title.** Because the typed graph stores edges by the
  target's durable `id`, a path move or title change does **not** invalidate any edge. The graph is
  correct the instant the kernel learns the note's new path/title — no edge rewriting required.
- **Preferred entry point: an explicit kernel operation.** Renames driven through the core API /
  CLI (`b2 mv` / a rename op) are transactional: the kernel knows the `id`, updates its
  `title ↔ id ↔ path` resolution, and repairs the *authored* layer in one step.
- **Repairing the authored layer.** The `[[Title]]`/path text inside the inbound files is
  human-facing, so on a **title** rename (or a path rename when links are path-based) the kernel
  **rewrites the link text in each inbound file's Markdown** so it both resolves *and* reads
  correctly — Markdown written first (source of truth), index updated after. On a pure **path** move
  with title-based wikilinks, no inbound file needs editing at all.
- **Out-of-band renames are tolerated.** If a file is moved/renamed outside B2 (Finder, `git mv`),
  a reindex re-establishes `id → path`; because edges key on `id`, backlinks resolve again after
  reindex even though no inbound file was touched. (Catch: a title rename done by hand-editing
  frontmatter leaves stale authored `[[OldTitle]]` text until a repair pass runs.)
- **Provenance is respected.** Mechanical link-text repairs are kernel-authored edits; they are not
  agent *suggestions* and don't enter the suggested→accepted review loop. They do not silently
  alter the *meaning* of any link (type/explanation are untouched).

### Acceptance criteria (testable scenarios)

- **Given** a vault where files B and C both link to A, **when** A is renamed (path and/or title)
  through the kernel, **then** every backlink from B and C still resolves to A, and `b2 neighbors A`
  / `b2 explain A` shows the same inbound set as before.
- **Given** title-based wikilinks, **when** A's title changes, **then** the `[[…]]` text in B and C
  is rewritten to the new title and the files round-trip losslessly (`parse → serialize → parse`).
- **Given** a pure path move with title-based links, **then** **no** inbound file is modified, yet
  all backlinks still resolve.
- **Given** A is renamed out-of-band and the vault is reindexed, **then** backlinks resolve again
  (directly realizing the locked invariant **"rename keeps every backlink resolving"**,
  [vision-and-scope.md](vision-and-scope.md)).
- The rename touches **only** inbound files that genuinely embed A's name; unrelated files are byte-
  identical afterward.

### Open

- **Do we rewrite link text at all, or decouple it?** If authored links carry the `id`
  (e.g. `[[id|Alias]]`), a title rename needs **zero** inbound edits and the alias is cosmetic. If
  links are bare `[[Title]]`, we must rewrite. This is the central data-model question
  ([tasks.md](tasks.md), "Typed relations in Markdown") and decides how much of this story is
  "update the index" vs. "rewrite N files."

---

## Story 2 — Delete a link; the target's *other* inbound links are unaffected

**As** someone editing a note,
**I want** removing one link from my note to drop exactly that one connection and nothing else,
**so that** deleting a link is a local, predictable edit that never disturbs other notes' links to
the same target.

### Setup

File **A** contains a link to file **B**. Files **C** and **D** also link to **B**. The user edits
**A** and deletes its `[[B]]` link. B is *not* renamed, moved, or deleted.

### Behavior — how the inbound links to B are updated

- **Only the `A → B` edge is removed.** The kernel reconciles A's derived edges from A's new
  content (incremental update, equivalent to a full reindex of A): A's outbound set loses `→ B`, so
  B's **inbound** set loses the edge *from A*. That is the whole update.
- **C's and D's links to B are untouched.** B still exists at the same `id`/path, so every *other*
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
