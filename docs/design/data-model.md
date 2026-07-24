---
b2id: 01KWSRJ21R41A1RRWBQ9JAT871
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
> it, not the other way round. The companion design docs are [invariants.md](invariants.md) (the
> normative register — the *why*) and [index-engine.md](index-engine.md) (the *how*); planned work is
> tracked in [GitHub Issues](https://github.com/AlteredCraft/B2/issues).

The model has exactly **two source-of-truth objects**, both plain Markdown:

1. **A note** — one `.md` file: YAML frontmatter + a Markdown body.
2. **A connection (edge)** — a directed link from one note to another: a plain link a human writes in
   the body, or a typed relation in frontmatter `b2_relations:` (committed by B2 on `b2 link`, or
   written by a human/importer) (§0).

Both are **authored** — a human (or B2, in its one managed zone) writes their structure in Markdown. A
real vault also holds **resources** — every non-`.md` file (a PDF, a PNG, a `.csv`, an `.html` clipping).
A resource is a **peer vault member**, *not* a third authored object: B2 can *read* it (metadata,
extracted text, inbound links) but cannot *author* structure into it, because Markdown is the only format
whose bytes B2 may write. So the source *tier* widens from "the `.md` files" to **the whole vault
directory**, while the two authored objects stay note + edge. Resources are defined in **§10**; §0–§9 are
about the authored objects and are unchanged by them.

### Two storage tiers

These two objects are the *knowledge*. B2 keeps just two storage tiers with sharply different durability
contracts — getting this split right is what keeps the vault pristine and the index honestly disposable:

1. **Markdown — source of truth for *knowledge*.** Notes + every committed edge, on your disk, fully
   usable with no B2. Stays **pristine**, and the **body is 100% the human's** — B2 never authors prose or
   structure into it. B2's only writes to a note are three, all minimal: stamping a missing `b2id` (§1),
   appending a committed edge to frontmatter `b2_relations:` on `b2 link` (§2), and the mechanical rewrite of
   an inbound wikilink's *path text* when its target moves (§6). The body is never authored by B2 — the one
   body write is that move-repair, which fixes a link the human already wrote rather than adding one.
2. **Index (`b2.sqlite`) — disposable cache.** The search indexes and the keyed graph — everything the
   product reads on hot paths. Holds **nothing** that can't be reconstructed from the Markdown.

The crucial relationship between them:

> **Index = projection of (the vault directory).** Drop `b2.sqlite` → re-derive the graph and search
> indexes from the vault → an identical index (the locked `full-reindex ≡ incremental-update`
> invariant). There is **no** durable B2-derived state outside your notes: every connection you commit
> lives in the Markdown itself, so nothing outside the vault can cause knowledge loss. A **resource**
> (§10) contributes only *derived* rows (metadata, extracted text, inbound edges) and holds no durable
> authored state, so the guarantee is unchanged.

### Folders — user-authored structure, filesystem-authoritative

The vault directory carries two kinds of authored material: the Markdown files (**content**) and the
directory tree itself (**structure**). A folder — *empty or not* — is user-authored exactly like a note,
and the **filesystem is authoritative** for it; B2 proxies the OS rather than modeling folders: create
makes missing parents but refuses an occupied target (`Vault::create_dir` — no `mkdir -p` idempotence;
the human asked to *create*), move is one `rename` (`move_dir`), delete is `remove_dir_all`
(`delete_dir`), and each resolves its target against the *disk*, never the index, so empty folders are
first-class throughout (`b2 mv`, `b2 rm -r`, the desktop tree). Folders are **never projected into the
index** — they carry nothing to chunk, embed, or link — so the file tree's structure listing
(`Vault::list_dirs`) is a **live fs walk** (dot-folders skipped, the ingest walk's routing rule): the tree
is one-to-one with the vault's managed (non-dot) directory tree *by construction*, in both directions —
a Finder `mkdir` appears on the next pulse, and a folder emptied by a move stays visible until the human
removes it. The "no durable
state outside Markdown" guarantee above scopes to **B2-derived data**; the human's own structure is vault
material, not B2 state.

---

## 0. The central decision — where a connection lives

Settled by one principle plus the locked rule that B2 changes the vault only on your command:

- **The body is the human's document — B2 never authors it, and never asks it to carry B2 syntax.**
  The body is what renders, exports, and prints; structure B2 injected there (a `## Relations` section
  appearing in a `resume.md`) would corrupt the document. So B2 writes **no** connections into the body.
  The same principle bounds *reading*: B2 reads the body strictly as ordinary Markdown — links are
  links, prose is prose — and no prose shape (a list marker, a leading verb) is ever B2 structure
  (§2, §7). *(The lone body write is the mechanical repair of an inbound wikilink's
  path on move — fixing a link the human already wrote, never adding one.)*
- **B2 writes a connection only when you commit one** ([invariants.md](invariants.md),
  "Review & trust") — with `b2 link`, or a body link you write yourself. Nothing lands in a note that you
  didn't ask for; there is no agent proposing edges behind your back.

So a connection lives in exactly one of two homes, **by origin** — and the two homes split by *what
they can say*, not just who writes them:

| Origin of the edge | Where it lives | SSOT |
|---|---|---|
| A plain body link | **Body** — a bare `[[path\|title]]`, a Markdown `[text](path)`, an embed — ordinary Markdown, always an untyped `references` edge | the body; B2 **reads**, never writes it |
| A typed relation (committed via `b2 link`, or human/importer-written) | **Frontmatter `b2_relations:`** — a typed-link string `- "<verb> [[path\|title]] — …"` (§2); the **only** home of a verb + explanation | frontmatter; B2's managed metadata zone |

**`b2 link` writes frontmatter, not body.** Committing a connection appends one typed-link string to the
source note's `b2_relations:` (Markdown first, index reconciled after). The user's body is byte-untouched;
the only change is one line in the frontmatter metadata. The edge then materializes as an
`origin='frontmatter'` edge derived from that Markdown — committing is the projection of an authored line,
not a bespoke index write (§3).

> One line: **the body holds the plain links the human writes (all `references`); frontmatter
> `b2_relations:` holds every *typed* relation — verb and explanation live only there; both are authored
> Markdown, and the graph is their union.**

**The graph is the union of the two homes** ([index-engine.md](index-engine.md) §3): the `edges` table is
a projection of body links (`origin=inline`) ∪ frontmatter relations (`origin=frontmatter`). Each edge has
exactly **one** home and B2 never copies between them — so there is nothing to keep "in sync," only a
one-way projection to rebuild. A `b2_relations:` entry may deliberately target a note the body already
links — that is the **augment** flow (§2): the typed edge coexists with the body's plain reference. The
overlap case — the *same* `(target, type)` authored in both homes (necessarily `references`, the only
type a body link can carry) — is resolved at projection time by **frontmatter-wins** dedup: the
frontmatter row (the richer record — it alone can carry an explanation) is kept, the redundant body
reference is ignored as a duplicate, never auto-removed from the file.

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
type: concept                           # optional, defaults to `note`; OKF-compatible discriminator
title: "Spaced repetition"              # optional, inert: recognized but NOT special — the title is the filename
description: "Why expanding intervals beat massed practice."
tags: [learning, memory]
created: 2026-06-20
updated: 2026-06-29
aliases: [SRS]                          # optional Obsidian-native extra titles
b2_relations:                           # B2's managed zone: typed edges (§2). origin=frontmatter
  - "contradicts [[notes/cramming-works|Cramming works]] — short-term recall only"
provenance:                             # optional; defaults to {by: human}
  by: human
---

Spaced repetition schedules reviews at expanding intervals…

It builds on [[concepts/memory|Human memory]] — the forgetting curve is the mechanism.
```

The body link above is **human-authored** (`origin=inline`) and untyped — an ordinary `references`
edge; the surrounding prose is just prose, never B2 structure. The `b2_relations:` entry is a *typed*
edge (`origin=frontmatter`) — one B2 wrote on **`b2 link`**, or a human/importer authored directly.
Verb and explanation live only in the frontmatter home; the body home carries plain, clickable links —
exactly the body-vs-metadata line §0 draws.

### Frontmatter schema

**Required**

- **`b2id`** — durable identity, ULID-style; **namespaced** so it never collides with a user's own
  `id`, an OKF `id`, or another tool's. The graph keys **every** edge by `b2id`, never by path or title
  ([invariants.md](invariants.md)). Set once and never changes; survives move, rename, split, and
  merge. *This is B2's one always-allowed edit to the vault:* B2 stamps a missing `b2id` **as needed**
  (on first sight of a note) — no `b2 init` gate, no refusing to index — because durable identity is the
  anchor everything else keys off and must travel in the file itself (it's what lets an out-of-band move
  be repaired, [invariants.md](invariants.md)). The stamp *is* the write — it lands in the note's
  frontmatter, so identity travels with the file and needs no separate record.

**Optional (B2-recognized)**

- **`type`** — what *kind* of note this is (`note`, `concept`, `source`, `person`, `daily`, …).
  Controlled-but-extensible; unknown values tolerated. This is the OKF entity discriminator (§5).
  **Optional, defaulting to `note`** — ingest treats an absent `type` as `note`, and its only consumer
  is display, so nothing keys on its presence. The new-note template therefore does **not** seed it
  (GH #80): the template stamps only what can't be reconstructed later. Not `b2`-namespaced, on purpose
  — it's a courtesy the human owns, not a key B2 machines on (only `b2id` and `b2_relations:` are that).
- **`title`** — **recognized but inert (no special meaning).** A note's title **is its filename**
  (basename with the `.md` extension removed); B2 derives the display title from the path alone and never
  privileges this frontmatter field. The key is still parsed and round-tripped losslessly like any other
  (a human or importer may keep a `title:` for other tools), and it never drives B2's display, its link
  aliases, or search.
- **`description`** — one-line summary; feeds the embedding `title:`/`text:` prompt and OKF export.
- **`tags`** — list of strings.
- **`created` / `updated`** — ISO-8601 date or datetime. `created` is set by B2 at creation
  (`b2 add`); `updated` is the human's (or another tool's) to maintain — B2 does not stamp it.
- **`aliases`** — Obsidian-native additional titles; B2 treats them as alternate link aliases.
- **`provenance`** — *optional, opt-in* note-level authorship: `{by: human | agent:<model-id>,
  source?, confidence?}`. Absent ⇒ treated as `{by: human}`. A hand-written frontmatter field for when
  you want a note's authorship recorded in the note itself; B2 neither requires nor manages it. (Edges
  carry no provenance — a committed edge is pristine; see §4.)
- **`b2_relations`** — **B2's managed zone for typed edges** (§2). A YAML list of typed-link
  strings — `- "<verb> [[path|title]] — explanation"` — the **only** place a relation verb and
  explanation live; located in frontmatter so it is metadata, not document content. **Namespaced** like
  `b2id` so it can never collide with a user's own or another tool's `relations:` key — and for the
  same reason, a generic un-namespaced `relations:` is *not* read by B2 (it is just another unknown
  key, preserved verbatim). B2 appends here on **`b2 link`** (never the body); humans and importers may
  write it too. Round-tripped losslessly; edges from it are `origin=frontmatter` (§3).

**Unknown keys** — preserved verbatim and byte-for-byte on round-trip (§6). B2 never strips frontmatter
it doesn't understand; the vault stays the user's, plus whatever other tools wrote.

---

## 2. Authored links & typed relations

A connection is written in one of two places with **two different jobs**: the **body** holds plain
links (by a human — ordinary Obsidian Markdown, clickable and meaningful with **no B2 running**; B2
*reads* them and never writes them), and frontmatter **`b2_relations:`** holds *typed* relations (by
B2 on `b2 link`, or by a human/importer). **The body carries no B2 syntax**: every body link — bare
wikilink, Markdown link, embed — is an untyped `references` edge, and no prose around it changes that.
The verb and the explanation are frontmatter-only.

### Bare wikilink ⇒ an untyped `references` edge

A normal `[[path|title]]` anywhere in prose is a connection of type **`references`**, `origin=inline`.
This is the untyped graph Obsidian already gives you; B2 simply keys it by `b2id`. It is **directed**
(A→B — the literal fact that A's text points at B), which preserves the backlink ↔ forward-link split:
`b2 neighbors` / `b2 explain` show it as *referenced-by* from B's side. Directed is the
information-preserving default — the symmetric "these are connected" view is always derivable from it
(in ∪ out), never the reverse — and it keeps the explicit symmetric verb (`contradicts`) meaningful
as a deliberate choice.

> See [[concepts/memory|Human memory]] for the underlying mechanism.

### Frontmatter `b2_relations:` ⇒ a *typed* edge (`origin=frontmatter`)

The typed-link syntax `<verb> [[path|title]] — explanation`, as a **quoted string** in a frontmatter
`b2_relations:` list — **the one and only home of a typed relation**. Optional trailing text after an
em-dash (or `:`) is the edge's **`explanation`**. This is where B2 writes a **committed** connection
(§4, `b2 link`) and the only structured place B2 authors edges — it is metadata, so it never appears
in the rendered/exported document (§0).

```yaml
b2_relations:
  - "supports [[concepts/forgetting-curve|Forgetting curve]] — the schedule exploits it"
  - "contradicts [[notes/cramming-works|Cramming works]]"
```

- **Quoted** so `[[`, `|`, and `:` are always YAML-safe; the reader accepts quoted or unquoted.
- An entry that is just a bare `[[path|title]]` (no verb) is accepted and reads as `references`.
- Humans and importers may write this block too; B2 appends to it on `b2 link` and never authors the
  body.

### Typing a body link — frontmatter *augments* the body

A `b2_relations:` entry may target a note the body already links. It **augments** that connection:
the body keeps its plain, clickable link exactly as the human wrote it, and the frontmatter carries
the stance. A different verb (`supports [[x]]` over a body `[[x]]`) simply adds the typed edge
alongside the untyped reference — both are real, separately-authored facts. The same verb
(`references [[x]] — why`) collapses into one edge, **frontmatter-wins** (§0/§3), so the explanation
survives. This is the intended UI affordance: select (e.g. alt-click) a body link, choose a verb and
optionally an explanation, and B2 appends one `b2_relations:` entry — the body is never touched.

### Relation vocabulary — a stance core + a tolerated tail

The verb set has two consumers — **you**, when you type a connection with `b2 link` (or by hand in
`b2_relations:`), and **queries / explainability** (`b2 neighbors --type supports`). Both want the core
**small, orthogonal, and stable**, so the same relationship always gets the same verb — and the core
encodes the one thing embedding similarity cannot infer: **stance**. The model already surfaces "these are related"
(`b2 similar`); whether the notes *agree* is what only the human at the typing moment knows.
Expressiveness lives in the tail; reliability lives in the core.

**The core (closed set — your typing palette on `b2 link`, and what queries can rely on):**

| Verb | Stance | Direction | Inverse (display only) |
|---|---|---|---|
| `references` | neutral | directed | referenced-by |
| `supports` | for | directed | supported-by |
| `contradicts` | against | symmetric | contradicts |

*Boundary notes:* `references` is both the **automatic** type of a bare link and the deliberate
"see also" — one neutral verb, hand-chosen or not; `supports` is a **directed** "A backs B";
`contradicts` is a **deliberate symmetric** "these state opposites" — tension has no aggressor, so no
direction is recorded.

**Extensibility model:**

- **Core** is the closed set above — the verbs `b2 link` offers as its palette and the verbs queries can
  rely on. Stable across versions.
- **Tail** — any other verb a human writes (`elaborates`, `part-of`, `supersedes`, `inspired-by`, …) is
  **tolerated and stored verbatim, never dropped**. Tooling treats tail verbs as opaque strings (no
  inverse label, no special traversal).
- **Promotion** — a tail verb that proves common can graduate into the core in a later version (gaining
  an inverse label). Demotion is just removal from the palette; stored data is untouched.

**Typing guidance:** use a stance verb (`supports` / `contradicts`) whenever the notes take a position
on each other; `references` is the honest default when they don't. A more specific relationship than
the core expresses can always be hand-authored as a tail verb.

**Conventions:**

- **lowercase kebab-case**, named from the source's perspective (`derived-from`, not `DerivedFrom`).
- **Edges are directed and stored once.** Every directed verb ships an inverse label (display-only):
  `b2 neighbors` / `b2 explain` compute inbound edges by scanning `dst_id` and label them with the
  inverse. The **symmetric** verb (`contradicts`) is its own inverse and traverses both ways with no
  special handling.
- B2 **never** writes a reciprocal link into the target file — that would be write-amplification and
  pollute a note the user didn't edit.

### Edge identity is *derived*, so the file stays clean

An authored edge — body **or** frontmatter — is identified by the tuple
**(src `b2id`, dst `b2id`, `type`, occurrence-index)**, all recoverable from the Markdown alone. No
edge-id is ever written into the file; a body link is the whole syntax of a `references` edge, and
`<verb> [[path|title]]` the whole syntax of a typed one. A committed edge carries **no provenance at
all** — it is a pristine authored line, nothing more (§4).

---

## 3. The connection / edge model (derived projection)

Every edge projects to one record. This is the shape the [index-engine.md](index-engine.md) `edges`
table holds; the Markdown is the source, this is the index.

| Field | Values | Source |
|---|---|---|
| `id` | derived tuple | edge identity, derived from `(src, dst, type, occurrence)` |
| `src_id`, `dst_id` | note `b2id`s — **never path** | resolved from the `[[path]]` at parse time |
| `type` | relation verb (§2) | the `b2_relations:` verb; `references` for every body link |
| `origin` | `inline` (body) \| `frontmatter` (`b2_relations:`) | which of the two homes (§0) the edge came from |
| `explanation` | free text, optional | trailing text after `—`/`:` (frontmatter entries only) |

- **Every edge is authored and active.** `origin` records *which home it came from* (§0) — `inline` or
  `frontmatter`. There is no lifecycle and no `status` column: with suggestions gone, an edge exists iff
  it is written in the Markdown, so every edge traces to an authored line in the file (body or
  frontmatter), never to a mutated index row — which is exactly what keeps `index = projection of
  (Markdown)` exact (§6). Committing with `b2 link` is just *appending that authored line* to frontmatter
  and re-projecting; it is not a status flip.
- **`src`/`dst` resolve path → `b2id` at parse time** and the edge stores only `b2id`s. This is why
  "rename keeps every backlink resolving" is a foreign-key truth, not a fix-up pass: a move rewrites `notes.path`
  and inbound `[[path|title]]` *text*, but no `edges` row changes ([index-engine.md](index-engine.md) §3).
- **The edge set is the union of the two homes, deduped.** `edges` projects body links (`origin=inline`,
  all `references`) ∪ frontmatter `b2_relations:` (`origin=frontmatter`, the typed home). Each edge has
  exactly one home. A frontmatter entry with a *different* verb than a body link to the same target is
  no duplicate — it is the augment case (§2), and both edges project. If the *same* `(src, dst, type)`
  is authored in **both** homes (necessarily `references`), projection keeps the frontmatter row and
  drops the redundant body reference — **frontmatter-wins**, because only the frontmatter entry can
  carry an explanation — never auto-editing the file.
- **A `dst` may be a resource, not a note.** A body embed/link to a non-`.md` file (`![[photo.png]]`,
  `[[papers/x.pdf]]`) resolves against the `resources` table, not `notes`; the edge records a
  `dst_resource_path` instead of a `dst_id`, and `src` is still a note (resources author no outbound edges
  in v1, §10). Full model in §10; schema in [index-engine.md](index-engine.md) §3.
- **A `dst` that resolves to *nothing* is a surfaced dangling edge, not a dropped one.** A note is one
  `.md` file (§1), so a `[[Hermes]]` naming a **folder** — or a plain typo — matches no note and no
  resource: the edge is still projected with `dst_id` **and** `dst_resource_path` both NULL (`dst_path_raw`
  keeps the authored text). These are the vault's *broken links*. `b2 neighbors`/`b2 explain` (and the
  desktop Connections pane) present them **distinctly** — as *unresolved*, with the authored target — so a
  mistyped or folder-pointing link reads as broken rather than silently vanishing (GH #12). Resolving the
  target (create the note, fix the path) turns the same edge into an ordinary connection on the next
  reindex — no separate authoring step. Folder-note resolution (Obsidian-style `Hermes/Hermes.md`) is a
  possible later refinement, deliberately out of scope here.

> **Why this projection is materialized, not computed on read.** A note's *outbound* edges are
> re-derivable by parsing that one file — which is exactly why this table is **disposable**. It is kept
> materialized so the queries parsing *can't* serve cheaply are fast: **backlinks** (inversion needs every
> *other* note), **typed multi-hop traversal**, and the **semantic⨝graph candidate query** behind
> `b2 similar` (nearest notes *minus* those already connected — the graph supplies the exclusion).
> Runtime parsing is the correctness spec; the table is its cache, not a third subsystem. Full rationale in
> [index-engine.md](index-engine.md) §3; the standing cost in §8.

---

## 4. Committing a connection

The data model has **no suggestion lifecycle, no review queue, no rejection memory, and no event
log.** A connection becomes real in exactly two ways, both **authored in Markdown**:

1. **A body link you write** — a plain `[[path|title]]`, Markdown link, or embed: an untyped
   `references` edge (§2; the body carries no typed syntax). B2 **reads** it on the next reindex; it
   never writes the body.
2. **`b2 link <src> <dst> [--type <verb>] [--explanation …]`** — B2 appends one typed-link string to the
   **source note's frontmatter `b2_relations:`** (Markdown first; **never the body**), then re-projects the
   note so the edge materializes from that Markdown as `origin='frontmatter'` (§3). `--type` defaults to
   `references`; the palette is the core vocabulary (§2). This is the *only* structured edge B2 authors,
   and it happens only on your explicit command. B2 writes the target as a **bare `[[path]]`** — no
   `|alias`: the filename is the note's title (§1), so the path already reads as the title, and there is no
   privileged frontmatter title to source an alias from. *(A human writing a body link may still add any
   `|alias` they like; B2 reads it and never rewrites it.)*

Both paths yield ordinary **authored, active** edges — there is no `suggested`/`rejected` status and no
in-place flip; an edge exists iff it is written in the Markdown (§3).

### No provenance tier

A committed edge is **pristine**: no `by`, no `confidence`, no `source`, no breadcrumb — nothing stapled
to the note beyond the `<verb> [[path|title]]` line itself. There is nowhere else for edge provenance to
live, and that is deliberate — provenance is *decision fuel* for a review step B2 doesn't have.
(Optional **note-level** `provenance:` frontmatter remains the human's to write — §1 — but B2 neither
requires nor manages it, and it is separate from edges.)

---

## 5. OKF compatibility (export is a no-op, not a migration)

Build *like* OKF for cheap interop; don't depend on it ([invariants.md](invariants.md),
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
([invariants.md](invariants.md)) hold by construction — they are the **"volatile vault over
a disposable index"** tenet ([invariants.md](invariants.md)) made
mechanical (the full register, cited by id: [invariants.md](invariants.md)):

- **Round-trip losslessness** (`parse → serialize → parse` is byte-identical). B2 preserves unknown
  frontmatter keys *and their order*, body text, whitespace, and comment tokens. The **only** bytes B2
  ever changes are the specific mechanical edits it is asked to make: (a) stamping a missing `b2id`,
  (b) rewriting an inbound `[[oldpath|title]]` → `[[newpath|title]]` on a move (**the lone body write**;
  aliases preserved verbatim), (c) appending one typed-link string to frontmatter `b2_relations:` on
  `b2 link`. **The body is never authored by B2** — (a) and (c) are frontmatter, and (b)
  only repairs a link the human already wrote. Every other byte is untouched — directly satisfying the
  Story-1/Story-2 acceptance criteria ([invariants.md](invariants.md)).
- **`full-reindex ≡ incremental-update`.** The **index = projection of (the vault directory)**: the
  edge set is a pure function of a note's Markdown plus the `path → b2id` resolution table. Re-deriving
  one note ≡ re-deriving the vault for that note's edges; dropping `b2.sqlite` and rebuilding from the
  vault yields an identical index.
- **`rename keeps every backlink resolving`.** Edges key on `b2id`; path is a repairable convenience copy.
  A move rewrites path *text* in inbound files and zero edge rows.

These are the same tripwires [index-engine.md](index-engine.md) §8 calls out; this doc is where they're
defined, that doc is where they're enforced in the store.

---

## 7. Rejected / deferred alternatives

- **B2 authoring the body — rejected.** The body is the rendered/exported
  document and must stay 100% the human's; B2 injecting a `## Relations` section (or any prose) would
  corrupt it (imagine a `resume.md`). So B2's committed edges go to **frontmatter `b2_relations:`** instead
  (§0, §2). The *only* body write B2 makes is the mechanical move-repair of an inbound wikilink's path —
  fixing a link the human already wrote, never adding one.
- **Body typed-line syntax (`- <verb> [[path|title]] — …` parsed from prose) — rejected.**
  The read-side sibling of the decision above: parsing a verb + explanation out of body prose would make
  B2 an interpreter of prose *shape* — a list item that happens to open with a lowercase word before a
  link (`- see [[x]] for background`) would silently become a typed edge of verb `see`, a misread no
  human intended and a "special syntax" tax that violates §0's first principle. Typed relations are
  frontmatter-only; body links are always plain `references`
  (§2). A frontmatter entry augments a body link rather than the body carrying the type.
- **Inline-in-body as the home for committed edges — rejected.** The trade: a frontmatter edge is *not*
  guaranteed clickable in vanilla Obsidian's reading view. Accepted because the body-pristine guarantee
  outweighs it — human body links stay clickable, and Obsidian can't render edge *types* regardless (§0).
- **A suggestion review layer / LLM relator — rejected.** A per-pair LLM adjudicator that types and
  explains every candidate pair doesn't scale (~notes × candidates model calls) and is exactly the
  model-compensating machinery the Bitter-Lesson tenet defers. Discovery is `b2 similar` (B2 surfaces
  candidates) + `b2 link` (you commit) — no
  inert-until-accepted layer, because B2 proposes nothing on its own.
- **A durable event-log tier — rejected.** An in-vault `.b2/log/` would exist to hold a suggestion
  queue and rejection memory — neither of which B2 has — and stamp history the Markdown already carries
  (the `b2id` lives in the note itself). Anything durable outside the notes weakens
  `index = projection of (the vault directory)` (§6), so no log tier exists.
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
to the testability stack, [invariants.md](invariants.md).)

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
b2_relations:
  - "supports [[concepts/memory|Human memory]] — applies the forgetting curve"
---
Spaced repetition exploits the [[concepts/memory|Human memory]] retrieval curve.

Expanding review intervals exploit the forgetting curve.
```

Here the body holds one plain link (`origin=inline`, `references`) — its prose is just prose — and the
`b2_relations:` entry types the same connection with a stance (`origin=frontmatter`, `supports`): the
augment shape from §2, exercising both homes at once. Running `b2 link` to commit a further edge would
append another `b2_relations:` entry — never a body line (§0).

Derived graph (no live model needed to assert):

- `references`: spaced-repetition → memory (origin=inline) — from the prose wikilink.
- `supports`: spaced-repetition → memory (origin=frontmatter, explanation="applies…").

`b2 neighbors concepts/memory` returns spaced-repetition twice (referenced-by, supported-by); both
files round-trip byte-identical; dropping and rebuilding the index reproduces the identical graph.
`b2 similar concepts/memory` would rank other notes by embedding proximity, minus the ones already
connected here.

---

## 9. Judgment calls — resolved

- **Where a connection lives** — **B2 never
  authors the body.** Human connections live in the body (B2 reads, never writes); **B2-committed edges
  live in frontmatter `b2_relations:`** (via `b2 link`) as typed-link strings (the same
  `<verb> [[path|title]] — …` syntax a human could write). The graph is the **union** of the two homes,
  deduped **frontmatter-wins** on same-`(target, type)` overlap, never auto-editing the file. The body
  write B2 makes is the move-repair only (§0, §2, §7).
- **Edge-provenance durability** — committed edges stay **pristine** (in frontmatter `b2_relations:`)
  and carry **no** provenance at all: with no review step there is nothing to record
  (§4). The model is **two tiers** (Markdown / disposable index) with `index = projection of
  (the vault directory)` (§6).
- **`b2id` backfill on ingest** — identity is **namespaced to `b2id`**, and stamping a missing one is
  **B2's single always-allowed edit** to the vault, done as needed on first sight (no `b2 init` gate, no
  refusing to index), written straight to the note's frontmatter with no separate log (§1).
- **Bare-wikilink default type** — a plain `[[path|title]]` is a **directed `references`** edge (§2): the
  minimal literally-true reading of "A's text points at B," strictly more expressive than a symmetric
  reading (the symmetric view derives from directed, not the reverse), it preserves the backlink
  signal, and it keeps `contradicts` meaningful as the explicit symmetric verb.
- **Relation vocabulary** — a **three-verb stance core** (`references` / `supports` / `contradicts`:
  neutral / for / against), the closed palette for `b2 link` + queries; a **tolerated tail** stored
  verbatim; a **promotion path**; plus the conventions and stance-first typing guidance (§2).
- **The title is the filename.** A note's title **is its filename** (basename minus `.md`);
  the frontmatter `title:` key is **recognized but inert** — parsed and round-tripped, never privileged for
  display, link aliases, or search (§1). Notes are uniform with **resources**, which are
  filename-titled too. Consequences: the display title is a pure function of the path (so an
  unindexed note still titles correctly), and **`b2 link` writes a bare `[[path]]`** with no `|alias`
  (§4). Existing `title:` frontmatter stays untouched on disk.
- **Typed relations are frontmatter-only, under a namespaced `b2_relations:` key.** One
  principle — B2 forces no "special" syntax on the body and reads it only as ordinary
  Markdown. Every body link is a
  plain `references` edge; verb + explanation live **only** in frontmatter (§2, §7). The
  key is **namespaced** like `b2id` so it never
  collides with a user's or another tool's key; a generic `relations:` is not read (§1).
  Dedup is **frontmatter-wins**: a frontmatter entry *augments* a body
  link — same-verb overlap keeps the frontmatter row (it alone carries the explanation), a different
  verb coexists with the body's reference (§0, §2, §3). The intended UI affordance follows: select a
  body link, pick a verb/explanation, B2 appends one `b2_relations:` entry — the body is untouched.

**Still open:** none — the data model is locked.

---

## 10. Resources — the second kind of vault member

§0–§9 define the **authored** objects — note and edge — whose structure a human (or B2, in frontmatter)
writes in Markdown. A real vault also holds **resources**: every non-`.md` file — a PDF, a PNG, a `.csv`,
an `.html` clipping. This section defines what a resource *is* in the model; the full findings, taxonomy,
rendering, and build plan are tracked in [GitHub issue #66](https://github.com/AlteredCraft/B2/issues/66), and
the schema in [index-engine.md](index-engine.md) §3.

A resource is a **peer vault member** — not a lesser one, and not a generalized note. The single
asymmetry is **authoring surface, not status**:

> **A note is where structure is *authored*; a resource is a peer document B2 cannot write.** Notes have
> frontmatter, authored edges, a durable `b2id`, and B2's write guarantees — because Markdown is the one
> format whose bytes B2 may touch. Resources have bytes, a vault-relative path, *derivable* text and
> vectors, and *inbound* links.

What the asymmetry does **not** mean: a resource is never *required* to be attached to a note. An unlinked
resource fully exists — walked, classified, in the file tree, in the index, openable. `text`/`html`/`pdf`
resources are **semantically self-sufficient**: their own content is chunked, keyword-searchable, and
embedded with no note involved (a vault of only PDFs is a searchable, `b2 similar`-navigable vault).

**Identity — path-keyed, index-only.** A resource has **no `b2id`**: there is nowhere to stamp one (binary
bytes are not B2's to edit; a sidecar file would be durable state outside Markdown, violating the two-tier
tenet) and nothing it would protect (a resource's inbound links are plain path text B2 can rewrite
mechanically). It is keyed by its vault-relative path, recorded only in the disposable index.

- **`b2 mv` on a resource** works like a note move minus the identity step: rewrite the inbound `[[path]]` /
  `![alt](path)` text, move the file, re-project. "Rename keeps every backlink resolving" holds *when B2
  does the move*.
- **An out-of-band move degrades one notch further than a note's.** A Finder-moved note re-binds by its
  stamped `b2id`; a Finder-moved resource cannot. Mitigation: the index stores a **blake3 content hash**
  per resource, so a dangling link whose old target vanished and whose hash reappears at **exactly one**
  new path is surfaced as a *proposed* repair — flagged, never silently rewritten (duplicate files →
  ambiguous → flag only). Same posture as the note out-of-band case ([index-engine.md](index-engine.md) §8).

**Edges — `src` is a note in v1; `dst` may be anything.** A *consequence* of the invariant, not a status
rule: every edge must trace to an authored line in Markdown (§3), and a resource has no writable home for
one — no frontmatter, no body B2 may touch. So in v1 resources are edge **targets** only. Two relief valves
keep this from hardening into an expressiveness wall: **(a) today**, the tolerated tail already authors the
inverse direction from the note side (`- "supported-by [[papers/x.pdf]]"` in `b2_relations:` — stored,
displayed, queryable as a tail verb); **(b) if needed**, resource-sourced edges get a designed future home
— a **vault-level B2-managed relations file** (the frontmatter managed-zone concept lifted to one
clearly-B2-owned Markdown file), so the edge is still authored Markdown and the invariant holds. Deferred
until the need is real.

**Derived text — one embedding space in v1.** Every resource class funnels to *text* — native (`text`),
extracted (`html` tag-strip, `pdf` text layer), or, for an `image`, the aggregated alt-text/captions from
the notes that embed it (a pure projection of authored Markdown). That text flows through the **existing**
bge space with zero new discipline — chunks *and* a per-document **centroid** (discovery's coarse stage
scans only centroids, [#38](https://github.com/AlteredCraft/B2/issues/38)) — so `b2 search` and
`b2 similar` cover the whole vault. Multimodal image embedding (a second vector space — its own plain
vector/centroid tables and `meta` entry, the #38 shape) and an LLM/OCR **Describer** (intrinsic text for
`image`/`media`/`binary`) are **documented future seams**, default-off — the Bitter-Lesson
defer-by-default posture (§4). Schema, the per-class extraction step, and the taxonomy:
[index-engine.md](index-engine.md) §3.

**Why a separate object, not a `kind` column on the note.** Two tables, two contracts, zero "unless it's a
resource" clauses. Generalizing `notes` to hold resources would staple a caveat onto every invariant, write
guarantee, and frontmatter behavior in §0–§9; a distinct `resources` table isolates the different *write*
contract instead of threading it through the note rules
(see [#66](https://github.com/AlteredCraft/B2/issues/66)).
