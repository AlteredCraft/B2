---
title: "B2 — Desktop UI MVP: the first adapter with pixels"
type: note
tags: [b2, ui, desktop, tauri, codemirror, adapter, spec]
created: 2026-07-05
status: draft
---

# B2 — Desktop UI MVP: the first adapter with pixels

> **The build spec for B2's first graphical surface.** The headless-first phase is done — the
> [`Vault`](../../crates/b2-core/src/vault.rs) façade is the one typed contract and
> [`b2-cli`](../../crates/b2-cli) is a dumb adapter over it. This doc specs the **second dumb adapter**:
> a **Tauri** desktop app with a **CodeMirror** frontend, realizing the "when the GUI finally arrives it's
> a second dumb adapter over the same contract" promise in
> [vision-and-scope.md](../vision-and-scope.md#approach-headless-first-the-ui-comes-last).
>
> **This doc owns:** the delivery-vehicle and editor-substrate decisions (and *why* the rejected
> alternatives lost), the repo layout, the adapter discipline the new crate must uphold, the MVP surface,
> the transport, the editing / external-edit-reconciliation plan, and the security posture. **It does not
> own:** the engine or the façade contract ([data-model.md](../data-model.md),
> [index-engine.md](../index-engine.md), [specs/index-engine-build.md](index-engine-build.md)); the `ui/`
> framework choice (deferred — §9); or packaging/distribution (deferred — §9). The thin-adapter directives
> the host crate must follow live in its own charter,
> [`crates/b2-desktop/CLAUDE.md`](../../crates/b2-desktop/CLAUDE.md); this doc is *why*, that file is the
> in-crate *rule*.

## 0. Scope & ground rules

B2 was built **headless-first on purpose** ([vision-and-scope.md](../vision-and-scope.md#approach-headless-first-the-ui-comes-last)):
push all capability and testability into a core exercised through one typed façade, ship a CLI as the
"UI before the UI," and defer the GUI as long as possible so progress is measured in green scenarios, not
screens. That phase paid off — the engine, the façade, and the CLI all ship and are covered by a fast,
deterministic, model-free suite. **This doc opens the next phase: the first real UI.**

The stance that makes it cheap and safe is that **the UI adds no new architecture** — it reuses the one
seam B2 already committed to. The `Vault` façade is the contract; every UI is a *client* of it. The CLI is
that client for the terminal; the desktop app is that client for a window. So the "distinction" between
core and UI is not something to invent — it already exists as the **adapter/core boundary**, and this doc
extends it by exactly one adapter plus one frontend toolchain.

**What this doc does not re-decide:** the engine invariant (`index = projection of (Markdown)`), the
storage tiers, the relation vocabulary, or the embedder seam. The UI observes and drives those; it never
owns them.

## 1. Decisions locked (2026-07-05)

| Concern | Locked choice | Rejected — and why |
|---|---|---|
| **Delivery vehicle** | **Tauri desktop app** (OS webview + Rust host in-process) | **TUI** — the MVP is a *rendered-document* surface; a terminal grid can't render long-form Markdown, images, or clickable links meaningfully. **Browser + `b2 serve`** — viable, but no native filesystem-watch and "a tab, not an app"; skipped straight to Tauri (prior good experience, and editing is a near fast-follow that wants native fs-watch). **Native Rust GUI** (egui/iced/slint) — true single binary, but no off-the-shelf CodeMirror-grade Markdown editor; you'd hand-build the whole document surface. |
| **Editor substrate** | **CodeMirror 6** (the buffer *is* the Markdown text; live-preview decorations for a document feel) | **ProseMirror / Tiptap / Wordgard** — the whole *node-tree* lineage. Their canonical model is a structured tree and Markdown is a serialization at the edges, so anything not modeled in the schema (YAML frontmatter, `[[path\|title]]` wikilinks, `relations:`) round-trips lossily and re-emits noisy diffs. That violates principle #1 ("the files *are* the format; export is a no-op"). **Wordgard** was evaluated specifically (Haverbeke's ProseMirror rethink) — it borrows CodeMirror's *change representation* but keeps a schema-constrained *node tree*, so it inherits the same fidelity problem. CodeMirror keeps the text canonical (and is what Obsidian's editor is built on). |
| **Transport** | **Tauri IPC** — the frontend `invoke`s `#[tauri::command]` handlers that call the façade | **HTTP (`b2 serve`)** — a *different* adapter for a *different* need (remote / browser / agent-over-HTTP). Building both now is the real over-complication: two transports to keep in sync for one user. `serve` is deferred, not cancelled (§9); the `ui/src/api.ts` seam (§3) keeps adding it later to ~one file. |

### 1.1 The principle-#5 reconciliation (single binary)

Principle #5 is "distributable as a single binary — download and run, no toolchain, no install ritual."
Tauri produces a **per-platform app bundle**, not a literal single file — but it uses the **OS webview**
(WebKit / WebView2 / WebKitGTK), so it bundles *no* browser engine and stays a small native artifact. The
*intent* of #5 — "download and run, nothing to install" — holds; "single file" relaxes to "single
per-platform bundle," an explicit, recorded refinement. And the `b2` CLI remains a literal single binary,
so #5 is still satisfied to the letter by the CLI while the GUI honors its spirit.

### 1.2 The editor's "document feel" without WYSIWYG

CodeMirror keeps the buffer as literal Markdown, but that does **not** mean the user stares at raw
asterisks. CM6 **live-preview decorations** (hide markup, render inline emphasis/links/headings in place)
give a rendered, document-like editing experience while the underlying buffer stays byte-honest — this is
exactly how Obsidian's Live Preview works, on CodeMirror. So the choice is *not* "raw text vs. WYSIWYG";
it's "byte-honest buffer that *renders* like a document" vs. "a tree that *serializes* to Markdown." B2
takes the former.

## 2. Repo layout & the adapter discipline

Three layers. The first two already exist; the UI adds to the second and introduces the third.

- **Engine (pure Rust, the contract):** [`b2-core`](../../crates/b2-core) (the [`Vault`](../../crates/b2-core/src/vault.rs) façade + engine) and [`b2-embed`](../../crates/b2-embed) (the embedder seam). *Unchanged.*
- **Adapters (dumb Rust clients of the façade):** [`b2-cli`](../../crates/b2-cli) (terminal) and **`b2-desktop`** (Tauri host). *One new crate.*
- **Presentation (JS toolchain):** **`ui/`** — Vite + CodeMirror, talks to `b2-desktop` over Tauri IPC. *New.*

```
B2/
├─ Cargo.toml               # workspace — add crates/b2-desktop to members
├─ crates/
│  ├─ b2-core/              # façade + engine            (unchanged)
│  ├─ b2-embed/             # embedder seam              (unchanged)
│  ├─ b2-cli/               # `b2` binary, dumb adapter  (unchanged)
│  └─ b2-desktop/           # NEW — Tauri host, dumb adapter over Vault
│     ├─ CLAUDE.md          #   the thin-adapter charter (the in-crate rule)
│     ├─ Cargo.toml         #   deps: tauri, b2-core, b2-embed   (NEVER the reverse)
│     ├─ tauri.conf.json    #   frontendDist → ../../ui/dist ; devUrl → http://localhost:5173
│     ├─ build.rs · icons/
│     └─ src/
│        ├─ main.rs         #   tauri::Builder + embedder wiring
│        └─ commands.rs     #   #[tauri::command] fns → Vault (thin)
└─ ui/                      # NEW — frontend (its own package.json, Vite, CodeMirror)
   ├─ package.json · index.html · vite.config.ts
   └─ src/
      ├─ api.ts             #   the ONE IPC seam — every invoke() lives here
      └─ …                  #   views/components (framework TBD — §9)
```

**Why this shape:**

- **`crates/` stays 100% Rust/Cargo.** The Tauri host is a normal workspace member; its only non-Rust
  files are Tauri's own config/icons, which is unavoidable and self-contained. The JS toolchain lives
  behind one top-level `ui/` boundary, so npm never pollutes the Rust tree. The `../../ui` path
  indirection in `tauri.conf.json` is a one-time, standard bit of Tauri-in-a-workspace setup.
- **Name the crate for its role, not its tech.** `b2-desktop` (parallel to `b2-cli`), never `b2-tauri` —
  if the shell is ever swapped the name must not lie.
- **The dependency arrow points one way.** `b2-desktop` depends on `b2-core` (and `b2-embed`); `b2-core`
  **never** learns about Tauri or the UI. This keeps the fast core suite (`cargo test -p b2-core`)
  untouched — Tauri/webview deps can't leak into it — exactly as `b2-embed`'s candle deps stay out of it
  today.
- **`.gitignore` gains** `ui/node_modules/`, `ui/dist/`, and Tauri's generated artifacts.

The rules the host crate must follow to stay a *dumb* adapter live in
[`crates/b2-desktop/CLAUDE.md`](../../crates/b2-desktop/CLAUDE.md) — §3 summarizes the argument; that file
is the enforceable in-crate charter.

## 3. The one seam that matters — a dumb adapter over the façade

The single discipline that makes all of this safe: **`b2-desktop` holds no engine logic.** It is the GUI
sibling of the CLI, and the CLI's rule ([root CLAUDE.md](../../CLAUDE.md): the CLI is "a *dumb* adapter …
holds no engine logic") applies verbatim.

- **Each command is deserialize → call `Vault` → serialize.** A `#[tauri::command]` parses its args, calls
  one façade method, and returns the result. If a handler grows a branch, a loop, or a rule, that logic
  belongs in `b2-core` behind the façade — add a façade op, not host logic. Two adapters (CLI + desktop)
  over one contract means the GUI and CLI **cannot drift in behavior**, and the desktop app inherits every
  test the façade already bought. The moment logic leaks into the host, that promise breaks and there are
  two implementations to test.
- **Reuse the `--json` view types as IPC payloads.** The façade already returns `Serialize` views for the
  CLI's `--json` mode (`NeighborView`, `ExplainView`, `ReindexReport`, …). Tauri serializes those directly
  to the webview, so the IPC contract is **nearly free** — the same leverage `serve` would have had. Do
  **not** invent a parallel DTO crate (if hand-written TS types ever churn, `ts-rs`/`tauri-specta` generate
  them — a later lever, §9, not a now-decision).
- **The frontend has its own façade: `ui/src/api.ts`.** Every `invoke()` call lives in this one module —
  the presentation-side mirror of the `Vault` seam. It keeps the UI testable without booting Tauri
  (mock the module), and keeps a future `serve`/HTTP transport swap to ~one file. This *one* seam is worth
  having; anything more is speculative.
- **Embedder wiring mirrors the CLI.** The host picks and injects the embedder exactly as `b2-cli` does:
  pure reads open with the fake ([`Vault::open`](../../crates/b2-core/src/vault.rs)); anything that
  re-embeds (a body write, `link`'s re-projection, `embed`) opens the real model
  ([`Vault::open_with_embedder`](../../crates/b2-core/src/vault.rs)), and fails fast with the "run `b2 init`"
  message if the model is absent — same contract as `reindex`/`link` in the CLI. (`project` — the
  model-free half of a reindex — opens the fake, so the first paint never waits on a model load;
  [projection-embedding-split.md](completed/projection-embedding-split.md) §6.)
- **Errors stay generic to the webview.** Map façade errors to user-facing, actionable messages the same
  way the CLI funnels through `user_message` — never leak sqlite/io/serde internals into the UI. `B2_DEBUG`
  opts into detail for the developer, matching the repo-wide logging policy.

## 4. The MVP surface

**The first screen is the vision, made visual: a document on the left, its unlinked-but-similar notes on
the right.** This is [connection discovery](../vision-and-scope.md#capability-areas-the-surface-high-level)
(capability area 5 — "the reason B2 exists"), lifted out of the terminal into a side-by-side, point-and-click
surface where the human — the precision gate — can *read both notes at once* before committing a link.

| UI affordance | Façade op | Status |
|---|---|---|
| Open & render a note (left pane) — Markdown → HTML, clickable in-app wikilinks | **`Vault::read(note)` → body + metadata** | **NEW — the only core addition** |
| File-tree pane — the vault as collapsible folders; click a file to open it | **`Vault::list_notes()` → `Vec<NoteSummary>`** (path-ordered, no body; the tree is folded from the flat list in `ui/`) | added post-MVP |
| Frontmatter drawer (top of note pane) — the note's raw YAML, verbatim & collapsible | **`Vault::read` extended: `NoteView.frontmatter`** (the byte-honest block between the fences, `None` if absent — not a re-serialization, so `relations:`/`aliases:`/unmodeled keys show as written) | added post-MVP |
| View-source toggle (`</>`, top-right of note pane) — flip the body between rendered Markdown and raw source | **no new op** — presentation-only, reuses `NoteView.body` (the verbatim block `Vault::read` already returns) | added post-MVP |
| Switch vault (folder icon, top bar) — native folder picker; repoints the app at another vault | **no façade op** — host state: `choose_vault` swaps `AppState`'s runtime root (root resolution is the host's job, main.rs), then every later command opens over the new root | added post-MVP |
| Related pane — semantically nearest *unlinked* notes | `Vault::similar` | exists |
| Related pane — hybrid keyword+semantic+graph search | `Vault::search` | exists |
| Backlinks / typed edges with their "why" (in/out) | `Vault::explain` / `neighbors` | exists |
| Commit a typed relation (verb picker) → frontmatter | `Vault::link` | exists |
| Index / refresh action + state | **`Vault::project` then `Vault::embed`** (the frontend sequences them; `plan_reindex` unchanged) | **project-then-embed (2026-07-07)** — the model-free projection paints the tree + keyword search first, then embedding streams behind as the cancellable background action ([specs/completed/projection-embedding-split.md](completed/projection-embedding-split.md) §6; progress/cancel plumbing per [specs/completed/async-indexing.md](completed/async-indexing.md)) |

**The one new façade op** is a read: fetch a note's raw body + metadata to render the left pane
(`Vault::read` / `get_note`). Everything else the MVP needs already exists. This honors the façade rule —
"add operations when a command needs them; do not pre-build a broad surface" (root CLAUDE.md). (The frontend
*could* read the `.md` off disk directly since it knows the vault path, but routing it through the façade
keeps the one-typed-contract discipline honest and centralizes path/`b2id` resolution.)

**MVP is read-only-first.** The opening cut renders, navigates, discovers, and links — no body editing yet.
Rationale: a read/navigate/discover/link surface has *zero* stale-write risk (it only renders; `link`
appends one frontmatter line through the façade and re-projects immediately), so it dodges the external-edit
reconciliation problem entirely (§5) and gets the discovery loop in front of us fastest. CodeMirror body
editing is the **immediate fast-follow**, not the MVP.

**Explicit MVP non-goals** (each is a later phase or a different concern): body editing (§5, fast-follow);
filesystem-watch reconciliation (§5, fast-follow); graph visualization; multi-vault; the `serve`/HTTP
transport (§1); packaging, signing, and distribution (§9).

## 5. Editing & external-edit reconciliation (the fast-follow, specced now)

Editing is a near fast-follow, so its shape is fixed here even though it's out of the MVP cut.

- **Writes route through the façade, Markdown-first.** A save is `Vault::write` / `update_body(note,
  markdown)` → re-project the note (re-embed it, repair backlinks) → return. Every save therefore gets
  `b2id` stamping and edge re-derivation for free, exactly like `add` / `link` / `mv`. The host writes no
  file directly; the façade does, so the invariant `index = projection of (Markdown)` is never bypassed.
- **The reconciliation problem is real and B2-specific.** B2's whole premise is that you *also* edit the
  vault in Obsidian/vim. So the app must handle "the file changed under me." Tauri's native
  **filesystem-watch** answers this directly: watch the vault, and on external change re-read and reload
  the affected note (and re-run `similar`/`explain` for the open note). This is a primary reason the
  desktop shell was chosen over a browser tab, which has no native fs-watch.
- **Interim guard if editing lands before fs-watch:** an `mtime` check on save — if the file changed on
  disk since it was opened, prompt to reload rather than clobber. Covers the common case cheaply until the
  watch lands.
- **Editor UX:** CodeMirror 6 with live-preview decorations (§1.2) for the document feel over a byte-honest
  Markdown buffer.

## 6. Security posture

- **Tauri IPC is in-process — no open port.** Choosing IPC over an HTTP `serve` is also a security win:
  there is no localhost server for another process (or a stray page in another browser tab) to reach. The
  attack surface `serve` would create (a writable HTTP endpoint on the vault) simply doesn't exist.
- **Least-privilege capabilities.** Use Tauri v2 **capabilities/permissions** to scope the app to exactly
  the commands and filesystem paths it needs (the active vault), nothing more.
- **Locked-down webview CSP; all assets bundled.** The frontend loads only bundled local assets — no remote
  scripts, styles, or fonts — matching local-first and keeping the webview's content trustworthy.
- **If `serve` is ever added (§9):** bind `127.0.0.1` only (never `0.0.0.0`) and require a same-origin /
  token guard on any write route. Recorded here so the future adapter starts safe.

## 7. Build, test, CI

- **The fast core suite is untouched.** `b2-desktop` only *depends on* `b2-core`, so Tauri/webview deps
  never enter `cargo test -p b2-core`. The desktop build is a separate, heavier job — the same shape
  `b2-embed`'s candle build already is — and stays out of the fast gate.
- **Thinness *is* the test strategy.** Because the host carries no logic, the façade's existing tests cover
  the behavior; `b2-desktop` needs only a few thin per-command tests (args deserialize → correct façade
  call → view serializes). Frontend logic is tested against the mockable `ui/src/api.ts` seam, no Tauri
  runtime required.
- **Determinism unchanged.** No wall-clock or randomness is pushed into the core; timestamps come from the
  façade clock (`now()` / `today()`), same as the CLI.
- **`just` recipes** grow to cover the app: e.g. `just ui-dev` (Vite dev server), `just app` (Tauri dev),
  `just app-build` (bundle). The core recipes (`test`, `check`, `eval`) are unaffected.

## 8. Build order

**Prerequisites for a fresh start:** a Rust toolchain (already required), **Node.js + npm** (for the `ui/`
Vite frontend), and the **Tauri v2 CLI** (`cargo install tauri-cli --locked`, or `npm i -D @tauri-apps/cli`).
The workspace `members` list in the root `Cargo.toml` is **explicit, not a glob**, so Step 0 adds
`"crates/b2-desktop"` to it once the crate exists (adding it before then breaks `cargo` metadata). The first
thing Step 0 resolves is the `ui/` framework (§9).

Sequenced like the [build spec](index-engine-build.md)'s step 0→N — each step is a provable increment.

- **Step 0 — Scaffold & wiring.** Add `crates/b2-desktop` (Tauri host) + `ui/` (Vite + CodeMirror) to the
  workspace. An empty window boots and an `invoke('ping')` round-trips through a trivial command. Proves the
  Rust↔JS seam end-to-end before any real surface.
- **Step 1 — Read a note.** Add the one new façade op `Vault::read` + a `read_note` command; the left pane
  renders a note (Markdown → HTML) with clickable wikilinks that navigate in-app.
- **Step 2 — The related pane.** `similar` + `search` commands; results render with snippets; click a result
  to open it. Backlinks via `explain`.
- **Step 3 — Commit a link (read-only MVP done).** `link` command + a verb picker over the closed relation
  core; committing writes frontmatter through the façade and the related pane updates. The discovery loop —
  read → discover → link — is now visual end-to-end.
- **Step 4 — Editing (fast-follow).** CodeMirror editing + `Vault::write` + save-through-façade + the `mtime`
  guard (§5).
- **Step 5 — Reconciliation (fast-follow).** Native fs-watch → auto-reload on external edits.
- **Later.** Packaging / signing / notarization / distribution; a `serve` adapter *if* a remote or
  agent-over-HTTP need appears.

## 9. Open questions / deferred (not deciding here)

- **The `ui/` framework** (Svelte / Solid / React, or none). A `ui/`-internal choice that does not touch
  this layout — its own decision, taken when Step 0 starts.
- **TS ↔ Rust type sharing.** Hand-write the handful of view types first; adopt `ts-rs` / `tauri-specta`
  codegen only if they churn. No speculative codegen.
- **Packaging & distribution.** Per-OS bundles, code signing, macOS notarization — principle #5's
  "download and run" endgame, deferred until the surface earns it.
- **The `serve`/HTTP adapter.** Kept as a documented future adapter, added the day a concrete remote /
  browser / agent-over-HTTP need exists (the `api.ts` seam keeps it cheap). Not built alongside Tauri.
- **Graph visualization, multi-vault, sync.** Out of scope, per the vision-and-scope deferred list.

## 10. Docs to mirror (doc-driven follow-ups)

Per the design-docs-are-source-of-truth discipline, this decision should be reflected outward once the
shape is agreed:

- [vision-and-scope.md](../vision-and-scope.md) — the "headless-first / the UI comes last" and "GUI
  deferred" language is now being *acted on*; add a "Decisions locked (2026-07-05)" entry pointing here and
  reframe the GUI from "deferred, not now" to "in progress via Tauri."
- [tasks.md](../tasks.md) — promote "GUI — deferred" from Backlog to an active work item tracking Steps 0→3.
- [README.md](../../README.md) — add `b2-desktop` (and `ui/`) to the crate map / docs table once the crate
  lands.
- `docs/architecture.html` — "three crates" → four once `b2-desktop` ships.
