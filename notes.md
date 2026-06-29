---
title: "B2 — Motivations & Problem"
type: note
tags: [b2, pkm, motivation, problem-statement]
created: 2026-06-28
status: draft
---

# B2 — Motivations & Problem

> Working name: **B2** ("second brain"). A personal knowledge-management vault that is
> local-first, owns nothing of your data, and treats AI-assisted **connection discovery**
> as a first-class feature rather than a bolt-on.

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
  - Search is keyword only
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

## Approach: headless-first (the UI comes last)

A custom UI **is** coming — that's settled, not in question — but it is **postponed as long as
possible.** The priority is to push as much **capability** and, above all, **testability** into
a headless core as we can *before* any UI exists. Obsidian is **not** the designated UI: B2 is
not an Obsidian plugin or skin. The core must be fully exercisable and verifiable with no screen
at all — every capability reachable and assertable without a pixel on screen.

## Getting there: capability & testability without a UI

The bet is that we reach extensive capability — and *prove* it works — long before any UI exists.

**The CLI is the "UI before the UI."** The first adapter over the headless core is a CLI, not a
screen: `b2 add`, `b2 search`, `b2 link`, `b2 suggest`, `b2 neighbors`, `b2 reindex`, `b2 explain`.
It's a real, daily-usable product with zero GUI; a trivially testable surface (run a command
against a fixture vault, assert the output); and it keeps the decoupled-core discipline honest —
the CLI holds *no logic*, it only calls the core API. When the GUI finally arrives it's a second
dumb adapter over the same contract, inheriting every test the CLI bought. A CLI is also the
easiest thing to ship as a single binary, so the headless phase doubles as the first distributable
artifact — UI-last and the binary goal pull the same way.

**The testability stack** — what actually buys confidence:

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

**Seeing & dogfooding without a screen:**

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

## Inspiration — not a copy

We take *ideas*, not implementations:

- **OKF (Google's Open Knowledge Format):** the "directory of Markdown + frontmatter" framing.
  Lesson — build *like* it for cheap interoperability, don't depend *on* it (v0.1, single
  vendor, defines no AI layer). https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing
- **Indexed storage posibilities** 
  - https://github.com/tobi/qmd
- **Basic Memory & the wider PKM landscape:** existence proof that local-first + AI-native +
  a typed Markdown graph + a rebuildable index is achievable. We borrow the *shape of the
  idea* — typed relationships, Markdown as truth, a derived index, hybrid retrieval — and
  design B2 fresh. **Not a fork, not a clean-room reimplementation.**
- The thing the whole field leaves on the table: **typed, explained connection discovery over
  a vault you fully own.** That is B2's reason to exist.

## Deliberately deferred (not deciding here)

- **Tech stack / language** — shaped partly by the single-binary goal. Open.
- Architecture: how parsing, the derived index, and discovery are organized.
- Semantic embeddings from day one vs. keyword + graph first.
- Multi-device sync.

## Next step

With motivation and problem pinned here, move to a short **vision & scope** sketch, then the
**data model** — *what a note is and what a connection is* — before any code.
