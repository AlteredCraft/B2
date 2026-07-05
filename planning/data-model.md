---
title: "B2 — Data Model"
type: note
tags: [b2, data-model, frontmatter, typed-links, edges, provenance, okf]
created: 2026-06-29
status: draft
---

# B2 — Data Model

> Defines **what a note is** and **what a connection is**, as the plain-Markdown source of truth —
> engine-independent. This is the yardstick the index-engine work measures against: the SQLite schema
> in [index-engine.md](index-engine.md) (§3) is a *derived projection* of this model, and must satisfy
> it, not the other way round. Context: [vision-and-scope.md](vision-and-scope.md) (principles, scope,
> locked decisions), [user-stories.md](user-stories.md) (link format & identity, kernel scenarios),
> [tasks.md](tasks.md) (the open pieces this doc closes).

The model has exactly **two source-of-truth objects**, both plain Markdown:

1. **A note** — one `.md` file: YAML frontmatter + a Markdown body.
2. **A connection (edge)** — a typed, directed link from one note to another, written by a human in the
   body, or committed by B2 to frontmatter `relations:` on `b2 link` (§0).

### Two storage tiers

These two objects are the *knowledge*. B2 keeps just two storage tiers with sharply different durability
contracts — getting this split right is what keeps the vault pristine and the index honestly disposable:

1. **Markdown — source of truth for *knowledge*.** Notes + every committed edge, on your disk, fully
   usable with no B2. Stays **pristine**, and the **body is 100% the human's** — B2 never authors prose or
   structure into it. B2's only writes to a note are three, all minimal: stamping a missing `b2id` (§1),
   appending a committed edge to frontmatter `relations:` on `b2 link` (§2), and the mechanical rewrite of
   an inbound wikilink's *path text* when its target moves (§6). The body is never authored by B2 — the one
   body write is that move-repair, which fixes a link the human already wrote rather than adding one.
2. **Index (`b2.sqlite`) — disposable cache.** The search indexes and the keyed graph — everything the
   product reads on hot paths. Holds **nothing** that can't be reconstructed from the Markdown.

The crucial relationship between them:

> **Index = projection of (Markdown).** Drop `b2.sqlite` → re-derive the graph and search indexes from
> the Markdown → an identical index (the locked `full-reindex ≡ incremental-update` invariant). There is
> **no** durable state outside your notes: every connection you commit lives in the Markdown itself, so
> nothing outside Markdown can cause knowledge loss. *(Before the 2026-07-04 relator cut there was a
> third tier — a durable `.b2/log/` event log holding pending suggestions + rejection memory. With the
> LLM relator and its suggestion queue gone, that tier had nothing left to hold and was removed;
> [vision-and-scope.md](vision-and-scope.md) "Decisions locked (2026-07-04)".)*

---

## 0. The central decision — where a connection lives

This closes the "remaining central question" in [tasks.md](tasks.md). It is settled by one principle
plus the locked rule that B2 changes the vault only on your command:

- **The body is the human's document — B2 never authors it.** The body is what renders, exports, and
  prints; structure B2 injected there (a `## Relations` section appearing in a `resume.md`) would
  corrupt the document. So B2 writes **no** connections into the body. *(The lone body write is the
  mechanical repair of an inbound wikilink's path on move — fixing a link the human already wrote, never
  adding one.)*
- **B2 writes a connection only when you commit one** ([vision-and-scope.md](vision-and-scope.md),
  "Review & trust") — with `b2 link`, or a body link you write yourself. Nothing lands in a note that you
  didn't ask for; there is no agent proposing edges behind your back.

So a connection lives in exactly one of two homes, **by origin**:

| Origin of the edge | Where it lives | SSOT |
|---|---|---|
| Human-authored | **Body** — a bare `[[path\|title]]`, or `- <verb> [[path\|title]] — …`, where the human wrote it | the body; B2 **reads**, never writes it |
| Committed via `b2 link` (also any human/importer-written relation) | **Frontmatter `relations:`** — a typed-link string `- "<verb> [[path\|title]] — …"` (§2) | frontmatter; B2's managed metadata zone |

**`b2 link` writes frontmatter, not body.** Committing a connection appends one typed-link string to the
source note's `relations:` (Markdown first, index reconciled after). The user's body is byte-untouched;
the only change is one line in the frontmatter metadata. The edge then materializes as an
`origin='frontmatter'` edge derived from that Markdown — committing is the projection of an authored line,
not a bespoke index write (§3).

> One line: **the body holds connections the human writes; frontmatter `relations:` holds connections you
> commit with `b2 link`; both are authored Markdown, and the graph is their union.**

**The graph is the union of the two homes** ([index-engine.md](index-engine.md) §3): the `edges` table is
a projection of body links (`origin=inline`) ∪ frontmatter relations (`origin=frontmatter`). Each edge has
exactly **one** home and B2 never copies between them — so there is nothing to keep "in sync," only a
one-way projection to rebuild. The single overlap case — a human manually re-authoring in the body a
connection already committed in frontmatter — is resolved at projection time by **inline-wins** dedup: the
body row is kept, the redundant frontmatter row is ignored (and surfaced by `b2 explain`), never
auto-removed.

**The trade we accept:** a B2-committed edge is metadata, so it is *not* guaranteed clickable in vanilla
Obsidian's reading view (frontmatter, not prose). Human body links are untouched and stay clickable, and
Obsidian's untyped graph could never show an edge's *type* anyway — so keeping committed edges out of the
body costs little and keeps the document pristine. Frontmatter relations are also the more OKF-native
shape (§5).

---

## 1. The note

A note is one `.md` file: YAML frontmatter, then a Markdown body.

```markdown
---
b2id: 01J9Z3K7QX8V2B4N6M0PQR7TS         # durable identity (ULID); B2's one mandatory key, never changes
type: concept                           # required; OKF-compatible discriminator
title: "Spaced repetition"              # human title; the natural link alias
description: "Why expanding intervals beat massed practice."
tags: [learning, memory]
created: 2026-06-20
updated: 2026-06-29
aliases: [SRS]                          # optional Obsidian-native extra titles
relations:                              # B2's managed zone: committed typed edges (§2). origin=frontmatter
  - "contradicts [[notes/cramming-works|Cramming works]] — short-term recall only"
provenance:                             # optional; defaults to {by: human}
  by: human
---

Spaced repetition schedules reviews at expanding intervals…

It elaborates [[concepts/memory|Human memory]] — applies the forgetting curve.
```

The body link above is **human-authored** (`origin=inline`); the `relations:` entry is one B2 wrote on
**`b2 link`** (`origin=frontmatter`). Both are typed edges in the same syntax (§2) — they differ only in
*home*, which is exactly the body-vs-metadata line §0 draws. A human may also write typed lines in the
body, and B2 reads them; B2 just never writes there.

### Frontmatter schema

**Required**

- **`b2id`** — durable identity, ULID-style; **namespaced** so it never collides with a user's own
  `id`, an OKF `id`, or another tool's. The graph keys **every** edge by `b2id`, never by path or title
  ([user-stories.md](user-stories.md)). Set once and never changes; survives move, rename, split, and
  merge. *This is B2's one always-allowed edit to the vault:* B2 stamps a missing `b2id` **as needed**
  (on first sight of a note) — no `b2 init` gate, no refusing to index — because durable identity is the
  anchor everything else keys off and must travel in the file itself (it's what lets an out-of-band move
  be repaired, [user-stories.md](user-stories.md)). The stamp *is* the write — it lands in the note's
  frontmatter, so identity travels with the file and needs no separate record.
- **`type`** — what *kind* of note this is (`note`, `concept`, `source`, `person`, `daily`, …).
  Controlled-but-extensible; unknown values tolerated. This is the OKF entity discriminator (§5).

**Optional (B2-recognized)**

- **`title`** — human title and the natural alias for inbound `[[path|title]]` links. If absent, B2
  derives a display title from the first H1, then the filename (derivation is display-only; it does
  not write).
- **`description`** — one-line summary; feeds the embedding `title:`/`text:` prompt and OKF export.
- **`tags`** — list of strings.
- **`created` / `updated`** — ISO-8601 date or datetime. `created` is set at creation; `updated` is
  maintained by B2 on B2-authored edits (manual edits may set it too).
- **`aliases`** — Obsidian-native additional titles; B2 treats them as alternate link aliases.
- **`provenance`** — *optional, opt-in* note-level authorship: `{by: human | agent:<model-id>,
  source?, confidence?}`. Absent ⇒ treated as `{by: human}`. A hand-written frontmatter field for when
  you want a note's authorship recorded in the note itself; B2 neither requires nor manages it. (Edges
  carry no provenance — a committed edge is pristine; see §4.)
- **`relations`** — **B2's managed zone for committed typed edges** (§2). A YAML list of typed-link
  strings — `- "<verb> [[path|title]] — explanation"`, the *same* syntax as a body typed line (§2), just
  located in frontmatter so it is metadata, not document content. B2 appends here on **`b2 link`** (never
  the body); humans and importers may write it too. Round-tripped losslessly; edges from it are
  `origin=frontmatter` (§3).

**Unknown keys** — preserved verbatim and byte-for-byte on round-trip (§6). B2 never strips frontmatter
it doesn't understand; the vault stays the user's, plus whatever other tools wrote.

---

## 2. Authored links & typed relations

A connection is written in one of two places, with **one shared syntax**: the **body** (by a human) or
frontmatter **`relations:`** (by B2 on `b2 link`, or by a human/importer). The verb-and-wikilink form is
identical in both; only the *home* differs (§0). Body constructs are ordinary Obsidian Markdown —
clickable and meaningful with **no B2 running**; B2 *reads* them and never writes them.

### Bare wikilink ⇒ an untyped `references` edge

A normal `[[path|title]]` anywhere in prose is a connection of type **`references`**, `origin=inline`.
This is the untyped graph Obsidian already gives you; B2 simply keys it by `b2id`. It is **directed**
(A→B — the literal fact that A's text points at B), which preserves the backlink ↔ forward-link split:
`b2 neighbors` / `b2 explain` show it as *referenced-by* from B's side. Directed is the
information-preserving default — the symmetric "these are connected" view is always derivable from it
(in ∪ out), never the reverse — and it keeps the explicit symmetric verbs (`relates`, `contradicts`)
meaningful as deliberate choices.

> See [[concepts/memory|Human memory]] for the underlying mechanism.

### `- <verb> [[path|title]] — explanation` ⇒ a *typed* edge

A list item beginning with a **relation verb** followed by a wikilink is a typed edge. Optional trailing
text after an em-dash (or `:`) is the edge's **`explanation`**.

```markdown
## Relations
- supersedes [[notes/old-plan|Old plan]] — replaced after the 2026-Q2 review
- example-of [[concepts/forgetting-curve|Forgetting curve]]
```

- A human may keep these under a `## Relations` heading or embed them anywhere in prose
  (Basic-Memory-style); a typed line is recognized **anywhere** in the body, so both round-trip. B2
  **reads** body typed lines but never writes them — its own edges go to frontmatter (below).
- The verb is plain text before a normal clickable wikilink, so Obsidian renders a clean list of links;
  the type is invisible structure to Obsidian and first-class structure to B2.

### Frontmatter `relations:` ⇒ a *typed* edge (`origin=frontmatter`)

The same `<verb> [[path|title]] — explanation` syntax, as a **quoted string** in a frontmatter
`relations:` list. This is where B2 writes a **committed** connection (§4, `b2 link`) and the only
structured place B2 authors edges — it is metadata, so it never appears in the rendered/exported document (§0).

```yaml
relations:
  - "supersedes [[notes/old-plan|Old plan]] — replaced after the 2026-Q2 review"
  - "example-of [[concepts/forgetting-curve|Forgetting curve]]"
```

- **Quoted** so `[[`, `|`, and `:` are always YAML-safe; the reader accepts quoted or unquoted.
- Parsed by the *same* verb/wikilink/explanation parser as a body typed line — one syntax, two homes.
- Humans and importers may write this block too (it supersedes the old "tolerated, not primary"
  framing); B2 appends to it on `b2 link` and never authors the body.

### Relation vocabulary — a tight, orthogonal core + a tolerated tail

The verb set has two consumers — **you**, when you type a connection with `b2 link` (or in the body), and
**queries / explainability** (`b2 neighbors --type supersedes`). Both want the core **small, orthogonal,
and stable**, so the same relationship always gets the same verb. Expressiveness lives in the tail;
reliability lives in the core.

**The core (closed set — your typing palette on `b2 link`, and what queries can rely on):**

| Category | Verb | Direction | Inverse (display only) |
|---|---|---|---|
| Referential | `references` | directed | referenced-by |
| Referential | `relates` | symmetric | relates |
| Expository | `elaborates` | directed | elaborated-by |
| Evidential ⭐ | `supports` | directed | supported-by |
| Evidential ⭐ | `refutes` | directed | refuted-by |
| Evidential ⭐ | `contradicts` | symmetric | contradicts |
| Structural | `example-of` | directed | has-example |
| Structural | `part-of` | directed | has-part |
| Versioning ⭐ | `supersedes` | directed | superseded-by |
| Versioning ⭐ | `derived-from` | directed | source-of |

The ⭐ categories — **evidential** ("argue the same / opposite") and **versioning** ("this supersedes
that") — are the ones the vision names as B2's reason to exist; they are non-negotiably first-class.

*Referential boundary (the one place classification can waver):* `references` is **automatic** (a bare
link, never hand-chosen); `relates` is a **deliberate symmetric** "these belong together"; `elaborates`
is a **deliberate directed** "A develops B."

**Extensibility model:**

- **Core** is the closed set above — the verbs `b2 link` offers as its palette and the verbs queries can
  rely on. Stable across versions.
- **Tail** — any other verb a human writes (`inspired-by`, `analogous-to`, …) is **tolerated and stored
  verbatim, never dropped**. Tooling treats tail verbs as opaque strings (no inverse label, no special
  traversal).
- **Promotion** — a tail verb that proves common can graduate into the core in a later version (gaining
  an inverse label). Demotion is just removal from the palette; stored data is untouched.

**Typing guidance:** prefer the **most specific** applicable core verb, falling back to `relates` (or a
bare `references` link) only when nothing more specific fits — so the vague symmetric default never
crowds out a real type.

**Conventions:**

- **lowercase kebab-case**, named from the source's perspective (`example-of`, not `HasExample`).
- **Edges are directed and stored once.** Every directed verb ships an inverse label (display-only):
  `b2 neighbors` / `b2 explain` compute inbound edges by scanning `dst_id` and label them with the
  inverse. **Symmetric** verbs (`relates`, `contradicts`) are their own inverse and traverse both ways
  with no special handling.
- B2 **never** writes a reciprocal link into the target file — that would be write-amplification and
  pollute a note the user didn't edit.

### Edge identity is *derived*, so the file stays clean

An authored edge — body **or** frontmatter — is identified by the tuple
**(src `b2id`, dst `b2id`, `type`, occurrence-index)**, all recoverable from the Markdown alone. No
edge-id is ever written into the file; `<verb> [[path|title]]` is the whole syntax in both homes. A
committed edge carries **no provenance at all** — it is a pristine authored line, nothing more (§4).

---

## 3. The connection / edge model (derived projection)

Every edge projects to one record. This is the shape the [index-engine.md](index-engine.md) `edges`
table holds; the Markdown is the source, this is the index.

| Field | Values | Source |
|---|---|---|
| `id` | derived tuple | edge identity, derived from `(src, dst, type, occurrence)` |
| `src_id`, `dst_id` | note `b2id`s — **never path** | resolved from the `[[path]]` at parse time |
| `type` | relation verb (§2) | the verb; `references` for a bare link |
| `origin` | `inline` (body) \| `frontmatter` (`relations:`) | which of the two homes (§0) the edge came from |
| `explanation` | free text, optional | trailing text after `—`/`:` |

- **Every edge is authored and active.** `origin` records *which home it came from* (§0) — `inline` or
  `frontmatter`. There is no lifecycle and no `status` column: with suggestions gone, an edge exists iff
  it is written in the Markdown, so every edge traces to an authored line in the file (body or
  frontmatter), never to a mutated index row — which is exactly what keeps `index = projection of
  (Markdown)` exact (§6). Committing with `b2 link` is just *appending that authored line* to frontmatter
  and re-projecting; it is not a status flip.
- **`src`/`dst` resolve path → `b2id` at parse time** and the edge stores only `b2id`s. This is why
  "rename keeps every backlink resolving" is a foreign-key truth, not a fix-up pass: a move rewrites `notes.path`
  and inbound `[[path|title]]` *text*, but no `edges` row changes ([index-engine.md](index-engine.md) §3).
- **The edge set is the union of the two homes, deduped.** `edges` projects body links (`origin=inline`)
  ∪ frontmatter `relations:` (`origin=frontmatter`). Each edge has exactly one home; if the *same*
  `(src, dst, type)` is authored in **both** the body and frontmatter (a human re-typing a committed
  edge), projection keeps the body row and drops the redundant frontmatter row — **inline-wins** —
  surfacing it via `b2 explain`, never auto-editing the file.

> **Why this projection is materialized, not computed on read.** A note's *outbound* edges are
> re-derivable by parsing that one file — which is exactly why this table is **disposable**. It is kept
> materialized so the queries parsing *can't* serve cheaply are fast: **backlinks** (inversion needs every
> *other* note), **typed multi-hop traversal**, and the **semantic⨝graph candidate query** behind
> `b2 similar` (nearest notes *minus* those already connected — the graph supplies the exclusion).
> Runtime parsing is the correctness spec; the table is its cache, not a third subsystem. Full rationale in
> [index-engine.md](index-engine.md) §3; the standing cost in §8.

---

## 4. Committing a connection — and why there is no third tier

With the LLM relator cut ([vision-and-scope.md](vision-and-scope.md), "Decisions locked (2026-07-04)"),
the data model has **no suggestion lifecycle, no review queue, no rejection memory, and no event log.** A
connection becomes real in exactly two ways, both **authored in Markdown**:

1. **A body link you write** — a bare `[[path|title]]` (an untyped `references` edge) or a typed
   `- <verb> [[path|title]] — explanation` line (§2). B2 **reads** it on the next reindex; it never
   writes the body.
2. **`b2 link <src> <dst> [--type <verb>] [--explanation …]`** — B2 appends one typed-link string to the
   **source note's frontmatter `relations:`** (Markdown first; **never the body**), then re-projects the
   note so the edge materializes from that Markdown as `origin='frontmatter'` (§3). `--type` defaults to
   `references`; the palette is the core vocabulary (§2). This is the *only* structured edge B2 authors,
   and it happens only on your explicit command.

Both paths yield ordinary **authored, active** edges — there is no `suggested`/`rejected` status and no
in-place flip; an edge exists iff it is written in the Markdown (§3).

### No provenance tier

A committed edge is **pristine**: no `by`, no `confidence`, no `source`, no breadcrumb — nothing stapled
to the note beyond the `<verb> [[path|title]]` line itself. There is nowhere else for edge provenance to
live, and that is deliberate — provenance was *decision fuel* for a review step that no longer exists.
(Optional **note-level** `provenance:` frontmatter remains the human's to write — §1 — but B2 neither
requires nor manages it, and it is separate from edges.)

### Why there is no event log

The durable `.b2/log/` tier existed for exactly two jobs — holding the **pending suggestion queue** and
remembering **rejections** so they weren't re-proposed. Both die with the relator. Its only other content
was `b2id.stamped`, and that is pure history: the `b2id` lives in the note's frontmatter, so the stamp is
reconstructible from the Markdown and needs no separate record. With nothing load-bearing left, the tier
(and its `replay(log) ⇒ review state` step) is **removed**. The consequence is the strongest form of the
disposable-index tenet: **`index = projection of (Markdown)`** — drop `b2.sqlite`, reindex, get an
identical index, with **no durable state anywhere outside your notes** (§6).

*(For the record: this reverses the three-tier / event-log design that §0, §3, and this section carried
through 2026-06-30. The reversal is scoped to the review layer — the body-vs-frontmatter home decision
(§0) and every invariant in §6 are unchanged.)*

---

## 5. OKF compatibility (export is a no-op, not a migration)

Build *like* OKF for cheap interop; don't depend on it ([vision-and-scope.md](vision-and-scope.md),
"Inspiration"). The model already lines up:

- **`type`** is the OKF entity discriminator — already required frontmatter (§1).
- **Resource URI** — B2 can mint a stable per-note URI from its `b2id` (`b2://<b2id>`, or a configured
  base) as a *derived* value (index, and optionally an `uri:` frontmatter key). It's a projection of
  identity we already have, so OKF export reads it off rather than computing a migration.
- **`index.md`** — a vault-root manifest listing notes/types, **derivable** from the frontmatter; B2 can
  emit it on demand so an OKF consumer has the collection entry point. It is generated, never a second
  source of truth.

Net: "export to OKF" is selecting and re-shaping fields that already exist — a no-op in spirit.

---

## 6. Invariants & serialization discipline

The model exists to make the three locked invariants
([vision-and-scope.md](vision-and-scope.md)) hold by construction — they are the **"volatile vault over
a disposable index"** tenet ([vision-and-scope.md](vision-and-scope.md#design-philosophy)) made
mechanical:

- **Round-trip losslessness** (`parse → serialize → parse` is byte-identical). B2 preserves unknown
  frontmatter keys *and their order*, body text, whitespace, and comment tokens. The **only** bytes B2
  ever changes are the specific mechanical edits it is asked to make: (a) stamping a missing `b2id`,
  (b) rewriting an inbound `[[oldpath|title]]` → `[[newpath|title]]` on a move (**the lone body write**),
  (c) appending one typed-link string to frontmatter `relations:` on `b2 link`, (d) optional
  cosmetic alias refresh. **The body is never authored by B2** — (a), (c), (d) are frontmatter, and (b)
  only repairs a link the human already wrote. Every other byte is untouched — directly satisfying the
  Story-1/Story-2 acceptance criteria ([user-stories.md](user-stories.md)).
- **`full-reindex ≡ incremental-update`.** The **index = projection of (Markdown)**: the edge set is a
  pure function of a note's Markdown plus the `path → b2id` resolution table. Re-deriving one note ≡
  re-deriving the vault for that note's edges; dropping `b2.sqlite` and rebuilding from the Markdown
  yields an identical index — there is no log term left to replay.
- **`rename keeps every backlink resolving`.** Edges key on `b2id`; path is a repairable convenience copy.
  A move rewrites path *text* in inbound files and zero edge rows.

These are the same tripwires [index-engine.md](index-engine.md) §8 calls out; this doc is where they're
defined, that doc is where they're enforced in the store.

---

## 7. Rejected / deferred alternatives

- **B2 authoring the body — rejected (Decision 1, 2026-06-30).** The body is the rendered/exported
  document and must stay 100% the human's; B2 injecting a `## Relations` section (or any prose) would
  corrupt it (imagine a `resume.md`). So B2's committed edges go to **frontmatter `relations:`** instead
  (§0, §2). The *only* body write B2 makes is the mechanical move-repair of an inbound wikilink's path —
  fixing a link the human already wrote, never adding one. **This reverses the earlier "accepted edges go
  inline in the body" decision.**
- **Inline-in-body as the home for accepted edges — superseded.** The trade: a frontmatter edge is *not*
  guaranteed clickable in vanilla Obsidian's reading view. Accepted because the body-pristine guarantee
  outweighs it — human body links stay clickable, and Obsidian can't render edge *types* regardless (§0).
- **A suggestion review layer — removed (2026-07-04).** B2 briefly generated LLM-typed suggestions into
  an inert review queue; the relator's per-pair cost didn't scale to a real vault and the machinery is
  gone (§4). Discovery is now `b2 similar` (B2 surfaces candidates) + `b2 link` (you commit) — no
  inert-until-accepted layer, because B2 proposes nothing on its own.
- **Stored reciprocal links — rejected.** Inverse edges are derived at query time; writing them back
  amplifies writes and edits notes the user didn't touch.
- **Per-edge ULIDs in the file — rejected.** Authored edge identity is derived from
  (`src`,`dst`,`type`,occurrence); explicit ids would clutter the note for no gain.
- **Edge provenance in Markdown (HTML comment or frontmatter field) — rejected.** A committed edge is a
  pristine `<verb> [[path|title]]` line and nothing more (§4). With no review step there is no provenance
  to keep — no `by`/`confidence`/`source` — so nothing is stapled to the note beyond the connection
  itself. This keeps committed edges pristine and the index honestly disposable.

---

## 8. A golden-vault sketch (for the test harness)

The smallest fixture that exercises the whole model — an authored typed edge and a bare reference. (Ties
to the testability stack, [vision-and-scope.md](vision-and-scope.md).)

`concepts/memory.md`
```markdown
---
b2id: 01JMEM0000000000000000000A
type: concept
title: "Human memory"
created: 2026-06-20
---
The brain encodes, stores, and retrieves information…
```

`notes/spaced-repetition.md`
```markdown
---
b2id: 01JSRS0000000000000000000B
type: concept
title: "Spaced repetition"
created: 2026-06-20
---
Spaced repetition exploits the [[concepts/memory|Human memory]] retrieval curve.

## Relations
- elaborates [[concepts/memory|Human memory]] — applies the forgetting curve
```

Here the `## Relations` block is **human-authored** in the body (so `origin=inline`); B2 reads it but
never writes there. Had you run `b2 link` to commit a `contradicts` edge, B2 would append it to
spaced-repetition's frontmatter `relations:` (`origin=frontmatter`) — never to this body section (§0).

Derived graph (no live model needed to assert):

- `references`: spaced-repetition → memory (origin=inline) — from the prose wikilink.
- `elaborates`: spaced-repetition → memory (origin=inline, explanation="applies…").

`b2 neighbors concepts/memory` returns spaced-repetition twice (referenced-by, elaborated-by); both
files round-trip byte-identical; dropping and rebuilding the index reproduces the identical graph.
`b2 similar concepts/memory` would rank other notes by embedding proximity, minus the ones already
connected here.

---

## 9. Judgment calls — resolved (2026-06-29; §0 revised 2026-06-30)

- **Where a connection lives (Decision 1–3, 2026-06-30; review layer removed 2026-07-04)** — **B2 never
  authors the body.** Human connections live in the body (B2 reads, never writes); **B2-committed edges
  live in frontmatter `relations:`** (via `b2 link`) as typed-link strings (Format A — the same
  `<verb> [[path|title]] — …` syntax as a body line). The graph is the **union** of the two homes,
  deduped **inline-wins** on overlap, never auto-editing the file. The body write B2 makes is the
  move-repair only. This **reverses** the earlier "accepted edges go inline in the body" call (§0, §2,
  §7).
- **Edge-provenance durability** — committed edges stay **pristine** (in frontmatter `relations:`), and
  as of 2026-07-04 carry **no** provenance at all: with the review step gone there is nothing to record
  (§4). The model is now **two tiers** (Markdown / disposable index) with `index = projection of
  (Markdown)`. *(Supersedes the earlier three-tier / `∪ log` resolution.)*
- **`b2id` backfill on ingest** — identity is **namespaced to `b2id`**, and stamping a missing one is
  **B2's single always-allowed edit** to the vault, done as needed on first sight (no `b2 init` gate, no
  refusing to index), written straight to the note's frontmatter with no separate log (§1).
- **Bare-wikilink default type** — a plain `[[path|title]]` is a **directed `references`** edge (§2): the
  minimal literally-true reading of "A's text points at B," strictly more expressive than symmetric
  `relates` (the symmetric view derives from directed, not the reverse), it preserves the backlink
  signal, and it keeps `relates`/`contradicts` meaningful as explicit symmetric verbs.
- **Relation vocabulary** — a **10-verb core** across 5 orthogonal categories (referential, expository,
  evidential, structural, versioning), the closed palette for `b2 link` + queries; a **tolerated tail**
  stored verbatim; a **promotion path**; plus the conventions and "most-specific-then-`relates`" typing
  guidance (§2).

**Still open:** none — the data model is locked. Next is the **index-engine build** against golden-vault
fixtures ([index-engine.md](index-engine.md), now reconciled with this two-tier model).

> Next ([tasks.md](tasks.md)): this model is the yardstick for the **index-engine evaluation** — whose
> recommendation ([index-engine.md](index-engine.md)) already targets this exact note/edge/provenance
> shape. With the data model fixed, the engine can be built against golden-vault fixtures.
