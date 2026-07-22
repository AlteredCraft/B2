---
b2id: 01KWSRPN2YBZ6PG293S7J1JYW7
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
  discovery**: surfacing the relationships across my vault I'd never find by hand — semantically, over
  everything — so I can make them real and typed, and the structure grows over time.

## The problem we're solving

Today's PKM tools force a choice:

- **User-owned & portable** (Obsidian, Logseq) — but AI is an afterthought, bolted on, and
  often breaks the portability that made the tool worth choosing.
- **AI-native** (Notion AI, Mem, Reflect) — but cloud-locked; you don't really own your data.

**No mainstream tool gives me both.** B2 targets exactly that gap: **self-owned, plain-Markdown
storage + AI-native connection discovery, running locally on my machine.**

## Vision (the north star)

Point B2 at a folder of my Markdown notes and it becomes a second brain that actively thinks
alongside me. It reads everything, understands what each note is about, and keeps **surfacing the
connections** between notes I'd never find by hand — ready for me to make them *typed*, not just
"related" — so the structure of my knowledge grows instead of rotting. The files stay plain Markdown on my disk,
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
are these tenets made mechanical; this is their canonical statement, and
**[invariants.md](invariants.md)** is the one-page register of the resulting invariants.

- **A volatile vault over a disposable index.** Your notes are meant to churn — move, split, merge,
  compress, trim orphans, big refactors — and B2 must *welcome* that, never penalize it. We guarantee
  it by keeping **nothing** that isn't re-derivable from your Markdown: the index is a pure projection
  (`index = projection of (Markdown)`), and dropping it and rebuilding yields an identical one (the
  locked `full-reindex ≡ incremental-update` invariant). There is **no** durable state outside your
  notes — every connection you commit lives in the Markdown itself (a body link, or a frontmatter
  `b2_relations:` entry), so losing the index loses nothing at all. An index that is never a source of
  truth can't drift into a liability; so we spend complexity to keep it *derivable*, not to keep it
  *correct under edit*. Idempotency is the mechanism; a vault you can rewrite fearlessly is the point.
  *(Until 2026-07-04 one durable thing lived outside Markdown — a thin event log remembering rejected
  suggestions; cutting the LLM relator removed the only thing it was for, so the third tier is gone.)*
- **Build for tomorrow's model (the Bitter Lesson).** Model capability is rising faster than any
  scaffolding we could write to compensate for today's limits, so we refuse to freeze those limits
  into B2's structure. Every AI part sits behind a **swappable seam** — today the embedder (a reranker
  when one lands) — so a better model is a drop-in, not a rewrite. Model-compensating machinery (query
  expansion, heavy prompt orchestration, an LLM adjudicating every candidate pair) is deferred or off by
  default — the 2026-07-04 removal of the per-pair relator is this tenet *applied*, not an exception to
  it. Where we *do* hand-engineer structure for tractability — the closed relation vocabulary that keeps
  queries reliable and gives you a stable typing palette ([data-model.md](data-model.md) §2) — we keep
  it a **policy we can relax**, never a structural assumption, so a more capable model can be let off the
  leash without a redesign. Orchestrate the minimum today's model needs, and no more.

## What B2 is (and is not)

- **Is:** an *intelligence engine over a folder of Markdown* — derived index, typed graph, hybrid
  retrieval, and connection discovery.
- **Is not (in v1):** an editor. Editing happens in any text editor, through the CLI, or via an
  agent. The GUI editor is deferred (headless-first; see below).
- **Is not, ever:** a cloud service, a proprietary format, or an Obsidian plugin.

## Approach: headless-first (the UI comes last)

> **Update 2026-07-05 — the postponement ends here.** The headless bet paid off (engine, façade, and CLI
> ship and are fully tested), so the first UI's **read-only MVP has shipped**: a **Tauri** desktop app
> (`crates/b2-desktop`) + a Vite/vanilla-TS frontend (`ui/`) — the *second dumb adapter over the façade* this
> section describes, **not** a rewrite (CodeMirror editing is the immediate fast-follow). Full plan:
> [specs/completed/desktop-ui-mvp.md](specs/completed/desktop-ui-mvp.md) (and "Decisions locked (2026-07-05)" below).

A custom UI **is** coming — that's settled, not in question — but it is **postponed as long as
possible.** The priority is to push as much **capability** and, above all, **testability** into
a headless core as we can *before* any UI exists. Obsidian is **not** the designated UI: B2 is
not an Obsidian plugin or skin. The core must be fully exercisable and verifiable with no screen
at all — every capability reachable and assertable without a pixel on screen.

**The CLI is the "UI before the UI."** The first adapter over the headless core is a CLI, not a
screen: `b2 add`, `b2 search`, `b2 similar`, `b2 link`, `b2 neighbors`, `b2 reindex`, `b2 explain`.
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
   search hits, and similarity candidates. Add a frozen snapshot of a copy of the real vault as a large
   integration fixture — real frontmatter, real link density.
3. *Property tests for the invariants:* round-trip losslessness (`parse → serialize → parse`),
   `full-reindex ≡ incremental-update`, and `rename keeps every backlink resolving`. These catch
   whole classes of bugs, not single cases.
4. *Deterministic seams for the AI parts.* Discovery's mechanism (candidate generation → similarity
   ranking) is fully testable with a fake embedder (deterministic vectors) — no live model is needed to
   prove the pipeline is correct. The one AI seam left is the embedder; the relator and its scripted fake
   went with the LLM-relator removal (2026-07-04).
5. *Split "is the plumbing right?" from "is the AI good?"* Fast deterministic tests on every change
   (fake embedder); a separate, occasional eval suite that hits the real embedder and scores retrieval
   quality (precision/MRR) against a hand-labeled set. Model quality never flakes CI.

### Seeing & dogfooding without a screen

- *Observability stands in for visual feedback.* `b2 explain <note>` dumps the local graph with each
  edge's explanation and index state; `b2 similar <note>` shows what B2 would surface; structured logs to tail.
- *A REPL / notebook over the core API* is the headless equivalent of clicking around — poke the
  system, find gaps, before any of it becomes a screen.
- *Dogfood the AI-native promise now, no MCP needed.* An agent (e.g. Claude) can drive the `b2`
  commands directly — the CLI is the agent's hands — exercising the "an agent reads, enriches, and
  connects my vault" loop today, headless, with no protocol layer and no UI.

**Milestones are scenarios, not screens.** Done is a passing scenario — "given vault X, `b2 similar
<note>` surfaces these nearest notes and `b2 link` commits the ones I pick" — not something to look at. As long as progress is
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
5. **Connection discovery** ⭐ — for any note, B2 surfaces the notes most *semantically similar* to it
   that I haven't linked yet — over my whole vault, instantly and locally — and I lock in the ones worth
   a real connection as a *typed* link (in the body, or committed to frontmatter). The machine finds the
   candidates; I supply the judgment and the type. The reason B2 exists.
6. **Review & trust** — B2 never writes a connection I didn't ask for. Every committed edge is one I
   authored in the body or explicitly locked in with `b2 link`; B2's only unbidden write to a note is
   stamping a missing `b2id`. Nothing pollutes the vault silently — the vault changes only on my command.
7. **Explainability** — B2 can always show *why*: the local graph, each edge's explanation, and what
   changed and when.

## Scope: v1 vs. later vs. never

**In scope for v1 (headless core + CLI):**
- A vault pointed at a folder; lossless parse/serialize of MD + YAML frontmatter.
- Note CRUD via the core API and CLI.
- A rebuildable derived index: **keyword (full-text) + the typed graph**, plus **semantic search if
  the chosen index engine provides it** (see Decisions locked).
- Typed links — authored *and* machine-derived — unified into one graph.
- **Connection discovery v1:** for any note, surface its semantically nearest not-yet-linked notes
  (vector KNN over stored embeddings) → the human locks in the worthwhile ones as typed links (a body
  link, or `b2 link` → frontmatter). No LLM in the loop; the human is the precision gate.
- Provenance on every note and edge.
- Full test coverage: golden vaults, property tests, deterministic AI seams (see the testability
  stack above).

**Deferred (post-v1, not now):**
- The **GUI** — *read-only first cut **shipped*** as a **Tauri** desktop app (the connection-discovery loop:
  render → discover → link; CodeMirror body editor is the immediate fast-follow) — see the 2026-07-05 locked
  decision below and [specs/completed/desktop-ui-mvp.md](specs/completed/desktop-ui-mvp.md). The broader editor + graph/review
  surface stays deferred.
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
- `b2 similar <note>` surfaces the notes nearest it in meaning that I hadn't linked — and some are
  genuinely worth connecting;
- I lock a few in with `b2 link` (or a body link of my own); they're written back as Markdown
  (frontmatter `b2_relations:`), and any editor shows them as normal links;
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
  ([tasks.md](tasks.md)). *(Superseded twice — see below: where accepted edges are written was revised
  2026-06-30, and the **third tier + the suggestion review layer were cut 2026-07-04** with the LLM
  relator, collapsing storage to two tiers and the invariant to `index = projection of (Markdown)`.)*
- **Graph is materialized, not parsed on read.** The typed graph is kept as a **disposable `edges` table
  in the index**, not resolved from the Markdown at query time. A note's *outbound* links are re-derivable
  by parsing it (which is why the table is disposable), but **backlinks, typed multi-hop traversal, the
  semantic⨝graph discovery join, and the inert suggestion queue** (suggestions are never on disk) are
  full-vault scans or impossibilities at read time, and indexed lookups once materialized. Runtime parsing
  is the correctness spec; the table is its cache — one more table in the store, not a third subsystem.
  Without it B2 is just vector + keyword search over Markdown; the traversable typed graph is the value-add
  (capability areas 3, 5). Rationale in [index-engine.md](index-engine.md) §3, cost in §8.

## Decisions locked (2026-06-30)

> **Partly superseded 2026-07-04 (see below).** The core here still holds — B2 writes typed edges to
> frontmatter, never the body (now on `b2 link`). But the LLM relator was cut, so the **review layer /
> pending-suggestion** references below are gone, and the graph's "three homes … ∪ the log
> (`origin=suggested`)" collapsed to **two** — body links (`origin=inline`) ∪ frontmatter `relations:`
> (`origin=frontmatter`).

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

## Decisions locked (2026-06-30) — embedder & distribution

Settles "the index-engine choice is a gating decision" for semantic search
([index-engine.md](index-engine.md) §6/§8); build/execution plan in [tasks.md](tasks.md) "Next up".

- **Local embedder = `candle` + `hf-hub`, dim 768.** Pure-Rust inference compiled into the binary (no
  external ONNX Runtime), behind the existing swappable `Embedder` seam — the "build for tomorrow's model"
  tenet in practice. 768 sets the `chunks_vec` column type; a model/dim change is a full re-embed.
  **Built 2026-07-01** in `crates/b2-embed` (`LocalEmbedder`). The default model changed from
  EmbeddingGemma-300M to **`BAAI/bge-base-en-v1.5`** (BERT, 768-dim, ungated): EmbeddingGemma is *gated* on
  Hugging Face (needs a token + license click), which defeats a friction-free `b2 init`; bge was the
  pre-authorized fallback and is validated. EmbeddingGemma stays selectable via config for anyone with a
  token.
- **The model is not bundled; it's an explicit `b2 init` download.** Keeps the single binary small
  (principle #5) and never surprise-downloads mid-command — `reindex`/`search` fail fast until `b2 init`
  has run. The download **source is configurable** (default HF repo; overridable to a mirror, another
  repo, or a local path for fully-offline installs), set in a global TOML config; the model is cached in a
  shared XDG dir, not per-vault.

## Decisions locked (2026-07-04) — discovery is similarity + human judgment, not an LLM relator

A course-correction from dogfooding B2 on a real 1000+ note vault. It **returns to the two tenets**
rather than departing from them — the machinery it removes is exactly what those tenets say to avoid.

- **The LLM relator is cut.** Connection discovery was built as *candidate generation → an LLM
  "relator" that types & explains every candidate pair → a review/accept queue.* On a real vault the
  relator's per-pair latency and dollar cost don't scale (the first pass is ~notes × candidates model
  calls), and — the deeper point — a per-pair LLM adjudicator is precisely the **"heavy prompt
  orchestration / model-compensating machinery" the Bitter-Lesson tenet says to defer.** So it goes: the
  `Relator` seam, the Claude-backed relator (the `b2-relate` crate), and the suggestion queue
  (`generate`/`accept`/`reject`) are removed.
- **Discovery is now semantic-similarity surfacing + human lock-in.** Point B2 at a note and it surfaces
  the notes most **semantically similar** to it that you haven't linked yet — instantly, locally, over
  your whole vault (vector KNN over already-stored embeddings; no model call, no network). *You* decide
  which are worth a real connection and **lock it in**: a link in the body (which you write), or a typed
  relation B2 commits to frontmatter `relations:` on **`b2 link`**. The discovery is the machine's; the
  **judgment stays yours** — you are the precision gate the relator used to be. This is still
  **AI-native** (principle #4): the intelligence simply moved from expensive per-pair judgment to cheap
  whole-vault semantic surfacing, which is where the Bitter Lesson says to put it.
- **Storage collapses from three tiers to two.** With no suggestions to queue and no rejections to
  remember, the durable `.b2/log/` event tier and its replay have **no load-bearing job left** (the only
  other thing they held — `b2id.stamped` — is history reconstructible from the Markdown itself, since the
  id lives in the file). So the log tier is removed and the core invariant **simplifies** from
  `index = projection of (Markdown ∪ log)` to **`index = projection of (Markdown)`** — which *strengthens*
  the "volatile vault over a disposable index" tenet: now literally nothing durable exists that your
  Markdown can't reconstruct. Two tiers remain: **pristine Markdown** (source of truth) + a **disposable
  SQLite index** (drop it, `reindex` rebuilds it identical).
- **What's retained, unchanged:** typed links (authored in the body, or committed to frontmatter), the
  materialized typed graph with **backlinks** (`b2 neighbors` / `b2 explain` — inbound *and* outbound),
  hybrid keyword + semantic + graph retrieval, the real local embedder behind its seam, and the closed
  10-verb relation vocabulary — now serving as *your* typing palette on `b2 link` rather than the
  relator's emit set. Mirrored across [data-model.md](data-model.md), [index-engine.md](index-engine.md),
  [tasks.md](tasks.md), and [user-stories.md](user-stories.md).

## Decisions locked (2026-07-05) — the first UI: a Tauri desktop app

The headless-first bet has paid off — engine, façade, and CLI ship and are fully tested — so the deferred UI
now begins, as the **second dumb adapter over the [`Vault`](../crates/b2-core/src/vault.rs) façade** this
document always promised. It adds **no new architecture**: one thin adapter crate plus a frontend. Full
rationale and the step-by-step build plan: [specs/completed/desktop-ui-mvp.md](specs/completed/desktop-ui-mvp.md).

- **Delivery vehicle = a Tauri desktop app.** Not a TUI (the MVP is a *rendered-document* surface a terminal
  can't do justice — no long-form Markdown, images, or clickable links), not a browser + `b2 serve` (no
  native filesystem-watch; "a tab, not an app"), not a native-Rust GUI (no off-the-shelf editor to build on).
  Tauri uses the **OS webview** — it bundles no browser engine — so **principle #5 relaxes** from "single
  binary" to "single per-platform bundle, download-and-run"; the `b2` CLI stays a literal single binary, so #5
  is still met to the letter by the CLI and in spirit by the app.
- **Editor substrate = CodeMirror 6**, not the ProseMirror / Tiptap / Wordgard node-tree lineage. A WYSIWYG
  tree makes Markdown a lossy serialization at the edges — frontmatter, `[[path|title]]` wikilinks, and
  `b2_relations:` get normalized and churn noisy diffs — a direct hit to **principle #1** ("the files *are* the
  format; export is a no-op"). CodeMirror keeps the Markdown buffer canonical (live-preview decorations give
  the document feel), and is what Obsidian's own editor is built on. (Wordgard — Haverbeke's ProseMirror
  rethink — was evaluated and rejected: it borrows CodeMirror's *change model* but keeps a *node tree*, so it
  inherits the same fidelity problem.)
- **Transport = Tauri IPC**, not an HTTP server. The frontend calls the façade through in-process commands; a
  `b2 serve` HTTP adapter is a *different* concern (remote / browser / agent-over-HTTP), **deferred** rather
  than built alongside — building both is the real over-complication.
- **One new adapter, `b2-desktop`, holds no engine logic.** A thin Tauri host (charter:
  [crates/b2-desktop/CLAUDE.md](../crates/b2-desktop/CLAUDE.md)) + a `ui/` CodeMirror frontend. `b2-core`
  never learns about the UI, so the fast core suite is untouched and the GUI inherits the façade's tests —
  the "second dumb adapter over the same contract, inheriting every test the CLI bought" promise, kept. The
  MVP ships **read-only-first** (render → discover → link, the connection-discovery loop made visual);
  CodeMirror editing and native fs-watch are the immediate fast-follow.

## Decisions locked (2026-07-08) — file-type support (resources)

The vault already *contains* non-`.md` files (any real Obsidian vault does); B2 pretended it didn't — the
walk was `.md`-only. This settles how B2 treats them. Full design, taxonomy, rendering, and build plan:
[research/file-type-support.md](research/file-type-support.md); mirrored into
[data-model.md](data-model.md) §10 (the resource object) and [index-engine.md](index-engine.md) §3 (the
`resources` table + extraction step).

- **Markdown is the vault's only *authoring surface*; every other file is a *resource*** — a first-class
  peer vault member: indexed, searchable, linkable, renderable, and **never required to be referenced by
  any note**. The invariant generalizes from `index = projection of (the .md files)` to
  **`index = projection of (the vault directory)`**; resources contribute only *derived* rows (metadata,
  extracted text, inbound edges) — **no new tier, no sidecar files**, both tenets intact.
- **The one asymmetry is authoring surface, not status.** A resource has bytes, a path, derivable text +
  vectors, and inbound links, but **no `b2id`** (nothing to stamp; sidecars would be durable state outside
  Markdown) and **no outbound edges in v1** (B2 authors structure only in Markdown, and a PNG has no home
  for it) — each consequence carrying a named relief valve (hash-assisted move repair; tail-verb inverse
  authoring; a future vault-level B2-managed relations file). *Not* subordination: an unlinked resource
  fully exists, and `text`/`html`/`pdf` are semantically self-sufficient.
- **Polymorphism = a closed class table with a total fallback**, not a trait hierarchy. Every file maps by
  **extension only** to one of `note` · `text` · `html` · `pdf` · `image` · `media`, with **`binary`** as
  the catch-all — so the taxonomy is *total* ("any file GitHub could store"), degrading gracefully, never
  refusing.
- **One embedding space in v1.** Every class funnels to *text* through the existing bge space; multimodal
  image embedding and an LLM/OCR **Describer** are **documented future seams, default-off** — the same
  Bitter-Lesson posture that cut the relator (2026-07-04), applied again.
- **Rendering = a viewer registry keyed by class, with a "No viewer available" fallback card** (metadata +
  backlinks + *Open in system default*). The one infrastructure key is the **Tauri asset protocol**
  (scoped, read-only) — which also unblocks inline images in the reading view, the gap
  [specs/completed/desktop-live-preview.md](specs/completed/desktop-live-preview.md) §8 named. Foreign
  **HTML is source-first in v1** (zero script-execution surface next to the IPC bridge); a sandboxed
  preview is a later slice.
- **Shipped in value-ordered slices** ([research/file-type-support.md](research/file-type-support.md) §8):
  ① inventory & graph (model-free, no new deps) → ② render mechanisms → ③ searchable resources → ④ PDF
  text → ⑤ semantic seams (future). Each independently shippable; later slices never rework earlier ones.

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
