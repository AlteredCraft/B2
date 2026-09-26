# CLAUDE.md: `b2-desktop` and `ui/`

Guidance for agents and humans working on the desktop app: this crate (the Tauri host) and
[`ui/`](../../ui) (the frontend). It adds to the [root CLAUDE.md](../../CLAUDE.md); everything
there still applies.

## What the app is for

The desktop app is where B2 gets used every day. It is the notes app that replaces Obsidian, so
it has to be a good place to read and write first, and a research tool second: the related notes,
**Why?**, typed links and cited chat sit *beside* the note, never in the way of it. A citation
opens its evidence in the centre pane, next to the answer.

Most `observed` issues will land here, because this is where friction is felt. Prefer making a
daily path smoother over adding a new surface.

## The one rule: hold no engine logic

This crate is the second **dumb adapter** over the [`Vault`](../b2-core/src/vault.rs) façade,
the GUI sibling of [`b2-cli`](../b2-cli). Every `#[tauri::command]` is: deserialize the arguments,
call one `Vault` method, serialize the result.

- A branch, loop or rule that wants to live in a handler belongs in `b2-core`, behind a new
  façade method. Add the method when a command needs it; don't pre-build.
- Return the CLI's existing `--json` view types. Don't define a parallel set of DTOs.
- `b2-core` never learns about Tauri, webviews or the UI.

Why it matters: two adapters over one contract can't drift, a fix in the core fixes both, and the
app inherits the engine's tests instead of needing its own copy (invariant E3). **Smell test:** a
command body longer than "parse, call, return", or a reach for a `b2-core` internal that isn't on
`Vault`, means a façade method is missing.

The same holds in `ui/`: the frontend decides how things look and how keys move, never what the
engine means. Pure logic lives in small tested modules (`treenav.ts`, `sidenav.ts`, `chat.ts`,
`droplink.ts`, `keymap.ts`, …) that run under node; `main.ts` is the wiring. Each module's header
comment explains its reasoning; read it before changing the module.

## Wiring

- **Embedder, as in the CLI.** Reads open with the fake embedder; anything that embeds a query or
  writes vectors opens the real model and fails fast with "run `b2 init`". The save, new-note,
  frontmatter-save and project paths are deliberately model-free, so editing never waits on a
  model; the background embed fills vectors afterwards.
- **Errors reach the webview generic and actionable.** Map façade errors as the CLI's
  `user_message` does, through a `thiserror` enum. The full internal detail is always logged
  host-side (`log_internal`, from `CmdError`'s `Serialize`), so a failure is diagnosable without
  `B2_DEBUG`.
- **Chat provider, the embedder's sibling** (`src/chat.rs`). Resolution is
  `b2_llm::LlmConfig::from_env` with Settings layered on top, the way a CLI flag would be. Chat
  config lives beside the remembered vault, never in it.
- **The API key lives in the macOS Keychain** (`src/keychain.rs`), never in a file. `B2_LLM_API_KEY`
  overrides it; a Keychain that refuses must not break chat (the key stays in force for the
  session); and the key never crosses back to the webview.
- **Logging** is installed in `main` (`logging::init_logging`), with a non-blocking writer because
  this process is long-lived. `main` holds the returned guard for the whole run.

## Keyboard: fully operable without a mouse (invariant K1)

**No action is reachable only by pointer.** A click-only, right-click-only or drag-only gesture is
a bug. Every new surface owes four things, cheap now and expensive to retrofit:

1. **Reachable:** a real `<button>` or a focusable element with a role and an activating key, in a
   sensible tab order. Keyboard activation dispatches the *same* click the mouse sends, so there is
   one activation path.
2. **Visible:** a focus ring. Never `outline: none` without an equal replacement.
3. **Escapable:** an overlay takes focus, traps Tab, and restores focus to what opened it.
   `Escape` dismisses innermost-first; `Enter` confirms (`syncOverlayFocus()` in `main.ts`).
4. **Discoverable:** a chord is declared once, in `ui/src/bindings.ts`, and everything else derives
   from it: handlers, the editor keymap, the shortcut sheet, `title` hints. The suite fails on a
   binding the sheet doesn't document, on a same-scope clash, on a clash with CodeMirror's stock
   keys, and on a clash with the menu bar.

Chords are user-rebindable (`keymap.ts`, `recorder.ts`); the layout persists in `localStorage` as
a viewing choice and is re-judged on load. The **menu bar** is declared in `src/menu.rs` and
mirrored in `ui/src/menukeys.ts` (checked at boot): change the two together.

### Traps that have already bitten

- **A repaint destroys focus.** The UI renders by swapping `innerHTML`, so anything focused is gone
  afterwards and the keyboard user lands on `<body>`. Restore by an identity that survives the swap
  (a path, a row key, a stable `id`), never by element. A control that must be restored needs a
  durable identity in the markup. By the end of `render()`, `document.activeElement` is already
  `<body>`; that is why `lastFocused` exists.
- **Rows re-focused after a repaint ring on `:focus`, not `:focus-visible`.** WebKit only grants
  `:focus-visible` to a script-focused node by inheriting it, and a repaint breaks that, so the
  ring would lag one key behind (`ui/style.css`, the focus block).
- **WebKit doesn't focus a button on click.** Click delegation sets the pane focus state
  explicitly, or the keyboard's idea of "where I am" drifts from the mouse's.
- **A menu accelerator never reaches the webview.** AppKit takes it first, so a chord in both the
  menu and `bindings.ts` only ever fires from the menu. Zoom (⌘= ⌘- ⌘0) is in the menu for that
  reason, and ⌘⇧= does not zoom in: Tauri splits accelerators on `+`, so `+` is unreachable.
- **The overlay layer is memoized** (`paintModal`): a modal's typed state lives only in the DOM, so
  an identical repaint must not wipe a half-written field.

## Rendering is a trust boundary (invariant E5)

**Authorship is not trust.** A note can come from anywhere (a shared vault, a web clip, a cloned
repo), and a chat answer is generated from notes by a model. Both are hostile input rendered into
B2's own window.

- **One Markdown-to-HTML path, and it sanitizes.** `renderMarkdown` (`ui/src/markdown.ts`) runs
  DOMPurify (`ui/src/sanitize.ts`) as its last step, so every surface is covered by construction.
- **Anything that reaches `innerHTML`** comes from `renderMarkdown` (note bodies) or `escapeHtml`
  (every value B2 interpolates into chrome). There is no third option.
- **CSP is the second layer, never the only one** (`tauri.conf.json`).
- **A note's links never navigate the webview.** The window *is* the app. Web links (`http`,
  `https`, `mailto`) are handed to the OS by the host, which re-checks the scheme against its own
  list; `links.ts` and `only_web_links_are_openable` must change together.
- **Chat answers** take the same sanitized path; citations are in-app buttons, never links; streamed
  tokens are written as `textContent`, so a half-arrived answer is never parsed.

## Drag and drop

`tauri.conf.json` sets `dragDropEnabled: false`. The in-app drags need it (with Tauri's
interception on, the DOM never sees `drop` on macOS). The price: WebKit's default for an
external file drop is to navigate to the file, which replaces the whole app. So `main.ts` cancels
every file drag first, then decides what to do with it.

The three gestures, each with a keyboard twin (K1):

- **Tree row onto a folder:** move it.
- **A file from Finder onto the tree:** import it (`import_file`). The bytes cross IPC as base64,
  because only the disabled channel carries paths. The twin is *Import files…* in the tree's
  menu, which gets paths from an OS picker (`import_path`).
- **A Similar card onto a line of the note being edited:** a `[[wikilink]]` lands at the end of
  that line, in the editor buffer, saved like a keystroke (`droplink.ts`). It writes the untyped
  body link; the card's *Link…* writes the typed frontmatter one. Never into a code block. The
  twin is *Insert link at cursor* in the card menu.

## Transport

Tauri IPC only. This crate runs no HTTP server. An HTTP adapter, if one is ever needed, is a
separate crate over the same façade, not a feature of this one.
