---
title: "B2 — qmd chunker upgrade (replace the minimal paragraph splitter)"
type: note
tags: [b2, core, chunking, embedding, retrieval, indexing, spec]
created: 2026-07-13
status: draft
---

# B2 — qmd chunker upgrade (replace the minimal paragraph splitter)

> **The build spec for replacing the placeholder paragraph chunker
> ([`b2-core/src/chunk.rs`](../../crates/b2-core/src/chunk.rs)) with the qmd-heuristic chunker
> ([index-engine.md](../index-engine.md) §1): size-targeted, overlapping, Markdown-aware chunks that
> carry a `heading_path` breadcrumb.** The current chunker emits **one chunk per blank-line-separated
> block**, so every heading becomes its own standalone embedding. Measured on a real vault, **62% of
> chunks are ≤20 words** (mostly bare headings and one-line list items). This is a three-way loss —
> embed speed, scan/storage cost, and retrieval quality — and this doc closes it. Executes **issue #19**.
>
> **This doc owns:** the new chunker algorithm and its B2 adaptations (the 512-token target, model-free
> token estimation, `heading_path`, overlap); the `heading_path` plumbing; the re-projection + the
> measurement gate. **It does not own:** the engine invariant or the reindex algorithm
> ([index-engine.md](../index-engine.md), [index-engine-build.md](completed/index-engine-build.md)); the
> projection/embedding split it reuses ([projection-embedding-split.md](completed/projection-embedding-split.md));
> the eval harness it scores against ([eval-strategy.md](eval-strategy.md)). Tree-sitter AST chunking for
> code files (qmd optional), per-model token budgets, and embed-order changes stay **deferred** (§8).

## 0. Scope & ground rules

Investigating slow embedding (the same thread that shipped model-selection + the per-model embed timer;
Metal is #40) surfaced the real driver: **the vault is embedding far more chunks than its content
warrants, most of them near-contentless.** The chunker — flagged as a placeholder in its own doc-comment
since day one — is the cause.

This doc changes **exactly one component** (the `chunk_body` function) and holds every engine decision
fixed:

- **The core stays model-free, synchronous, and deterministic** (root `CLAUDE.md`). The chunker takes
  **no embedder** and uses **no tokenizer** (that lives in `b2-embed`, behind the seam); it sizes chunks
  by a cheap, deterministic **proxy** and relies on the embedder's own truncation as the safety net. No
  wall-clock, no randomness, no new deps in `b2-core`.
- **The invariant `index = projection of (Markdown)` is untouched.** Chunking is pure re-projection: a
  different `chunk_body` is a *drop & rebuild* with no schema change (the `chunks` table already carries
  `token_count` and `heading_path`) and no invariant change. Incremental reindex re-chunks changed notes,
  exactly as today.
- **Chunk ids remain non-stable across a re-index** (index-engine-build.md §1.2) — nothing depends on
  them surviving a re-chunk.

**In scope:** the qmd-heuristic `chunk_body`; populating `heading_path`; the re-projection; and the
before/after measurement (embed speed via the shipped per-model timer, retrieval quality via the eval).
**Out of scope (§8):** code-aware (tree-sitter) chunking; threading a per-model token budget through the
`Embedder` trait; prepending `heading_path` into the *embedded* text (a knob to eval, not a commitment).

**Handoff — start at §6 Step 1.** The chunker is a self-contained, pure function behind a stable seam,
so it is unit-testable in isolation before any re-projection.

## 1. The problem, grounded in the code

[`chunk_body`](../../crates/b2-core/src/chunk.rs) splits the body into **paragraphs** — maximal runs of
non-blank lines separated by blank lines, one `Chunk` each, no size target, no merging, `heading_path`
left `NULL`, `token_count` = whitespace word count. In Markdown that means **every heading, list item,
and table is its own chunk.**

Evidence (a real 6-note / 128-chunk vault; the note with the most chunks, a DR plan):

| seq | words | text |
|----:|------:|------|
| 0 | 7 | `# Hermes DR Plan — Mac mini` |
| 2 | 3 | `## Threat model` |
| 7 | 8 | `### Tier 1 — irreplaceable, back up daily` |
| 8 | 139 | the actual table |

Vault-wide: avg **21 chunks/note**, and **62% of chunks ≤20 words**.

Why it costs so much: **embed cost per chunk has a large fixed component** — a 30-token sequence still
runs the full 12-layer BERT forward. So embedding 128 fragments costs far more than ~10 right-sized
chunks holding the *same content*. The loss is three-way:

1. **Speed** — inflated embedding *count* (∝ embed time via fixed-per-pass overhead + padding waste;
   content FLOPs are ~constant, so realistically a few-× embed speedup, stacking on a smaller model /
   Metal). It also makes ["batching across notes"](../../crates/b2-core/src/ingest.rs) largely moot:
   fewer, larger, more-uniform chunks fill batches naturally and vary less in length.
2. **Scans + storage** — discovery/search scan `embed::l2_sq` over **all** vectors is O(vectors); ~10×
   fewer vectors → proportionally cheaper scans and DB.
3. **Quality (the real prize)** — a bare `## Threat model` vector is near-contentless and fragments a
   section's meaning across many vectors. Section-sized chunks with a `heading_path` breadcrumb retrieve
   much better, improving `similar`/`search` directly.

## 2. The qmd heuristic (the reference, index-engine.md §1)

- **~900-token chunks, ~15% overlap.**
- **Markdown-aware break-point scoring** at the target boundary: H1 = 100, H2 = 90, code-fence = 80,
  … blank-line = 20, list-item = 5.
- A **~200-token backward scan** from the target with **quadratic distance decay**, so the chosen
  boundary is the *best-scoring structural break near the target*, not an arbitrary cut.
- A **`heading_path`** breadcrumb (the H1 › H2 › H3 stack the chunk falls under).
- *(Optional in qmd: tree-sitter AST chunking for code files — deferred, §8.)*

## 3. Decisions locked (2026-07-13)

Four adaptations port qmd onto B2's model-free core and bge-family embedder. All four are settled.

- **D1 — target ~450 tokens (NOT qmd's 900).** *Reason:* bge-base/small **truncate at 512 tokens**
  (`model.rs` `MAX_TOKENS = 512`, itself capped by the model's `max_position_embeddings`). A 900-token
  chunk would be silently truncated at embed time — its tail is never embedded, so retrieval on that
  content is simply lost. ~450 sits under 512 with headroom for the D2 proxy's error and any D3
  breadcrumb. A `ChunkConfig` field (`target_tokens`, default 450) — see **D5**.

- **D2 — size by a `chars/4` proxy (model-free); truncation is the net.** The core cannot call the real
  tokenizer: it lives in `b2-embed`, and `b2-core` is model-free by rule. The sharper reason —
  **chunking runs in the model-free `project_vault` pass**
  ([projection-embedding-split.md](completed/projection-embedding-split.md)): a real tokenizer would force
  the model to load *during projection* and make first-paint wait on it again, undoing that split. So
  estimate tokens as `chars / chars_per_token` (a `ChunkConfig` field, default 4.0 — English ≈ 4
  chars/token; **code and tables run denser**, so it is a lever, not a law, D5). Approximation is safe: boundaries are soft
  (±tens of tokens is irrelevant to retrieval — the boundary *score* matters far more), and the embedder
  already **truncates at 512** as a hard backstop, so a proxy under-estimate merely clips the tail of one
  unusually dense chunk (a table, code), never corrupts the index. Target conservatively for headroom.
  *(Threading a real budget via `Embedder::max_tokens()` is deferred, §8 — it re-couples projection to
  the model for a gain the proxy already captures.)*

- **D3 — store `heading_path` unconditionally; the *prepend* is an eval-gated toggle.** The chunker tracks
  the heading stack and stamps each chunk with its breadcrumb (e.g. `What lives on this box > Tier 1`),
  replacing today's `NULL`. Whether to *also* **prepend** that breadcrumb into the **embedded text**
  (contextual chunk headers — injects the section's topic into the vector, so a heading-less table becomes
  findable by its section, §1's failure case) is a real retrieval knob with a token cost: it ships as a
  `ChunkConfig` field (`prepend_heading_path`, **default off**, see **D5**), and Step 3 A/Bs it on the
  eval. This is the deterministic, structural cousin of **contextual retrieval** (prepend section context
  before embedding); the LLM-generated per-chunk context is the richer someday-upgrade, but it needs a
  model and so stays out of this model-free pass (§8). Storing is unconditional — cheap, useful for
  display, and required for the toggle to be possible.

- **D4 — ~15% overlap, tunable.** Consecutive chunks share ~15% of content, so `char_start..char_end`
  ranges **overlap** (they no longer partition the body). Fine: each range still addresses the exact
  slice that produced its `text` (anchoring for explain/highlight holds), and `UNIQUE(note_b2id, seq)` is
  unaffected. A `ChunkConfig` field (`overlap_frac`, default 0.15), tuned against the eval, not fixed by
  fiat — see **D5**.

- **D5 — every lever lives on a `ChunkConfig` struct; `chunk_body` takes `&ChunkConfig`.** D1–D4 are
  *defaults*, not literals baked into the algorithm: `chunk_body(body: &str, cfg: &ChunkConfig) ->
  Vec<Chunk>`, where `ChunkConfig` carries the whole tuning surface and its `Default` reproduces the
  shipped values (so CLI/desktop callers pass `&ChunkConfig::default()` and nothing about their ergonomics
  changes).

  | field | default | lever |
  |---|---|---|
  | `target_tokens` | 450 | chunk size (D1) |
  | `overlap_frac` | 0.15 | overlap (D4) |
  | `chars_per_token` | 4.0 | the D2 token proxy |
  | `backscan_tokens` | 200 | boundary-search window |
  | `weights` | qmd's H1=100…list=5 | the boundary scorer |
  | `prepend_heading_path` | false | D3 contextual header |

  *Reason:* this component will be **tuned for some time**, and the boundary weights + backscan window are
  as consequential to cut quality as size/overlap — leaving them as hardcoded literals hides the levers
  that matter most. With a config: (a) the Step-3 eval **sweeps parameters in one process** (a loop over
  configs) instead of one recompile per cell — and Step 3 already A/Bs the D3 toggle and size; (b) unit
  tests construct configs instead of depending on ambient constants; (c) later exposure (a settings knob,
  a per-vault override) is trivial. D3's toggle already had to be threaded *somewhere*, so the
  "keep `chunk_body` argumentless" simplification was already spent — this collects every knob in one place
  rather than scattering a lone bool. It stays **pure, deterministic, and model-free**: a plain params
  struct with `Default` is no async/generics/traits/macros, so it satisfies the root `CLAUDE.md`
  "no speculative abstraction" rule — the concrete need (repeated tuning + the eval sweep) is present
  today, not speculative.

## 4. What changes in the code (surface)

- **`chunk.rs`** — `chunk_body(body: &str, cfg: &ChunkConfig) -> Vec<Chunk>` stays a **pure function**
  (all tuning on `cfg`, whose `Default` = the shipped values (D5), so it stays trivially testable *and*
  sweepable). New public type **`ChunkConfig`**. New internals: a line/block scan that accumulates toward
  `cfg.target_tokens`, a boundary scorer over the `cfg.backscan_tokens` backward window, overlap
  carry-over, and a running heading stack. `Chunk` gains **`heading_path: Option<String>`**; `token_count`
  now holds the **`chars / cfg.chars_per_token` token estimate** used for sizing (D2), documented as an
  estimate — not exact tokens (it was a whitespace word count under the paragraph splitter).
- **`db.rs`** — `replace_chunks` writes the `heading_path` column (today it inserts `NULL`); the schema
  already has it, so **no migration**.
- **`ingest.rs`** — **unchanged.** It calls `chunk_body` and hands `(id, text)` pairs to the embed pass
  exactly as now; fewer, larger chunks flow through the same batching loop.
- **No schema change, no invariant change, no new dependency.**

## 5. Correctness & determinism invariants (unchanged)

- **Pure & deterministic.** `chunk_body` is a total function of `(body, ChunkConfig)` — no wall-clock, no
  randomness (root `CLAUDE.md`). Same body + config ⇒ same chunks ⇒ reproducible index.
- **Idempotent re-projection.** Drop `.b2/` and rebuild ⇒ identical chunks/FTS/edges; vectors + centroids
  re-derive on the embed pass. An incremental reindex re-chunks only changed notes (`would_reembed`).
- **The embedder is the truncation safety net.** `model.rs` already truncates >512 tokens, so a proxy
  underestimate degrades gracefully (a rare tail loss), never a crash or a bad index.

## 6. Build order

### Step 1 — the chunker (start here)
Implement the qmd heuristic in `chunk.rs` behind the `chunk_body(body, &cfg)` seam (D5): token-target
accumulation (D1/D2 proxy), the Markdown boundary scorer (`cfg.weights`, H1..list-item, + quadratic
backward decay over `cfg.backscan_tokens`), `cfg.overlap_frac` overlap (D4), and the running
`heading_path` stack (D3). Add `heading_path` to `Chunk` and the `ChunkConfig` struct (D5). **Deterministic unit tests** (extend `crates/b2-core/tests/chunks.rs`): chunk
sizes cluster near the target and never exceed the proxy cap; a heading + its section land in **one**
chunk (the regression this fixes — assert `## Threat model` is not its own chunk); `heading_path` is
correct through nested headings; overlap present and bounded; empty/all-blank body ⇒ empty; a giant
single paragraph splits at the target. Golden-vault b2ids stay fixed; only chunk rows change.
**Update the two existing paragraph-splitter assertions** in the same file: `chunks_are_projected_for_each_note`
asserts `srs_chunks == 2` (spaced-repetition splits into two blank-line paragraphs) — under qmd sizing that
small note coalesces into **one** chunk, so the expectation becomes `== 1` (its `seq == 0` /
`starts_with("Spaced repetition exploits")` check still holds); `fts_index_tracks_chunks_and_matches_body_text`
keys on `note_b2id`, not `seq`, so it survives the reshape unchanged. Splitting inside a fenced code block
or table is a **separate guard tracked in #41** (a follow-up to §8, not Step 1).

### Step 2 — wire `heading_path` (sketch)
Populate the column via `db::replace_chunks`; surface it where it helps (explain/UI later — not required
for the retrieval win). Confirm FTS triggers still fire on the reshaped chunks.

### Step 3 — re-project & measure (the gate, sketch)
Drop & rebuild the dogfood vault. Capture, before vs after: **chunk count** and **embed throughput**
(the shipped per-model timer in Settings), and the **retrieval-quality delta** via
[`cargo run -p b2-embed --example eval`](eval-strategy.md) under paragraph vs qmd chunking. If D3's
prepend toggle is worth it, A/B it here.

## 7. Acceptance / the eval gate

Per **#19**, the upgrade is *unlocked by the eval*. Ship qmd chunking only if, on the hand-labelled
retrieval eval, it **does not regress** (target: **improves**) quality vs. the paragraph chunker — and it
should cut chunk count and embed time. The eval is the arbiter (eval-strategy.md); the embed timer
quantifies the speed win. A quality regression means retune (target size, overlap, D3 prepend) or hold.

**Caveat — make the gate sensitive *before* trusting it.** The eval's metric is *note-rank* ("does hybrid
search rank the right note first?", eval-strategy.md §1), but chunking's wins are mostly **sub-note**:
tighter intra-section retrieval and making a heading-less table/section findable by its topic (D3's whole
point). On a small corpus of easily-separable notes, note-rank can read **"no change"** even when
retrieval genuinely improved — so a naive D3 A/B may come back a coin-flip because the metric cannot *see*
what it changed. Before leaning on the eval as arbiter: **add queries that probe exactly these failure
modes** — content buried in a table, a heading-less subsection, a paraphrase that must resolve into a deep
section — and confirm the corpus is large/varied enough that a chunking delta moves the score rather than
overfitting a handful of labels (eval-strategy.md §3, "grow the set"). A gate the change is invisible to
is not a gate.

## 8. Open questions / deferred

- **Tree-sitter / code-aware chunking** (qmd optional) — defer; prose-oriented boundary scoring first.
- **Don't-split-inside-a-fence/table guard** (#41) — a small prose-mode guard against a forced cut
  bisecting a code block or table; near-term follow-up, distinct from the deferred tree-sitter work above.
- **Per-model token budget** (D2 alternative: `Embedder::max_tokens()` threaded into chunking) — defer;
  the hardcoded ~450 target covers every current + registry model (all 512-window bge).
- **`heading_path`-into-embedded-text** (D3 sub-decision) — an eval knob, decided in Step 3, not up front.
- **Overlap / target-size / boundary-weight / backscan tuning** — all `ChunkConfig` fields (D5); tune
  against the eval, not by guess.
- **Eval sensitivity to chunking** — the note-rank metric may be blind to sub-note improvements; grow
  `queries.json` with buried-in-table / heading-less-section / deep-paraphrase probes **before** Step 3,
  or the D3 A/B is uninformative (§7).

## 9. Docs to mirror (on ship)

- [`chunk.rs`](../../crates/b2-core/src/chunk.rs) top comment — drop the "minimal placeholder / UPGRADE
  PLAN" note; describe the shipped heuristic.
- [index-engine-build.md](completed/index-engine-build.md) §1.2 **BUILD NOTE** — "minimal chunker STILL
  ships" → "qmd heuristic shipped."
- [index-engine.md](../index-engine.md) §1 — mark the chunking heuristic as implemented, not aspirational.
- [tasks.md](../tasks.md) — reflect the ship; **close #19** with the before/after numbers.
- This spec moves to `planning/specs/completed/` on ship (per the repo convention).
