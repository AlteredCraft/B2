# Data model

What a note and a connection are, in plain Markdown, for anyone changing how B2 reads or
writes them. This is the yardstick the engine is measured against: the SQLite schema in
[index-engine.md](index-engine.md) §3 is derived from this model and must satisfy it, never
the reverse.

Related pages: [invariants.md](invariants.md) is the normative register, cited by id.
[index-engine.md](index-engine.md) is the *how*. The *why* behind each choice is an
[ADR](../ADRs/README.md): why connections live where they do (ADR-0010), why B2 never writes
the body (ADR-0004), why identity is the path (ADR-0003), why there is no suggestion queue
(ADR-0009).

The model has exactly two source-of-truth objects, both plain Markdown:

1. **A note.** One `.md` file: YAML frontmatter plus a Markdown body.
2. **A connection (edge).** A directed link from one note to another: a plain link you write
   in the body, or a typed relation in frontmatter `b2_relations:` (§0).

Both are authored by a human. A real vault also holds **resources**: every non-`.md` file. A
resource is a peer vault member, not a third authored object. B2 can read it but cannot author
structure into it, because Markdown is the only format whose bytes B2 may write. So the source
*tier* is the whole vault directory, while the two authored objects stay note plus edge.
Resources are defined in §10; they change nothing in §0 to §9.

### The two storage tiers

1. **Markdown: the source of truth for knowledge.** Your notes plus every committed edge, on
   your disk, fully usable with no B2. The files stay pristine, and the body is 100% yours
   (W2). The short list of on-command writes is W3. Reading your vault writes nothing at all
   (W1).
2. **The index (`.b2/b2.sqlite`): a disposable cache.** The search indexes and the keyed
   graph. It holds nothing that cannot be rebuilt from the Markdown.

The rules: two tiers, sharply split (S1); `index = projection of (the vault directory)` (S2,
ADR-0002). Drop `b2.sqlite`, reindex, get an identical index (S3). No durable B2-derived state
lives outside your notes (S4). A resource (§10) contributes only derived rows, so the
guarantee is unchanged.

### Folders

Your vault directory carries two kinds of authored material: the Markdown files (content) and
the directory tree itself (structure). A folder, empty or not, is user-authored exactly like a
note, and the filesystem is authoritative for it. B2 proxies the OS rather than modeling
folders:

- `create_dir` makes missing parents but refuses an occupied target. There is no `mkdir -p`
  idempotence: you asked to *create*.
- `move_dir` is one `rename`. `delete_dir` is `remove_dir_all`.
- Each resolves its target against the disk, never the index, so empty folders work everywhere.

Folders are never projected into the index. They carry nothing to chunk, embed, or link. The
tree listing (`Vault::list_dirs`) is a live filesystem walk (dot-folders skipped, §1), so the
tree matches the vault's managed subtree by construction, in both directions. S4 scopes to
*B2-derived* data; your own structure is vault material, not B2 state.

---

## 0. Where a connection lives

Ruled by [ADR-0010](../ADRs/0010-typed-graph-two-homes-closed-vocabulary.md) and
[ADR-0004](../ADRs/0004-markdown-is-the-only-surface-b2-writes.md). A connection lives in
exactly one of two homes, by origin. The homes split by *what they can say*, not just by who
writes them:

| Origin of the edge | Where it lives | Source of truth |
|---|---|---|
| A plain body link | The body: a bare `[[path\|title]]`, a Markdown `[text](path)`, an embed. Ordinary Markdown, always an untyped `references` edge | The body. B2 reads it, never writes it |
| A typed relation (committed by `b2 link`, or written by you or an importer) | Frontmatter `b2_relations:`, as a typed-link string `- "<verb> [[path\|title]] — …"` (§2). The only home of a verb plus explanation | Frontmatter, B2's managed metadata zone |

**`b2 link` writes frontmatter, not the body.** Committing appends one typed-link string to
the source note's `b2_relations:` (Markdown first, index reconciled after). The edge then
materializes as an `origin=frontmatter` edge derived from that Markdown line. Committing is
the projection of an authored line, not a bespoke index write (§3).

In one line: the body holds the plain links you write (all `references`); frontmatter
`b2_relations:` holds every *typed* relation, verb and explanation; both are authored
Markdown, and the graph is their union (G2).

Each edge has exactly one home, and B2 never copies between them. So there is nothing to keep
in sync, only a one-way projection to rebuild. A `b2_relations:` entry may deliberately target
a note the body already links: that is the augment flow (§2). The overlap case, the *same*
`(target, type)` in both homes (necessarily `references`, the only type a body link can
carry), is resolved at projection time by frontmatter-wins dedup. The frontmatter row is kept,
because only it can carry an explanation. The redundant body reference is ignored as a
duplicate, never auto-removed from your file.

---

## 1. The note

A note is one `.md` file whose vault-relative path has no dot-prefixed segment: YAML
frontmatter, then a Markdown body. Any segment counts, so `notes/.templates/daily.md` is no
more a note than `.scratch.md` is.

```markdown
---
type: concept                           # optional, defaults to `note`; OKF entity discriminator
title: "Spaced repetition"              # optional, INERT — the title is the filename (L2)
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

The body link is human-authored (`origin=inline`) and untyped: an ordinary `references` edge.
The surrounding prose is just prose. The `b2_relations:` entry is a typed edge
(`origin=frontmatter`).

### Hidden means hidden

A dot-prefixed name is outside the managed subtree, whatever it is: a folder (`.git/`,
`.obsidian/`, B2's own `.b2/`), a resource (`.DS_Store`), or a Markdown file (`.scratch.md`).
The convention is the filesystem's, not B2's, so B2 reads it the same way everywhere (S2,
[GH #136](https://github.com/AlteredCraft/B2/issues/136)):

- The walk skips it before it routes it (`pathspec::is_hidden`, applied above the
  note/resource dispatch in `collect_vault_files`). So "hidden" cannot mean one thing for a
  PDF and another for a Markdown draft. A dot-prefixed `.md` is never a note: no chunks,
  embeddings, graph presence, listing, search, or file-tree appearance.
- The file itself is untouched. Skipping is not deleting (W4). That is the point: keep a
  scratch draft out of B2's way without leaving the folder you work in.
- B2 will not author one either. Every authoring destination refuses a dot-prefixed segment,
  because creating a member the walk would never see is a silent desync between disk and
  index.

Migration note: rows for a previously indexed dot-prefixed `.md` are pruned on the next
reindex, and inbound links re-dangle (G5). Renaming it back restores it exactly.

### Frontmatter keys

**Required: nothing.** A note is a `.md` file; that is the whole entry requirement. Identity
is the vault-relative path (L1, ADR-0003), not a key in the file. There is no frontmatter B2
needs present, none it will add, and a note with no frontmatter block at all is an ordinary,
fully indexed note.

Optional keys B2 recognizes:

- **`type`**: what kind of note this is (`note`, `concept`, `source`, `person`, `daily`, …).
  Controlled but extensible; unknown values are tolerated; the OKF entity discriminator (§5).
  Defaults to `note`. Its only consumer is display, so nothing keys on its presence and the
  new-note template does not seed it (GH #80: the template stamps only what cannot be
  reconstructed later). Not `b2`-namespaced on purpose: a courtesy you own, not a key B2
  machines on.
- **`title`**: recognized but inert (L2). A note's title *is* its filename (basename minus
  `.md`). The key is parsed and round-tripped losslessly like any other, and never drives
  display, link aliases, or search.
- **`description`**: a one-line summary. Feeds the embedding prompt and OKF export.
- **`tags`**: a list of strings.
- **`created` / `updated`**: ISO-8601 date or datetime. `created` is set by B2 at creation
  (`b2 add`); `updated` is yours (or another tool's) to maintain. B2 does not stamp it.
- **`aliases`**: Obsidian-native additional titles; treated as alternate link aliases.
- **`provenance`**: optional, opt-in note-level authorship:
  `{by: human | agent:<model-id>, source?, confidence?}`. Absent means `{by: human}`. B2
  neither requires nor manages it. Edges carry no provenance (§4).
- **`b2_relations`**: B2's managed zone for typed edges (§2). A YAML list of typed-link
  strings, the only place a relation verb and explanation live. Namespaced, and B2's *only*
  key, so it can never collide with your `relations:` key or another tool's. For the same
  reason a generic un-namespaced `relations:` is *not* read; it is just another unknown key,
  preserved verbatim. B2 appends here on `b2 link` (never the body); you and importers may
  write it too.

**Unknown keys** are preserved verbatim and byte-for-byte on round trip (W5, §6). A `b2id:`
line an older B2 left behind is exactly that: an unknown key, never read, never removed
(ADR-0003).

---

## 2. Authored links and typed relations

### A bare wikilink is an untyped `references` edge

A normal `[[path|title]]` anywhere in prose is a connection of type `references`,
`origin=inline`: the untyped graph Obsidian already gives you, typed and materialized. It is
directed (A→B, the literal fact that A's text points at B). That preserves the split between
backlinks and forward links (`b2 neighbors` shows it as *referenced-by* from B's side), is the
information-preserving default, and keeps the explicit symmetric verb (`contradicts`)
meaningful as a deliberate choice.

### A frontmatter `b2_relations:` entry is a typed edge

The typed-link syntax is `<verb> [[path|title]] — explanation`, as a quoted string in a
`b2_relations:` list. This is the one and only home of a typed relation. The optional trailing
text after an em dash (or `:`) is the edge's `explanation`.

```yaml
b2_relations:
  - "supports [[concepts/forgetting-curve|Forgetting curve]] — the schedule exploits it"
  - "contradicts [[notes/cramming-works|Cramming works]]"
```

- Quoted, so `[[`, `|`, and `:` are always YAML-safe. The reader accepts quoted or unquoted.
- An entry that is just a bare `[[path|title]]` (no verb) is accepted and reads as
  `references`.
- You and importers may write this block too. B2 appends to it on `b2 link` and never authors
  the body.

### Typing a body link: frontmatter augments the body

A `b2_relations:` entry may target a note the body already links. It *augments* that
connection: the body keeps its plain, clickable link exactly as written, and the frontmatter
carries the stance. A different verb (`supports [[x]]` over a body `[[x]]`) adds the typed
edge alongside the untyped reference; both are real, separately authored facts. The same verb
(`references [[x]] — why`) collapses into one edge, frontmatter-wins (§0, §3), so the
explanation survives. This is the intended UI gesture: select a body link, choose a verb and
optionally an explanation, and B2 appends one `b2_relations:` entry. The body is never
touched.

### The relation vocabulary: a stance core plus a tolerated tail

Small, orthogonal, stable core; expressiveness in the tail (G3, ADR-0010).

The core (a closed set; the `b2 link` palette, and what queries can rely on):

| Verb | Stance | Direction | Inverse (display only) |
|---|---|---|---|
| `references` | neutral | directed | referenced-by |
| `supports` | for | directed | supported-by |
| `contradicts` | against | symmetric | contradicts |

Boundary notes: `references` is both the automatic type of a bare link and the deliberate
"see also". `supports` is a directed "A backs B". `contradicts` is a deliberate symmetric
"these state opposites"; tension has no aggressor, so no direction is recorded.

Extensibility: the core is stable across versions. Any other verb you write (`elaborates`,
`part-of`, `supersedes`, …) is tolerated and stored verbatim, never dropped. Tooling treats
tail verbs as opaque strings: no inverse label, no special traversal. A tail verb that proves
common can be promoted into the core later (gaining an inverse label). Demotion is just
removal from the palette; stored data is untouched.

Typing guidance: use a stance verb whenever the notes take a position on each other.
`references` is the honest default when they don't.

Conventions: lowercase kebab-case, named from the source's perspective (`derived-from`, not
`DerivedFrom`). Edges are directed and stored once (G4): inbound edges are computed by
scanning `dst_path` and labelled with the inverse; the symmetric verb is its own inverse. B2
never writes a reciprocal link into the target file. That would amplify writes and edit a note
you didn't touch.

### Edge identity is derived, so the file stays clean

An authored edge, body or frontmatter, is identified by the tuple
`(src path, dst path, type, occurrence-index)`, all recoverable from the Markdown alone. No
edge id is ever written into the file. A committed edge carries no provenance at all: it is a
pristine authored line, nothing more (§4).

---

## 3. The edge record (derived projection)

Every edge projects to one record. This is the shape the [index-engine.md](index-engine.md) §3
`edges` table holds. The Markdown is the source; this is the index.

| Field | Values | Source |
|---|---|---|
| `id` | derived | edge identity, from `(src, dst, type, occurrence_index)` |
| `src_path` | note path | the authoring note (always a note, G6) |
| `dst_path` | note path, or NULL | resolved from the authored `[[path]]` at parse time |
| `dst_resource_path` | resource path, or NULL | set instead of `dst_path` when the target is a non-`.md` file (§10) |
| `dst_path_raw` | text | the target exactly as authored; what a dangling edge keeps |
| `type` | relation verb (§2) | the `b2_relations:` verb; `references` for every body link |
| `origin` | `inline` \| `frontmatter` | which of the two homes (§0) the edge came from |
| `explanation` | free text, optional | trailing text after `—` or `:` (frontmatter entries only) |
| `caption` | text, optional | a Markdown link's text, or an embed's alt text |

- **Every edge is authored and active** (G1). `origin` records which home it came from. There
  is no lifecycle and no `status` column: an edge exists exactly when it is written in the
  Markdown, which is what keeps `index = projection of (Markdown)` exact.
- **`src` and `dst` are vault-relative paths, resolved at parse time.** The authored
  `[[path]]` is normalized by the resolver (the wikilink `+ ".md"` ladder) and stored as the
  path it named. `dst_path_raw` keeps the text exactly as written. So an edge stores what you
  authored, and the graph carries no identifier the vault does not. A B2-performed move is
  therefore one transaction: rewrite the inbound `[[path|title]]` text, re-key the moved
  note's rows, re-project the inbound sources. That is how L1's "rename keeps every backlink
  resolving" holds. A move made outside B2 is a delete plus a create, and its inbound links
  project as surfaced dangling edges (G5).
- **The edge set is the union of the two homes, deduped** (G2). A frontmatter entry with a
  *different* verb than a body link to the same target is no duplicate; that is the augment
  case (§2), and both edges project. If the *same* `(src, dst, type)` is authored in both
  homes (necessarily `references`), projection keeps the frontmatter row. Frontmatter wins
  because only it can carry an explanation. The file is never auto-edited.
- **A `dst` may be a resource, not a note.** A body embed or link to a non-`.md` file
  (`![[photo.png]]`, `[[papers/x.pdf]]`) resolves against the `resources` table and records
  `dst_resource_path`. `src` is still a note (§10).
- **A `dst` that resolves to nothing is a surfaced dangling edge, not a dropped one** (G5). A
  note is one `.md` file (§1), so a `[[Hermes]]` naming a folder, or a plain typo, matches no
  note and no resource. The edge is still projected, with both `dst_path` and
  `dst_resource_path` NULL. `b2 neighbors` and `b2 explain` (and the desktop Connections pane)
  present these distinctly, as *unresolved* with the authored target, so a mistyped link reads
  as broken rather than silently vanishing
  ([GH #12](https://github.com/AlteredCraft/B2/issues/12)). Resolving the target turns the
  same edge into an ordinary connection on the next reindex. Folder-note resolution
  (Obsidian-style `Hermes/Hermes.md`) is a possible later refinement, deliberately out of
  scope.

Why this projection is materialized rather than computed on read, and why that does not make
it a third source of truth (G6), is [index-engine.md](index-engine.md) §3. The standing cost
is its §8.

---

## 4. Committing a connection

There is no suggestion lifecycle, no review queue, no rejection memory, and no event log
(ADR-0009). A connection becomes real in exactly two ways, both authored in Markdown:

1. **A body link you write.** A plain `[[path|title]]`, Markdown link, or embed: an untyped
   `references` edge (§2). B2 reads it on the next reindex; it never writes the body.
2. **`b2 link <src> <dst> [--type <verb>] [--explanation …]`.** B2 appends one typed-link
   string to the source note's frontmatter `b2_relations:` (Markdown first, never the body),
   then re-projects the note so the edge materializes as `origin=frontmatter`. `--type`
   defaults to `references`; the palette is the core vocabulary (§2). B2 writes the target as
   a bare `[[path]]` with no `|alias`: the filename is the note's title (L2), so the path
   already reads as the title. (You may still write any `|alias` you like in a body link; B2
   reads it and never rewrites it.)

The GUI adds one more authoring *gesture* over the same model: dragging a Similar card onto a
line of the note being edited types a `[[wikilink]]` at that line's end. That is the untyped,
body kind of link, landing in the editor's buffer exactly as `[[` completion does. B2 still
authors nothing (W1), and you are still the precision gate.

**No provenance tier.** A committed edge is pristine: no `by`, no `confidence`, no `source`.
Nothing is stapled to the note beyond the `<verb> [[path|title]]` line itself. Provenance is
decision fuel for a review step B2 doesn't have. (The optional note-level `provenance:`
frontmatter stays yours to write, §1, and is separate from edges.)

---

## 5. OKF compatibility

Build *like* OKF for cheap interop; don't depend on it. Export is a no-op, not a migration.
The model already lines up:

- **`type`** is the OKF entity discriminator: recognized frontmatter, defaulting to `note`
  (§1).
- **Resource URI**: a per-note URI is derivable from its vault-relative path under a
  configured base. It is exactly as durable as the path (L1), the same handle Obsidian, a
  static-site generator, and a file manager all give you (ADR-0003).
- **`index.md`**: a vault-root manifest listing notes and types, derivable from the
  frontmatter, so an OKF consumer has a collection entry point. Generated, never a second
  source of truth.

Net: "export to OKF" is selecting and re-shaping fields that already exist. The export surface
itself (minting URIs, emitting the manifest) is not built;
[GH #103](https://github.com/AlteredCraft/B2/issues/103) tracks it.

---

## 6. Serialization rules

W5's lossless round trip is what makes the two-tier split safe. It is a property of the
parser/serializer, so it is specified here:

- Unknown frontmatter keys are preserved verbatim and in order. Body text, whitespace, and
  comment tokens are byte-preserved.
- The only bytes B2 ever changes are the specific mechanical edits it is asked to make (W3).
  Exactly one of those touches the body: rewriting an inbound `[[oldpath|title]]` to
  `[[newpath|title]]` on a move, aliases preserved verbatim. An operation you did not invoke
  changes no byte at all (W1).
- Nothing is reformatted, reordered, or normalized in passing. Not YAML quoting style, not
  list indentation, not line endings.

These are the tripwires [index-engine.md](index-engine.md) §8 budgets: this doc defines them,
that one enforces them in the store.

---

## 7. Rejected and deferred alternatives

The reasoning for each rejection lives in the ADR that made the call; this is the index.

| Rejected | Why, in one line | Recorded in |
|---|---|---|
| B2 authoring the body (a `## Relations` section) | The body is the rendered/exported document and must stay 100% yours | ADR-0004 |
| Typed-line syntax parsed from body prose | Would make B2 an interpreter of prose *shape*: `- see [[x]] for background` becoming verb `see` (L4) | ADR-0004, ADR-0010 |
| The body as the home for committed edges | Accepted trade: a frontmatter edge isn't guaranteed clickable in vanilla Obsidian, which can't render edge *types* regardless (§0) | ADR-0010 |
| A suggestion review layer / per-pair LLM relator | Notes × candidates model calls, and exactly the model-compensating machinery M1 defers | ADR-0009, ADR-0005 |
| A durable event-log tier (`.b2/log/`) | It would exist to hold a queue and rejection memory B2 doesn't have; anything durable outside the notes weakens S2/S4 | ADR-0002 |
| A stamped machine identity (`b2id:`), removed 2026-08-13 ([GH #170](https://github.com/AlteredCraft/B2/issues/170)) | Its one real buy (re-binding after an out-of-band move) cost an unbidden write, a machine key in every file, a collision subsystem, and a carve-out on S3 | ADR-0003 |
| Content-hash identity | Churns the graph on a typo fix. (Hash-keying the *vector store* is the opposite case and shipped with the same change: derived data, where content-addressing is exactly right, M4) | ADR-0003, ADR-0006 |
| Stored reciprocal links | Inverse edges are derived at query time (G4); writing them back amplifies writes and edits notes you didn't touch | ADR-0010 |
| Per-edge ULIDs in the file | Authored edge identity is derived (§2); explicit ids clutter the note for no gain | ADR-0010 |
| Edge provenance in Markdown | With no review step there is no provenance to keep (§4) | ADR-0009 |

---

## 8. The golden vault (the test fixture's shape)

The smallest fixture that exercises the whole model: an authored typed edge and a bare
reference.

`concepts/memory.md`
```markdown
---
type: concept
title: "Human memory"
created: 2026-06-20
---
The brain encodes, stores, and retrieves information…
```

`notes/spaced-repetition.md`
```markdown
---
type: concept
title: "Spaced repetition"
created: 2026-06-20
b2_relations:
  - "supports [[concepts/memory|Human memory]] — applies the forgetting curve"
---
Spaced repetition exploits the [[concepts/memory|Human memory]] retrieval curve.

Expanding review intervals exploit the forgetting curve.
```

The body holds one plain link (`origin=inline`, `references`), and the `b2_relations:` entry
types the same connection with a stance (`origin=frontmatter`, `supports`): the augment shape
from §2, exercising both homes at once. The derived graph (no live model needed to assert):

- `references`: spaced-repetition → memory (`origin=inline`), from the prose wikilink.
- `supports`: spaced-repetition → memory (`origin=frontmatter`, explanation "applies…").

`b2 neighbors concepts/memory` returns spaced-repetition twice (referenced-by, supported-by).
Both files round-trip byte-identical. Dropping and rebuilding the index reproduces the
identical graph.

---

## 9. Judgment calls: all resolved

The data model is locked; nothing is open. Every decision and its rejected alternatives are in
[ADRs/](../ADRs/README.md), chiefly ADR-0002 (two tiers), ADR-0003 (path identity), ADR-0004
(write discipline), ADR-0009 (no suggestion queue), and ADR-0010 (the two homes, the dedup
rule, and the verb core). The normative claims are the register's S, W, L, and G entries.

---

## 10. Resources: the second kind of vault member

§0 to §9 define the authored objects. A real vault also holds resources: every non-`.md`
file. This section defines what a resource *is* in the model; the schema is
[index-engine.md](index-engine.md) §3.

A resource is a peer vault member, not a lesser one, and not a generalized note. The single
asymmetry is the authoring surface, not status:

> A note is where structure is *authored*; a resource is a peer document B2 cannot write.
> Notes have frontmatter, authored edges, and B2's write guarantees, because Markdown is the
> one format whose bytes B2 may touch. Resources have bytes, derivable text and vectors, and
> inbound links. Identity is not part of the asymmetry: both are keyed by their vault-relative
> path (L3), so the note rules are the resource rules with an authoring surface added, not a
> second identity model.

What the asymmetry does *not* mean: a resource is never required to be attached to a note. An
unlinked resource fully exists: walked, classified, in the file tree, in the index, openable.

**Identity: path-keyed, index-only.** There is nowhere to stamp a machine id (binary bytes are
not B2's to edit; a sidecar file would be durable state outside Markdown, violating S4) and
nothing one would protect. That reasoning is now the whole vault's; ADR-0003 read it back onto
notes.

- **`b2 mv` on a resource** is simply *the* move: rewrite the inbound `[[path]]` /
  `![alt](path)` text, move the file, re-project. Identical terms to a note (§3).
- **Placing one is not authoring one.** "B2 cannot write a resource" is about its *content*:
  B2 never edits the bytes, and there is no format-specific writer. Putting a file *into* the
  vault on explicit command (the desktop's drag-from-Finder import, `Vault::import_file`) is
  file management, the same act as moving or deleting it (W3). The copy is byte-honest: B2
  places exactly the bytes it was handed, then projects them, routing on the extension exactly
  as the walk does. A dropped `.md` is therefore a *note* arriving, frontmatter and all.
- **An out-of-band move degrades exactly as a note's does:** identification, not repair. The
  old path's rows prune, the new path projects as a new member, and every inbound link
  surfaces as a dangling edge (G5). The index keeps a blake3 content hash per resource, and
  notes carry a `body_hash`, so a *proposed*-repair feature stays buildable on data already
  stored. It is recorded as future investigation in GH #170 and deliberately not built: a
  proposal is still yours to accept (W4).

**Edges: `src` is a note in v1; `dst` may be anything** (G6). This is a consequence of the
invariant, not a status rule: every edge must trace to an authored line in Markdown (§3), and
a resource has no writable home for one. Two relief valves keep this from becoming an
expressiveness wall: (a) today, the tolerated tail already authors the inverse direction from
the note side (`- "supported-by [[papers/x.pdf]]"`); (b) if needed, resource-sourced edges get
a designed future home, a vault-level B2-managed relations file, so the edge is still authored
Markdown and the invariant holds ([GH #102](https://github.com/AlteredCraft/B2/issues/102),
deferred until the need is real).

**What is built, and what is designed.** The inventory-and-graph half ships: the walk
classifies every non-`.md` file by extension into one of six classes (`text`, `html`, `pdf`,
`image`, `media`, `binary`, the total fallback), records
`(path, class, size, mtime, content_hash)`, prunes what the walk no longer meets, and resolves
body embeds and links at them into `dst_resource_path` edges. Classification is by extension
only: deterministic, and a misclassification degrades rather than mis-executes.

The content half is locked design, not shipped: every class funnels to *text*. Native for
`text`, extracted for `html` (tag-strip) and `pdf` (text layer), and, for an `image`, the
aggregated alt-text and captions from the notes that embed it (a pure projection of authored
Markdown). That text then flows through the existing bge space with zero new discipline
(chunks plus a per-document centroid).
[GH #108](https://github.com/AlteredCraft/B2/issues/108) tracks it,
[#109](https://github.com/AlteredCraft/B2/issues/109) the PDF extraction dependency, and
[#107](https://github.com/AlteredCraft/B2/issues/107) render mechanisms. Until they land, a
resource is an inventoried, linkable, openable peer that search and discovery do not reach.
Multimodal image embedding and an LLM/OCR describer are documented future seams, off by
default (M3, [GH #110](https://github.com/AlteredCraft/B2/issues/110)).

**Why a separate object, not a `kind` column on the note.** Two tables, two contracts, zero
"unless it's a resource" clauses. Generalizing `notes` to hold resources would staple a caveat
onto every invariant, write guarantee, and frontmatter behavior in §0 to §9. A distinct
`resources` table isolates the different *write* contract instead of threading it through the
note rules.
