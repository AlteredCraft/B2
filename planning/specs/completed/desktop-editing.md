---
title: "B2 — Desktop editing: model-free saves over a byte-honest splice"
type: note
tags: [b2, ui, desktop, editing, codemirror, autosave, write, spec]
created: 2026-07-07
status: implemented
---

# B2 — Desktop editing: model-free saves over a byte-honest splice

> **The build spec for Step 4 of the desktop plan — in-editor body editing**
> ([desktop-ui-mvp.md](desktop-ui-mvp.md) §8, tracked as
> [#13](https://github.com/AlteredCraft/B2/issues/13)). The read-only MVP renders, navigates,
> discovers, and links; this doc adds the first **write** surface: edit a note's body in the app and
> have the save land Markdown-first through the façade. The shape §5 of the MVP spec fixed is held —
> writes route through a new façade op, a guard stops a save from clobbering an external edit,
> CodeMirror 6 over a byte-honest buffer — and this doc takes the five decisions that shape left open.
>
> **This doc owns:** the `Vault::write` façade op (the body splice, the revision guard, the model-free
> re-projection); the `NoteView.revision` read token; the host `write_note` command; the editor UX cut
> (edit mode, autosave, the save chain, the conflict bar, the trailing background embed); and the build
> order. **It does not own:** the engine invariant or the projection/embedding split
> ([index-engine.md](../../index-engine.md), [projection-embedding-split.md](projection-embedding-split.md));
> the editor-substrate decision ([desktop-ui-mvp.md](desktop-ui-mvp.md) §1); the
> thin-adapter charter ([`b2-desktop/CLAUDE.md`](../../../crates/b2-desktop/CLAUDE.md)).
> **Live-preview decorations** and **fs-watch reconciliation** stay deferred (§9 /
> [#14](https://github.com/AlteredCraft/B2/issues/14)).

## 0. Scope & ground rules

Editing is the highest-frequency op a knowledge tool has, and B2's premise is that the vault is *also*
edited by Obsidian/vim at any moment. So the save path has to be three things at once: **fast** (an
autosave world — saves are routine, not events), **safe against external writers** (never silently
clobber), and **honest to the file** (B2 authors exactly `b2id` and `relations:` in your frontmatter —
a body save must not grow a third write site). Every decision below follows from those three.

Held fixed, per the MVP spec (§5) and the workspace rules:

- **Writes are Markdown-first through the façade.** The host writes no file itself; `Vault::write`
  writes the `.md` and re-projects it, so `index = projection of (Markdown)` is never bypassed.
- **The core stays model-free, synchronous, deterministic.** The save path takes **no embedder** —
  it projects (chunks + FTS + edges) and leaves vectors to the embed pass, exactly the split
  [projection-embedding-split.md](projection-embedding-split.md) §4 built.
- **The host stays a dumb adapter.** `write_note` is deserialize → one façade call → serialize; the
  autosave/debounce/conflict *flow* lives in the frontend controller, where UI flow belongs.

**In scope:** `NoteView.revision`; `Vault::write`; a `db::open` busy-timeout hardening; the host
`write_note` command; CodeMirror 6 entering `ui/` behind an Edit toggle; autosave + the serialized
save chain + the conflict bar + the debounced trailing embed.
**Out of scope (later, §9):** live-preview decorations; fs-watch auto-reload (#14); a CLI `b2 write`;
frontmatter editing in-app; `updated:` stamping (decided *against*, §3 — listed so it isn't re-asked).

## 1. The problem, grounded in the code

| Layer | What exists today | The gap |
|---|---|---|
| **Note parse/serialize** ([`note.rs`](../../../crates/b2-core/src/note.rs)) | `ParsedNote` is byte-honest: raw text verbatim + exact frontmatter/body spans (`body_start`). Its only mutations are surgical inserts (`stamp_b2id`, `add_relation`). | No body mutation exists — but the spans make one a **splice**: `raw[..body_start] + new_body`. Frontmatter bytes are untouched by construction. |
| **Façade** ([`vault.rs`](../../../crates/b2-core/src/vault.rs)) | `read` returns `NoteView` (body + metadata, verbatim from disk). Write ops exist for *other* mutations: `add` (new file), `link` (frontmatter append), `move_note` (rename + rewrite) — all Markdown-first → re-project via [`ingest_file`](../../../crates/b2-core/src/ingest.rs). | No body-write op, and `NoteView` carries **no token** a later save could validate against — the app cannot detect "the file changed under me". |
| **Ingest** ([`ingest.rs`](../../../crates/b2-core/src/ingest.rs)) | The split factored the vault-wide passes (`project_vault` model-free / `embed_vault` model-bound), but the single-note path `ingest_file` still embeds **inline** (it takes the embedder). | No model-free single-note re-projection. Reusing `ingest_file` for save would put the model on the save path — latency per save, and editing would *hard-require* a provisioned model. |
| **Desktop** (`ui/` + host) | The note pane renders body → HTML (`marked`), with a view-source toggle; `render()` rebuilds panes by `innerHTML` swap. | No editor, no save command — and the innerHTML-swap render would destroy a live editor, so edit mode needs an explicit carve-out (§6). |

**Root cause, in one line:** the vault has every write path *except* the human's most common one — and
the read op doesn't capture what a safe save needs to check.

## 2. The enabling insights

Three existing properties make this slice small:

1. **The splice is already safe.** `ParsedNote` records the exact byte offset where the body starts, so
   "replace the body" is `raw[..body_start] ∥ new_body` — frontmatter (and its every byte, including
   keys B2 doesn't model) is preserved *by construction*, not by careful re-serialization. A note with
   no frontmatter has `body == whole file`, and the splice degenerates correctly to "replace the file".
2. **Projection is already model-free and convergent.** The save re-projects with the same machinery
   `project_vault` uses per note: re-chunk on body change (stale vectors cleared by
   [`replace_chunks`](../../../crates/b2-core/src/db.rs)), FTS via triggers, edges re-derived. Missing
   vectors are healed by **any** later embed pass, because the pending set is DB-derived
   (projection-embedding-split.md §2) — so a save needs no embedder and no coordination with one.
3. **Durability is the file write.** Markdown is the source of truth and the index is disposable — a
   save is durable the instant `fs::write` returns; everything after is derived state that self-heals.
   No fsync ceremony, no journal: the durability story *is* the two-tier model.

## 3. Decisions locked (2026-07-07)

| Concern | Locked choice | Rejected — and why |
|---|---|---|
| **Guard token** | **Content hash.** `Vault::read` captures `revision` = blake3 of the **raw file bytes**; `Vault::write` re-hashes the file and refuses with a typed [`Error::WriteConflict`] when it differs. Detects exactly the guarded thing — *the bytes you read are no longer the bytes on disk*. | **mtime** (§5's shorthand) — a proxy: false-positives on touch/identical-re-save, can miss sub-second edits; it detects "something happened", not "your read is stale". **Both** — mtime as prefilter buys nothing over hashing KB-sized files with blake3. |
| **Save semantics** | **Model-free save + trailing embed.** `Vault::write` = validate revision → splice body → `fs::write` → **project** the note (chunks + FTS + edges; stale vectors cleared) → return the new `revision`. The frontend debounces the existing guarded background `embed` behind the save chain. Saves are ~ms, work with **no model provisioned**, and coalesce under rapid saving; keyword search reflects the edit immediately, semantic/`similar` seconds later (the same keyword-first honesty the split shipped). | **Embed inline** (reuse `ingest_file`) — puts the model on the save path: warm-load + embed latency per save, and editing would *fail fast* without a provisioned model. Wrong dependency for the most basic op. |
| **Editor cut** | **Edit mode first.** The rendered view (clickable wikilinks — the discovery surface) stays the reading default; an **Edit** toggle swaps the body for CodeMirror 6 with Markdown syntax highlighting. Live-preview decorations are a follow-on (§9). | **Live-preview now** — front-loads a large decoration build onto the slice whose real risk is the write path, and a half-built live preview regresses *reading*. |
| **Save trigger** | **Autosave-on-idle** (~1s debounce) + flush on edit-mode exit / note switch / window blur; **Cmd+S** forces an immediate flush. No dirty-state UX, no lost work. A conflict **pauses autosave** and shows a bar (Reload / Keep mine) rather than re-firing. | **Explicit save only** — reintroduces the lost-work/dirty-prompt UX class, and under-uses the ms-fast model-free save chosen precisely because rapid saving matters. |
| **Frontmatter** | **Never touched.** The body splice is the entire write; B2 stays a co-author of exactly `b2id` + `relations:`. No `updated:` stamping — under autosave it would churn a line no human wrote into every diff. `mtime` + `revision` already track recency/change. | **Stamp `updated:` on save** — makes B2 a routine frontmatter author and a second write site inside a "body-only" op. If ever wanted, it's its own opt-in decision. |

**Last save wins — by construction.** `write` returns the new `revision`, and the editor **single-flights**
saves (one in flight, at most one trailing save holding the latest buffer). Saves from the session form a
serialized revision chain — each based on the last — so the guard never fires on your own saves and
*always* fires on an external writer's. That is "last save wins" in the only correct sense: within the
session, unconditionally; across writers, never silently.

## 4. The core seam — `revision`, `Vault::write`, `project_file`

- **`NoteView.revision: String`** — blake3 hex of the raw file bytes, captured by `Vault::read` from the
  same `fs::read_to_string` it already performs. (Distinct from `notes.body_hash`, which hashes the
  *body* for the ingest dirty-check; `revision` hashes the *whole file* so frontmatter-only external
  edits also conflict rather than being silently overwritten... they wouldn't be overwritten by a body
  splice — but a save based on a stale read would resurrect a body the user edited against outdated
  frontmatter context; conflict is the honest call.)
- **`ingest::project_file(conn, root, rel_path, idgen) -> Projected`** — the single-note, **model-free**
  re-projection: `project_note_and_chunks` (with the projection-pass predicate — no vector-state
  consultation, no `ensure_embedding_space`) + `project_edges`. The single-note sibling of
  `project_vault`, and the piece `Vault::write` re-projects with. `ingest_file` (the `add`/`link`/`mv`
  path) is untouched — it still embeds inline by design (those ops already require the model).
- **`Vault::write(note_ref, body, base_revision) -> WriteReport { path, revision }`**:
  1. resolve `note_ref` (path or `b2id`, as `read`) → the file; read raw bytes.
  2. **guard:** `blake3(raw) != base_revision` → `Err(Error::WriteConflict)`. A missing file surfaces
     as the read error it is (the note was deleted externally — reload-level news, not a splice target).
  3. **splice:** `raw[..body_start] ∥ body` (whole file when no frontmatter) → `fs::write`. The buffer
     is written **verbatim** — no newline normalization, no trimming; the file is the user's.
  4. **re-project** via `project_file` (stamps a missing `b2id` through the one ordinary path — the
     only case where bytes beyond the body change).
  5. return `revision` = blake3 of the **final on-disk bytes** (re-read after step 4, so a stamp is
     reflected) — the token the editor chains the next save on.
- **`Error::WriteConflict`** — a new `thiserror` variant; both adapters map it to one generic,
  actionable message ("This note changed on disk since it was opened. Reload the note, then reapply
  your edit."). No internals, per the error policy.
- **Hardening: `PRAGMA busy_timeout` in `db::open`.** A save can now race the background embed — two
  short-statement writers on one WAL database. Today no busy timeout is set, so contention surfaces as
  an immediate `SQLITE_BUSY` error instead of a few-ms wait. Set a modest timeout (e.g. 5s) at open;
  this also covers the pre-existing `link`-during-reindex race.

**Determinism preserved.** No wall-clock, no randomness: `revision` is a pure content hash, `indexed_at`
comes from SQLite as today, and the only id minted is a missing `b2id` through the injected `idgen`.

## 5. The host — one dumb command

- **`write_note(state, note, body, baseRevision) -> WriteReport`** — opens the **fake** vault
  (`open_vault(state, false)`: the save path is model-free by §3), calls `vault.write`, returns.
  Outside the reindex slot, like `project`: short, model-free, and safe against a racing vault switch
  for the same reason (§6 of the split spec — it writes the vault it captured at dispatch,
  idempotently).
- **Conflict recognition — the resolved mechanism (Step 2).** `WriteConflict` crosses the IPC
  boundary as its generic message, and the frontend recognizes it by **matching that exact string**
  — the minimal option (no change to the string-error contract every other command uses). The
  contract: the message is a **stable constant**, pinned host-side by the
  `write_conflict_is_generic_and_recognizable` test
  ([`commands.rs`](../../../crates/b2-desktop/src/commands.rs)) and mirrored as a constant in
  `ui/src/api.ts` — **change them together**. The frontend must match with `startsWith`, not
  equality: `B2_DEBUG` appends `\n(debug: …)` to every message.

## 6. The frontend — edit mode, the save chain, the conflict bar

**Editor lifecycle.** CodeMirror 6 enters `ui/` here (`@codemirror/state`/`view`/`commands`/
`language` + `@codemirror/lang-markdown` — the minimal set, no themes/frameworks). An **Edit** toggle
on the note pane (sibling of the view-source toggle) mounts a CM6 instance over `NoteView.body`.
One structural rule: **while editing, the note pane is the editor's** — `render()`'s innerHTML swap
must not rebuild it (same pattern as the persistent reindex-progress element: the editor lives outside
the swapped region, or the pane render short-circuits while `state.editing`). Destroying a live editor
mid-keystroke because a toast fired is the bug this rule exists to prevent.

**The save chain (single-flight, last save wins).**

```
buffer change → debounce ~1s → save(buffer):
  if a save is in flight → mark trailing (keep latest buffer only)
  else → api.writeNote(path, buffer, state.current.revision)
           ok → state.current.revision = report.revision; fire trailing if marked;
                schedule the trailing embed (below); refresh connections (edges may have changed)
           conflict → pause autosave; show the conflict bar
flush (immediate save, skipping the debounce): edit-toggle off · note switch · window blur · Cmd+S
```

**The conflict bar.** On `WriteConflict`: autosave pauses, a bar renders over the editor —
*"This note changed on disk."* — with two actions:
- **Reload** — discard the buffer: `read` the note fresh, remount the editor on the new body/revision.
- **Keep mine** — `read` fresh to obtain the *current* revision only, then `write(buffer, fresh_revision)`
  — an explicit, informed overwrite through the same guarded op (no `force` flag in the façade; the
  override *is* a fresh read + write, and a further external edit in that window still conflicts).

**The trailing embed.** After the save chain settles (~2s with no further saves), fire the existing
guarded background `embed` (same progress affordance, which for one note flashes briefly). If it's
refused (`ReindexInFlight` — a big embed is running), skip: the pending set is DB-derived, so the next
embed/reindex heals it (split spec §7.2). `similar`/semantic search for the edited note lag by those
seconds; keyword search and the graph are current from the save itself.

**What refreshes.** After a save: `explain`/connections (a body edit can add/remove edges) — the tree
does **not** need reloading (title lives in frontmatter, which a save never touches). After the
trailing embed: `refreshDiscovery()` so `similar` reflects the new vectors.

## 7. Reliability & correctness invariants

1. **Frontmatter bytes are invariant under save.** For any note and any buffer,
   `write` changes only bytes at/after `body_start` — except a missing-`b2id` stamp, which is B2's one
   always-allowed write and goes through the same path it always has.
2. **A save is durable at `fs::write`.** Projection follows in the same call, but any interruption
   after the file write leaves Markdown authoritative and the index behind — healed by the next
   `project`/save/reindex (`incremental ≡ full`).
3. **The revision chain never self-conflicts; external writes always conflict.** Serialized saves each
   carry the revision the previous save returned; any byte change B2 didn't make (or an out-of-band
   change between a read and a save) hashes differently and is refused, never merged, never clobbered.
4. **The save path needs no model.** `write` succeeds with no embedder provisioned; changed chunks
   simply join the DB-derived pending set, and *any* later embed pass fills them (convergence).
5. **A saved note equals a reindexed note.** After `write` + a completed embed, the note's rows
   (chunks, FTS, vectors-by-text, edges) are identical to what a full rebuild from the same Markdown
   produces — the save is a one-note slice of Flow ①, nothing bespoke.

## 8. Build order

Each step is a provable increment. **All three steps shipped 2026-07-07** (marked below, with
as-built notes); the doc is retained as the editing surface's design record.

### Step 1 — the core write op ✅ shipped 2026-07-07 (commit `887a595`)

**Goal.** Add the revision token to `read`, the model-free single-note projection, and `Vault::write`
with the splice + guard. Pure core; fake-embedder tests only.

> **As-built note:** the test list below shipped with one substitution —
> `write_stamps_a_missing_b2id_and_the_revision_reflects_it` became
> `write_returns_the_revision_of_the_final_on_disk_bytes`. An *indexed* note's file always carries
> its stamp (projection stamps on first sight), so `write`'s stamp arm is defensive rather than
> reachable; the test instead pins the contract the save chain hangs on (the returned revision
> hashes the final on-disk bytes). Everything else landed as written.

**Files & current-state anchors.**
- [`crates/b2-core/src/note.rs`](../../../crates/b2-core/src/note.rs) — add `ParsedNote::replace_body`
  (the splice; sibling of `stamp_b2id`/`add_relation`, same reparse-after-mutate discipline).
- [`crates/b2-core/src/ingest.rs`](../../../crates/b2-core/src/ingest.rs) — add `project_file` (compose
  `project_note_and_chunks(…, consult_vectors: false)` + `project_edges`); `ingest_file` unchanged.
- [`crates/b2-core/src/vault.rs`](../../../crates/b2-core/src/vault.rs) — `NoteView.revision`;
  `Vault::write`; `WriteReport`.
- [`crates/b2-core/src/error.rs`](../../../crates/b2-core/src/error.rs) — `WriteConflict` variant.
- [`crates/b2-core/src/db.rs`](../../../crates/b2-core/src/db.rs) — `busy_timeout` pragma in `open`.
- Adapters' error maps (`b2-cli` `user_message`, `b2-desktop` `error.rs`) gain the one generic
  conflict message (they match exhaustively-with-intent; a new variant must not fall into a debug arm).

**The moves (in order).**
1. `ParsedNote::replace_body(&mut self, body: &str)` — truncate at `body_start` (or clear, when no
   frontmatter) and append `body` verbatim; reparse to refresh spans/fields.
2. `ingest::project_file` — as §4; returns the note's `Projected` (b2id + stamped).
3. `NoteView.revision` — blake3 hex of the raw text `read` already loads.
4. `Vault::write(note_ref, body, base_revision)` — resolve → read → guard → splice → `fs::write` →
   `project_file` → re-read + hash → `WriteReport { path, revision }`.
5. `Error::WriteConflict` + the two adapter message arms; `busy_timeout` in `db::open`.

**New tests (model-free).**
- `write_replaces_body_and_preserves_frontmatter_bytes` — golden note: save a new body; the
  frontmatter region is byte-identical, the body is the buffer verbatim, `read` round-trips it.
- `write_conflicts_when_the_file_changed_on_disk` — read (capture revision) → mutate the file
  externally → `write` with the stale revision errors `WriteConflict` and the file is untouched;
  a fresh read + write then succeeds (the "Keep mine" path).
- `sequential_writes_chain_revisions_without_conflict` — write A (take returned revision) → write B
  with it → no conflict; final body is B (last save wins).
- `write_reprojects_keyword_graph_and_clears_stale_vectors` — on an embedded vault: save a changed
  body → chunks/FTS reflect the new text, edges re-derived from it, the note's chunks are in the
  missing-vector set; `Vault::embed` then fills exactly them (convergence, invariant 5).
- `write_needs_no_embedding_space` — save into a **projected-only** vault (no `chunks_vec`): succeeds,
  no space created (the model-free proof, mirroring the split's).
- `write_stamps_a_missing_b2id_and_the_revision_reflects_it` — save a b2id-less note: stamped via the
  ordinary path; the returned revision hashes the *final* on-disk bytes.

**Definition of done.** `cargo test -p b2-core` green (existing + the six above); no model deps added;
`Vault::write` takes no embedder and issues no query against `chunks_vec` beyond `replace_chunks`'s
existing stale-vector clear.

### Step 2 — host command ✅ shipped 2026-07-07

As specced: `write_note(note, body, base_revision)` + thin `write_note_impl` (fake vault, outside
the slot, registered in `main.rs`); conflict recognition resolved as the **stable string** (§5);
three thin tests (`write_note_saves_through_the_facade_and_chains_revisions`,
`write_conflict_is_generic_and_recognizable` — pins the exact message,
`write_note_runs_outside_the_reindex_slot`). The crate charter's embedder-wiring bullet now lists
`write_note` beside `project` as the two model-free write-side ops.

### Step 3 — the editor ✅ shipped 2026-07-07 (dogfood passed same day)

**Goal.** CodeMirror 6 edit mode over the note pane, with autosave-on-idle, the serialized save
chain, the conflict bar, and the trailing background embed — the §6 flow, verbatim. **Entirely in
`ui/`**: Steps 1–2 delivered the whole backend; no Rust changes. TypeScript + Vite, no framework
(vanilla TS is a locked choice), no test runner in `ui/` — verification is `npx tsc --noEmit`
(there is no `typecheck` npm script; `npm run build` = tsc + vite) plus the dogfood checklist below.

> **As-built notes.** Landed as specced, plus three protective touches learned wiring it:
> (1) the close-of-edit flush **refuses to unmount on failure** — a conflict (bar up) or a failed
> save keeps the editor and buffer alive and aborts the navigation that triggered it, so leaving
> edit mode can never drop unsaved work; (2) `commitLink` mid-edit flushes the buffer first and then
> adopts the post-link revision (B2's own `relations:` append changes the file bytes and would
> otherwise false-conflict the next autosave — skipped while the conflict bar is up, where adopting
> would let a resume clobber the external edit the bar guards); (3) `errText` moved from main.ts
> into api.ts beside `WRITE_CONFLICT_MESSAGE`/`isWriteConflict` — the recognizer is part of the IPC
> contract, so its pieces live together. Post-dogfood, two tests were added out of a coverage
> audit: `write_an_empty_body_and_recover` (b2-core — select-all-delete under autosave: zero-chunk
> projection + chain recovery) and a cross-language assert in
> `write_conflict_is_generic_and_recognizable` (b2-desktop) that reads `ui/src/api.ts` and pins the
> mirrored constant to the host's exact string, automating §5's "change them together".

**Files & current-state anchors.**
- [`ui/package.json`](../../../ui/package.json) — add the minimal CM6 set: `@codemirror/state`,
  `@codemirror/view`, `@codemirror/commands`, `@codemirror/language`, `@codemirror/lang-markdown`.
  No themes, no `codemirror` meta-package.
- [`ui/src/types.ts`](../../../ui/src/types.ts) — add `WriteReport { path, revision }`
  (`NoteView.revision` already landed with Step 1).
- [`ui/src/api.ts`](../../../ui/src/api.ts) — the one IPC seam. Add
  `writeNote(note, body, baseRevision): Promise<WriteReport>` →
  `invoke("write_note", { note, body, baseRevision })` — **Tauri v2 maps camelCase JS keys to the
  command's snake_case params automatically** (`baseRevision` → `base_revision`); do not hand-write
  snake_case keys. Also export the conflict recognizer here (it's part of the IPC contract):
  a `WRITE_CONFLICT_MESSAGE` constant equal to the host's exact string
  (`"This note changed on disk since it was opened. Reload the note, then reapply your edit."`)
  and `isWriteConflict(e): boolean` = `errText(e).startsWith(WRITE_CONFLICT_MESSAGE)` —
  **startsWith**, because `B2_DEBUG` appends `\n(debug: …)`. Pinned host-side by the
  `write_conflict_is_generic_and_recognizable` test; change both together (§5).
- [`ui/src/state.ts`](../../../ui/src/state.ts) — add the *renderable* editing state only:
  `editing: boolean` (edit mode owns the note pane) and `editConflict: boolean` (the bar is up).
  Debounce timers, the in-flight/trailing save flags, and the CM6 `EditorView` instance are
  **module-locals in main.ts**, not state — they never drive a render.
- [`ui/src/main.ts`](../../../ui/src/main.ts) — the controller: the edit-toggle action, editor
  mount/unmount, the save chain, flush points, Cmd+S, the conflict-bar actions, the trailing embed.
  `doReindex`/`paintReindex` are the house pattern for "background work + targeted repaint".
- [`ui/src/render.ts`](../../../ui/src/render.ts) — `noteBarHtml` builds the note pane's top bar; the
  `</>` view-source toggle (`data-toggle-source`, in `.note-bar-head`) is the sibling the **Edit**
  toggle sits next to. `notePaneHtml` is the builder that must **not** run while editing.
- [`ui/style.css`](../../../ui/style.css) — editor host + conflict bar styles.

**The critical structural rule — the render carve-out.** `render()` rebuilds every pane by
`innerHTML` swap, and it runs on *every* state change — including `flash()`'s toast, which triggers
a render immediately **and again ~4.5s later** when the toast clears. Any of those would destroy a
live `EditorView` mid-keystroke. The rule: **while `state.editing`, `render()` must not touch
`#note-pane`** — short-circuit that one pane's swap (the other panes keep rendering; the tree,
side pane, and toasts stay live). The editor chrome (Done button, conflict bar, editor host) is
built once at mount by main.ts, owned imperatively until exit — the same "persistent element +
targeted repaint" precedent as the reindex-progress affordance. Repaint the conflict bar with a
small `paintEditor()`-style helper, never a pane rebuild.

**The moves (in order).**
1. Deps + `WriteReport` type + `api.writeNote` + `WRITE_CONFLICT_MESSAGE`/`isWriteConflict`.
2. The **Edit** toggle in `noteBarHtml` (sibling of `data-toggle-source`; disabled when
   `state.loading`). Entering edit mode sets `state.editing`, renders once (which now skips the
   note pane), then mounts CM6 into `#note-pane`: doc = `state.current.body`, extensions =
   `markdown()`, `history()`, `keymap` (default + history), and an `updateListener` whose
   `docChanged` schedules the autosave.
3. The save chain, exactly §6's diagram: module-locals `saveInFlight`, `trailingDirty`,
   `autosavePaused`. `scheduleAutosave()` = debounce ~1s → `saveNow()`. `saveNow()` single-flights:
   if a save is in flight, set `trailingDirty` and return; else call
   `api.writeNote(state.current.path, buffer, state.current.revision)`. On success: update
   `state.current.revision = report.revision` **and `state.current.body` = the saved buffer** (so
   exiting edit mode renders the saved text with no re-read), fire the trailing save if dirty,
   schedule the trailing embed, and refresh connections (`api.explain` — a body edit can change
   edges). On `isWriteConflict`: set `autosavePaused` + `state.editConflict`, paint the bar.
   **Autosave success is silent** — no toast per save (a toast triggers renders and trains the
   user to ignore toasts); only conflicts and real errors surface.
4. Flush points — an immediate `saveNow()` (skipping the debounce) on: edit-toggle off (Done),
   `openNote` (flush **before** switching), window `blur`, and Cmd+S (`keydown` handler,
   `preventDefault`, only while editing). Exiting edit mode: flush, unmount (`view.destroy()`),
   clear `state.editing`, render.
5. The conflict bar (part of the persistent editor chrome): *"This note changed on disk."* with
   **Reload** (discard buffer: `api.readNote` fresh → remount the editor on the new body/revision →
   clear conflict, resume autosave) and **Keep mine** (`api.readNote` fresh for the *revision only*
   → `saveNow()` with the buffer against it → clear conflict, resume). No force flag exists — the
   override *is* a fresh read + write, and a further external edit in that window still conflicts.
6. The trailing embed: after the chain settles (~2s with no saves and none in flight), run the
   guarded background embed. Client-side, skip if `state.reindexing` (already running); otherwise
   reuse `doReindex`'s embed invocation shape (set `reindexing`, stream progress into
   `paintReindex`, clear in `finally` — factor a small shared helper rather than duplicating). If
   the host refuses (`ReindexInFlight` race), **skip silently** — the pending set is DB-derived, so
   the next embed/reindex heals it (split spec §7.2). After it completes, `refreshDiscovery()` so
   `similar` reflects the new vectors.
7. Styles: the editor host fills the note pane below the (kept) top bar; the conflict bar is a
   fixed strip above the editor; match the existing pane chrome.

**Interactions to keep correct (learned building Steps 1–2).**
- A save during a background embed is *by design* fine (slot-free command + `busy_timeout` in
  `db::open`) — do not serialize saves behind `state.reindexing`.
- A vault switch mid-edit: `switchVault` resets `state.current` — flush before it proceeds (hook
  the same flush as `openNote`), then let its reset also clear `state.editing` (unmount first).
- The wikilink/`data-open` click delegation stays active while editing (tree + side pane are
  live) — that's the `openNote` flush point doing its job, not a bug.
- `state.current.revision` is the **only** base a save may present; never re-hash or cache
  elsewhere — one source of truth for the chain.

**Definition of done.** `npx tsc --noEmit` and `npm run build` clean; `cargo test -p b2-desktop`
still green (no Rust changes expected); and the manual dogfood passes: edit in-app while the same
note is open in vim/Obsidian — an external save conflicts (never clobbers), **Reload** and **Keep
mine** both behave; rapid typing produces ms saves and **one** trailing embed; a save mid-reindex
works; editing works under `B2_EMBEDDER=fake` **and** with no model provisioned (semantic panes
degrade honestly); exiting edit mode shows the saved text; the tree, search, toasts, and discovery
all stay live while editing (the carve-out holds).

## 9. Open questions / deferred

- **Live-preview decorations** (desktop-ui-mvp §1.2's document feel) — pure frontend, slots into the
  same pane; sized as its own effort. **Tracked:
  [#30](https://github.com/AlteredCraft/B2/issues/30); specced 2026-07-08 →
  [../desktop-live-preview.md](../desktop-live-preview.md).**
- **fs-watch auto-reload** (Step 5) — replaces "stale until conflict" with live reconciliation; the
  conflict bar remains the fallback for unwatchable cases. **Tracked:
  [#14](https://github.com/AlteredCraft/B2/issues/14).**
- **CLI `b2 write`** — the façade op is adapter-ready; add the command the day a CLI/agent need
  appears (piped stdin → body). Not this slice.
- **Frontmatter editing in-app** — a different write site with different safety questions (YAML
  validity, B2's managed keys); the drawer stays read-only for now.

## 10. Docs to mirror (doc-driven follow-ups)

- [tasks.md](../../tasks.md) — point the Active "Step 4" item here (it currently cites the MVP spec §8).
  *(Done alongside this doc.)*
- [desktop-ui-mvp.md](desktop-ui-mvp.md) — §5 sketched editing ("mtime guard");
  add a pointer that this doc executes it, with the guard upgraded to a content-hash revision.
- [index-engine.md](../../index-engine.md) / [index-engine-build.md](index-engine-build.md) —
  no invariant change; note the new single-note model-free projection entry point if Flow ① prose
  warrants it when Step 1 lands.
- [`b2-desktop/CLAUDE.md`](../../../crates/b2-desktop/CLAUDE.md) — `write_note` joins `project` in the
  "opens the fake vault" column once Step 2 ships.
