# CLAUDE.md — `b2-desktop`

Guidance for Claude Code (and humans) working in this crate. It **inherits** the workspace rules in the
[root CLAUDE.md](../../CLAUDE.md) (idiomatic Rust, error policy, determinism, user-facing-error policy) and
**adds** the one rule that defines this crate's existence: **stay a dumb adapter.** The full rationale and
the MVP plan live in [planning/specs/completed/desktop-ui-mvp.md](../../planning/specs/completed/desktop-ui-mvp.md); this file
is the enforceable in-crate rule.

## What this crate is

`b2-desktop` is the **Tauri host** for B2's desktop UI — the **GUI sibling of [`b2-cli`](../b2-cli)**. It is
a *second* dumb adapter over the [`Vault`](../b2-core/src/vault.rs) façade: it owns a window, wires the
embedder, exposes `#[tauri::command]` handlers, and hands the [`ui/`](../../ui) frontend a way to call the
core. That is all it is.

The frontend (HTML/JS/CSS + CodeMirror, under `ui/`) is a *separate toolchain*; the contract between it and
this crate is the **command set** plus the frontend's `ui/src/api.ts` seam. Keep that command set minimal.

## The one rule: hold no engine logic

**Every command is: deserialize args → call one `Vault` method → serialize the result.** Nothing else.

- If a handler wants a branch, a loop, a computation, or a rule — that logic belongs in
  [`b2-core`](../b2-core) **behind the façade**. Add a `Vault` op; do not add logic here.
- If the MVP needs a capability the façade lacks (e.g. reading a note's body for the left pane), the fix is
  a **new façade method**, not a workaround in the host. Add ops when a command needs them; never pre-build
  a broad surface.
- Reach for the façade's **existing `--json` view types** as command return values (`NeighborView`,
  `ExplainView`, `ReindexReport`, …). Tauri serializes them straight to the webview. **Do not** define a
  parallel set of DTOs.

### Why thin — the argument, not just the edict

The `Vault` façade is B2's **one typed contract**; every UI is a *client* of it. Keeping the host thin is
what makes that architecture pay off:

- **No behavioral drift.** Two adapters (CLI + desktop) over one contract can't diverge — a fix in the core
  fixes both. The moment logic leaks into this crate, the GUI and CLI become two implementations of the same
  behavior, and they *will* drift.
- **Inherited tests.** A thin host means the façade's existing suite already covers the behavior; this crate
  needs only a few per-command tests (args in → right façade call → view out). Logic here would need its own
  parallel tests that the CLI already has.
- **The promise stays true.** [vision-and-scope.md](../../planning/vision-and-scope.md) says the GUI is "a
  second dumb adapter over the same contract, inheriting every test the CLI bought." That is only true while
  this crate stays dumb. Thinness is not tidiness; it's the load-bearing property.

**Smell test:** if a `#[tauri::command]` body is longer than "parse, call, return," or if you're reaching
for a `b2-core` internal that isn't on `Vault`, stop — the missing piece is a façade method.

## Dependency direction (one-way, always)

`b2-desktop` → depends on → `b2-core` (and `b2-embed`). **Never the reverse.** `b2-core` must never learn
about Tauri, webviews, or the UI. This is what keeps the fast core suite (`cargo test -p b2-core`) free of
Tauri/webview deps — the same way `b2-embed`'s candle deps stay out of it. If you find yourself wanting to
add a UI concern to `b2-core`, that's the signal you're putting logic in the wrong layer.

## Wiring conventions (mirror the CLI)

- **Embedder injection like [`b2-cli`](../b2-cli):** pure reads open with the fake
  ([`Vault::open`](../b2-core/src/vault.rs)); anything that embeds a query or writes vectors (`search`,
  `link`'s re-projection, `embed`) opens the real model
  ([`Vault::open_with_embedder`](../b2-core/src/vault.rs)) and fails fast with the "run `b2 init`"
  message if it's absent. Two write-side ops are deliberately **model-free** and open the fake:
  `project` — the model-free half of a reindex
  ([specs/completed/projection-embedding-split.md](../../planning/specs/completed/projection-embedding-split.md) §6),
  so the first tree paint never waits on a model load — and `write_note` — the save path
  ([specs/completed/desktop-editing.md](../../planning/specs/completed/desktop-editing.md) §3), so editing works with no
  model provisioned and saved chunks are healed by the trailing background embed.
- **Errors stay generic to the webview.** Map façade errors to user-facing, actionable messages exactly as
  the CLI funnels through `user_message` in [`b2-cli/src/main.rs`](../b2-cli/src/main.rs) — **never** leak
  sqlite/io/serde internals into the UI. Use a `thiserror` enum for this crate's errors (matched → mapped),
  never `anyhow` for anything the UI presents. `B2_DEBUG` opts into developer detail. The **full** internal
  detail is *always* logged server-side to stderr (`log_internal`, called from `CmdError`'s `Serialize`
  impl — the one boundary every command error crosses to the webview), so a failed command is diagnosable
  under `tauri dev` without `B2_DEBUG`: the log carries everything, the webview only the generic string.
  (Root CLAUDE.md error policy + parent `Projects/CLAUDE.md` logging policy.)
- **Determinism unchanged.** Push no wall-clock or randomness into `b2-core`; timestamps come from the
  façade clock (`now()` / `today()`), same as the CLI.

## Transport

**Tauri IPC only** — the frontend `invoke`s these commands. This crate runs **no HTTP server**. An
HTTP/`serve` transport is a *different, deferred adapter* for a *different need* (remote / browser /
agent-over-HTTP); it does not belong here. See [the spec §1/§9](../../planning/specs/completed/desktop-ui-mvp.md).
