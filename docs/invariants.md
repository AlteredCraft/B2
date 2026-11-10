# Invariants

What must always be true of B2. Each entry is one testable claim with a stable id (`S2`, `D1`)
that code comments and the other docs cite.

- If this page disagrees with another doc or with the code, this page wins. Fix the other side.
- Changing an entry is a deliberate decision, never a side effect of another edit.
- The reasons behind the big ones are in [decisions.md](decisions.md).

## S. Storage

- **S1. Two tiers.** The vault (Markdown, resources, the folder tree) is the source of truth.
  `.b2/b2.sqlite` is a disposable cache, and nothing in it is authoritative.
- **S2. The index is a pure projection of the vault directory.** Drop it, reindex, and the
  index comes back logically identical: every derived row, vectors included, ignoring only
  `indexed_at`, `mtime` and internal row ids. Markdown is the only format B2 writes. A
  dot-prefixed name, file or folder, is outside the vault: every walk skips it and every
  authoring command refuses it as a destination.
- **S3. Incremental reindex ≡ full rebuild.** Re-deriving one note lands on the state a
  from-scratch build would produce for it. Only the whole-vault pass prunes (rows for files
  that are gone, vectors no chunk references). A single-note path never does.
- **S4. No durable B2-derived state outside the Markdown.** No event log, no sidecar files,
  no fact that lives only in the index. Folders are yours: never projected, always read live
  from disk.
- **S5. A schema change is a version bump plus a rebuild, never a data migration.**

## W. Writes

- **W1. No unbidden writes.** Reading, walking, projecting and reindexing write nothing to
  the vault. `reindex` runs on a read-only vault and leaves a git-tracked vault with no diff.
- **W2. B2 never authors the note body**, and never asks it to carry B2 syntax. The one body
  edit B2 composes is the move repair: rewriting the path inside inbound links to a note it
  moved, aliases untouched.
- **W3. The on-command writes are a closed list:**
  - `b2 link` appends one `b2_relations:` entry to frontmatter.
  - The move repair (W2).
  - The editor save and the frontmatter save: a byte-exact splice of bytes you wrote,
    guarded by a content-hash revision.
  - Import: the bytes handed over, copied verbatim.
  - Create, move and delete of notes, resources and folders, on explicit command.
- **W4. B2 never deletes, moves or repairs your files on its own.** Orphans, dangling links
  and moves made outside B2 are surfaced, never silently fixed.
- **W5. Round trips are lossless.** `parse → serialize → parse` is byte-identical outside
  the one edit performed. Unknown frontmatter keys survive verbatim and in order. B2's only
  key is `b2_relations`; a generic `relations:` or an old `b2id:` is an unknown key like any
  other.

## L. Identity and links

- **L1. A note's identity is its vault-relative path, and every edge is keyed by path.**
  Nothing is stamped into a file. A move made through B2 rewrites the inbound link text and
  re-keys the index as one operation, and a move that fails leaves every file as it was. A
  move made outside B2 is a delete plus a create: inbound links dangle visibly (G5).
- **L2. A note's title is its filename.** A frontmatter `title:` is round-tripped and inert.
- **L3. Resources (non-`.md` files) share the identity model.** Path-keyed, index-only, no
  sidecars. B2 reads a resource and never writes its bytes.
- **L4. The body is read as ordinary Markdown.** Every body link (wikilink, Markdown link,
  embed) is an untyped, directed `references` edge. No prose shape is ever B2 structure.

## G. The typed graph

- **G1. Every edge is authored and active.** An edge exists exactly when it is written in
  the Markdown. There is no status, no suggestion queue and no lifecycle. Committing a link
  means writing a line and re-projecting, never editing the index in place.
- **G2. Two homes, frontmatter wins.** The edge set is the union of body links
  (`origin=inline`, always untyped) and frontmatter `b2_relations:` entries
  (`origin=frontmatter`, the only home of a verb and an explanation). The same
  `(target, type)` in both keeps the frontmatter row. A different verb over a body-linked
  target coexists with it. Nothing is copied between homes or removed from a file.
- **G3. A closed three-verb core plus a tolerated tail.** `references`, `supports` and
  `contradicts` are the palette and what queries rely on. Any other verb is stored verbatim
  and treated as opaque.
- **G4. Edges are directed and stored once.** Inverse labels are display-only. B2 never
  writes a reciprocal link into the target.
- **G5. A link that resolves to nothing is a surfaced dangling edge, never a dropped one.**
  It keeps the authored text and heals on the next reindex once the target exists.
- **G6. The `edges` table is a cache; parsing is the definition of correct.** It is rebuilt
  on every reindex. An edge's source is always a note; its target may be a note or a
  resource.

## M. Models

- **M1. Two AI seams, both defined in `b2-core`: `Embedder` and `LlmProvider`.** The core is
  model-free and tested against deterministic fakes, and a real model drops in with no
  schema or flow change. Chat stores nothing, so swapping chat models never touches the
  index. Machinery that compensates for a weak model stays out or off by default. A new AI
  part arrives as a new seam.
- **M2. The embedding space has one recorded identity:** `meta.(embed_model_id, embed_dim)`,
  with the compute device folded in (a Metal build tags the id `@metal`). On a mismatch
  `search` fails fast, `reindex` drops the vectors and re-embeds, and `open` never changes
  the vector space.
- **M3. One embedding space.** Every embedded member goes through the same text model.
  Today that is notes only.
- **M4. Vectors live in plain tables, scored in-process, keyed by the hash of the text that
  was embedded.** The tables are created at embed time, so their existence means "this vault
  has an embedding space" and their absence means keyword-only. Identical text has one
  vector, a moved note re-embeds nothing, and a hash no chunk references is garbage for the
  whole-vault pass to collect.
- **M5. Note content never leaves the machine unbidden.** The default chat endpoint is
  local. A cloud endpoint exists only by explicit configuration, and the privacy
  consequence is explained where it is configured.

## D. What gets shown

- **D1. Discovery always serves the ranked list.** `b2 similar` and the related-notes pane
  rank by best-passage distance. `limit` is a cap that under-fills only when there aren't
  enough scorable notes. No statistic computed over the anchor's own candidates may hide a
  candidate or empty the list, so an empty list means "nothing to compare", never "nothing
  relates". The z-score only paints the strength band. Row order, band and any future fold
  must never disagree: a fold must be a prefix of the ranked order with the rest one gesture
  away, and must win the eval bake-off before it ships.
- **D2. A served search result is a claim of evidence.** A query is vouched for when the
  vault holds lexical evidence (IDF-weighted coverage of the query's terms) or semantic
  evidence (a best cosine over a bar calibrated per embedding model). The verdict has three
  states, each a different behaviour:
  - **Evidence:** serve the rows.
  - **No evidence:** "no matches" and none of the rows, with no way to reveal them.
    `--json` still returns the rows beside `vouched: false`.
  - **No calibrated bar for the active model** (the fake, any unmeasured model): serve as
    always, with no verdict.

  `limit` is a cap, never a quota. `--exclude` subtracts rows, never evidence. A bar is
  earned in the eval harness and checked on a real vault before it ships.

## E. Engineering

- **E1. The core is deterministic.** No wall clock and no randomness in `b2-core`. Time is
  passed in.
- **E2. `cargo test` is fast, deterministic and model-free.** Model quality is measured by
  the eval harness, out of CI. `#[ignore]` is forbidden.
- **E3. `Vault` is the one typed API, and both adapters are dumb:** deserialize, one façade
  call, serialize. Dependencies point from adapters to core, never back. Façade operations
  are added when a command needs them.
- **E4. Errors shown to people are generic and actionable.** Internal detail goes to logs
  and `B2_DEBUG`.
- **E5. Rendering is a trust boundary.** Note content and model output are untrusted. There
  is one Markdown-to-HTML path, and it sanitizes. Every value B2 puts into its own chrome is
  escaped. The CSP is a second layer, never the only one. A note's link never navigates the
  webview: web links are handed to the OS behind a scheme allow-list, and every other scheme
  is refused.

## C. Concurrency

- **C1. Many readers, one builder.** Any number of processes may open one index. A reader
  against a complete, current schema takes no write lock and is never refused. Building or
  rebuilding (the schema, the vector tables) is atomic and serialized on SQLite's own write
  lock: an opener sees a complete schema or waits a bounded time, then reports a stuck
  writer instead of hanging. A current stamp over missing tables is rebuilt from empty. An
  index stamped by a newer `b2` is refused and left untouched. Writers stay single in
  flight.

## K. Keyboard

- **K1. The desktop app is fully operable from the keyboard.** Every pointer action has a
  keyboard path, focus is always visible, and overlays trap and restore focus. Every chord
  is declared in one registry (the macOS menu bar's included), is rebindable, and is checked
  for conflicts.
