---
b2id: 01KWSRNSA6ZVQEM14NSS345RRY
title: "B2 — Tasks"
type: note
tags: [b2, tasks, planning]
created: 2026-06-28
updated: 2026-07-07
status: active
---

# B2 — Tasks

The working queue. Start at [README.md](../README.md) for the map; the *why / what / how* live in the
design docs, which are the **source of truth** (the code is a projection of them):
[vision-and-scope.md](vision-and-scope.md), [data-model.md](data-model.md),
[index-engine.md](index-engine.md), [user-stories.md](user-stories.md), and the build specs under
[specs/](specs/).

> **Backlog now lives in GitHub Issues.** Planned-but-unstarted work moved to the
> [issue tracker](https://github.com/AlteredCraft/B2/issues) (2026-07-06); this file holds only the
> current focus and the design anchors that code comments cite. Shipped history is in git and in the
> specs — it is no longer duplicated here.

## Current state (2026-07-07)

The **headless engine is complete** and **two dumb adapters over the [`Vault`](../crates/b2-core/src/vault.rs)
façade** have shipped:

- **Engine** — index-engine build steps 0→5 ([specs/completed/index-engine-build.md](specs/completed/index-engine-build.md));
  the real candle-backed embedder + out-of-CI eval ([specs/eval-strategy.md](specs/eval-strategy.md)); the
  note-authoring CLI (`add` / `mv` / `link` / `explain` / `similar` / `search` / `reindex` incl. `--dry-run`).
- **The discovery pivot (2026-07-04)** — cut the LLM relator, the `b2-relate` crate, and the `.b2/log/`
  tier; discovery is now `b2 similar` (surface near-but-unlinked) + `b2 link` (the human commits). The
  invariant simplified to **`index = projection of (Markdown)`** (two tiers). See
  [vision-and-scope.md](vision-and-scope.md) "Decisions locked (2026-07-04)".
- **Desktop UI** — the read-only MVP ([specs/completed/desktop-ui-mvp.md](specs/completed/desktop-ui-mvp.md), Steps 0→3): read a
  note, surface similar-but-unlinked notes, commit a typed link; file-tree, frontmatter drawer, view-source
  toggle, in-app vault switcher. Plus **async, cancellable indexing**
  ([specs/completed/async-indexing.md](specs/completed/async-indexing.md)) — the desktop `reindex` is a non-blocking background
  action with live progress + Cancel.
- **Decoupled projection & embedding (2026-07-07)** — the keyword-first index
  ([specs/completed/projection-embedding-split.md](specs/completed/projection-embedding-split.md), Steps 1→3): `reindex` is now
  the composition of two separately-invokable façade passes — `Vault::project` (model-free: notes + chunks +
  FTS + edges) and `Vault::embed` (fill DB-derived missing vectors, metered + cancellable). `search` falls
  back to BM25-only on a projected-but-unembedded vault, and the desktop sequences project → paint tree →
  embed, so a cold vault is browsable/searchable in seconds while embedding streams behind. Closed
  [#15](https://github.com/AlteredCraft/B2/issues/15); follow-ons split out to #25/#26/#27.
- **In-editor body editing (2026-07-07)** — the desktop's first write surface
  ([specs/completed/desktop-editing.md](specs/completed/desktop-editing.md), Steps 1→3, dogfooded):
  `Vault::write` (byte-honest body splice, content-hash revision guard, model-free save), the
  `write_note` host command, and CodeMirror 6 edit mode with autosave-on-idle, a single-flight save
  chain, the conflict bar (Reload / Keep mine), and the trailing background embed. Closes
  [#13](https://github.com/AlteredCraft/B2/issues/13).

## Active — next up

Editing follow-ons, in whichever order the friction says:

1. **Live-preview decorations** — the document feel over the same CM6 pane — **specced 2026-07-08
   in [specs/desktop-live-preview.md](specs/desktop-live-preview.md)** (edit-mode-only, hybrid
   reveal, hand-rolled engine, zero new deps) — [#30](https://github.com/AlteredCraft/B2/issues/30).
2. **Step 5 — native fs-watch auto-reload** (replaces "stale until conflict" with live
   reconciliation; revisit frontend save-chain test extraction when this is specced) —
   [#14](https://github.com/AlteredCraft/B2/issues/14).

## Backlog → GitHub Issues

Everything planned-but-unstarted is tracked as an issue:

| Theme | Issues |
|---|---|
| **Desktop** (beyond editing) | graph visualization [#22](https://github.com/AlteredCraft/B2/issues/22) · packaging/signing/distribution [#23](https://github.com/AlteredCraft/B2/issues/23) · auto-index-on-open (split §9) [#25](https://github.com/AlteredCraft/B2/issues/25) |
| **Indexing & performance** | "semantic: N/M embedded" signal (split §9) [#26](https://github.com/AlteredCraft/B2/issues/26) · relevance-ordered embedding (split §9) [#27](https://github.com/AlteredCraft/B2/issues/27) · cross-process CLI reindex + `b2 status` + Ctrl-C (async §8) [#16](https://github.com/AlteredCraft/B2/issues/16) · faster/smaller embedder spike (async §8) [#17](https://github.com/AlteredCraft/B2/issues/17) |
| **Engine & quality** | property tests for the invariants [#18](https://github.com/AlteredCraft/B2/issues/18) · qmd chunker upgrade [#19](https://github.com/AlteredCraft/B2/issues/19) · distance-weighting for `b2 similar` [#20](https://github.com/AlteredCraft/B2/issues/20) |
| **Adapters & docs** | `serve` HTTP adapter [#24](https://github.com/AlteredCraft/B2/issues/24) · docs HTML + test-count badge sync [#21](https://github.com/AlteredCraft/B2/issues/21) |

## Design anchors referenced from code

Compaction kept these because code comments cite them by name; the **canonical** home for each is the
linked doc.

- **① Connection discovery** (resolved 2026-07-01) — a candidate is the graph's *complement*, **near ∖
  connected**: per anchor chunk, KNN its **stored** `chunks_vec` vector (no re-embed, passage↔passage),
  score each other note by its **best** chunk-pair (max-sim), subtract the anchor's 1-hop neighbors
  (distance is **exclusion-only** — 2-hop candidates survive unboosted), rank → top-N. Canonical:
  [index-engine.md](index-engine.md) §3; distance-weighting is the deferred experiment
  ([#20](https://github.com/AlteredCraft/B2/issues/20)).
- **Testability (steps 4–5 principle)** — `cargo test` stays fast, deterministic, and **model-free** (the
  fake embedder); real-model work (`b2 init`, `--example eval`, the retrieval eval) runs **out of CI**.
  Canonical: [CLAUDE.md](../CLAUDE.md) + [specs/eval-strategy.md](specs/eval-strategy.md).
- **"Next up" / build plan** — the execution order other docs point to is this file's **Active** section
  above (build steps themselves: [specs/completed/index-engine-build.md](specs/completed/index-engine-build.md) §4).
- **Typed relations in Markdown** — the authored-reference layer (`relations:` frontmatter + body links).
  Canonical: [data-model.md](data-model.md) §0/§3.
