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
   body, or by B2 in frontmatter `relations:` on accept (§0).

### Three storage tiers

These two objects are the *knowledge*. B2 keeps three storage tiers with sharply different durability
contracts — getting this split right is what keeps the vault pristine and the index honestly disposable:

1. **Markdown — source of truth for *knowledge*.** Notes + accepted edges, on your disk, fully usable
   with no B2. Stays **pristine**, and the **body is 100% the human's** — B2 never authors prose or
   structure into it. B2's only writes to a note are three, all minimal: stamping a missing `b2id` (§1),
   appending an accepted edge to frontmatter `relations:` (§2, §4), and the mechanical rewrite of an
   inbound wikilink's *path text* when its target moves (§6). The body is never authored by B2 — the one
   body write is that move-repair, which fixes a link the human already wrote rather than adding one.
2. **Index (`b2.sqlite`) — disposable cache.** The search indexes, the keyed graph, and the *live*
   review queue — everything the product reads on hot paths. Holds nothing that can't be reconstructed.
3. **Event log (`.b2/log/`) — durable, append-only history.** Every consequential operation B2 or an
   agent performs — suggestion generated / accepted / rejected, note created / moved, link rewritten —
   with verbose payloads (model id, confidence, evidence). Insurance, observability, and forensics;
   **not** read on hot paths. This is also the *"structured event stream to tail"* the headless-first
   observability story already calls for ([vision-and-scope.md](vision-and-scope.md)).

The crucial relationship between them:

> **Index = projection of (Markdown ∪ log).** Drop `b2.sqlite` → re-derive the graph from the Markdown
> and **replay** the review state from the log → an identical index (the locked
> `full-reindex ≡ incremental-update` invariant). Lose the log and you lose only *history* and rejection
> memory — **never a committed connection.** Nothing outside Markdown can cause knowledge loss.

---

## 0. The central decision — where a connection lives

This closes the "remaining central question" in [tasks.md](tasks.md). It is settled by one principle
plus the already-locked "inert until accepted":

- **The body is the human's document — B2 never authors it.** The body is what renders, exports, and
  prints; structure B2 injected there (a `## Relations` section appearing in a `resume.md`) would
  corrupt the document. So B2 writes **no** connections into the body. *(The lone body write is the
  mechanical repair of an inbound wikilink's path on move — fixing a link the human already wrote, never
  adding one.)*
- **Agent suggestions are inert until accepted** ([vision-and-scope.md](vision-and-scope.md), "Review &
  trust"), so a pending suggestion lives in the review layer and never touches the file at all.

So a connection lives in exactly one of three homes, **by origin**:

| Origin of the edge | Where it lives | SSOT |
|---|---|---|
| Human-authored | **Body** — a bare `[[path\|title]]`, or `- <verb> [[path\|title]] — …`, where the human wrote it | the body; B2 **reads**, never writes it |
| B2-**accepted** suggestion (also any human/importer-written relation) | **Frontmatter `relations:`** — a typed-link string `- "<verb> [[path\|title]] — …"` (§2) | frontmatter; B2's managed metadata zone |
| Agent-**suggested**, not yet accepted | **Review layer** (index queue + `.b2/` log) — never the file | the log |

**Acceptance is the bridge — and it writes frontmatter, not body.** Promoting a suggestion appends one
typed-link string to the source note's `relations:` (Markdown first, index reconciled after). The user's
body is byte-untouched at every step; the only change is one line in the frontmatter metadata. The edge
then re-materializes as an `origin='frontmatter'`, `status='active'` edge derived from that Markdown —
acceptance is a re-projection, not an in-place flip (§3, §4).

> One line: **the body holds connections the human writes; frontmatter `relations:` holds connections B2
> commits on your behalf; the review layer holds connections the agent proposes; accept moves one from
> the review layer into frontmatter.**

**The graph is the union of the three homes** ([index-engine.md](index-engine.md) §3): the `edges` table
is a projection of body links (`origin=inline`) ∪ frontmatter relations (`origin=frontmatter`) ∪ the log
(`origin=suggested`). Each edge has exactly **one** home and B2 never copies between them — so there is
nothing to keep "in sync," only a one-way projection to rebuild. The single overlap case — a human
manually re-authoring in the body a connection already accepted in frontmatter — is resolved at
projection time by **inline-wins** dedup: the body row is kept, the redundant frontmatter row is ignored
(and surfaced by `b2 explain`), never auto-removed.

**The trade we accept:** a B2-accepted edge is metadata, so it is *not* guaranteed clickable in vanilla
Obsidian's reading view (frontmatter, not prose). Human body links are untouched and stay clickable, and
Obsidian's untyped graph could never show an edge's *type* anyway — so keeping accepted edges out of the
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
relations:                              # B2's managed zone: accepted typed edges (§2). origin=frontmatter
  - "contradicts [[notes/cramming-works|Cramming works]] — short-term recall only"
provenance:                             # optional; defaults to {by: human}
  by: human
---

Spaced repetition schedules reviews at expanding intervals…

It elaborates [[concepts/memory|Human memory]] — applies the forgetting curve.
```

The body link above is **human-authored** (`origin=inline`); the `relations:` entry is one B2 wrote on
**accept** (`origin=frontmatter`). Both are typed edges in the same syntax (§2) — they differ only in
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
  be repaired, [user-stories.md](user-stories.md)). Every stamp is logged (`b2id.stamped`, §4); it is
  the single minimal write needed to establish identity.
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
  source?, confidence?}`. Absent ⇒ treated as `{by: human}`. By default B2 records note authorship as a
  `note.created` **log** event (§4), keeping notes pristine; write this field only when you want a
  note's own authorship durably visible in its frontmatter. (Edge-level provenance is separate; see §4.)
- **`relations`** — **B2's managed zone for accepted typed edges** (§2). A YAML list of typed-link
  strings — `- "<verb> [[path|title]] — explanation"`, the *same* syntax as a body typed line (§2), just
  located in frontmatter so it is metadata, not document content. B2 appends here on **accept** (never
  the body); humans and importers may write it too. Round-tripped losslessly; edges from it are
  `origin=frontmatter` (§3).

**Unknown keys** — preserved verbatim and byte-for-byte on round-trip (§6). B2 never strips frontmatter
it doesn't understand; the vault stays the user's, plus whatever other tools wrote.

---

## 2. Authored links & typed relations

A connection is written in one of two places, with **one shared syntax**: the **body** (by a human) or
frontmatter **`relations:`** (by B2 on accept, or by a human/importer). The verb-and-wikilink form is
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
`relations:` list. This is where B2 writes an **accepted** suggestion (§4) and the only structured place
B2 authors edges — it is metadata, so it never appears in the rendered/exported document (§0).

```yaml
relations:
  - "supersedes [[notes/old-plan|Old plan]] — replaced after the 2026-Q2 review"
  - "example-of [[concepts/forgetting-curve|Forgetting curve]]"
```

- **Quoted** so `[[`, `|`, and `:` are always YAML-safe; the reader accepts quoted or unquoted.
- Parsed by the *same* verb/wikilink/explanation parser as a body typed line — one syntax, two homes.
- Humans and importers may write this block too (it supersedes the old "tolerated, not primary"
  framing); B2 appends to it on accept and never authors the body.

### Relation vocabulary — a tight, orthogonal core + a tolerated tail

The verb set has two consumers — the **discovery agent** (which must classify every proposed connection
into a type) and **queries / explainability** (`b2 neighbors --type supersedes`). Both want the core
**small, orthogonal, and stable**, so the same relationship always gets the same verb. Expressiveness
lives in the tail; reliability lives in the core.

**The core (closed set — what discovery emits and the eval suite scores against):**

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

- **Core** is the closed set above — the only verbs discovery emits, the verbs queries can rely on, and
  the verbs the eval suite scores. Stable across versions.
- **Tail** — any other verb a human writes (`inspired-by`, `analogous-to`, …) is **tolerated and stored
  verbatim, never dropped**. Discovery never invents tail verbs, and tooling treats them as opaque
  strings (no inverse label, no special traversal).
- **Promotion** — a tail verb that proves common can graduate into the core in a later version (gaining
  an inverse label + discovery support). Demotion is just removal from the emit set; stored data is
  untouched.

**Classification rule:** discovery prefers the **most specific** applicable core verb and falls back to
`relates` only when nothing more specific fits — so the vague symmetric default never crowds out a real
type.

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
edge-id is ever written into the file; `<verb> [[path|title]]` is the whole syntax in both homes. Edge
provenance is never stapled to the note either; its history lives in the event log (§4).

---

## 3. The connection / edge model (derived projection)

Every edge — authored or suggested — projects to one record. This is the shape the
[index-engine.md](index-engine.md) `edges` + `edge_provenance` tables hold; the Markdown is the source,
this is the index.

| Field | Values | Source |
|---|---|---|
| `id` | ULID (suggested) / derived tuple (authored) | edge identity: minted for suggestions, derived for authored |
| `src_id`, `dst_id` | note `b2id`s — **never path** | resolved from the `[[path]]` at parse time |
| `type` | relation verb (§2) | the verb; `references` for a bare link |
| `origin` | `inline` (body) \| `frontmatter` (`relations:`) \| `suggested` (log) | which of the three homes (§0) the edge came from |
| `status` | `active` \| `suggested` \| `rejected` | lifecycle (§4) |
| `explanation` | free text, optional | trailing text after `—`/`:`, or the agent's rationale |
| *(provenance, joined)* | see §4 | — |

- **`origin` vs `status` are orthogonal, within limits.** `origin` records *which home it came from*
  (§0); `status` records *where it is in the review lifecycle*. The legal combinations are constrained:
  an `inline`/`frontmatter` edge is always `active`; a `suggested` edge is `suggested` until it is
  **rejected** or **accepted**. Acceptance is **not an in-place flip**: B2 appends the typed-link string
  to the source note's frontmatter `relations:`, the `suggested` row **leaves the review queue**, and the
  connection **re-materializes as an `origin='frontmatter'`, `status='active'` edge derived from the
  Markdown**. So an `active` edge always traces to an authored line in the file (body or frontmatter),
  never to a mutated queue row — which is exactly what keeps `index = projection of (Markdown ∪ log)`
  exact (§4, §6).
- **`src`/`dst` resolve path → `b2id` at parse time** and the edge stores only `b2id`s. This is why
  "rename keeps every backlink resolving" is a foreign-key truth, not a fix-up pass: a move rewrites `notes.path`
  and inbound `[[path|title]]` *text*, but no `edges` row changes ([index-engine.md](index-engine.md) §3).
- **The edge set is the union of the three homes, deduped.** `edges` projects body links
  (`origin=inline`) ∪ frontmatter `relations:` (`origin=frontmatter`) ∪ replayed log
  (`origin=suggested`). Each edge has exactly one home; if the *same* `(src, dst, type)` is authored in
  **both** the body and frontmatter (a human re-typing an accepted edge), projection keeps the body row
  and drops the redundant frontmatter row — **inline-wins** — surfacing it via `b2 explain`, never
  auto-editing the file.

> **Why this projection is materialized, not computed on read.** A note's *outbound* edges are
> re-derivable by parsing that one file — which is exactly why this table is **disposable**. It is kept
> materialized so the queries parsing *can't* serve cheaply are fast: **backlinks** (inversion needs every
> *other* note), **typed multi-hop traversal**, the **semantic⨝graph discovery join**, and the
> **suggestion queue** (suggested edges are inert and never on disk — §4 — so there is nothing to parse).
> Runtime parsing is the correctness spec; the table is its cache, not a third subsystem. Full rationale in
> [index-engine.md](index-engine.md) §3; the standing cost in §8.

---

## 4. Provenance, the suggestion lifecycle & the event log

The key realization: **provenance is *decision fuel* while a suggestion is pending, and *history* once
it's decided.** Those two roles want different homes — which is exactly what the three storage tiers
(intro) are for. `confidence` and `source` are the inputs to your accept/reject call; the moment you
decide, their job is done and they become history.

### Provenance fields (per edge)

- **`by`** — `human` or `agent:<model-id>` (e.g. `agent:claude-opus-4-8`). Who proposed it.
- **`source`** — the evidence: a candidate-generation signal (`"semantic+co-citation"`), a query, a
  chunk reference. Free-form; the input to your accept/reject call (capability area 7, explainability).
- **`confidence`** — `0.0–1.0`, for triaging the review queue.
- **`created`** — when the suggestion was generated.
- **`decided`** — when a human accepted or rejected it.

### Lifecycle: `suggested → active | rejected`

```
            agent proposes              human accepts
   (none) ──────────────▶ suggested ──────────────────▶ active
                              │   append to frontmatter → (origin=frontmatter,
                              │   relations:, row leaves    re-derived
                              │   queue, edge re-derives     from Markdown)
                              │ human rejects
                              ▼
                           rejected   (remembered, never re-surfaced, never in the file)
```

- **`suggested`** — lives in the **review layer**: the *live* queue in the index, durably recorded as a
  `suggestion.generated` event in the log. **Never in the file.** `b2 suggest <note>` lists them with
  type, explanation, and the full decision fuel. The vault on disk is byte-unchanged — the literal
  meaning of *inert until accepted*.
- **`active` (accept)** — B2 appends `- "<type> [[path|title]] — <explanation>"` to the **source note's
  frontmatter `relations:`** (Markdown first; **never the body**), then reconciles the index: the
  `suggested` row **leaves the review queue** and the edge **re-materializes from the Markdown** as an
  `origin='frontmatter'`, `status='active'` edge (acceptance is a re-projection, not an in-place status
  flip). B2 appends a `suggestion.accepted` event. The accepted edge is **pristine** — no provenance
  breadcrumb, no fingerprint; its `edge_provenance` row is gone. Once you've vetted it, it's an ordinary
  typed edge; the *history* of how it got there lives in the log.
- **`rejected`** — a `suggestion.rejected` event; the identity tuple (`src,dst,type`) is remembered so
  the same pair+type isn't proposed again. Never written to the file.

### Where each piece of provenance lives — resolved

| Stage | Lives in | Durability |
|---|---|---|
| Pending suggestion (full provenance) | index (live queue) + log (`suggestion.generated`) | log is durable; index replayable from it |
| Accepted edge — the *connection* | **Markdown** (frontmatter `relations:`), pristine | source of truth |
| Accepted edge — the *history* (who proposed, confidence, when) | **log** only | durable; never touches the note |
| Rejection memory | log (`suggestion.rejected`) + index tombstone | durable |

This is what lets accepted edges stay pristine **and** nothing be forgotten. We deliberately do **not**
persist edge *provenance* in Markdown — no HTML-comment breadcrumb, no provenance fields beside the
relation — because provenance after a decision is history, and history belongs in the log, not stapled to
your notes. (The accepted *connection* itself does live in frontmatter; its *provenance* does not.)

### The event log

- **Location:** in-vault **`.b2/log/`** — a dotfolder Obsidian ignores. Keeping it in the vault means
  your *entire* B2 state (knowledge + history) is one portable folder: clone it to a new machine and
  pending suggestions and rejection memory come along.
- **Format:** structured **append-only JSONL** to start, behind a **thin sink interface**
  (`append(event)` / `replay()`), so switching to an append-only SQLite log later is an implementation
  change, not a data-model change.
- **Events** (illustrative): `suggestion.generated|accepted|rejected`,
  `note.created|updated|moved|deleted`, `b2id.stamped`, `link.rewritten_on_move`. Verbose payloads
  (model id, confidence, evidence, old→new path) — cheap to write, there if ever wanted for debug or
  maintenance.
- **Replay = review-state recovery.** The pending queue and rejection tombstones are the one part of the
  index that *isn't* derivable from Markdown (suggestions are never written to notes). Replaying the log
  reconstructs them, so "drop the index, rebuild identical" stays literally true (§6). An **accepted**
  suggestion contributes **no** live queue row on replay — its connection re-derives from the inline
  Markdown link and the `suggestion.accepted` event is pure history — so it is never double-counted as
  both a queue row and an `active` edge. `replay(log) ⇒ review state` is a pure function — a clean
  deterministic seam for tests.
- **Compaction:** append-only logs grow; a later compaction step snapshots current review state and
  truncates superseded events. Trivial at personal-vault scale — **flagged for later, not v1.**
- **Bonus:** this is the same event stream `b2 explain` reads to answer "how did this edge come to be?"
  and the observability tail the headless-first story already wanted — one artifact, three jobs.

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
  (c) appending one typed-link string to frontmatter `relations:` on suggestion-accept, (d) optional
  cosmetic alias refresh. **The body is never authored by B2** — (a), (c), (d) are frontmatter, and (b)
  only repairs a link the human already wrote. Every other byte is untouched — directly satisfying the
  Story-1/Story-2 acceptance criteria ([user-stories.md](user-stories.md)).
- **`full-reindex ≡ incremental-update`.** The **index = projection of (Markdown ∪ log)**: the edge set
  is a pure function of a note's Markdown plus the `path → b2id` resolution table, and the review queue is
  a pure function of the log (`replay(log) ⇒ review state`). Re-deriving one note ≡ re-deriving the
  vault for that note's edges; dropping `b2.sqlite` and rebuilding from (Markdown ∪ log) yields an
  identical index.
- **`rename keeps every backlink resolving`.** Edges key on `b2id`; path is a repairable convenience copy.
  A move rewrites path *text* in inbound files and zero edge rows.

These are the same tripwires [index-engine.md](index-engine.md) §8 calls out; this doc is where they're
defined, that doc is where they're enforced in the store.

---

## 7. Rejected / deferred alternatives

- **B2 authoring the body — rejected (Decision 1, 2026-06-30).** The body is the rendered/exported
  document and must stay 100% the human's; B2 injecting a `## Relations` section (or any prose) would
  corrupt it (imagine a `resume.md`). So B2's accepted edges go to **frontmatter `relations:`** instead
  (§0, §2). The *only* body write B2 makes is the mechanical move-repair of an inbound wikilink's path —
  fixing a link the human already wrote, never adding one. **This reverses the earlier "accepted edges go
  inline in the body" decision.**
- **Inline-in-body as the home for accepted edges — superseded.** The trade: a frontmatter edge is *not*
  guaranteed clickable in vanilla Obsidian's reading view. Accepted because the body-pristine guarantee
  outweighs it — human body links stay clickable, and Obsidian can't render edge *types* regardless (§0).
- **Inline suggestions in the body — rejected.** Would render as real connections and pollute the vault
  before acceptance, violating "inert until accepted." (Suggestions stay in the review layer entirely.)
- **Stored reciprocal links — rejected.** Inverse edges are derived at query time; writing them back
  amplifies writes and edits notes the user didn't touch.
- **Per-edge ULIDs in the file — rejected.** Authored edge identity is derived from
  (`src`,`dst`,`type`,occurrence); explicit ids would clutter the note for no gain.
- **Edge provenance in Markdown (HTML comment or frontmatter field) — rejected** in favor of the event
  log (§4). The accepted *connection* lives in frontmatter `relations:`, but its *provenance* (who
  proposed it, confidence, when) does not — that is history, and history belongs in the in-vault `.b2/`
  log. This keeps accepted edges pristine and the index honestly disposable.

---

## 8. A golden-vault sketch (for the test harness)

The smallest fixture that exercises the whole model — an authored typed edge, a bare reference, and one
inert suggestion. (Ties to the testability stack, [vision-and-scope.md](vision-and-scope.md).)

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
never writes there. Had the agent's `contradicts` suggestion been **accepted**, B2 would append it to
spaced-repetition's frontmatter `relations:` (`origin=frontmatter`) — never to this body section (§0).

Derived graph (no live model needed to assert):

- `references`: spaced-repetition → memory (origin=inline, status=active) — from the prose wikilink.
- `elaborates`: spaced-repetition → memory (origin=inline, status=active, explanation="applies…").
- *Suggested (review layer — index queue + `.b2/` log, not in any note):* `contradicts`:
  spaced-repetition → `notes/cramming-works` (origin=suggested, status=suggested, by=agent:…,
  confidence=0.82) — **present in `b2 suggest`, absent from every note on disk** until accepted.

`b2 neighbors concepts/memory` returns spaced-repetition twice (referenced-by, elaborated-by); both
files round-trip byte-identical; dropping and rebuilding the index reproduces the identical graph.

---

## 9. Judgment calls — resolved (2026-06-29; §0 revised 2026-06-30)

- **Where a connection lives (Decision 1–3, 2026-06-30)** — **B2 never authors the body.** Human
  connections live in the body (B2 reads, never writes); **B2-accepted edges live in frontmatter
  `relations:`** as typed-link strings (Format A — the same `<verb> [[path|title]] — …` syntax as a body
  line); pending/rejected suggestions live in the review layer. The graph is the **union** of the three
  homes, deduped **inline-wins** on overlap, never auto-editing the file. The body write B2 makes is the
  move-repair only. This **reverses** the earlier "accepted edges go inline in the body" call (§0, §2,
  §7).
- **Edge-provenance durability** — accepted edges stay **pristine** (in frontmatter `relations:`);
  provenance lives in the index while a suggestion is pending and in the in-vault `.b2/` event log as
  history thereafter (§4). The three-tier model (Markdown / disposable index / durable log) and
  `index = projection of (Markdown ∪ log)` fall out of this.
- **`b2id` backfill on ingest** — identity is **namespaced to `b2id`**, and stamping a missing one is
  **B2's single always-allowed edit** to the vault, done as needed on first sight (no `b2 init` gate, no
  refusing to index), logged as `b2id.stamped` (§1).
- **Bare-wikilink default type** — a plain `[[path|title]]` is a **directed `references`** edge (§2): the
  minimal literally-true reading of "A's text points at B," strictly more expressive than symmetric
  `relates` (the symmetric view derives from directed, not the reverse), it preserves the backlink
  signal, and it keeps `relates`/`contradicts` meaningful as explicit symmetric verbs.
- **Relation vocabulary** — a **10-verb core** across 5 orthogonal categories (referential, expository,
  evidential, structural, versioning), closed for discovery + queries + eval; a **tolerated tail** stored
  verbatim; a **promotion path**; plus the conventions and "most-specific-then-`relates`" classification
  rule (§2).

**Still open:** none — the data model is locked. Next is the **index-engine build** against golden-vault
fixtures ([index-engine.md](index-engine.md), now reconciled with this three-tier / event-log model).

> Next ([tasks.md](tasks.md)): this model is the yardstick for the **index-engine evaluation** — whose
> recommendation ([index-engine.md](index-engine.md)) already targets this exact note/edge/provenance
> shape. With the data model fixed, the engine can be built against golden-vault fixtures.
