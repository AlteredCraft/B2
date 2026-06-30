---
title: "B2 — Vision & Scope"
type: note
tags: [b2, pkm, motivation, problem-statement, vision, scope]
created: 2026-06-28
status: draft
---

# B2 — Vision & Scope

> Working name: **B2** ("second brain"). A personal knowledge-management vault that is
> local-first, owns nothing of your data, and treats AI-assisted **connection discovery**
> as a first-class feature rather than a bolt-on.
>
> This document fixes the *motivation*, *where B2 is going*, and *what's in vs. out* —
> deliberately not *how* it's built.

## What B2 is (one line)

A great PKM for my own MacBook — plain Markdown on the filesystem, fully self-owned — that
an AI agent can read, enrich, and connect, surfacing relationships between my notes that I'd
never find by hand.

## Why I'm building it (motivation)

- I live in **Obsidian** today and love one thing above all: my notes are **plain `.md`
  files with YAML frontmatter on my own disk**. Zero lock-in. My data is always completely
  accessible, with or without the app. That property is non-negotiable.
- But Obsidian is **limiting** where it matters most to me:
  - AI is **bolted on** through plugins — inconsistent, and the plugin reliance quietly
    erodes the portability that is the whole point.
  - The graph is **untyped and mostly decorative** — pretty, but it doesn't actually
    *discover* or *explain* connections.
  - Finding the non-obvious links between notes is still **manual labor** — exactly the
    bookkeeping a capable agent should be doing for me.
  - Search is keyword only.
- I want the ownership of Obsidian **and** genuinely native AI — specifically **connection
  discovery**: an agent that proposes typed, explained relationships across my vault and
  helps the structure grow over time.

## The problem we're solving

Today's PKM tools force a choice:

- **User-owned & portable** (Obsidian, Logseq) — but AI is an afterthought, bolted on, and
  often breaks the portability that made the tool worth choosing.
- **AI-native** (Notion AI, Mem, Reflect) — but cloud-locked; you don't really own your data.

**No mainstream tool gives me both.** B2 targets exactly that gap: **self-owned, plain-Markdown
storage + AI-native connection discovery, running locally on my machine.**

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

## Principles / non-negotiables

1. **Plain Markdown + YAML frontmatter on the filesystem is the source of truth.** No
   proprietary store. The vault is fully usable and readable without B2; "export" is a no-op
   because the files *are* the format.
2. **Local-first.** Runs entirely on my MacBook; no account or cloud required to use it. Any
   sync or cloud feature is optional and off the critical path.
3. **Self-owned, zero lock-in.** I can walk away at any time and keep every note, intact.
4. **AI-native, not AI-bolted-on.** Connection discovery and agent-assisted structure are
   core, designed in from the start — not plugins layered on later.
5. **Distributable as a single binary.** The end goal is something I (and eventually others)
   can download and run — no toolchain, no install ritual. *(This constrains the tech stack;
   see open questions.)*

## Design philosophy

The principles above are *what B2 must always be* for its user. These two tenets are *how we build* to
keep them true over time — the architectural stance the rest of the design serves. The invariants in
[data-model.md](data-model.md) (§6) and the disposable index in [index-engine.md](index-engine.md) (§3)
are these tenets made mechanical; this is their canonical statement.

- **A volatile vault over a disposable index.** Your notes are meant to churn — move, split, merge,
  compress, trim orphans, big refactors — and B2 must *welcome* that, never penalize it. We guarantee
  it by keeping almost nothing that isn't re-derivable from your Markdown: the index is a pure
  projection (`index = projection of (Markdown ∪ log)`), and dropping it and rebuilding yields an
  identical one (the locked `full-reindex ≡ incremental-update` invariant). The one durable thing B2
  keeps that your notes *can't* reconstruct is a thin append-only event log
  ([data-model.md](data-model.md) §4), whose single load-bearing job is remembering what you've
  **rejected** so it isn't re-proposed — everything else in it is prunable. An index that is never a
  source of truth can't drift into a liability; so we spend complexity to keep it *derivable*, not to
  keep it *correct under edit*. Idempotency is the mechanism; a vault you can rewrite fearlessly is the
  point.
- **Build for tomorrow's model (the Bitter Lesson).** Model capability is rising faster than any
  scaffolding we could write to compensate for today's limits, so we refuse to freeze those limits
  into B2's structure. Every AI part sits behind a **swappable seam** — embedder, relator, reranker —
  so a better model is a drop-in, not a rewrite. Model-compensating machinery (query expansion, heavy
  prompt orchestration) is deferred or off by default. Where we *do* hand-engineer structure for
  tractability — the closed relation vocabulary that makes discovery scoreable and queries reliable
  ([data-model.md](data-model.md) §2) — we keep it a **policy we can relax**, never a structural
  assumption, so a more capable model can be let off the leash without a redesign. Orchestrate the
  minimum today's model needs, and no more.

## What B2 is (and is not)

- **Is:** an *intelligence engine over a folder of Markdown* — derived index, typed graph, hybrid
  retrieval, and connection discovery.
- **Is not (in v1):** an editor. Editing happens in any text editor, through the CLI, or via an
  agent. The GUI editor is deferred (headless-first; see below).
- **Is not, ever:** a cloud service, a proprietary format, or an Obsidian plugin.

## Approach: headless-first (the UI comes last)

A custom UI **is** coming — that's settled, not in question — but it is **postponed as long as
possible.** The priority is to push as much **capability** and, above all, **testability** into
a headless core as we can *before* any UI exists. Obsidian is **not** the designated UI: B2 is
not an Obsidian plugin or skin. The core must be fully exercisable and verifiable with no screen
at all — every capability reachable and assertable without a pixel on screen.

**The CLI is the "UI before the UI."** The first adapter over the headless core is a CLI, not a
screen: `b2 add`, `b2 search`, `b2 link`, `b2 suggest`, `b2 neighbors`, `b2 reindex`, `b2 explain`.
It's a real, daily-usable product with zero GUI; a trivially testable surface (run a command
against a fixture vault, assert the output); and it keeps the decoupled-core discipline honest —
the CLI holds *no logic*, it only calls the core API. When the GUI finally arrives it's a second
dumb adapter over the same contract, inheriting every test the CLI bought. A CLI is also the
easiest thing to ship as a single binary, so the headless phase doubles as the first distributable
artifact — UI-last and the binary goal pull the same way.

### The testability stack — what actually buys confidence

1. *One typed core API as the contract.* All logic lives behind it; CLI and tests are clients.
   Testing the API exhaustively is testing the app.
2. *Golden-vault fixtures.* Small Markdown corpora with known structure → assert the derived graph,
   search hits, and suggestions. Add a frozen snapshot of a copy of the real vault as a large
   integration fixture — real frontmatter, real link density.
3. *Property tests for the invariants:* round-trip losslessness (`parse → serialize → parse`),
   `full-reindex ≡ incremental-update`, and `rename keeps every backlink resolving`. These catch
   whole classes of bugs, not single cases.
4. *Deterministic seams for the AI parts.* Discovery's mechanism (candidate generation → ranking →
   typing → review state) is fully testable with a fake embedder (deterministic vectors) and a
   scripted relator (canned LLM output). No live model is needed to prove the pipeline is correct.
5. *Split "is the plumbing right?" from "is the AI good?"* Fast deterministic tests on every change
   (replay recorded model transcripts as fixtures); a separate, occasional eval suite that hits a
   real model and scores suggestion quality (precision/recall) against a hand-labeled set. Model
   quality never flakes CI.

### Seeing & dogfooding without a screen

- *Observability stands in for visual feedback.* `b2 explain <note>` dumps the local graph, why a
  suggestion fired, and index state; structured logs / an event stream to tail.
- *A REPL / notebook over the core API* is the headless equivalent of clicking around — poke the
  system, find gaps, before any of it becomes a screen.
- *Dogfood the AI-native promise now, no MCP needed.* An agent (e.g. Claude) can drive the `b2`
  commands directly — the CLI is the agent's hands — exercising the "an agent reads, enriches, and
  connects my vault" loop today, headless, with no protocol layer and no UI.

**Milestones are scenarios, not screens.** Done is a passing scenario — "given vault X, `b2 suggest
<note>` returns these typed, explained links" — not something to look at. As long as progress is
measured in green scenarios, there's no pressure to build a UI to check whether it works; the tests
already said so. That is what lets the UI be deferred for a long time without flying blind.

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
- Full test coverage: golden vaults, property tests, deterministic AI seams (see the testability
  stack above).

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

## Decisions locked (2026-06-29)

- **Link format & identity.** Authored links are wikilinks of the form **`[[path|title]]`** — a
  vault-relative `path` target plus a `title` display alias — so the vault stays clickable, portable,
  and fully usable in Obsidian with **no B2 running** (principle #1). The durable identity is a
  frontmatter **`b2id`** (ULID-style, namespaced so it never collides with a user/OKF/tool `id`); the
  typed graph keys every edge by `b2id`, never by path or title. The inline `path` is a **repairable
  convenience copy** of the edge's true (`b2id`) target: the kernel keeps `title ↔ b2id ↔ path` in sync
  and rewrites inbound `path` text on move. Net: people and Obsidian see `[[path|title]]`; the graph
  sees a `b2id → b2id` edge — so reorganizes/splits/merges never lose a connection. Mechanics and
  scenarios in [user-stories.md](user-stories.md) ("Link format & identity").
- **Data model.** [data-model.md](data-model.md) is locked. Two source-of-truth objects (note, edge) in
  plain Markdown over **three storage tiers**: pristine Markdown (knowledge) · a disposable SQLite index ·
  a durable, append-only in-vault `.b2/` event log — with `index = projection of (Markdown ∪ log)`. Typed
  relations are keyed by `b2id`; agent **suggestions stay in the review layer, inert until accepted**.
  Edge provenance lives in the **event log, not the note** (accepted edges stay pristine); `b2id` is B2's
  one always-allowed frontmatter write. A bare link is a **directed `references`** edge; the typed
  vocabulary is a **10-verb core + tolerated tail**. Settles the data-model "central question"
  ([tasks.md](tasks.md)). *(Where accepted edges are written was revised 2026-06-30 — see below.)*
- **Graph is materialized, not parsed on read.** The typed graph is kept as a **disposable `edges` table
  in the index**, not resolved from the Markdown at query time. A note's *outbound* links are re-derivable
  by parsing it (which is why the table is disposable), but **backlinks, typed multi-hop traversal, the
  semantic⨝graph discovery join, and the inert suggestion queue** (suggestions are never on disk) are
  full-vault scans or impossibilities at read time, and indexed lookups once materialized. Runtime parsing
  is the correctness spec; the table is its cache — one more table in the store, not a third subsystem.
  Without it B2 is just vector + keyword search over Markdown; the traversable typed graph is the value-add
  (capability areas 3, 5). Rationale in [index-engine.md](index-engine.md) §3, cost in §8.

## Decisions locked (2026-06-30)

- **B2 never authors the body; accepted edges live in frontmatter (revises the data-model §0 "central
  decision").** The body is the rendered/exported document and stays **100% the human's** — B2 must never
  inject prose or structure into it (a `## Relations` section appearing in a `resume.md` is the
  anti-example). So **B2-accepted typed edges are written to frontmatter `relations:`**, not the body, as
  typed-link strings — `- "<verb> [[path|title]] — explanation"`, the *same* syntax a human would write
  in the body, just located in metadata (**Format A**). Human-authored connections stay where the human
  wrote them in the body; B2 **reads** those and never rewrites them. Pending suggestions remain in the
  review layer. **B2's only body write is the mechanical wikilink-path rewrite on move** — repairing a
  link the human already made, never adding one.
- **The graph is the union of three homes, one home per edge, no two-way sync.** `edges` is a one-way
  projection of body links (`origin=inline`) ∪ frontmatter `relations:` (`origin=frontmatter`) ∪ the log
  (`origin=suggested`). Nothing is mirrored between homes, so there is nothing to keep in sync — drop the
  index and rebuild. The lone overlap (a human re-authoring in the body an edge already accepted in
  frontmatter) resolves at projection time **inline-wins**: keep the body row, ignore the redundant
  frontmatter row, surface it via `b2 explain`, never auto-edit the file.
- **The trade.** A frontmatter edge is not guaranteed clickable in vanilla Obsidian's reading view.
  Accepted deliberately: human body links stay clickable, Obsidian can't render edge *types* anyway, and
  frontmatter relations are the more OKF-native shape — a small cost for a pristine document. Full
  re-derivation in [data-model.md](data-model.md) §0, §2, §7, §9.

## Inspiration — not a copy

We take *ideas*, not implementations:

- **OKF (Google's Open Knowledge Format):** the "directory of Markdown + frontmatter" framing.
  Lesson — build *like* it for cheap interoperability, don't depend *on* it (v0.1, single
  vendor, defines no AI layer). https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing
- **Indexed storage possibilities** —
  - https://github.com/tobi/qmd
- **Basic Memory & the wider PKM landscape:** existence proof that local-first + AI-native +
  a typed Markdown graph + a rebuildable index is achievable. We borrow the *shape of the
  idea* — typed relationships, Markdown as truth, a derived index, hybrid retrieval — and
  design B2 fresh. **Not a fork, not a clean-room reimplementation.**
- The thing the whole field leaves on the table: **typed, explained connection discovery over
  a vault you fully own.** That is B2's reason to exist.

## Open questions / deliberately deferred (not deciding here)

- **Tech stack / language** — shaped partly by the single-binary goal. Open.
- Architecture: how parsing, the derived index, and discovery are organized.
- Semantic embeddings from day one vs. keyword + graph first (gated on the index-engine choice
  above).
- Multi-device sync.

## Next step

With motivation, problem, vision, and scope pinned here, move to the **data model** — *what a note
is and what a connection is* — before any code.
