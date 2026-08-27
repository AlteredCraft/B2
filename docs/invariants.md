# Invariants

The rules that must always be true of B2. Read this before you change B2's behavior.
Find the entries for the area you are touching and keep them true. Cite entries by id (S2, D1).

How this page works:

- Each entry is one testable claim. If this page disagrees with any other doc, or with the
  code, this page wins. Fix the other side.
- Changing an entry is a deliberate decision, never a side effect of another edit. It means
  writing or superseding a record in [ADRs/](../ADRs/README.md), which holds the *why* behind
  every entry.
- [data-model.md](data-model.md) defines the shapes these rules govern (the *what*).
  [index-engine.md](index-engine.md) defines the machinery (the *how*).
- The product non-negotiables (local-first, zero lock-in, single binary) live here as entries
  too.

Two ideas drive the whole list. First: your vault is volatile and the index is disposable, so
you can rewrite your notes without fear. Second: build for tomorrow's model, so a better model
drops in without a redesign.

## S. Storage: two tiers, one projection

- **S1. Two tiers, sharply split.** The vault (Markdown files, resources, and the folder tree)
  is the source of truth. `.b2/b2.sqlite` is a disposable cache. Nothing in the index is
  authoritative. ([data-model.md](data-model.md), "The two storage tiers")
- **S2. The index is a pure projection: `index = projection of (the vault directory)`.** Drop
  `b2.sqlite`, reindex, and you get an identical index back. Identical means *logically*
  identical, the standard the projection tests assert: every derived row, vectors and
  centroids included, excluding only what may differ between two builds of the same files
  (`indexed_at`, `mtime`, and internal chunk row ids; chunks are keyed by `(note, seq)`).
  Markdown is the vault's only
  *authored* format: the only format whose bytes B2 may write. Resources contribute derived
  rows only. Folders are never projected at all; B2 reads them live off disk. The projected
  domain is the vault's *managed subtree*: a dot-prefixed name is not vault material, whether
  it is a folder, a resource, or a `.md` file. Every walk skips it before deciding what it is,
  and every authoring command refuses it as a destination, so B2 never creates a file it would
  then never see. The file itself stays on disk, untouched, simply outside the projection.
  ([data-model.md](data-model.md) §1, §10, [index-engine.md](index-engine.md) §3,
  [GH #136](https://github.com/AlteredCraft/B2/issues/136))
- **S3. Full reindex ≡ incremental update.** Re-deriving one changed note lands on exactly
  the state a from-scratch rebuild would produce for it. Reconciling deletions is scoped to
  the whole-vault pass: it prunes rows for files the walk no longer finds, while a
  single-note path touches only its own note's rows and never prunes
  ([index-engine.md](index-engine.md) §8). There is no identity carve-out: identity is the
  path (L1), and the filesystem guarantees one file per path, so "two files with one
  identity" cannot arise; a copy is just another note at another path.
  ([index-engine.md](index-engine.md) §8,
  [GH #170](https://github.com/AlteredCraft/B2/issues/170))
- **S4. No durable B2-derived state outside the Markdown.** No event log, no sidecar files, no
  facts that live only in the index. The scope is *B2-derived* data. Your own folder tree is
  vault material, and the filesystem is authoritative for it: folders are never projected, and
  the tree listing is a live walk of the disk. ([data-model.md](data-model.md), "Folders")
- **S5. A schema change is a version bump plus a rebuild, never a data migration.**
  Disposability makes this free. A migration script would be evidence that S2 broke.
  ([index-engine.md](index-engine.md) §3)

## W. Writes: the vault changes only on your command

- **W1. B2 makes no unbidden writes. Period.** Every byte B2 writes to the vault is the
  mechanics of a command you ran (W3). Reading a vault (walking it, projecting it, reindexing
  it) writes nothing. `reindex` runs unchanged on a read-only vault, and a git-versioned vault
  shows no diff from having been indexed. ([data-model.md](data-model.md) §1)
- **W2. B2 never authors the note body, and never asks it to carry B2 syntax.** The body is
  100% your document. The one body edit B2 composes itself is the mechanical move repair:
  when a note moves, B2 rewrites the *path text* inside inbound `[[oldpath|alias]]` links so
  they keep resolving. It fixes a link you already wrote. It never adds one, and aliases
  survive verbatim. (The editor save, `Vault::write`, also replaces the body, but with bytes
  *you* wrote; B2 is only the carrier, W3.) ([data-model.md](data-model.md) §0)
- **W3. The on-command writes are a short, closed list:**
  - `b2 link` appends one `b2_relations:` entry (frontmatter, never the body).
  - The move repair of W2.
  - The editor save (`Vault::write`): a byte-honest splice of *your own* body bytes, guarded
    by a content-hash revision.
  - The frontmatter save (`Vault::write_frontmatter`): the same guarded splice of *your own*
    frontmatter bytes, body untouched. B2 owns no line in that block and judges none.
  - Import (`Vault::import_file` / `import_path`): the handed bytes copied verbatim, then
    projected.
  - Create, move, and delete of notes, resources, and folders, on explicit command.
- **W4. B2 never deletes, moves, or archives your files on its own.** Consequences of your
  edits (orphans, dangling links, hash-matched move candidates) are surfaced, flagged, or
  proposed. They are never silently applied. ([index-engine.md](index-engine.md) §8)
- **W5. Round trips are lossless.** `parse → serialize → parse` is byte-identical outside the
  one edit performed. Unknown frontmatter keys survive verbatim, in order. B2's one key is
  namespaced (`b2_relations`) so it can never collide with yours; a generic `relations:` key
  is *not* read. A `b2id:` line left by an older B2 is now just an unknown key: never read,
  never rewritten, never removed. Deleting `.b2/` is the whole upgrade
  ([GH #170](https://github.com/AlteredCraft/B2/issues/170)).
  ([data-model.md](data-model.md) §1, §6)

## L. Identity and links

- **L1. A note's identity is its vault-relative path, and every edge is keyed by path.** Both
  link homes are already written by path: a body `[[path]]`, or a frontmatter `b2_relations:`
  entry. So an edge stores what you authored, resolved at projection time, with no machine id
  in the file. Consequence: a rename keeps every backlink resolving *when B2 does the move*. A
  B2 move rewrites the inbound path text and re-keys the moved note's rows in one transaction.
  A move made outside B2 is a delete plus a create: the inbound links surface as dangling
  (G5), identified rather than silently dropped.
  ([data-model.md](data-model.md) §1, §3,
  [GH #170](https://github.com/AlteredCraft/B2/issues/170))
- **L2. A note's title is its filename.** The frontmatter `title:` key is recognized but
  inert: round-tripped, never driving display, aliases, or search. `b2 link` therefore writes
  a bare `[[path]]` with no alias. ([data-model.md](data-model.md) §1)
- **L3. Notes and resources share one identity model.** Both are keyed by the vault-relative
  path, index-only, with no sidecar files ever. The one asymmetry left is the authoring
  surface, not status or identity: a note has frontmatter and authored edges because Markdown
  is the one format B2 may write; a resource is a peer document B2 can read and never write.
  ([data-model.md](data-model.md) §10)
- **L4. The body is read strictly as ordinary Markdown.** Every body link (wikilink, Markdown
  link, embed) is an untyped, *directed* `references` edge. No prose shape (a list marker, a
  leading verb) is ever B2 structure. ([data-model.md](data-model.md) §2)

## G. The typed graph

- **G1. Every edge is authored and active.** An edge exists exactly when it is written in the
  Markdown. There is no `status` column, no suggestion queue, no lifecycle, and nothing inert.
  Committing a connection means appending an authored line and re-projecting, never mutating
  the index in place. ([data-model.md](data-model.md) §3, §4)
- **G2. The edge set is the union of exactly two homes, deduped frontmatter-wins.** Body links
  (`origin=inline`, always untyped) plus frontmatter `b2_relations:` entries
  (`origin=frontmatter`, the *only* home of a verb and explanation). If the same
  `(target, type)` appears in both homes, the frontmatter row is kept, because only it can
  carry the explanation. A *different* verb over a body-linked target coexists (the augment
  case). Nothing is ever copied between homes or auto-removed from a file.
  ([data-model.md](data-model.md) §0 to §3)
- **G3. The relation vocabulary is a closed three-verb core plus a tolerated tail.**
  `references` (neutral), `supports` (for), `contradicts` (against, symmetric) is the typing
  palette and what queries can rely on. Any other verb is stored verbatim as an opaque tail.
  The closed core is a policy B2 can relax later (promotion), never a structural assumption.
  ([data-model.md](data-model.md) §2)
- **G4. Edges are directed and stored once.** Inverse labels are display-only, computed at
  read time. B2 never writes a reciprocal link into the target file.
  ([data-model.md](data-model.md) §2)
- **G5. A link target that resolves to nothing projects as a surfaced dangling edge, never a
  dropped one.** Broken links read as broken (`dst` NULL, the authored text kept) and heal on
  the next reindex once the target exists. ([data-model.md](data-model.md) §3,
  [GH #12](https://github.com/AlteredCraft/B2/issues/12))
- **G6. The stored graph is a cache; parsing is the definition of correct.** The `edges` table
  exists for what parsing one file cannot serve: backlinks, typed traversal, and discovery's
  exclusion. It is rebuilt from scratch on every reindex. In v1, resources are edge *targets*
  only; `src` is always a note, because an edge must trace to an authored Markdown line.
  ([index-engine.md](index-engine.md) §3, [data-model.md](data-model.md) §10)

## M. The AI seams and the embedding space

- **M1. Only enumerated AI seams: `Embedder` and `LlmProvider`.** `b2-core` is model-free and
  tested against deterministic fakes; a real model drops in through its seam with no schema or
  flow change. `Embedder` (text to vector) carries the index's one recorded identity (M2).
  `LlmProvider` (chat: streamed, cancellable) deliberately carries none: nothing it produces
  is stored, so swapping chat models never touches the index. Machinery that compensates for a
  weak model (per-pair adjudication, query expansion, heavy orchestration) is deferred or off
  by default. If a reranker lands, it is the next enumerated seam, not an exception.
  ([index-engine.md](index-engine.md) §5, §6,
  [GH #151](https://github.com/AlteredCraft/B2/issues/151)/[#153](https://github.com/AlteredCraft/B2/issues/153))
- **M2. The embedding space has one recorded identity: `meta.(embed_model_id, embed_dim)`, and
  the compute device folds into it.** A Metal build tags the id `@metal`. Any identity change
  is a model swap: `search` fails fast rather than mixing spaces, `reindex` drops the vectors
  and re-embeds, and `open` never mutates the vector space.
  ([index-engine.md](index-engine.md) §6,
  [GH #40](https://github.com/AlteredCraft/B2/issues/40))
- **M3. One embedding space in v1.** Notes are the embedded members today; the designed
  resource-content path funnels every member to *text* through the same model when it ships
  ([data-model.md](data-model.md) §10, designed rather than built). Multimodal spaces and
  describers are documented future seams, off by default
  ([GH #110](https://github.com/AlteredCraft/B2/issues/110)).
- **M4. Vectors live in plain tables, scored in-process; their existence is the signal; and
  they are keyed by the hash of what was embedded.** The vector tables are created at embed
  time, so "the tables exist" means "this vault has an embedding space". The fallbacks key on
  that (BM25-only search on a projected-but-unembedded vault). `embeddings` is
  content-addressed (`text_hash → vector`): the embed input is exactly the chunk's stored
  text, so identical text has one vector, a moved note re-embeds nothing, and the only
  invalidation rule is "a hash no chunk references is garbage", pruned by the whole-vault
  pass. Centroids are the same derived data keyed by note path: refreshed by the embed pass,
  dropped on a re-chunk. Model identity is not part of the key because it does not need to be:
  a swap drops the whole table (M2). ([index-engine.md](index-engine.md) §3, §4,
  [GH #38](https://github.com/AlteredCraft/B2/issues/38)/[#170](https://github.com/AlteredCraft/B2/issues/170))
- **M5. Note content is never sent off your machine unbidden.** A cloud model endpoint exists
  only by your explicit configuration. The default chat endpoint is local, and a chat request
  carries your question *and* retrieved note passages. So the consent moment is the
  configuration moment, explained in place (plain privacy copy beside the cloud-models
  setting, never a later popup). ([GH #151](https://github.com/AlteredCraft/B2/issues/151))

## D. Surfacing and disclosure

- **D1. Discovery answers a relative question, the default view answers a quality one, and no
  anchor-local statistic ever makes a candidate unreachable.** The rules:
  - "What in my vault belongs next to this note?" is relative. So `b2 similar` and the
    discovery pane rank by best-passage distance, and the full ranked list stays reachable.
    `limit` is a cap; it under-fills only when there are not enough scorable notes.
  - An empty surface must never claim "nothing relates" from anchor-local statistics. Such a
    test cannot tell *nothing is related* from *everything is related*: the same geometry read
    from opposite ends. A single-domain vault went dark on 16 of 17 notes this way
    ([GH #196](https://github.com/AlteredCraft/B2/issues/196)).
  - What the default view *shows* is a claim of quality, and always filling to `limit` makes
    that claim falsely. A pane that always finds ten trains you to distrust all ten (measured
    in real-vault dogfooding, 2026-08).
  - A quality signal may therefore set a default *disclosure boundary*: a fold that is a
    prefix of the ranked order, with everything below it collapsed but one gesture away. A
    signal that would admit rank 5 while folding rank 2 is inadmissible: row order, band, and
    fold must never visibly disagree.
  - Every such signal is evidence-gated. It must win the measured bake-off on the orthogonal
    corpus, on the dense single-domain fixture (where a non-empty default view is absolute),
    and on real vaults via `make calibrate`. "No fold at all" is an admissible winner.
  - Every such signal must be continuous in population size: a threshold may move banding or
    the fold, never which rows exist or can be reached.
  - Strength stays a within-list grading painted from the z-score, and it gates nothing.

  The first bake-off found no admissible fold
  ([GH #200](https://github.com/AlteredCraft/B2/issues/200), 2026-08-22), so the default view
  is still the whole served prefix. The permission above stands for the next candidate; the
  measurement retired one rule, not the axis. The harness keeps #200's structural-zero
  tripwire, which re-arms on its own if a fold ever ships.
  ([index-engine.md](index-engine.md) §3, GH #196/#197/#200/#202)
- **D2. A served search result is a claim of evidence, and `limit` is a quota nowhere in
  B2.** The vector half always has k nearest (*nearest* is a fact about the vault, never
  evidence about the query), and RRF fuses ranks, discarding the absolute signals. Left alone,
  the pipeline cannot answer zero: a nonsense query would serve `limit` confident-looking
  results. The rules:
  - A result in the default view must trace to positive evidence: a lexical match, or semantic
    closeness clearing a bar calibrated per model in the eval harness. A query the vault holds
    no evidence for answers **"no matches"**, honestly empty. The nearest-by-meaning list is
    never presented as matches.
  - The rule over the signals is *lexical OR semantic*: two independent signals, so the test
    can tell "nothing matches" from "everything matches" where one signal could not (D1's own
    lesson). The engine carries both: `hybrid_search` returns the fused order untouched, plus
    each hit's rank in each list and its own distance, plus the query's best cosine
    ([GH #201](https://github.com/AlteredCraft/B2/issues/201)). `Vault::search_evidence` reads
    the lexical half beside it and serves exactly the rows `search` does, in the same order.
  - The lexical half is IDF-weighted term coverage: how much of the query's own weight the
    vault carries. A word found in most chunks weighs almost nothing; a word found in none
    weighs the most. So a stopword is a measurement, not a word list. Its first form was a
    hard df ceiling, and that form failed the transfer check on the dense fixture (it called
    `drone` and `comb` stopwords in a beekeeping vault). The lesson is kept: a constant read
    off one corpus describes that corpus. The fix was to change the *rule*, not re-tune the
    number.
  - Any bar is a distributional constant: it is earned against labelled negative queries
    before it ships, and it must pass the real-vault transfer check (`make calibrate`, process
    rule 5 in [evals.md](evals.md)). A labelled relevant query the bar would cut is the
    search-side tripwire, asserted at zero with no headroom.
  - The verdict is three-state, and each state is a different behavior
    ([GH #202](https://github.com/AlteredCraft/B2/issues/202)): evidence found → serve as
    always. No evidence → the honest empty state and *none* of the rows. This is strict: no
    reveal, no `--all`, no expander, because any of those would put the nearest list forward
    as candidates after all. The cost is accepted because it is bounded to one query at a
    time, never a whole vault. No calibrated bar for the active model → serve as always, never
    "no matches"; that third state is what the fake embedder and every unmeasured model
    produce, and folding it into "no evidence" would blank a dev vault.
  - `b2 search --json` is therefore an object: the rows plus the verdict. This is a documented
    break of the array contract, because a query-level reading has nowhere to live in a list
    of rows. It keeps serving the rows at `vouched: false` where the human surfaces show none:
    an agent handed rows *plus* an explicit verdict can be honest about them where a reader
    given rows alone cannot.
  - `search_evidence_excluding` (`b2 search --exclude`) is the same read minus a caller-named
    set of notes, for agent follow-up loops. The subtraction is the caller's, never the
    verdict's: the verdict and its signals still read the whole vault, and a heavily excluded
    query may honestly under-fill.
  - The per-hit *tail* fold (cutting where a real query's evidence runs out) is unshipped by
    measurement ([GH #206](https://github.com/AlteredCraft/B2/issues/206)): the fused order is
    not an evidence order, so no admissible prefix cut reaches more than 23 of the 367 filler
    rows an oracle fold would cut. The tail complaint is an *ordering* problem, and its
    payment is the reranker seam M1 reserves. The bake-off re-arms every run.

  ([index-engine.md](index-engine.md) §4, GH #201/#202/#206, [evals.md](evals.md))

## E. Engineering discipline: what keeps the above true

- **E1. The core is deterministic.** No wall clock and no randomness inside `b2-core`.
  Timestamps are passed in (the `created` params), and nothing is minted at all: identity is
  the path, so the id generator is gone rather than merely injected. Clocks and log
  subscribers live in the adapters. ([CLAUDE.md](../CLAUDE.md), Conventions)
- **E2. `cargo test` is fast, deterministic, and model-free; model quality never enters CI.**
  Real-model work lives behind `b2 init` and the out-of-CI eval harness. `#[ignore]` is
  forbidden: a hard-to-write test is a signal to re-anchor on the invariant or fix the system.
  ([evals.md](evals.md))
- **E3. The `Vault` façade is the one typed API, and every adapter is dumb.** CLI and desktop
  commands are deserialize, one façade call, serialize. Logic that wants to live in an adapter
  belongs behind the façade. Dependencies point one way (adapters to core, never back). Façade
  operations are added on need, never pre-built.
  ([crates/b2-desktop/CLAUDE.md](../crates/b2-desktop/CLAUDE.md))
- **E4. User-facing errors are generic and actionable, never leaking internals.** Full detail
  goes to logs and `B2_DEBUG`, not to the terminal or the webview.
  ([CLAUDE.md](../CLAUDE.md), Conventions)
- **E5. Note content is untrusted input; rendering is a trust boundary.** Authorship is not
  trust: a `.md` can come from anywhere (a shared vault, a downloaded or web-clipped note), so
  B2 treats rendered note content, and model output (the same class of input, M1), as hostile.
  Two rules hold together: B2 HTML-escapes every value *it* interpolates into UI chrome, and
  the single Markdown-to-HTML render seam (`renderMarkdown`) sanitizes its output before it
  reaches the DOM. The webview CSP (`default-src 'self'`, no inline scripts) is a second,
  independent layer: defense in depth, never the sole guard. The same posture governs a note's
  links: the webview *is* the application, so a note's link never navigates it. A web link
  (`http`, `https`, `mailto`) is an OS handoff performed host-side behind a scheme allow-list,
  and every other scheme is refused.
  ([crates/b2-desktop/CLAUDE.md](../crates/b2-desktop/CLAUDE.md),
  [GH #77](https://github.com/AlteredCraft/B2/issues/77))

## C. Concurrency: many readers, one builder

- **C1. Any number of processes may hold one vault's index open at once.** Concurrent
  *readers* are unrestricted, and a reader against a complete schema at the current
  `schema_version` is never refused: that open takes no write lock at all, so a running
  reindex cannot turn a `search` into an error. Creating or rebuilding the projection is the
  one step that must be atomic and serialized: an `open` observes a complete schema at the
  current version, or waits out a bounded budget for the opener building one. Never a partial
  schema. The no-partial half is absolute; the waiting half is deliberately not: past the
  budget, a stuck writer is reported rather than hung on. That report is the one refusal this
  entry permits, and it names a failure instead of hanging. "Complete" is checked, not
  assumed: a current stamp over
  missing tables is stale and rebuilt from empty, because surviving rows would look up-to-date
  to an incremental reindex and break S3. The same holds for the vector tables (M4).
  Concurrent *writers* stay single-in-flight through the `reindex` advisory lock, which
  readers never take. ([index-engine.md](index-engine.md) §3,
  [GH #111](https://github.com/AlteredCraft/B2/issues/111)/[#114](https://github.com/AlteredCraft/B2/issues/114))

## K. Interaction: keyboard-first

- **K1. B2 is fully operable from the keyboard; the mouse is an accelerator, never a
  requirement.** Every action the mouse can take has a keyboard path: a focusable control in a
  sensible tab order, or a documented shortcut. This covers the whole desktop surface: the
  file tree and open/create/rename/move/delete, search and find-in-note, edit mode (⌘E) and
  every in-editor chord, discovery and linking, chat (⌘J), the graph, and each menu and modal
  (`Escape` dismisses, `Enter` confirms, focus is trapped while an overlay is open and
  restored on close). Focus is always visible and follows platform/ARIA conventions. Three
  consequences:
  - A chord live in the app is B2's to document, whoever authored it. The macOS menu bar's
    accelerators are declared in `b2-desktop/src/menu.rs` rather than inherited from Tauri's
    default, so the reference sheet can list them and the conflict gate can see them.
  - The chords are yours, not B2's. Every chord B2 dispatches is re-recordable from
    Settings → Keyboard and stored as a UI preference (`localStorage`, like the theme; never
    vault state, never the index). The one exception is narrow and stated per row
    (`Binding.fixed`): the platform's own reflexes, like ⏎/Esc in a text field or ⏎/Space on a
    button, which handing out would break what every other app does.
  - A rebinding is judged before it is accepted: refused on a same-scope clash or a menu-bar
    chord; advised, and allowed, when an inner surface or the editor also binds it.

  The `b2` CLI satisfies this by nature; K1 governs the GUI adapter.
  ([crates/b2-desktop/CLAUDE.md](../crates/b2-desktop/CLAUDE.md),
  [GH #78](https://github.com/AlteredCraft/B2/issues/78)/[#119](https://github.com/AlteredCraft/B2/issues/119)/[#121](https://github.com/AlteredCraft/B2/issues/121))
