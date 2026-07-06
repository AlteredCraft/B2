---
title: "B2 — Async, cancellable indexing (desktop)"
type: note
tags: [b2, ui, desktop, tauri, reindex, indexing, async, spec]
created: 2026-07-06
status: draft
---

# B2 — Async, cancellable indexing (desktop)

> **The build spec for making `reindex` a first-class background action in the desktop app —
> live progress, cancellable, and non-blocking — without pushing async, threads, or non-determinism into
> the model-free core.** The engine already reindexes incrementally and *can* report per-batch progress
> ([`Vault::reindex_with_progress`](../../crates/b2-core/src/vault.rs) → [`ingest::ReindexProgress`](../../crates/b2-core/src/ingest.rs));
> the [`b2-cli`](../../crates/b2-cli) adapter renders that as a live line. The **desktop adapter throws it
> away** — it calls the no-progress `Vault::reindex()` and freezes the whole UI behind one Promise. This
> doc closes that gap.
>
> **This doc owns:** the async-indexing UX contract (progress + cancel), the one small core seam a
> cancellable reindex needs (a cooperative-cancel checkpoint that keeps the core sync + deterministic), the
> host's task-lifecycle + IPC-streaming responsibilities, the reliability invariants a partial/cancelled
> index must satisfy, and the build order. It also **consolidates the previously-scattered "non-blocking
> embedding" discussion** (was `tasks.md` Backlog) into one place.
>
> **It does not own:** the engine invariant or the reindex algorithm itself
> ([index-engine.md](../index-engine.md), [specs/index-engine-build.md](index-engine-build.md)); the desktop
> adapter's general shape and the thin-adapter discipline ([specs/desktop-ui-mvp.md](desktop-ui-mvp.md),
> [`crates/b2-desktop/CLAUDE.md`](../../crates/b2-desktop/CLAUDE.md)). The **progressive / keyword-first**
> index and the **cross-process CLI background reindex** are *related but separate* efforts — §7 and §8
> record them and say why they're deferred behind this one.

## 0. Scope & ground rules

The desktop MVP shipped read-only-first ([desktop-ui-mvp.md](desktop-ui-mvp.md) §4), and dogfooding a
~1000-note vault surfaced the gap this doc fills: **the first cold index of a large vault is slow, and the
desktop app makes it feel broken** — a busy cursor, a disabled UI, no progress, and no way to stop it.

This doc adds **exactly one capability** — reindex becomes an observable, cancellable background action —
and holds every existing decision fixed:

- **The core stays model-free, synchronous, and deterministic.** No `async`/`tokio`, no threads, no
  wall-clock, no randomness enter `b2-core` (root CLAUDE.md). All threading, atomics, and IPC live in the
  **host** (`b2-desktop`), which already runs commands on Tauri's worker pool. The *only* core change is a
  cooperative-cancel **checkpoint** — a function call at a boundary that already exists — which is itself
  deterministic.
- **The host stays a dumb adapter** ([`b2-desktop/CLAUDE.md`](../../crates/b2-desktop/CLAUDE.md)). A
  background-task lifecycle (spawn / track / cancel) and IPC streaming are **host infrastructure** — the
  same category as vault-root resolution, embedder wiring, and the native folder picker the charter already
  accepts as legitimate host responsibilities. *What* to embed, the incremental decision, edge derivation,
  and the cancel checkpoint stay in the core; *how the window drives and interrupts it* stays in the host.
- **The invariant `index = projection of (Markdown)` is untouched.** A cancelled index is a *smaller*
  projection, never a wrong one (§5).

**In scope:** live progress + cancel for the desktop `reindex`, and the small core seam it needs.
**Out of scope (later, §7–§8):** progressive keyword-first ordering; auto-index-on-open UX; a CLI Ctrl-C
cancel; a cross-process background reindexer; a faster/smaller embedder.

## 1. The problem, grounded in the code

| Layer | What exists today | The gap |
|---|---|---|
| **Core** (`b2-core`) | [`reindex_with_progress(force, on_progress)`](../../crates/b2-core/src/vault.rs) drives [`ingest_vault_with_progress`](../../crates/b2-core/src/ingest.rs), which fires [`ReindexProgress`](../../crates/b2-core/src/ingest.rs) **per embed batch** (`ingest.rs` embed loop). Incremental: unchanged, fully-embedded notes are skipped. | `on_progress` returns `()` — **no cooperative-cancel seam**. A reindex runs to completion or not at all. |
| **CLI** (`b2-cli`) | Consumes the callback → prints a live `embedding n/N · <path> (k chunks)` line on an interactive stderr (`b2-cli/src/main.rs`). | (fine — reference implementation of *using* progress) |
| **Desktop host** (`b2-desktop`) | [`commands::reindex`](../../crates/b2-desktop/src/commands.rs) calls the **no-progress** `vault.reindex()` and returns one `ReindexReport`. `#[tauri::command(async)]` runs it off the main thread, so the *window* paints. | **Progress is discarded** (never reaches the webview) and **there's no cancel**. The one existing seam the CLI uses is simply not wired here. |
| **Frontend** (`ui/`) | `doReindex()` flips a global `state.loading` that disables the entire UI + shows the busy cursor, then awaits one Promise (`ui/src/main.ts`). | **Blocking-by-choice**: the window is responsive but the *app* is frozen, with no signal of life and no stop button. |

**Root cause, in one line:** the engine already produces the progress the UI needs — the desktop adapter
just doesn't stream it, and no cancel checkpoint exists yet.

**One property that makes this cheap and safe:** `ingest_vault_with_progress` projects **every** note's
chunks + FTS + edges (Phase 1 / Phase 2) and only *then* embeds vectors (Phase 1b), writing autocommitting
per statement (it takes a `&Connection` and `execute`s directly — no whole-run transaction). So at any
moment mid-embed, the DB already holds a **complete keyword + graph index** for the whole vault; only some
notes have vectors yet. A partial or cancelled index is therefore already **consistent and durable**, and
incremental reindex heals the rest on the next run. This is the load-bearing fact behind both cancel-only
(§2) and the deferred progressive index (§7).

## 2. Decisions locked (2026-07-06)

| Concern | Locked choice | Rejected — and why |
|---|---|---|
| **Stop semantics** | **Cancel-only.** A running index can be cancelled; re-running resumes cheaply because incremental reindex skips already-embedded notes and a partial index is consistent (§1). | **True pause/resume** — a paused-state machine (freeze mid-run, resume exactly) is materially more state to hold for marginal benefit: incremental reindex already makes *cancel + re-run* a near-free resume. Recorded as a possible follow-on (§9), not built. **Progress-only, no stop** — leaves a huge cold index un-interruptible; fails the core ask. |
| **Where the background work runs** | **In-process, on the host.** The desktop is one long-lived process; the reindex runs on a Tauri worker thread (the `(async)` command *is* the background task), streaming progress while the UI thread stays free. | **Detached OS process + `b2 status`** (the `tasks.md` backlog idea) — designed for the *stateless one-process-per-command CLI*, where in-process isn't an option. It buys cross-process progress at the cost of a process lifecycle; unnecessary and heavier for a resident app. Kept as a distinct CLI-world effort (§8). |
| **Progress transport** | **[`tauri::ipc::Channel<ReindexProgress>`](https://v2.tauri.app/develop/calling-frontend/#channels)** passed as a command argument — typed, ordered, scoped to the one call. | **Global `emit`/event bus** — a broadcast channel for a point-to-point, per-invocation stream; more room to leak across invocations/vaults. Channels are Tauri v2's purpose-built answer for streaming command progress. |
| **Cancel seam in the core** | The **existing per-batch progress callback returns [`ControlFlow<()>`](https://doc.rust-lang.org/std/ops/enum.ControlFlow.html)** (`Continue`/`Break`); the embed loop checks it at each batch boundary — the checkpoint that already exists. The host owns the `AtomicBool`; the closure maps it to `Break`. | **A second `should_continue: &dyn Fn() -> bool` param** — orthogonal but adds a parameter + a second closure for no real gain, since the batch boundary is already where progress fires. Reuse the one checkpoint; keep the surface minimal (core value). |
| **UI during reindex** | **Non-blocking.** Reading, searching, and navigating stay live (SQLite WAL = one writer + concurrent readers; each read command opens its own short-lived `Vault`). Only the **Reindex** action is disabled (single-in-flight) and a **progress + Cancel** affordance appears. | **Global freeze** (today's `state.loading`) — throws away the whole point of running off-thread; the app is idle-capable during a long index and should feel it. |

## 3. The core seam — a cancellable, still-deterministic reindex

Evolve the **one** progress-bearing entry point; do not proliferate variants.

- **Callback returns `ControlFlow<()>`.** `reindex_with_progress`'s `on_progress` becomes
  `&mut dyn FnMut(ReindexProgress) -> ControlFlow<()>`. The embed loop in `ingest_vault_with_progress`
  inspects the return after each batch and `break`s out of the embed phase on `ControlFlow::Break`. The
  convenience `Vault::reindex()` passes `|_| ControlFlow::Continue(())`; the CLI's closures return
  `Continue` (a one-line change at its two call sites) — no behavior change for the non-cancel path, which
  stays **byte-identical** to today.
- **On cancel, still finish the cheap, model-free work.** Break out of *embedding* only; then run **Phase 2
  (edge projection) for every projected note** as a normal run would. Result: all notes have chunks + FTS +
  edges (keyword search + graph complete), a prefix of notes have vectors, and the index is fully
  consistent — exactly the state an incremental reindex expects, so the next run embeds only the notes the
  cancel left unfinished (per-note granularity — a note interrupted mid-embed re-embeds in full, at most one
  note's worth of redo; see §5.2).
- **Report the outcome honestly.** [`ReindexReport`](../../crates/b2-core/src/vault.rs) gains
  **`cancelled: bool`**; its `indexed` / `embedded` / `stamped` counts then describe the partial work
  truthfully (e.g. "indexed 1000, embedded 240, cancelled"). No new outcome enum — the existing report
  already carries the counts.
- **Determinism preserved.** The cancel check is a pure function call at a deterministic checkpoint; no
  wall-clock, no randomness, no threads enter the core. A run that is never cancelled produces the same
  bytes as before. Tests stay model-free (fake embedder; assert a `Break` after N batches stops embedding,
  leaves Phase-2 edges complete, and a follow-up reindex finishes the rest — `incremental ≡ eventual full`).

This is the entire core change: **one callback return type + one `bool` on the report.** Everything else is
host and frontend.

**Cancel granularity = one embed batch (`ingest::EMBED_BATCH`).** The flag is checked *after* each batch
(a batch is written before the check, so no torn writes), so a cancel is observed within the time to embed
one batch — the forward pass is atomic and can't be interrupted mid-pass. Two levers keep that latency
small, both surfaced by dogfooding (2026-07-06) after the "Cancel sticks" report:

- **`EMBED_BATCH` was cut 32 → 16.** The tokenizer pads every chunk in a batch to the batch's longest, so
  an over-large batch runs the whole pass at the longest length. On a real variable-length vault, 16 was
  ~40% *faster* than 32 (less padding waste) while roughly halving cancel latency. See its doc-comment.
- **Candle is built optimized even in dev.** `just app` is `cargo tauri dev` — a **debug** host, where
  candle's forward pass is ~13× slower (a 123-chunk force-reindex was **4m38s**, so a batch ≈ 70s and Cancel
  appeared frozen). A workspace `[profile.dev.package."*"] opt-level = 3` (with our own `b2-*` crates pinned
  back to opt-0 for fast rebuilds) drops that to **~13s**, so a batch — and thus the cancel latency — is a
  couple of seconds. This is a build-profile fix, not a code change, but it is load-bearing for the desktop
  cancel UX; recorded here so it isn't lost.

## 4. The host — task lifecycle + IPC streaming (still a dumb adapter)

All threading/atomics/IPC live here; none of it is engine logic.

- **`reindex` becomes a streaming command.** Signature gains a `Channel<ReindexProgress>`:
  `reindex(state, on_event: Channel<ReindexProgress>)`. It opens the real-model vault (as today), calls
  `reindex_with_progress`, and in the progress closure (a) `on_event.send(p)` and (b) returns
  `ControlFlow::Break` iff the shared cancel flag is set — else `Continue`. Returns the final
  `ReindexReport` (with `cancelled`) so the Promise still resolves with a summary.
- **A cancel flag + single-in-flight guard in `AppState`.** Add
  `reindex_cancel: Arc<AtomicBool>` and a `reindex_running: AtomicBool` (or an `Option<…>` task slot behind
  the existing `Mutex`). Starting a reindex clears the cancel flag and sets `running`; finishing clears
  `running`. A second `reindex` while one runs is a clean no-op refusal (the UI also disables the button).
- **A `cancel_reindex` command.** Sets `reindex_cancel`. Because `reindex` runs on a *different* Tauri
  worker thread, `cancel_reindex` runs concurrently and the reindex closure observes the flag at its next
  batch boundary and breaks — cooperative, no thread-killing, no torn writes.
- **Vault switch cancels first.** `choose_vault` / `set_root` ([`commands.rs`](../../crates/b2-desktop/src/commands.rs))
  sets the cancel flag and waits for the in-flight run to wind down (to its next checkpoint) *before*
  repointing the root, so a reindex can never keep writing the old vault after the app has moved on.
- **Errors stay generic.** A mid-run failure (e.g. model unload) still funnels through `CmdError` →
  [`user_message`](../../crates/b2-desktop/src/error.rs); the channel simply stops. No sqlite/io/serde leaks
  to the webview.

**Why this is still "dumb":** the charter forbids *engine logic* in the host, not *infrastructure*. Task
spawn/track/cancel and IPC streaming are how the window *drives and interrupts* the façade — the same class
as the root `Mutex` and the OS dialog the charter already blesses. The command body is still "resolve →
call one façade op (`reindex_with_progress`) → serialize," with progress forwarded and a flag consulted;
there is no branching engine rule here.

## 5. Reliability & correctness invariants

The plan is only worth shipping if a cancelled index is never a broken one. The invariants:

1. **Partial index is consistent.** Phase 1/2 give every note chunks + FTS + edges before any embedding, so
   keyword search and the graph are *complete* at any cancel point; only vectors are partial (§1).
2. **`incremental ≡ eventual full`.** A cancelled run leaves already-embedded notes byte-identical to a
   fresh embed; a re-run embeds exactly the unfinished *notes* (`note_fully_embedded` is false for them).
   Vectors are tracked per note, not per chunk, so a note interrupted mid-embed re-embeds in full — at most
   one note's worth of redo. No corruption; nothing already-complete is recomputed.
3. **Determinism unchanged.** Non-cancel path is byte-identical to today; the cancel checkpoint is pure and
   deterministic (§3).
4. **Single in-flight per process**, and **vault-switch cancels first** (§4) — a reindex can never write a
   vault the app has left, and two reindexes can't race the same DB.
5. **Concurrent reads are safe.** Reads open their own connection; SQLite WAL permits them alongside the one
   reindex writer — this is what lets the UI stay live (§2).
6. **Cancel is cooperative, never a kill.** The worker thread finishes its current batch and returns
   normally; no thread is aborted mid-write, so there are no torn rows.
7. **Errors stay generic and actionable** to the webview (§4).

## 6. Build order

Each step is a provable increment (mirrors [desktop-ui-mvp.md](desktop-ui-mvp.md) §8 / the build spec).

- **Step 1 — Stream progress (no core change).** Wire the existing `reindex_with_progress` in
  `commands::reindex` behind a `Channel<ReindexProgress>`; the frontend replaces the global freeze with a
  **live progress bar/toast** (`embedding n/N · <path> · k chunks`, determinate once embedding starts) while
  the rest of the UI stays usable. Proves the stream end-to-end using only what the core already exposes.
- **Step 2 — Cancel.** Core: callback returns `ControlFlow<()>`, embed loop breaks on `Break`, Phase 2 still
  runs, `ReindexReport.cancelled` added (§3). Host: `reindex_cancel` flag + `cancel_reindex` command (§4).
  Frontend: a **Cancel** button on the progress affordance; on cancel, flash "Indexed partial — re-run to
  finish." Core tests: fake-embedder cancel-after-N-batches leaves a consistent, resumable index.
- **Step 3 — Concurrency hardening.** Single-in-flight guard; vault-switch cancels the in-flight run first;
  confirm reads stay live during a reindex (WAL). Thin host tests for the guard + switch-cancels-first.
- **Later (follow-on, §7).** Progressive/keyword-first ordering + auto-index-on-open UX.

Step 1 alone removes the "looks frozen, no progress" pain with zero core risk; Steps 2–3 deliver the
"pause/cancel" ask and the reliability guarantees.

## 7. Progressive enhancement (keyword-first) — the follow-on, and why it's cheap here

*(Was `tasks.md` Backlog → "Non-blocking embedding — progressive keyword-first index." Consolidated here.)*

**Goal:** let the user *use* the vault in a diminished (keyword-only) state while the semantic half fills in
behind a long cold index. §1's ordering means B2 is **already 90% there**: Phase 1 inserts all chunk text +
FTS up front, so BM25 keyword search works the moment projection finishes — long before embedding
completes. The remaining work is **UX, not architecture**:

- **Surface the diminished state honestly.** While embedding is in flight (or was cancelled), the UI already
  knows `semantic` availability (`vault_info`); extend it to a "semantic: N/M notes embedded" signal so
  search results can flag "keyword-only for now" instead of silently under-ranking. (Ties to the existing
  "never overstate the fake" honesty rule.)
- **Auto-detect an unindexed vault on open** and offer/start the background index immediately, so a
  first-run large vault is usable (keyword) in seconds and semantic arrives progressively — rather than
  waiting behind a manual **Reindex** click.
- **(Optional) order embedding by relevance** — e.g. embed the open note and its neighbors first — so the
  discovery pane lights up for what the user is looking at soonest.

Deferred behind §2–§6 because it depends on the progress + cancel plumbing landing first, and because the
consistent-partial-index guarantee (§5) is its prerequisite — both delivered above.

## 8. Cross-process / CLI background reindex — separate, still deferred

*(Was `tasks.md` Backlog → "Non-blocking embedding — background reindex + `b2 status`." Consolidated here.)*

For the **CLI** (stateless, one process per command), "reindex can't block" wants a *different* answer than
the desktop's in-process task: `b2 reindex` detaches and returns immediately; a separate process embeds
while `search` reads the index live (WAL permits one writer + concurrent readers across processes); `b2
status` reports progress. Cost: a background-process lifecycle + cross-process progress. **Not needed for
the desktop** (§2) and not in scope here; recorded so the desktop's in-process choice isn't mistaken for the
CLI's answer. A CLI **Ctrl-C cancel** becomes trivial once §3 lands (the CLI's closure returns `Break` on a
signal flag) — a small, separate follow-on.

**Also parked (a raw-speed lever, not a structural fix):** swap bge-base (768-dim) for bge-small (384-dim,
~3× faster) or a quantized/ONNX path behind the existing [`Embedder`](../../crates/b2-core/src/embed.rs)
seam — **measure retrieval quality (the eval) before changing the default**. Independent of everything above.

## 9. Open questions / deferred (not deciding here)

- **True pause/resume** — only if cancel + re-run proves insufficient in dogfooding (§2). Would need an
  explicit paused state and a resume entry point; the incremental machinery already does most of the work.
- **Progress denominator during Phase 1.** The bar is indeterminate during projection (fast) and determinate
  once embedding starts (`notes_embedded/notes_to_embed`, already in `ReindexProgress`). An upfront
  `plan_reindex` count would make it determinate from t=0 at the cost of a double scan — not worth it unless
  Phase 1 becomes visibly slow on huge vaults.
- **Auto-index-on-open** vs. keep it a manual action (§7) — a first-run UX call, taken when §7 starts.
- **Reranker / chunker changes** — orthogonal ([index-engine.md](../index-engine.md) §5, build spec §1.2).

## 10. Docs to mirror (doc-driven follow-ups)

Per the design-docs-are-source-of-truth discipline:

- [tasks.md](../tasks.md) — the Backlog "Non-blocking embedding — deferred approaches" bullet is **moved
  here** (§7–§8); leave a pointer, and promote "async, cancellable indexing" to an active work item tracking
  Steps 1→3. *(Done alongside this doc.)*
- [specs/desktop-ui-mvp.md](desktop-ui-mvp.md) — §4's `reindex` row is "exists"; add a pointer noting its
  async/progress/cancel behavior is specced here. *(Done alongside this doc.)*
- [README.md](../../README.md) / `docs/architecture.html` — no change until this ships; then note the desktop
  reindex is a cancellable background action.
