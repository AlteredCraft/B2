---
title: "B2 — Invariants"
type: note
tags: [b2, invariants, architecture, canonical]
created: 2026-07-22
status: active
---

# B2 — Invariants

> The normative register of what must always be true of B2. Each entry is one testable/reviewable
> claim; the linked doc holds the elaboration and rationale. This page is the top of the design set —
> the *why* — with the *what* in [data-model.md](data-model.md) and the *how* in
> [index-engine.md](index-engine.md); product non-negotiables (local-first, zero lock-in,
> single-binary) are captured as invariants here.
>
> **On conflict, this page wins and the other doc gets fixed.** Changing this page is a deliberate
> decision, never a drive-by edit. Cite entries by id (S2, G2, …).

The register is the two design tenets — *a volatile vault over a disposable index* and *build for
tomorrow's model* — made mechanical.

## S — Storage: two tiers, one projection

- **S1 — Two tiers, sharply split.** The vault (Markdown + resources + the directory tree) is the
  source of truth; `.b2/b2.sqlite` is a disposable cache. Nothing in the index is authoritative.
  ([data-model.md](data-model.md) "Two storage tiers")
- **S2 — The index is a pure projection: `index = projection of (the vault directory)`.** Drop
  `b2.sqlite`, reindex, get an identical index. **Markdown is the vault's sole authored subset** —
  the only format whose bytes B2 may write; resources contribute derived rows only, and folders are
  never projected at all (read live off disk). The projected *domain* is the vault's **managed
  subtree**: a dot-prefixed name is not vault material of any kind — folder, resource, or `.md` alike
  — so it is skipped by every walk before routing, and refused as an authoring destination (B2 never
  creates a member it would then never see). The files stay on disk untouched; they are simply outside
  the projection. ([data-model.md](data-model.md) §1 "Hidden means hidden", §10,
  [index-engine.md](index-engine.md) §3, [GH #136](https://github.com/AlteredCraft/B2/issues/136))
- **S3 — `full-reindex ≡ incremental-update`, unconditionally.** Re-deriving one changed note
  converges on exactly the state a from-scratch rebuild would produce — including pruning rows for
  deleted files on a whole-vault pass. There is no carve-out: with identity **path-keyed** (L1), the
  filesystem itself guarantees one member per path, so the "two files presenting one identity" state
  that once needed one (a Finder-duplicated note, GH #81) cannot arise — a copy is simply another
  note at another path. ([index-engine.md](index-engine.md) §8, [GH #170](https://github.com/AlteredCraft/B2/issues/170))
- **S4 — No durable B2-derived state outside the Markdown.** No event log, no sidecar files, no
  index-only authored facts. Scope: *B2-derived* data — the human's own directory tree is vault
  material, for which the **filesystem is authoritative** (folders are never projected; the tree
  listing is a live fs walk). ([data-model.md](data-model.md) "Folders")
- **S5 — Schema change = version bump + rebuild, never a data migration.** Disposability is what
  makes this free; a migration script would be evidence S2 broke.
  ([index-engine.md](index-engine.md) §3)

## W — Write discipline: the vault changes only on your command

- **W1 — B2 makes no unbidden writes. Period.** Every byte B2 writes to the vault is the mechanics of
  an operation the human explicitly invoked (W3). Reading a vault — walking it, projecting it,
  reindexing it — writes **nothing**, so `reindex` runs unchanged on a read-only vault and a
  git-versioned vault shows no diff from having been indexed. This is the claim the `b2id` stamp
  used to hold an asterisk over; removing the stamp (GH #170) is what let the asterisk go.
  ([data-model.md](data-model.md) §1)
- **W2 — B2 never authors the body, and never asks it to carry B2 syntax.** The body is 100% the
  human's document. The lone body write is the mechanical move-repair: rewriting an inbound
  `[[oldpath|alias]]`'s *path text* when its target moves — fixing a link the human already wrote,
  never adding one, aliases preserved verbatim. ([data-model.md](data-model.md) §0)
- **W3 — The on-command writes are enumerated and minimal:** append one `b2_relations:` entry on
  `b2 link` (frontmatter, never the body); the move-repair of W2; the editor save (`Vault::write` — a
  byte-honest splice of the *human's own* body bytes, guarded by a content-hash revision); the
  frontmatter save (`Vault::write_frontmatter` — the same-guard splice of the *human's own*
  frontmatter bytes, body untouched, and otherwise unjudged: B2 owns no line in that block);
  and create/move/delete of notes, resources, and folders on explicit command.
- **W4 — B2 never deletes, moves, or archives vault files of its own accord.** Consequences of human
  edits (orphans, dangling links, hash-matched move candidates) are *surfaced*, flagged, or proposed —
  never silently applied. ([index-engine.md](index-engine.md) §8)
- **W5 — Round-trip losslessness.** `parse → serialize → parse` is byte-identical outside the specific
  edit performed; unknown frontmatter keys survive verbatim, in order. B2's one key is namespaced
  (`b2_relations`) so it can never collide; a generic `relations:` key is *not* read. A `b2id:` line
  left by an older B2 is now exactly an unknown key — never read, never rewritten, never removed;
  nothing needs migrating, and deleting `.b2/` is the whole upgrade (GH #170).
  ([data-model.md](data-model.md) §6, §1)

## L — Identity & links

- **L1 — A note's identity is its vault-relative path, and the graph keys every edge by path**
  (GH #170). Both link homes are *already* written by path — a body `[[path]]`, a frontmatter
  `b2_relations:` entry — so an edge stores what the human authored, resolved at projection time,
  with no machine id in the file and nothing to key on that the vault does not already carry.
  Consequence: **rename keeps every backlink resolving *when B2 does the move*** — a move rewrites
  the inbound path *text* and re-keys the moved note's rows in one transaction. A move made
  **outside** B2 is a delete plus a create: the inbound links surface as dangling (G5) — identified,
  never silently dropped — which is exactly the durability a path handle has in Obsidian, and one
  notch better in that B2 says so. ([data-model.md](data-model.md) §1, §3)
- **L2 — A note's title is its filename.** The frontmatter `title:` key is recognized but inert —
  round-tripped, never driving display, aliases, or search. `b2 link` therefore writes a bare
  `[[path]]`, no alias. ([data-model.md](data-model.md) §1, §9)
- **L3 — Notes and resources share one identity model: the vault-relative path, index-only, with no
  sidecar files ever.** Since GH #170 the remaining asymmetry is **authoring surface alone**, not
  status and no longer identity: a note has frontmatter and authored edges because Markdown is the
  one format whose bytes B2 may write; a resource is a peer document B2 can read and never write.
  ([data-model.md](data-model.md) §10)
- **L4 — The body is read strictly as ordinary Markdown.** Every body link — wikilink, Markdown link,
  embed — is an untyped, **directed** `references` edge; no prose shape (list marker, leading verb)
  is ever B2 structure. ([data-model.md](data-model.md) §2)

## G — The typed graph

- **G1 — Every edge is authored and active.** An edge exists iff it is written in the Markdown; there
  is no `status` column, no suggestion queue, no lifecycle, and nothing inert. Committing is
  appending an authored line and re-projecting, never an index mutation.
  ([data-model.md](data-model.md) §3, §4)
- **G2 — The edge set is the union of exactly two homes, deduped frontmatter-wins.** Body links
  (`origin=inline`, always untyped) ∪ frontmatter `b2_relations:` (`origin=frontmatter`, the **sole**
  home of a verb + explanation). Same `(target, type)` in both homes keeps the frontmatter row (it
  alone carries the explanation); a *different* verb over a body-linked target coexists (the augment
  case). Nothing is ever copied between homes or auto-removed from a file.
  ([data-model.md](data-model.md) §0–§3)
- **G3 — The relation vocabulary is a closed three-verb stance core plus a tolerated tail.**
  `references` (neutral) / `supports` (for) / `contradicts` (against, symmetric) is the typing
  palette and what queries rely on; any other verb is stored verbatim as an opaque tail. The closed
  core is a *policy we can relax* (promotion path), never a structural assumption.
  ([data-model.md](data-model.md) §2)
- **G4 — Edges are directed and stored once.** Inverse labels are display-only, computed at read
  time; B2 never writes a reciprocal link into the target file. ([data-model.md](data-model.md) §2)
- **G5 — An unresolvable link target projects as a surfaced dangling edge, never a dropped one.**
  Broken links read as broken (`dst` NULL, authored text kept) and heal on the next reindex once the
  target exists. ([data-model.md](data-model.md) §3, GH #12)
- **G6 — The materialized graph is a cache; runtime parsing is the correctness definition.** The
  `edges` table exists for what parsing can't serve — backlinks, typed traversal, the discovery
  exclusion — and is rebuilt from scratch on every reindex. In v1 resources are edge *targets* only;
  `src` is always a note, because an edge must trace to an authored Markdown line.
  ([index-engine.md](index-engine.md) §3, [data-model.md](data-model.md) §10)

## M — The AI seams & the embedding space

- **M1 — Only enumerated AI seams: `Embedder` and `LlmProvider`.** `b2-core` is model-free and
  tested against deterministic fakes; a real model drops in through its seam with **no schema or
  flow change**. `Embedder` (text → vector) carries the index's one recorded identity (M2);
  `LlmProvider` (chat — streamed, cooperatively cancellable) deliberately carries **none**: nothing
  it produces is stored, so swapping chat models never touches the index. Model-compensating
  machinery (per-pair adjudication, query expansion, heavy orchestration) is deferred or off by
  default — the Bitter-Lesson tenet. A reranker, if it lands, is the next enumerated seam, not an
  exception. ([index-engine.md](index-engine.md) §5–§6, GH #151/#153)
- **M2 — The embedding space has one recorded identity: `meta.(embed_model_id, embed_dim)` — and the
  compute device folds into it** (a Metal build tags the id `@metal`). Any identity change is a model
  swap: `search` **fails fast** rather than mixing spaces, `reindex` drops and re-embeds, and `open`
  **never** mutates the vector space. ([CLAUDE.md](../../CLAUDE.md) "Embedding-space discipline", GH #40)
- **M3 — One embedding space in v1.** Every vault member funnels to *text* through the same model;
  multimodal spaces and describers are documented future seams, default-off.
  ([data-model.md](data-model.md) §10)
- **M4 — Vectors live in plain tables, scored in-process; their existence *is* the signal; and they
  are keyed by the hash of what was embedded.** The vector tables are created at embed time, so
  "tables exist" = "this vault has an embedding space" — the fallbacks (BM25-only search on a
  projected-but-unembedded vault) key on it. `embeddings` is **content-addressed**
  (`text_hash → vector`, GH #170): the embed input is exactly the chunk's stored text, so identical
  text has one vector, a renamed or moved note re-embeds nothing, and the store needs no
  invalidation rule beyond "a hash no chunk references is garbage" — pruned by the whole-vault pass.
  Centroids are the same derived data keyed by note path — refreshed by the embed pass, dropped on
  re-chunk. Model identity is not part of the key because it does not have to be: a swap drops the
  whole table (M2). ([CLAUDE.md](../../CLAUDE.md), #38)
- **M5 — Note content is never sent off-machine unbidden.** A cloud model endpoint exists only by
  explicit user configuration: the default chat configuration is a local endpoint, and a chat
  request carries the question *and* retrieved note passages — so the consent moment is the
  configuration moment, informed in place (plain-language privacy copy beside the Cloud-models
  setting, never a later popup). (GH #151)

## E — Engineering discipline (what keeps the above true)

- **E1 — The core is deterministic.** No wall-clock and no randomness inside `b2-core`; timestamps
  are injected (`created` params) and nothing is minted at all — since GH #170 identity is the path,
  so the id generator that was the core's other randomness source is gone rather than merely
  injected. Clocks and log subscribers live in the adapters.
  ([CLAUDE.md](../../CLAUDE.md) Conventions)
- **E2 — `cargo test` is fast, deterministic, and model-free; model quality never enters CI.**
  Real-model work lives behind `b2 init` / the out-of-CI eval. `#[ignore]` is forbidden — a
  hard-to-write test is a signal to re-anchor on the invariant or fix the system.
  ([CLAUDE.md](../../CLAUDE.md), the eval harness under `crates/b2-embed/evals/`)
- **E3 — The `Vault` façade is the one typed API, and every adapter is dumb.** CLI and desktop
  commands are deserialize → one façade call → serialize; logic that wants to live in an adapter
  belongs behind the façade. Dependencies point one way (adapters → core, never back); façade ops are
  added on need, never pre-built. ([crates/b2-desktop/CLAUDE.md](../../crates/b2-desktop/CLAUDE.md))
- **E4 — User-facing errors are generic and actionable, never leaking internals.** Full detail goes
  to logs / `B2_DEBUG`, not to the terminal or webview. ([CLAUDE.md](../../CLAUDE.md) Conventions)
- **E5 — Note content is untrusted input; rendering is a trust boundary.** Authorship is not trust: a
  `.md` can come from anyone (a shared vault, a downloaded or web-clipped note), so B2 treats rendered
  note content as hostile. Two rules hold together — B2 HTML-escapes every value *it* interpolates into
  UI chrome, and the **single** Markdown→HTML render seam (`renderMarkdown`) sanitizes its output before
  it reaches the DOM, so no note can inject executable markup at any call site (reading view, table
  widget, any future one). The webview CSP (`default-src 'self'`, no inline scripts) is a second,
  independent layer — defense-in-depth, never the sole guard. The same posture governs a note's
  **links**: the webview *is* the application, so following one in place would replace B2 with a web
  page in a window with no way back — a note's link therefore never navigates the webview. A web link
  (`http`, `https`, `mailto`) is an **OS handoff** performed host-side, behind a scheme allow-list,
  exactly as *Open in system default* is for a resource; every other scheme is refused rather than
  handed to an OS that would launch whatever app claims it.
  ([crates/b2-desktop/CLAUDE.md](../../crates/b2-desktop/CLAUDE.md), GH #77)

## C — Concurrency: many readers, one builder

- **C1 — Any number of processes may hold one vault's index open at once.** The index is a
  disposable projection, so concurrent *readers* are unrestricted and **a reader is never
  refused** — opening an index already at the current `schema_version` takes no write lock at
  all, so a running reindex cannot turn a `search` into an error. Creating and rebuilding that
  projection is the one step that must be **atomic and serialized**: an `open` observes a complete
  schema at the current `schema_version`, or waits out a bounded budget for the opener that is
  building one — **never a partial schema**, and never one another opener is still rebuilding. The
  no-partial half is absolute; the waiting half is not, and deliberately: past the budget a stuck
  writer is reported rather than hung on. "Complete" is checked, not assumed:
  a current stamp over missing tables is treated as stale and rebuilt from empty, since surviving
  rows would look up-to-date to an incremental reindex and break S3. The same holds for the
  index's *other* drop-and-rebuild, the vector tables (M4). Concurrent *writers* remain
  single-in-flight by the `reindex` advisory lock — which readers never take, and so cannot cover
  this. ([index-engine.md](index-engine.md) §3, GH #111, #114)

## K — Interaction: keyboard-first

- **K1 — B2 is fully operable from the keyboard; the mouse is an accelerator, never a requirement.**
  Every action a human can take with the mouse has a keyboard path — a focusable control in a sensible
  tab order, or a documented shortcut — across the whole desktop surface: file-tree navigation and
  open/create/rename/move/delete, global search and find-in-note, entering/leaving edit mode (⌘E) and
  every in-editor formatting chord, connection discovery and linking, the graph, and each menu/modal
  (`Escape` dismisses, `Enter` confirms, focus is trapped while an overlay is open and restored on
  close). Focus is always visible and follows platform/ARIA conventions. **A chord that is live in the
  app is B2's to document, whoever authored it**: the macOS menu bar's accelerators are declared rather
  than inherited from Tauri's default (`b2-desktop/src/menu.rs`), so the reference sheet can list them
  and the conflict gate can see them — a chord nothing enumerates cannot be found, and the app cannot
  warn about landing on it. **The chords are the user's, not B2's**: every chord B2 itself dispatches
  can be re-recorded from Settings → Keyboard and is stored as a UI preference (`localStorage`, like
  the theme — never vault state, never the index). The exception is narrow and stated per row
  (`Binding.fixed`): what a text field does with ⏎ and Esc, what a dialog's default button does, what a
  `<button>` does with ⏎/Space. Those are the platform's reflexes, and handing them out would be
  offering to break what every other app on the machine does. A rebinding is judged before it is
  accepted — refused if another command already answers to that keystroke in the same scope, or if the
  menu bar takes it first; advised, and allowed, when an inner surface or the editor also binds it.
  The `b2` CLI satisfies this by
  nature; K1 governs the GUI adapter. ([crates/b2-desktop/CLAUDE.md](../../crates/b2-desktop/CLAUDE.md),
  GH #78, #119, #121)
