---
b2id: 01KX1TDVWHCXBM4XCN1GEWBXAB
title: "B2 — Beyond Markdown: file-type support (resources)"
type: note
tags: [b2, resources, file-types, images, pdf, html, rendering, ingestion, research]
created: 2026-07-08
status: draft
---

# B2 — Beyond Markdown: file-type support (resources)

> **Findings + design for supporting non-`.md` files in the vault.** Centralizes the analysis
> scattered across the shipped specs and issues (§1), derives the model from the two tenets
> ([vision-and-scope.md](../vision-and-scope.md#design-philosophy)), and specifies one polymorphic
> approach covering ingestion (§3–§5), the graph (§4), and rendering with a "no viewer available"
> fallback (§6). The bar: **any file type GitHub could store** — the taxonomy must be *total*, with
> graceful degradation, never a refusal. The §9 judgment calls were **resolved 2026-07-08**; the
> design below is written in its locked form.

## Rollout — propagation & build (todo)

The design is **locked** (§9); what remains is propagating it into the canonical docs, then the code
slices (§8). Tracked here so state survives across sessions:

- [x] **Stage A — mirror the locked model into the canonical docs** (2026-07-12) — the `resource`
      object + widened invariant → [data-model.md](../data-model.md) §10; the `resources` table + the
      per-class extraction step → [index-engine.md](../index-engine.md) §3; the locked decisions →
      [vision-and-scope.md](../vision-and-scope.md) "Decisions locked (2026-07-08)"; the working-queue
      pointer → [tasks.md](../tasks.md).
- [ ] **Stage B — slice-1 build spec** under `specs/` — inventory & graph (§8 slice 1): walk all files
      → `resources` table, classify by extension, parse `![alt](…)` / `[…](…)` / `![[…]]` → resource
      edges, file tree + fallback card, `b2 explain` / `b2 mv` over resources. **Model-free, no new deps.**
- [ ] **Slices 2–4** — render mechanisms · searchable resources · PDF text (§8); spec each when reached.
- [ ] **Slice 5** — semantic seams (Describer, multimodal embedder), future/unscheduled (§8, §5).

## TL;DR / recommendation

**Markdown is the vault's only *authoring surface*; every other file is a *resource* — a
first-class vault member in its own right: indexed, searchable, linkable, renderable, and never
required to be referenced by any note.**

- **The two-tier model is unchanged; the source tier widens.** The invariant generalizes from
  `index = projection of (the .md files)` to **`index = projection of (the vault directory)`** —
  resources contribute only *derived* index rows (metadata, extracted text, inbound edges), never
  durable state. Drop `b2.sqlite`, reindex, get it back identical. No new tier, no sidecar files.
- **Polymorphism = a closed class table with a total fallback**, not a trait hierarchy. Every file
  maps by extension to one of six classes — `note`, `text`, `html`, `pdf`, `image`, `media` — with
  **`binary`** as the catch-all. Each class answers the same three questions: *what text does it
  yield for the index?* (§5), *can it be a graph endpoint?* (§4), *how does it render?* (§6).
- **Resources are path-keyed peers with one asymmetry: authoring.** No `b2id` (nothing to stamp a
  PNG with) and no outbound edges in v1 — not because resources are subordinate, but because B2
  can only read and write *authored structure* in Markdown, and a PNG has no home for it. A
  resource needs no note to exist, be indexed, be found, or be opened. Where edges touch a
  resource they originate in notes for now (`![[photo.png]]`, `[[papers/x.pdf]]`, `b2 link`),
  with named relief valves if resource-sourced edges ever prove needed (§4).
- **One embedding space in v1.** Every class funnels to *text* (native, extracted, or — for images —
  aggregated alt-text/captions from the notes that embed them), embedded in the existing bge space.
  Multimodal image embedding is a **documented future seam** (a second `vec0` table under the same
  `meta` discipline), not a v1 build — the Bitter-Lesson posture that cut the relator (§7).
- **Rendering = a viewer registry keyed by class, with a fallback card.** Selecting any file in the
  tree opens *something*: the note pane for `.md`, an `<img>`/`<audio>`/`<video>`/PDF/text viewer per
  class (§6), and for everything else a **"No viewer available"** card showing metadata + backlinks +
  *Open in system default*. The one infrastructure key is the **Tauri asset protocol** (scoped,
  read-only) — which also unblocks inline images in the reading view, the gap
  [desktop-live-preview.md](../specs/completed/desktop-live-preview.md) §8 already named.

---

## 1. Where we are today — the scattered analysis, centralized

No design doc covers non-`.md` files; what exists is fragments in shipped specs, issues, and code
behavior. Collected:

**Code reality (what the engine does now):**

- **The walk is `.md`-only.** `ingest.rs` filters `extension == "md"`
  (`crates/b2-core/src/ingest.rs:712`); every other file is invisible — not in `notes`, not in the
  file tree (`list_notes` reads the DB), not searchable, not a graph endpoint.
- **Only wikilinks are parsed.** `link.rs` reads bare `[[path|alias]]` and typed
  `- <verb> [[..]]` lines. Markdown's own `![alt](path)` / `[text](path)` syntax is not parsed at
  all; an Obsidian-style `![[img.png]]` embed is scanned as a bare wikilink.
- **Non-note targets become dangling edges.** `resolve_link_target` tries the exact path then
  `path + ".md"` (`db.rs:623`); a `[[photo.png]]` target resolves to nothing, so the edge row keeps
  `dst_id = NULL` with `dst_path_raw` retained (there is already a partial index on dangling edges,
  `db.rs:165`). Downstream surfaces drop these — the same UX hole as folder wikilinks
  ([#12](https://github.com/AlteredCraft/B2/issues/12)).
- **The desktop can't render a vault image even inside a note.** The webview CSP is locked down with
  all assets bundled ([desktop-ui-mvp.md](../specs/completed/desktop-ui-mvp.md) §security), and no
  asset protocol is configured, so a `<img src="relative/path">` emitted by `marked` loads nothing.

**Prior analysis (where the feature already surfaced):**

- [desktop-live-preview.md](../specs/completed/desktop-live-preview.md) §8 defers images as
  "block-widget territory (StateField, **asset-protocol work for vault-relative images — the reading
  view shares that gap**). Each its own slice." — the closest thing to a plan on record.
- [vision-and-scope.md](../vision-and-scope.md) chose Tauri *because* "a terminal grid can't render
  long-form Markdown, **images**, or clickable links" — rendering non-text was a stated reason for
  the delivery vehicle, then never specced.
- [index-engine.md](../index-engine.md) §1 notes qmd's "optional tree-sitter AST chunking for **code
  files**" as borrowable — code-as-content was on the radar from the start (ties to
  [#19](https://github.com/AlteredCraft/B2/issues/19), the chunker upgrade).
- [research/vector-store-alternatives.md](vector-store-alternatives.md) records that LanceDB's
  *multimodal* store didn't justify leaving SQLite — i.e. multimodal embeddings are anticipated, and
  the store decision already survived that contact.
- Adjacent issues: [#12](https://github.com/AlteredCraft/B2/issues/12) (non-note wikilink targets
  silently dropped), [#31](https://github.com/AlteredCraft/B2/issues/31) (deleted files leave ghost
  rows — *more* pressing once resources join the walk, since they churn more than notes),
  [#19](https://github.com/AlteredCraft/B2/issues/19) (chunking per content shape).

**Net:** the vault already *contains* these files (any real Obsidian-style vault does); B2 currently
pretends it doesn't. The feature was implicitly promised (Tauri rationale, live-preview deferral) but
never designed. This doc is that design.

---

## 2. First principles — what the tenets already decide

Both tenets bear directly, and they do most of the deciding:

- **Volatile vault over a disposable index** ⇒ resources must add **no durable state**. No sidecar
  metadata files, no `.b2/` records, no identity stamps outside Markdown. Whatever B2 knows about a
  PNG must be re-derivable from (a) the PNG's bytes + path and (b) the Markdown that links to it.
  This single constraint eliminates most of the design space (§7).
- **Build for tomorrow's model** ⇒ image/PDF *semantics* (captioning, OCR, multimodal embedding) sit
  behind seams and default off. The relator died because per-item model calls didn't scale and
  compensated for today's models; an LLM captioner is the same shape. What we build now is the
  **model-free projection** — extraction, linking, rendering — leaving the semantic seams ready.
- **The body is the human's; B2 authors nothing** ⇒ resources, which have no frontmatter and no
  body B2 could safely write, simply get **no writes at all**. B2 never touches a non-`.md` file's
  bytes (`b2 mv` moves them; that is the one operation, and it is path-only).

One principled asymmetry follows, and it is the heart of the model — an asymmetry of **authoring
surface, not of status** (locked as the §9 addendum):

> **A note is where structure is *authored*; a resource is a peer document B2 cannot write.**
> Notes have frontmatter, authored edges, durable identity, and B2's write guarantees — because
> Markdown is the one format whose bytes B2 may touch and whose links humans write in prose.
> Resources have bytes, a path, derivable text and vectors, and inbound links.

What the asymmetry does **not** mean — a resource is never required to be attached to a note:

- An unlinked resource fully exists: walked, classified, in the tree, in the index, openable.
- `text`/`html`/`pdf` resources are **semantically self-sufficient** — their own content is
  chunked, keyword-searchable, and embedded with no note involved. A vault of *only* PDFs is a
  searchable, `b2 similar`-navigable vault.
- Rendering never depends on inbound links; every class opens standalone (§6).
- The classes with no extractable text yet (`image`/`media`/`binary`) index thin *today*
  (filename + any inbound captions, §5) — a v1 data-availability stopgap that the Describer seam
  erases by giving them **intrinsic** derived text. It is not a principle that a resource's
  meaning comes from notes.

This is why "support any file type" does **not** mean "generalize the note." It means adding a
second kind of vault member with a different *write* contract, not a lesser one.

---

## 3. The taxonomy — total over "anything GitHub stores"

Class is determined by **extension only** (deterministic, no content sniffing; a mislabeled file
degrades gracefully rather than mis-executing — §6 security). The table is closed; `binary` is the
total fallback, so *every* file classifies.

| Class | Extensions (v1 set) | Index text (§5) | Viewer (§6) |
|---|---|---|---|
| `note` | `.md` | native (chunks, as today) | note pane (render/edit — as today) |
| `text` | `.txt` `.csv` `.json` `.yaml` `.toml` `.log`, code (`.rs` `.py` `.ts` `.js` `.sh` …) | raw content | read-only text pane |
| `html` | `.html` `.htm` | tag-stripped text | source view; sandboxed preview later (§6, §9) |
| `pdf` | `.pdf` | text layer (slice 4) | PDF viewer (§6) |
| `image` | `.png` `.jpg` `.jpeg` `.gif` `.webp` `.svg` `.avif` | filename + inbound alt/captions | `<img>` |
| `media` | `.mp3` `.wav` `.mp4` `.mov` `.webm` | filename | native `<audio>`/`<video>` |
| `binary` | **everything else** | filename | fallback card ("no viewer available") |

Guards, mirroring the reindex-robustness posture ([index-engine.md](../index-engine.md) §8):

- **Size cap on extraction** (e.g. skip text extraction above ~10 MB; the metadata row is always
  written). GitHub's own 100 MB blob limit is the outer bar; B2 indexes *about* a big file without
  reading all of it.
- **Degrade, never abort.** A `text`-classed file that isn't valid UTF-8, or a PDF whose text layer
  fails to parse, degrades to `binary` treatment (metadata row, no chunks) — the resource analogue
  of `SkippedNote`, except the file still *exists* in the index and tree.
- Dotfolders stay skipped, as today. (A `.b2ignore` is a separate, deferred idea.)

---

## 4. Identity & the graph — path-keyed, dst-only

**Identity: a resource is keyed by its vault-relative path, in the index only** (locked, §9 #2). No
`b2id` — there is nowhere to stamp one (binary bytes are not B2's to edit; sidecars violate §2), and
nothing it would protect: identity-under-rename matters for notes because *authored knowledge* keys
off `b2id`; a resource's inbound links are plain path text that B2 can rewrite mechanically.

Consequences, stated honestly against the locked invariants:

- **`b2 mv` on a resource works exactly like a note move** minus the identity step: rewrite
  inbound `[[path]]`/`![alt](path)` text (the edges name the N inbound files), move the file,
  re-project. "Rename keeps every backlink resolving" holds *when B2 does the move*.
- **Out-of-band moves degrade one notch further than notes.** A Finder-moved note re-binds by its
  stamped `b2id`; a Finder-moved resource can't. Mitigation (locked in with §9 #2; cheap): the index
  stores a **blake3 content hash** per resource; on reindex, a dangling resource link whose old
  target vanished and whose hash reappears at **exactly one** new path is flagged as a proposed
  repair (surfaced like other repairables — flagged, never silently rewritten). Duplicate files make
  the match ambiguous → flag only.
- **Edges: `src` is a note in v1; `dst` may be anything.** A *consequence*, not a status rule:
  every edge must trace to an authored line in Markdown (the invariant), and a resource has no
  writable home for one — no frontmatter, no body B2 may touch. Two relief valves keep this from
  hardening into an expressiveness wall: **(a) today**, the tolerated-tail vocabulary already
  authors the inverse direction from the note side (`- "supported-by [[papers/x.pdf]]"` in
  `relations:` — stored verbatim, displayed, queryable as a tail verb); **(b) if needed**,
  resource-sourced and resource↔resource edges get a designed future home — a **vault-level
  B2-managed relations file**, the frontmatter managed-zone concept lifted to one clearly-B2-owned
  Markdown file, so the edge is still authored Markdown and the invariant holds (§7, deferred). A
  companion note remains an available pattern for rich annotation, never a requirement.
- **Schema shape:** a new `resources` table (path PK, class, size, mtime, content_hash, indexed_at)
  — **not** a generalization of `notes` (locked, §9 #3). `edges.dst_id` stays a note `b2id` or
  NULL; resource resolution records `dst_resource_path` (resolved against `resources`) so a
  dangling path and a resolved resource are distinguishable. The existing `dst_path_raw` +
  dangling-index machinery is already half of this.
- **Parser work:** `link.rs` learns the two Markdown-native forms — `![alt](path)` / `[text](path)`
  (relative paths only; `http(s)://` targets are not vault members and stay unparsed) and the
  `![[file.ext]]` embed. All produce `references` edges (the closed 10-verb core is untouched);
  the **alt/caption text is captured on the edge** — it becomes the image's index text (§5). An
  embed is recorded as `references` with an `embed` marker (display nicety, not a new verb).

The `notes`/`resources` split keeps every existing invariant statement true *verbatim* for notes,
and states resources' different contract — path identity, flagged (not guaranteed) out-of-band
move repair — explicitly rather than as exceptions to the note rules.

---

## 5. Index projection — every class funnels to text, one embedding space

The projection pass gains a per-class **extraction step**; everything downstream (chunks, FTS,
vectors, search) is unchanged plumbing:

```
walk vault → classify (extension) → per class:
  note   → frontmatter + body        → chunks → FTS + vectors     (today, unchanged)
  text   → raw content               → chunks → FTS + vectors
  html   → strip tags → text         → chunks → FTS + vectors
  pdf    → text layer → text         → chunks → FTS + vectors     (slice 4; dep decision deferred there, §9 #5)
  image  → filename + Σ inbound alt/caption text → one chunk → FTS + vector when nonempty (§9 #6)
  media  → filename                  → FTS row only
  binary → filename                  → FTS row only
```

- **Chunks generalize from `note_b2id` to a document reference** (note b2id *or* resource path).
  Search resolves hits up to the owning document; results now carry a `kind`. Since the index is
  disposable, this is a `schema_version` bump + rebuild — **no migration code, ever**. That is the
  disposable-index tenet paying rent.
- **One embedding space.** Extracted text embeds through the existing bge space — it *is* text, so
  the `meta (embed_model_id, embed_dim)` discipline is untouched. Nothing multimodal in v1.
- **The image trick — model-free semantics from authored context.** An image's index text =
  filename tokens + the aggregated alt-text/captions from every note that embeds it. It is a pure
  projection of Markdown (the alt text is authored!), costs nothing, and makes
  `b2 search "sailboat"` find `IMG_2041.jpg` the moment any note captions it. This is the honest v1
  stand-in for image understanding — a stopgap, not a doctrine: the Describer seam below is what
  gives an image *intrinsic* index text, with no note involved (§2, §9 addendum). Nonempty caption text is **embedded too** (locked, §9 #6) — it
  is ordinary authored text, so it flows through the bge space with zero new discipline, and it is
  what lets `b2 similar` surface a captioned image next to related notes.
- **Two seams documented for tomorrow's model, built never-earlier-than-needed:**
  1. **Describer** (`file bytes → text`): an OCR pass, an LLM captioner, a PDF-figure summarizer —
     each is "better extraction," slotting into the extraction step per class. One-time per file,
     unlike the per-pair relator — but still deferred, default-off. Its output is the one derivation
     *too expensive to re-derive on every rebuild*, so it persists in a **content-addressed cache
     outside the vault** (`blake3(bytes) → text`, in the shared XDG dir like the model files):
     durable across index drops *and* file renames, never a vault write (see the ghost-md
     rejection, §7). Deterministic extraction (PDF text layer, HTML strip) is cheap enough to redo
     at reindex and needs no cache.
  2. **Multimodal embedder** (`image → vector`): a *second* vector space — a separate `vec0` table
     with its own `(model_id, dim)` in `meta`, because CLIP-style image vectors are not comparable
     to bge text vectors. Same fail-fast-on-mismatch discipline. `b2 similar` over images joins the
     party only when this lands.
- **Payoff already at v1:** `b2 similar papers/attention.pdf` works as soon as PDFs have text
  chunks — nearest *unlinked notes* to a paper is exactly the discovery loop, extended to material
  you didn't write. Search, `b2 explain <file>` (backlinks), and the graph all get resources "for
  free" from the same plumbing.

Extraction is deterministic and model-free, so it lives in the fast suite with fixtures — no change
to the testability stack. (The PDF-parsing *dependency* — which crate, and whether it lives in
`b2-core` or a new crate — is **deferred to slice 4** by design; §9 #5.)

---

## 6. Rendering — a viewer registry with a total fallback

The user-facing contract: **selecting any file in the tree opens something.** The note pane is
today's special case of a general dispatch:

| Class | Viewer | Notes |
|---|---|---|
| `note` | note pane (render ⇄ edit ⇄ source) | unchanged |
| `image` | `<img>` centered, zoom-to-fit | via asset protocol |
| `media` | native `<audio>` / `<video>` element | the webview does the work |
| `text` | read-only text pane (`<pre>` v1; CM6 read-only + syntax highlight later) | |
| `pdf` | v1: fallback card + *Open in system default*; v2: bundled pdf.js | |
| `html` | v1: rendered **source** (like view-source) + *Open in browser*; sandboxed `<iframe sandbox>` preview later | locked, §9 #4; security below |
| `binary` | **fallback card** | the total catch-all |

**The fallback card — "No viewer available".** Filename, class, size, modified date, content hash —
and the **backlinks panel**: which notes reference this file, with their link context. Even a file
B2 can't render is a full citizen — the card stands on the resource's own metadata, and backlinks
appear when they exist: context, never a requirement. Plus one action: *Open in system default*
(Tauri opener plugin — an OS handoff, never in-webview execution).

**Infrastructure key: the Tauri asset protocol**, scoped to the vault root, read-only. This is the
one CSP change ([desktop-ui-mvp.md](../specs/completed/desktop-ui-mvp.md) locked it down; the scope
widens deliberately and minimally). It serves three consumers at once:

1. the standalone image/media viewers above;
2. **inline images in the note reading view** — `![alt](path)` in `marked` output finally loads
   (the gap live-preview §8 named); the "render them inline" option the vision implies;
3. (later) live-preview image block widgets in the editor — same URLs, CM6 decoration work only.

**Security posture** (vault files are *untrusted input* to a privileged webview):

- Images/media go through `<img>`/`<audio>`/`<video>` only — no script execution path (SVG in an
  `<img>` does not run scripts; never inline vault SVG into the DOM).
- HTML is the dangerous class: rendering it live in the app webview would execute foreign script
  next to the IPC bridge. Hence source-first in v1 (locked, §9 #4); any later preview is
  `<iframe sandbox>` with scripts off. *Open in browser* is the pressure valve.
- The asset protocol scope is the vault root, read-only, nothing else; `Open in system default`
  delegates to the OS rather than interpreting bytes ourselves.

**CLI parity:** the CLI needs no viewer, but gains honesty: `b2 explain <file>` (backlinks +
metadata), resources in `b2 search` results (typed), and `b2 mv` covering resources. The façade
grows only what these commands need, per its charter.

---

## 7. Rejected / deferred alternatives

- **Stamping identity into non-md files (EXIF/XMP, PDF metadata, xattrs) — rejected.** B2 never
  writes a byte it doesn't own; xattrs don't survive git; per-format metadata writers are a
  compatibility tarpit. Resources are path-keyed (§4).
- **Sidecar metadata files (`photo.png.md`, `.b2/resources.yaml`) — rejected as machinery.** A
  *user-authored companion note* is idiomatic and already works; B2 *generating* sidecars would
  pollute the vault with files the human didn't write — the same instinct that keeps B2 out of the
  body. (A human is free to adopt a sidecar convention; B2 needs no special support for it.)
- **"Ghost mds" — machine-generated hidden `.md` twins holding each resource's extracted text /
  LLM description, so "everything is a Markdown file" — rejected (considered 2026-07-08).** The
  uniformity it promises already exists one step later: every class funnels to *text* at the chunk
  boundary (§5), so the pipeline is uniform without a serialize-to-vault round trip — the ghost
  only changes *where derived text lives*, and it moves it into the authored tier. Costs: the
  invariant goes circular (part of the "Markdown" becomes a projection of other files); staleness
  becomes a real two-files-on-disk sync problem — the exact class of bug the disposable index
  exists to make structurally impossible; the vault/git/Obsidian surface fills with files the human
  didn't write; and ghosts aren't real notes anyway (not link targets, not search results in their
  own right, no b2id), so every downstream surface would grow "unless it's a ghost" clauses — the
  asymmetry the `resources` table isolates instead. The two real needs inside the idea are met
  elsewhere: expensive model-derived text persists in the content-addressed XDG cache (§5,
  Describer), and human-editable knowledge about a file is a companion note. The ghost is the
  midpoint between those two that inherits the drawbacks of both.
- **Generalizing `notes` to hold resources (a `kind` column) — rejected (locked, §9 #3).** It
  blurs the one asymmetry the model runs on (§2): every invariant, write guarantee, and frontmatter
  behavior would need a "unless it's a resource" clause. Two tables, two contracts, zero clauses.
- **Resources as edge sources — deferred, with the future home already designed (§4).** Not a
  status rule — an edge must be authored in Markdown, and a resource has no writable home for one.
  If the need materializes (resource-sourced or resource↔resource relations, or projecting
  HTML/PDF *internal* links), the home is a **vault-level B2-managed relations file**: the
  managed-zone concept at vault scope, still authored Markdown, still `index = projection of (the
  vault directory)`. Until then the tolerated tail authors inverse directions from the note side.
- **Content sniffing for classification — rejected.** Extension-only is deterministic and cheap;
  misclassification degrades safely (§3, §6). Sniffing buys ambiguity and a security surface.
- **LLM captioning / OCR / multimodal embedding in v1 — deferred behind the §5 seams.** The
  Bitter-Lesson call: don't build model-compensating machinery now; leave the seams the way the
  Embedder seam was left for the relator's successor.
- **External URL targets (`[text](https://…)`) as vault members — out of scope.** The vault is a
  directory; the web is not. (A future "web clipper saves a file into the vault" lands as a plain
  resource with zero new model.)

---

## 8. Build plan — slices in value order

Each slice is independently shippable and dogfoodable; later slices never rework earlier ones.

1. **Inventory & graph (model-free, no new deps).** Walk all files → `resources` table (+ pruning,
   which also bounds [#31](https://github.com/AlteredCraft/B2/issues/31)'s blast radius); classify;
   parse `![alt](…)`, `[…](…)`, `![[…]]` → resource-resolved edges with captured alt text; file
   tree shows resources; **fallback card** with backlinks + *Open in system default*; `b2 explain`
   / `b2 mv` cover resources. *The vault stops lying about its own contents.*
2. **Render mechanisms.** Asset protocol (scoped, read-only) → image + media viewers, **inline
   images in the note reading view**; read-only text viewer; HTML source view + *Open in browser*.
   *Selecting anything shows something.*
3. **Searchable resources (still model-free).** Extraction for `text`/`html`; chunks → FTS +
   vectors through the existing space; image alt-text aggregation; typed search results in CLI +
   desktop. *`b2 search` and `b2 similar` see the whole vault.*
4. **PDF text.** The extraction-dependency decision (§9 #5) lands here, not before; then PDFs join
   slice 3's pipeline. Optionally the pdf.js viewer upgrade.
5. **Semantic seams (future, unscheduled).** Describer (OCR/captioning), multimodal embedder as a
   second vector space, live-preview inline-image widgets, sandboxed HTML preview.

Testing follows the house pattern: golden-vault fixtures gain a `resources/` folder (an image, a
snippet of HTML, a tiny text file, a binary blob); every slice asserts against the fake embedder;
extraction fixtures are deterministic. `full-reindex ≡ incremental-update` property extends over
resource add/change/delete.

---

## 9. Judgment calls — resolved (2026-07-08)

1. **The name = `resource`** — chosen over the recommended "attachment" (Obsidian's term), and over
   "asset"/"file". The neutral noun: a source paper or dataset is material in its own right, not an
   appendage — the model's real asymmetry is *authoring surface*, not rank (see the addendum
   below, which this choice foreshadowed). Schema: the `resources` table.
2. **Identity = path-keyed, with the hash-assist repair flag** (§4). Resources are keyed by
   vault-relative path, index-only; no `b2id`, no sidecars. `b2 mv` repairs inbound links fully;
   an out-of-band move gets a blake3-hash-matched repair *proposal* (unique match only — flagged,
   never silent). The three locked invariants stay note-scoped and true verbatim.
3. **Schema = a separate `resources` table**, not a generalized `notes` (§4, §7). Two tables, two
   contracts, zero "unless it's a resource" clauses.
4. **HTML = source-first in v1** (§6): highlighted source + *Open in browser* (the OS sandbox is
   the right home for foreign HTML); an in-app `<iframe sandbox>` preview (scripts off) is a later
   slice. Zero script-execution surface ships in v1.
5. **PDF extraction dependency = explicitly deferred to slice 4** — a scheduled decision, not an
   open thread. PDFs are full inventory/graph/viewer citizens from slice 1; only their text waits.
   Pick the crate and its home (`b2-core` vs a new extraction crate) against the workspace as it
   exists when that slice starts; no earlier architecture depends on the answer.
6. **Image index text is embedded when nonempty**, not FTS-only (§5). It is ordinary authored
   text; it flows through the existing bge space with zero new discipline, and it is what lets
   `b2 similar` surface a captioned image next to related notes.

**Addendum (2026-07-08, same session) — no subordination mandate.** Reviewing the locked set
surfaced one framing to strike: nothing in the model may *require* a resource to be attached or
subordinate to a note. Locked: resources are **peer vault members**; the only asymmetry is the
**authoring surface** (Markdown is the one place B2 reads and writes authored structure), and each
consequence of it carries a named relief valve — an unlinked resource is fully indexed, and the
text-bearing classes are semantically self-sufficient (§2); thin `image`/`media`/`binary` indexing
is a data-availability stopgap the Describer erases with intrinsic text (§5); `src`-is-a-note is
v1 mechanics, with tail-verb inverse authoring today and the vault-level B2-managed relations file
as the designed future home for resource-sourced edges (§4, §7). None of decisions 1–6 change.

**Still open: none — the design is locked.** Next: mirror into the canonical docs —
[data-model.md](../data-model.md) gains the resource object + the widened projection statement,
[index-engine.md](../index-engine.md) §3 gains the `resources` table + extraction step,
[vision-and-scope.md](../vision-and-scope.md) records the locked decisions — and slice 1 gets a
build spec under `specs/`.
