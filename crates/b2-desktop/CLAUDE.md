# CLAUDE.md — `b2-desktop`

Guidance for Claude Code (and humans) working in this crate. It **inherits** the workspace rules in the
[root CLAUDE.md](../../CLAUDE.md) (idiomatic Rust, error policy, determinism, user-facing-error policy) and
**adds** the one rule that defines this crate's existence: **stay a dumb adapter.** The full rationale and
the read→discover→link→edit MVP shipped (its build history is in git); this file
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
- **The promise stays true.** [invariants.md](../../docs/design/invariants.md) (E3) says the GUI is "a
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
  message if it's absent. Four write-side ops are deliberately **model-free** and open the fake:
  `project` — the model-free half of a reindex
  ([#15](https://github.com/AlteredCraft/B2/issues/15)),
  so the first tree paint never waits on a model load — `write_note` — the save path
  ([#13](https://github.com/AlteredCraft/B2/issues/13)), so editing works with no
  model provisioned and saved chunks are healed by the trailing background embed —
  `create_note` — the tree's New-note action, the same posture as the save path (the new
  note is projected immediately; its vectors fill on the next embed pass) — and
  `write_frontmatter` — the drawer's frontmatter save
  ([#79](https://github.com/AlteredCraft/B2/issues/79)): an unchanged body keeps its
  chunks and vectors, so nothing ever re-embeds on that path.
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
- **Structured logging installed here, like the CLI.** `logging::init_logging` (called first in `main`)
  is the desktop's opt-in `B2_LOG`/`B2_DEBUG`/`B2_LOG_FILE` subscriber — the GUI sibling of the CLI's, same
  JSONL shape (b2-core only emits; the subscriber + clock live in the adapter, keeping the core
  wall-clock-free). Two host-driven differences, both documented in `logging.rs`: a `tracing-appender`
  **non-blocking** writer (this process is long-lived + multi-threaded, so log I/O must not block the
  GUI/reindex threads), and the *implied* default scoped to `b2=debug` (not bare `debug`) so Tauri/wry/hyper
  tracing stays out of the file. `main` must hold the returned `WorkerGuard` for the whole run.

## The keyboard contract (invariant K1)

[invariants.md](../../docs/design/invariants.md) **K1** — *B2 is fully operable from the keyboard; the
mouse is an accelerator, never a requirement* — names this file as its elaboration home. This is it.
K1 governs the **GUI**: the `b2` CLI satisfies it by nature, so everything below is about
`b2-desktop` + [`ui/`](../../ui) ([#78](https://github.com/AlteredCraft/B2/issues/78)).

**The rule, in one line: no action reachable only by pointer.** If a gesture exists only as a click,
a right-click, or a drag, it is a bug — not a missing nicety.

### The four obligations

Every new surface owes all four. They are cheap while you're building it and expensive to retrofit.

1. **Reachable.** A focusable control in a sensible tab order, or a documented chord. A `<div>` you
   attach a click handler to is a mouse-only control; make it a `<button>`, or give it `tabindex` +
   `role` + a key that activates it. Graph nodes are the worked example: SVG `<g>` elements with
   `tabindex="0"` + `role="button"`, whose ⏎/Space handler **dispatches the same click** the mouse
   sends, so there is one activation path rather than two to keep in step.
2. **Visible.** `:focus-visible` rings, in `style.css`'s focus block. A control you can reach but
   can't see you've reached is not reachable. Never `outline: none` without putting an equal-or-louder
   affordance in its place (the pane gutters' accent line is the one legitimate case). **A row in a
   pane that repaints rings on plain `:focus`** instead — `.tree-row`, and discovery's
   `[data-side-row]`. Not a style preference: WebKit grants `:focus-visible` to a script-focused node
   only by inheriting it from the *currently* focused one, and an `innerHTML` swap leaves that as
   `<body>`, so a row re-focused by key after a repaint comes back focused but **ringless** until the
   user's next keystroke re-arms the heuristic — which reads as the ring lagging one key behind the
   arrows (the reported bug). `:focus` is the state the repaint actually restores, and it costs
   nothing here because WebKit never focuses a button on click (below), so those rows only ever hold
   focus from a key.
3. **Escapable.** An overlay takes focus on open, **traps ⇥** while it's up, and **restores focus** to
   whatever opened it on close. `Escape` dismisses innermost-first; `Enter` confirms. All of this is
   one hook — `syncOverlayFocus()` in `main.ts`, called at the end of every `render()` and acting only
   on the open/close *edge*, so a toast timer's repaint never steals focus mid-⇥.
4. **Discoverable.** A chord nobody can find is not kept. Add the row to `ui/src/shortcuts.ts` (the
   `?` sheet) **in the same change** that wires it, and put the chord in the control's `title` — and,
   where the action lives in a menu, beside the menu item, which is where a keyboard user learns the
   shortcut that lets them skip the menu next time.

### Where the pieces live

- **`ui/src/treenav.ts`** — the file tree's pure logic, and the reason it's a separate module: the
  paint (`render.ts`) and the arrow keys **must** agree on row order down to the last tie-break, so
  the sort and the flatten live in one tested place. A tree you can arrow through in a different
  order than you can see is worse than no arrows at all. The tree follows the ARIA `tree` pattern —
  `role="tree"`/`treeitem`, `aria-level`, a **roving `tabindex`** (one Tab stop for the whole tree, not
  one per file: a 1500-note vault is not a tab sequence), ↑↓ between visible rows, →← to expand/enter
  and collapse/exit, Home/End, and first-letter typeahead.
- **`ui/src/sidenav.ts`** — treenav's sibling for the **discovery pane**: the same ARIA `tree` pattern
  over section heads and cards (↑↓, →← to fold or step in/out, Home/End, a roving `tabindex`), and the
  row *keys* the paint and the arrows share. One thing genuinely differs, and it's why this is its own
  module rather than a parameter of treenav's: a tree row expands to reveal *child rows*, a card
  expands to reveal *its own body*, so "foldable" and "has child rows" come apart. A card row is a
  `<div>` (it contains the open `<button>`; nested buttons are illegal), so ⏎/Space there **dispatch
  that button's click** — the graph nodes' rule again, one activation path.
- **`ui/src/shortcuts.ts`** — the `?` sheet's one table. Modifiers as macOS glyphs (⌘ ⇧ ⌫ ⏎); keys
  macOS spells out in its own menus stay words (Esc, Tab, Space, Home/End).
- **`ui/src/main.ts`** — the wiring: the tree's and the side pane's own `keydown` (bound to each pane,
  so they answer before the global chords), the global chord table, and the focus plumbing under
  *"keyboard: focus plumbing"*.

### Two things that bite

- **A repaint destroys focus — and it has already happened by the time you look.** `innerHTML` swaps
  are how this UI renders, so anything holding focus is gone after one, and the keyboard user is
  silently ejected to `<body>` by an unrelated toast or watcher pulse. Two consequences, and the
  second is the one that bites: (a) restore by *identity that outlives the swap*, never by element —
  `paintTree` re-focuses the tree's row **by path**, `paintSide` the discovery row **by row key**, and
  `captureReturnFocus` returns a thunk (tree path → row key → id → element). A pane that swaps its
  `innerHTML` with no such restoration is a pane that ejects the keyboard on every toast, which is
  exactly how discovery behaved until it got one; (b) you cannot read `document.activeElement` at the end of `render()` to
  learn what triggered an overlay, because `render()` swapped `#modal-root`/`#menu-root` on the way
  there and the answer is already `<body>`. Hence `lastFocused`, tracked continuously from `focusin`
  — destroying a focused node fires no `focusin`, so the last value is still the trigger. Any control
  that has to be *returned to* therefore needs a stable `id` (`#settings-shortcuts` is one).
- **WebKit doesn't focus a button on click.** So a `focusin` listener alone won't see a mouse user's
  selection, and the keyboard's idea of "where I am" would drift from the mouse's. The click
  delegation sets `state.treeFocus` and `state.sideFocus` explicitly for this reason — and it is the
  same fact that makes the `:focus` ring above safe.

## Transport

**Tauri IPC only** — the frontend `invoke`s these commands. This crate runs **no HTTP server**. An
HTTP/`serve` transport is a *different, deferred adapter* for a *different need* (remote / browser /
agent-over-HTTP); it does not belong here. See [#24](https://github.com/AlteredCraft/B2/issues/24).
