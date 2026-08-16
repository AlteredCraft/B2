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
- **Chat provider injection, the embedder's sibling** (`src/chat.rs`, GH #155). `ask` picks a provider the
  way the CLI's `open_llm` does — `B2_LLM=fake` forces the deterministic `FakeLlm`, else the configured
  OpenAI-compatible endpoint — and resolution itself stays in `b2_llm::LlmConfig::from_env`, with this
  host's Settings laid over it exactly as a CLI flag would be. Two differences from the embedder are
  deliberate. **Chat carries no index identity** (contrast M2): nothing it produces is stored, so changing
  the chat model costs no reindex, and the endpoint + model persist beside the remembered vault rather than
  in the vault. And **the API key lives in the Keychain, under an env override** (GH #176, `src/keychain.rs`):
  a **Cloud models** configuration (M5) needs a bearer token, and this host remembers one in the macOS
  Keychain — encrypted at rest, ACL'd per application — rather than in a file. A secret B2 stores is a
  secret B2 is responsible for; a plaintext file in Application Support is not where that responsibility
  gets taken, and neither is `security add-generic-password -w`, which would put the token in `ps`. Three
  rules follow, and each has a test. `B2_LLM_API_KEY` **wins** over the stored key, so a shell can point one
  launch at another provider or decline to have B2 keep a secret at all. A store that refuses **must not
  break chat**: the key stays in force for the run and the configuration reads `ApiKeySource::Session`, which
  is the pre-#176 behavior as a fallback rather than a failure. And the key still never crosses back to the
  webview: the status view carries `api_key_source`, never the key.
- **Structured logging installed here, like the CLI.** `logging::init_logging` (called first in `main`)
  is the desktop's opt-in `B2_LOG`/`B2_DEBUG`/`B2_LOG_FILE` subscriber — the GUI sibling of the CLI's, same
  JSONL shape (b2-core only emits; the subscriber + clock live in the adapter, keeping the core
  wall-clock-free). One host-driven difference, documented in `logging.rs`: a `tracing-appender`
  **non-blocking** writer (this process is long-lived + multi-threaded, so log I/O must not block the
  GUI/reindex threads). The *implied* default filter — `b2=debug`, not bare `debug`, so foreign
  `tracing`/`log` records stay out of the file — is now the rule in **both** adapters (GH #154).
  `main` must hold the returned `WorkerGuard` for the whole run.

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
   affordance in its place (the pane gutters' accent line is the one legitimate case). **Anything a
   pane re-focuses across its own repaint rings on plain `:focus`** instead — `.tree-row`,
   discovery's `[data-side-row]`, the graph's `.gnode`, and the pane chrome restored by `id`. Not a
   style preference: WebKit grants `:focus-visible` to a script-focused node only by inheriting it
   from the *currently* focused one, and an `innerHTML` swap leaves that as `<body>`, so a row
   re-focused by key after a repaint comes back focused but **ringless** until the user's next
   keystroke re-arms the heuristic — which reads as the ring lagging one key behind the
   arrows (the reported bug). `:focus` is the state the repaint actually restores, and it costs
   nothing on a button because WebKit never focuses one on click (below), so those rows and chips
   only ever hold focus from a key. A graph node is the one non-button in the list — a `<g>` *is*
   focused by a click — but every node click re-anchors the scene, leaves the graph, or opens the
   link modal, so the ring never outlives the frame it appeared in.
3. **Escapable.** An overlay takes focus on open, **traps ⇥** while it's up, and **restores focus** to
   whatever opened it on close. `Escape` dismisses innermost-first; `Enter` confirms. All of this is
   one hook — `syncOverlayFocus()` in `main.ts`, called at the end of every `render()` and acting only
   on the open/close *edge*, so a toast timer's repaint never steals focus mid-⇥.
4. **Discoverable.** A chord nobody can find is not kept. A chord is **declared once**, in
   `ui/src/bindings.ts` — id, label, chord, scope — and everything else derives: the handler matches
   it with `isBound(e, id)`, the editor's keymap takes it from `chordFor(id)`, and the `?` sheet
   (`ui/src/shortcuts.ts`) names the id rather than spelling the chord. So add the binding, then add
   its row to the sheet: `shortcuts.test.ts` fails on a binding no row documents, which is what makes
   "in the same change" a rule the suite keeps rather than one you have to remember. Put the chord in
   the control's `title` too — projected with `displayKeys([id])`, never typed out — and, where the
   action lives in a menu, beside the menu item, which is where a keyboard user learns the shortcut
   that lets them skip the menu next time.

   The `label` is not decoration: since [#121](https://github.com/AlteredCraft/B2/issues/121) that
   sheet **edits** the table as well as printing it, so your chord appears as a button the user can
   re-record, and the label is what the recorder and the conflict messages call your command. A row's
   prose covers several commands at once ("Focus the files, the note, or discovery"); the label names
   exactly one. If your chord genuinely isn't B2's to hand out — ⏎ in a text field, ⏎ on a dialog's
   default button, the ⏎/Space a `<button>` would answer to — set `fixed` to the reason instead, and
   expect to argue for it: `bindings.test.ts` pins that set.

   Three things the registry will tell you before a user does. `conflicts()` fails the suite if your
   chord already means something else in the same scope, so pick the scope honestly — it's what
   separates "⏎ commits *this* dialog" from a clash. `editorkeys.test.ts` compares B2's chords
   against CodeMirror's ~100 stock bindings, so if your chord needs to work while the note is being
   edited, that check is what proves the editor isn't already using it. And `menukeys.test.ts`
   compares them against the **menu bar's** (below) — the one clash no scope and no ordering can
   win, because the keystroke never reaches the webview. All three now answer to the *user* as well:
   `ui/src/keymap.ts` asks the same functions about the table a candidate rebinding would produce,
   so the honest scope you picked is also what keeps the recorder from crying wolf at them.

### Where the pieces live

- **`ui/src/treenav.ts`** — the file tree's pure logic, and the reason it's a separate module: the
  paint (`render.ts`) and the arrow keys **must** agree on row order down to the last tie-break, so
  the sort and the flatten live in one tested place. A tree you can arrow through in a different
  order than you can see is worse than no arrows at all. The tree follows the ARIA `tree` pattern —
  `role="tree"`/`treeitem`, `aria-level`, a **roving `tabindex`** (one Tab stop for the whole tree, not
  one per file: a 1500-note vault is not a tab sequence), ↑↓ between visible rows, →← to expand/enter
  and collapse/exit, Home/End, and first-letter typeahead. Since #121 the *keys* those moves answer to
  are the registry's, not this module's: `arrowMove` switches on a binding id (`tree.row.next`), and
  `treeNavFor(e)` is the lookup — so the arrows are rebindable and the checkers can see them, while
  command → move stays here where the pattern is documented. `sidenav.ts` and `settingstabs.ts` moved
  the same way.
- **`ui/src/sidenav.ts`** — treenav's sibling for the **discovery pane**: the same ARIA `tree` pattern
  over section heads and cards (↑↓, →← to fold or step in/out, Home/End, a roving `tabindex`), and the
  row *keys* the paint and the arrows share. One thing genuinely differs, and it's why this is its own
  module rather than a parameter of treenav's: a tree row expands to reveal *child rows*, a card
  expands to reveal *its own body*, so "foldable" and "has child rows" come apart. A card row is a
  `<div>` (it contains the open `<button>`; nested buttons are illegal), so ⏎/Space there **dispatch
  that button's click** — the graph nodes' rule again, one activation path.
- **`ui/src/shortcuts.ts`** — the one chord table, rendered as Settings' **Keyboard** section (`?`
  opens the dialog there). Modifiers as macOS glyphs (⌘ ⇧ ⌫ ⏎); keys macOS spells out in its own
  menus stay words (Esc, Tab, Space, Home/End). A row's chords are *chips*, one per chord: a chip
  naming a movable B2 command paints as a `<button>` that opens the recorder, and everything else —
  the platform's own keys, the menu bar's, a `fixed` chord — paints as a `<kbd>`. The affordance is
  the contract, so what looks pressable is exactly what B2 can move.
- **`ui/src/keymap.ts` + `ui/src/recorder.ts`** — the customizable keyboard
  ([#121](https://github.com/AlteredCraft/B2/issues/121)). `keymap.ts` is the algebra (lay the user's
  rebindings over `DEFAULT_BINDINGS`, install the result with `setActiveBindings`) and the judgement
  (`chordProblems` runs all four checkers over the *candidate* table: a same-scope clash or a menu-bar
  chord refuses, a shadow or a CodeMirror overlap advises). `recorder.ts` turns a keydown into a chord
  spelled the way the table spells it, and reads the one thing no table can: **silence**. A chord that
  produces no keydown was taken upstream — macOS or another app — and since nothing can enumerate
  another process's hotkeys, that observation stays true across OS updates and third-party installs in
  a way a static table can't. It under-reports rather than false-alarms, deliberately; the UI copy says
  so. (That reasoning is why [#122](https://github.com/AlteredCraft/B2/issues/122)'s Carbon
  `CopySymbolicHotKeys` table was closed rather than built.) The layout lives in `localStorage` beside
  the theme — a viewing choice, never vault state, so it never reaches the host — and `adoptOverrides`
  re-judges every stored entry on load, because a hand-edited store must not be able to install a
  keyboard you can't use to fix itself.
- **`src/menu.rs` + `ui/src/menukeys.ts`** — the **menu bar**, and the app's third keyboard
  ([#119](https://github.com/AlteredCraft/B2/issues/119)). Set no menu and Tauri installs
  `Menu::default()`, whose dozen accelerators (⌘Q ⌘W ⌘M ⌘H ⌥⌘H ⌘Z ⇧⌘Z ⌘X ⌘C ⌘V ⌘A ⌃⌘F) are live in
  the window and enumerable by nobody — and AppKit dispatches a menu key equivalent inside
  `NSApplication.sendEvent`, *before* the key window's responder chain, so they never reach the
  webview's `keydown` and the registry cannot observe them either. So `menu.rs` declares the menu as
  a table (the items stay `PredefinedMenuItem`s: the Edit menu is load-bearing — it is what routes
  cut/copy/paste into the webview), the `menu_chords` command exports it, and `menukeys.ts` mirrors
  it for the two jobs a runtime fetch can't do — the suite's gate, and the sheet's first paint. The
  mirror is checked against the host at every boot (`menuDrift`), the way `WRITE_CONFLICT_MESSAGE`
  and `VAULT_CHANGED_EVENT` are pinned across the same seam: **change the two together.** Note what
  the gate is *not*: `conflicts()` asks a same-scope question, and scope buys nothing here — a menu
  accelerator is taken before the webview is consulted, so `menuOverlaps` compares across every
  scope.
- **`ui/src/chat.ts`** — the chat pane's pure logic, and sidenav.ts's client rather than its rival
  ([#155](https://github.com/AlteredCraft/B2/issues/155)). Chat is the right column's *third mode*, so its
  transcript is emitted as the same `SideRow` shape discovery's cards are, and `sideRows` delegates here
  when the pane is in that mode — which is what makes the whole ARIA `tree` walk, the roving tabstop and
  (the load-bearing one) `paintSide`'s focus-restoration-by-row-key apply to a surface that repaints on
  every streamed token, for free. The pane is where it is *because* of the citations: a citation opens its
  note in the centre pane, and chat in the centre would make every citation a choice between the answer and
  the evidence. Two things it deliberately does not own: what a citation opens (that is `data-open` in the
  markup, so the mouse and ⏎ share one path) and the streaming paint (`paintChatStream`, main.ts — a full
  render per token would swap the pane's `innerHTML` a hundred times an answer and eject the keyboard with
  every one).
- **`ui/src/icons.ts`** — the icon registry, and the reason a fourth obligation didn't need
  adding above: an icon here is `aria-hidden` with no opt-out, so it is always *beside* an
  accessible name and never instead of one. It maps a **meaning** (`resourceIcon(class)`,
  `foldChevron(open)`, `folderIcon(open)`) onto a Bootstrap Icons glyph, so "what does a PDF
  look like in B2" has one answer rather than one per pane — the failure the two systems it
  replaced both had, in opposite ways: Unicode text markers (`▶ ▼`, `▣ ▶ ▤ ◇ ≡ ◆`) that named
  no file type and rendered at the system font's whim, and inline SVG pasted per call site
  with its own stroke width. `ui/scripts/gen-icons.ts` vendors the named subset into
  `icons.gen.ts` and, in `--check` mode, is the first thing `npm test` runs.
- **`ui/src/settingstabs.ts`** — the Settings dialog's rail: the section list and its ARIA `tabs`
  moves (↑↓ with wrap, Home/End; ⌃Tab cycles from anywhere in the dialog). Its own module for
  treenav.ts's reason — the paint and the arrows must agree on order, so the order is defined once
  and both read it. Adding a section is a row there plus a panel in `render.ts`.
- **`ui/src/main.ts`** — the wiring: the tree's and the side pane's own `keydown` (bound to each pane,
  so they answer before the global chords), the global chord table, and the focus plumbing under
  *"keyboard: focus plumbing"*.

### Two things that bite

- **A repaint destroys focus — and it has already happened by the time you look.** `innerHTML` swaps
  are how this UI renders, so anything holding focus is gone after one, and the keyboard user is
  silently ejected to `<body>` by an unrelated toast or watcher pulse. Two consequences, and the
  second is the one that bites: (a) restore by *identity that outlives the swap*, never by element —
  `paintTree` re-focuses the tree's row **by path**; `capturePaneFocus` is the note and side panes'
  one mechanism (discovery row key → graph node's scene id → stable `id` → the pane itself, which is
  the honest floor for a wikilink or a backlink card that has no durable identity);
  `captureModalFocus` is the overlay layer's, by `id` alone — a modal control that can't be named
  after the swap is one a repaint must leave alone rather than guess a replacement for; and
  `captureReturnFocus` returns the overlay's own thunk (tree path → row key → node id → id →
  element). A pane that swaps its `innerHTML` with no such restoration is a pane that ejects the
  keyboard on every toast, which is exactly how discovery behaved until it got one — and how the
  note pane behaved until [#91](https://github.com/AlteredCraft/B2/issues/91). **A control that has
  to be restored therefore needs a durable identity in the markup**: a stable `id` on pane chrome
  (the note bar's chips, search mode's `clear`) and on *every* Settings control (the rail's tabs,
  the theme segments, the panel itself), `data-gnode` on a graph node; (b) you cannot read
  `document.activeElement` at the end of `render()` to learn what triggered an overlay, because
  `render()` swapped `#modal-root`/`#menu-root` on the way there and the answer is already
  `<body>`. Hence `lastFocused`, tracked continuously from `focusin` — destroying a focused node
  fires no `focusin`, so the last value is still the trigger. The identity rule from (a) is what it
  returns *by*, which is why the Settings button `#open-settings` has an id.

  The overlay layer is also **memoized** (`paintModal`), for the reason the panes are plus one of
  its own: a modal's *typed* state lives only in the DOM, so identical HTML must not swap the link
  modal's half-written explanation away because a toast fired.
- **WebKit doesn't focus a button on click.** So a `focusin` listener alone won't see a mouse user's
  selection, and the keyboard's idea of "where I am" would drift from the mouse's. The click
  delegation sets `state.treeFocus` and `state.sideFocus` explicitly for this reason — and it is the
  same fact that makes the `:focus` ring above safe.

## The rendering trust boundary (invariant E5)

[invariants.md](../../docs/design/invariants.md) **E5** — *note content is untrusted input; rendering is a
trust boundary* — names this file as its elaboration home, the way K1 does above. E5 governs the **GUI**:
the `b2` CLI prints text, so nothing there parses into a document
([#77](https://github.com/AlteredCraft/B2/issues/77)).

**The threat model, in one line: authorship is not trust.** A `.md` is a file, and files travel — a shared
or synced vault, a note someone sent you, a web clip, a repo you cloned to read. So "it's the user's own
local Markdown" is not a security property, and a note body is **hostile input** that B2 renders into its
own window.

**The rule: exactly one Markdown→HTML path, and it sanitizes.** `renderMarkdown` ([`ui/src/render.ts`](../../ui/src/render.ts))
is that path, and the sanitizer ([`ui/src/sanitize.ts`](../../ui/src/sanitize.ts), DOMPurify) is wired into
it as `marked`'s `postprocess` hook rather than called by each caller. That placement is the whole design:
the reading view, live preview's `TableWidget`, and any surface added tomorrow are covered **by
construction**, and sanitizing is the *last* thing that touches the HTML — nothing, not even B2's own
`md-table` wrapper, is spliced in afterwards. `sanitize.ts` documents each of the four points where the
config tightens DOMPurify's document-friendly default (`data-*` except the wikilink's `data-target`,
`<form>`, `id`/`name`), and each one is a claim about what a *note* may do, not a Markdown feature removed.

**CSP is the second layer, never the only one.** The webview policy
([`tauri.conf.json`](tauri.conf.json)) — `default-src 'self'; img-src 'self' data:; style-src 'self'
'unsafe-inline'` — does neutralize script execution, and it owns remote loads. It is not a substitute:
`form-action` doesn't fall back to `default-src` (so a note-authored `<form action="https://…">` posts
outward under a policy that looks locked down), CSP says nothing about DOM clobbering of the ids this UI
re-finds its controls by, and a future relaxation would silently re-open everything. Two independent
layers, and neither one is allowed to be the argument for skipping the other.

**A note's links never navigate the webview.** The window holds the whole application, so a click that
follows `https://…` in place replaces B2 with a web page — no address bar, no back button, nothing to
press. That is a *reachability* failure before it is a security one, and the fix is the same OS handoff
the resource card already makes: `ui/src/links.ts` decides which hrefs are the system's (`http`, `https`,
`mailto` — GFM autolinks a bare email into the third), main.ts's click delegation cancels the click, and
`open_external` hands the URL to the user's browser. The host re-checks the scheme against its own copy of
that list — the frontend's is *routing*, the host's is the **refusal**, and the refusal is what matters:
`open` launches whatever app has registered a scheme, so an unfiltered handoff would let a `.md` name a
program to run. Two layers again, and the schemes are spelled on both sides of the seam, so change them
together (`only_web_links_are_openable` in `src/commands.rs`, `links.test.ts` in the UI). ⏎ on a focused
anchor dispatches a click, so the keyboard and the mouse share the one activation path (K1).

**A model's answer is note content that has been through a model** ([#155](https://github.com/AlteredCraft/B2/issues/155)).
Flow ④ feeds retrieved passages — which anyone could have authored — to an LLM and renders what comes back,
so a chat answer is untrusted by both arguments at once: it is *derived from* hostile input, and it is
generated text that nobody reviewed. It therefore takes the same one path every note body takes
(`renderMarkdown`, sanitized), and its **citations never become links**: a citation is a `data-open` button
that opens the note in-app, so there is no `href` for the webview to follow and no second activation path to
keep in step with the mouse's. The *streaming* half is stricter still and deliberately so — tokens are
written into the live element as `textContent` (`paintChatStream` in `ui/src/main.ts`), so a half-arrived
answer is never parsed as markup at all.

**What a new surface owes:** anything that reaches `innerHTML` gets its content from `renderMarkdown` (note
bodies) or from `escapeHtml` (every value B2 interpolates into chrome — titles, paths, snippets). There is
no third option; a raw string built from vault data and assigned to `innerHTML` is the bug this invariant
exists to prevent.

## Drag and drop: one setting, two gestures, and a navigation the window can't survive

`tauri.conf.json` sets **`dragDropEnabled: false`** on the window, and both drag gestures depend on it —
in opposite directions, which is why the setting is worth its own section.

With Tauri's native drag-drop interception **on** (the default), wry consumes drag events for its own
file-drop channel and the DOM never sees `dragover`/`drop` on macOS: `dragstart` fires, but no drop zone
ever activates. So the in-app gesture — dragging a **tree row** to move it — requires the setting to be
off. That is the whole reason it is off.

Turning it off hands every *external* drag to WebKit, whose default for a dropped file is to **navigate
to it**. In a browser that's a tab; here the webview *is* the application, so the app is replaced by a
rendering of the file with no address bar and no way back (the same failure a note's `https://` link
would cause — hence `links.ts`). An unhandled file drop is therefore not a missing feature but a
window-destroying bug, and `ui/src/main.ts` cancels `dragover`/`drop` for **every** file drag, wherever
it lands, before deciding whether it can do anything with it.

What it does with one is **import** it (`Vault::import_file`): dropped on a folder row — or a file row,
which means that file's folder, mirroring the right-click rule — the bytes are placed in that folder and
projected. A `.md` arrives as a note, anything else as a resource. The cursor carries the difference:
copy over a tree target, no-drop elsewhere.

Two consequences fall out of the setting, both of which look like odd choices until you know why:

- **The drop sends bytes, not a path.** WebKit hands the page *content*; only Tauri's own drag-drop
  channel carries paths, and that channel is the thing we turned off. So the payload crosses the IPC as
  base64 (`ui/src/importfiles.ts` encodes, `import_file` decodes) and is size-capped frontend-side,
  because the whole file has to fit in memory to make the trip.
- **The keyboard half is a different command.** A drag is pointer-only, so K1 obliges an equal path:
  *Import files…* in the tree's context menu (⇧F10 reaches it) opens an OS picker, and a picker yields
  **paths** — hence `import_path`, the same façade op from the other end, with no byte transport and no
  cap. Two commands for one gesture is not duplication; it is the two shapes the OS offers.

## Transport

**Tauri IPC only** — the frontend `invoke`s these commands. This crate runs **no HTTP server**. An
HTTP/`serve` transport is a *different, deferred adapter* for a *different need* (remote / browser /
agent-over-HTTP); it does not belong here. See [#24](https://github.com/AlteredCraft/B2/issues/24).
