---
title: "B2 — Vision & Scope"
type: note
tags: [b2, pkm, vision, scope]
created: 2026-06-28
status: draft
---

# B2 — Vision & Scope

> Anchored by the motivations and principles in **notes.md**. This sketch fixes *where B2 is
> going* and *what's in vs. out* — deliberately not *how* it's built.

## Vision (the north star)

Point B2 at a folder of my Markdown notes and it becomes a second brain that actively thinks
alongside me. It reads everything, understands what each note is about, and keeps **discovering
and explaining the connections** between notes — *typed*, not just "related" — so the structure
of my knowledge grows on its own instead of rotting. The files stay plain Markdown on my disk,
mine forever; B2 is the **intelligence layer over them, not a container around them**. Humans and
AI agents are both first-class users: I write and wander; the agent reads, enriches, links, and
proposes — and I stay in control of what it commits.

## Who it's for

- **Primarily me**, on my MacBook, on my real vault. A personal tool first.
- **Eventually others**, via a single downloadable binary — same tool, no cloud.
- **The AI agent is a first-class user**, not a feature bolted to the side. B2 is designed to be
  *operated by an agent* as much as by a human.

## What B2 is (and is not)

- **Is:** an *intelligence engine over a folder of Markdown* — derived index, typed graph, hybrid
  retrieval, and connection discovery.
- **Is not (in v1):** an editor. Editing happens in any text editor, through the CLI, or via an
  agent. The GUI editor is deferred (headless-first; see notes.md).
- **Is not, ever:** a cloud service, a proprietary format, or an Obsidian plugin.

## Capability areas (the surface, high-level)

1. **Vault** — point B2 at a folder of `.md` + frontmatter; it never owns or moves files without
   being asked.
2. **Notes (CRUD)** — create / read / update / move / delete through a stable core API (and its
   CLI), always writing Markdown first.
3. **Typed links** — relationships carry a *type* (`elaborates`, `contradicts`, `example-of`,
   `supersedes`, …), not an undifferentiated edge.
4. **Hybrid retrieval** — keyword + graph (and semantic where the index engine allows) — fixing
   Obsidian's keyword-only limit.
5. **Connection discovery** ⭐ — B2 proposes *typed, explained* relationships between notes I never
   linked: "these argue the same thing from different angles," "this supersedes that." The reason
   B2 exists.
6. **Review & trust** — every agent-proposed link or edit is **provenance-tagged and inert until
   accepted**. I (or a policy) promote suggestions; nothing pollutes the vault silently.
7. **Explainability** — B2 can always show *why*: the local graph, why a suggestion fired, what the
   agent changed.

## Scope: v1 vs. later vs. never

**In scope for v1 (headless core + CLI):**
- A vault pointed at a folder; lossless parse/serialize of MD + YAML frontmatter.
- Note CRUD via the core API and CLI.
- A rebuildable derived index: **keyword (full-text) + the typed graph**, plus **semantic search if
  the chosen index engine provides it** (see Decisions locked).
- Typed links — authored *and* machine-derived — unified into one graph.
- **Connection discovery v1:** candidate generation (graph + keyword/co-occurrence) → typed,
  explained suggestions → review/accept loop, with the LLM step behind a swappable seam.
- Provenance on every note and edge.
- Full test coverage: golden vaults, property tests, deterministic AI seams (per notes.md).

**Deferred (post-v1, not now):**
- The **GUI** — the eventual editor + graph/review surface.
- Multi-device **sync**.
- Multiple vaults; large-scale performance work.

**Non-goals (explicitly not B2):**
- Cloud storage or any required account.
- A proprietary or lock-in file format.
- Being an Obsidian plugin or skin.
- Real-time multi-user collaboration.
- A general chatbot / RAG Q&A product — retrieval serves *connection discovery and structure*, not
  a chat assistant.

## What "v1 done" looks like (a scenario, not a screen)

I point B2 at a copy of my real vault and, entirely from the CLI:

- it indexes every note and builds the typed graph;
- `b2 suggest <note>` returns typed, explained connections I hadn't made — and some are genuinely
  useful;
- I accept a few; they're written back as Markdown with provenance, and any editor shows them as
  normal links;
- every bit of this is covered by tests that pass with no live model and no screen.

That milestone proves the thesis — *self-owned Markdown + AI-native connection discovery* — with
zero UI.

## Decisions locked (2026-06-28)

- **Semantic search is engine-gated.** If the index engine we choose (architecture phase) provides
  vector/semantic search, it's **in v1**; if not, it's a **fast follow** — not a distant deferral.
  This makes the **index-engine choice a gating decision**, and is where `qmd` and alternatives get
  evaluated.
- **Full CRUD lives in the CLI.** B2 is self-sufficient and testable without any external editor.
- **v1 connection discovery = discovering & reviewing links only.** Broader agent-maintained
  structure (MOCs, deduplication, tag suggestions) is explicitly post-v1.
