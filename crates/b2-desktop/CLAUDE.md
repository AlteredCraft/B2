# CLAUDE.md: `b2-desktop` and `ui/`

Rules for the desktop app: this crate (the Tauri host) and [`ui/`](../../ui) (the frontend).
They add to the [root CLAUDE.md](../../CLAUDE.md); everything there still applies.

## What the app is for

The desktop app is where B2 gets used every day. It replaces Obsidian, so it has to be a
good place to read and write first and a research tool second: related notes, **Why?**,
typed links and cited chat sit *beside* the note, never in the way of it.

Most `observed` issues land here, because this is where friction is felt. Prefer making a
daily path smoother over adding a new surface.

## The one rule: hold no engine logic

This crate is the second dumb adapter over the [`Vault`](../b2-core/src/vault.rs) façade
(E3). Every `#[tauri::command]` is: deserialize the arguments, call one `Vault` method,
serialize the result.

- A branch, loop or rule that wants to live in a handler belongs in `b2-core`, behind a new
  façade method. Add the method when a command needs it; don't pre-build.
- Return the CLI's existing `--json` view types. Don't define a parallel set of DTOs.
- `b2-core` never learns about Tauri, webviews or the UI.
- Tauri IPC is the only transport. This crate runs no HTTP server.

**Smell test:** a command body longer than "parse, call, return", or a reach for a `b2-core`
internal that isn't on `Vault`, means a façade method is missing.

The same holds in `ui/`. The frontend decides how things look and how keys move, never what
the engine means. Pure logic lives in small tested modules (`treenav.ts`, `sidenav.ts`,
`chat.ts`, `droplink.ts`, `keymap.ts`) that run under node, and `main.ts` is the wiring.
Read a module's header comment before changing it.

## Wiring

- **Embedder.** Reads open with the fake embedder. Anything that embeds a query or writes
  vectors opens the real model and fails fast with "run `b2 init`". Save, new-note,
  frontmatter-save and project are deliberately model-free, so editing never waits on a
  model; the background embed fills vectors in afterwards.
- **Errors** reach the webview generic and actionable (E4), mapped through a `thiserror`
  enum as the CLI's `user_message` does. The full detail is always logged host-side, so a
  failure is diagnosable without `B2_DEBUG`.
- **Chat provider** (`src/chat.rs`). `b2_llm::LlmConfig::from_env` with Settings layered on
  top, the way a CLI flag would be. Chat config lives beside the remembered vault, never in
  it.
- **The API key lives in the macOS Keychain** (`src/keychain.rs`), never in a file.
  `B2_LLM_API_KEY` overrides it, a Keychain that refuses must not break chat, and the key
  never crosses back to the webview.
- **Logging** is installed in `main` with a non-blocking writer, and `main` holds the
  returned guard for the whole run.

## Keyboard (K1)

**No action is reachable only by pointer.** A click-only, right-click-only or drag-only
gesture is a bug. Every new surface owes four things:

1. **Reachable:** a real `<button>`, or a focusable element with a role and an activating
   key, in a sensible tab order. Keyboard activation dispatches the same click the mouse
   sends, so there is one activation path.
2. **Visible:** a focus ring. Never `outline: none` without an equal replacement.
3. **Escapable:** an overlay takes focus, traps Tab, and restores focus to what opened it.
   `Escape` dismisses innermost-first and `Enter` confirms (`syncOverlayFocus()` in
   `main.ts`).
4. **Discoverable:** a chord is declared once, in `ui/src/bindings.ts`, and the handlers,
   the editor keymap, the shortcut sheet and the `title` hints all derive from it. The
   suite fails on an undocumented binding, a same-scope clash, a clash with CodeMirror's
   stock keys, and a clash with the menu bar.

Chords are rebindable (`keymap.ts`, `recorder.ts`). The layout persists in `localStorage`
and is re-judged on load. The menu bar is declared in `src/menu.rs` and mirrored in
`ui/src/menukeys.ts`: change the two together.

### Traps that have already bitten

- **A repaint destroys focus.** The UI renders by swapping `innerHTML`, so whatever was
  focused is gone and the keyboard user lands on `<body>`. Restore by an identity that
  survives the swap (a path, a row key, a stable `id`), never by element. That is what
  `lastFocused` is for.
- **Rows re-focused after a repaint ring on `:focus`, not `:focus-visible`.** WebKit grants
  `:focus-visible` to a script-focused node only by inheritance, and a repaint breaks that
  (`ui/style.css`, the focus block).
- **WebKit doesn't focus a button on click.** Click delegation sets the pane focus state
  explicitly, or the keyboard's idea of "where I am" drifts from the mouse's.
- **A menu accelerator never reaches the webview.** AppKit takes it first, so a chord in
  both the menu and `bindings.ts` only ever fires from the menu. Zoom (⌘= ⌘- ⌘0) is in the
  menu for that reason. ⌘⇧= can't zoom in, because Tauri splits accelerators on `+`.
- **The overlay layer is memoized** (`paintModal`). A modal's typed state lives only in the
  DOM, so an identical repaint must not wipe a half-written field.

## Rendering is a trust boundary (E5)

**Authorship is not trust.** A note can come from a shared vault, a web clip or a cloned
repo, and a chat answer is generated from notes by a model. Both are hostile input rendered
into B2's own window.

- **One Markdown-to-HTML path, and it sanitizes.** `renderMarkdown` (`ui/src/markdown.ts`)
  runs DOMPurify (`ui/src/sanitize.ts`) as its last step, so every surface is covered by
  construction.
- **Anything that reaches `innerHTML`** comes from `renderMarkdown` (note bodies) or
  `escapeHtml` (every value B2 puts into chrome). There is no third option.
- **CSP is the second layer, never the only one** (`tauri.conf.json`).
- **A note's links never navigate the webview.** Web links (`http`, `https`, `mailto`) are
  handed to the OS by the host, which re-checks the scheme against its own list. `links.ts`
  and `only_web_links_are_openable` change together.
- **Chat answers** take the same sanitized path. Citations are in-app buttons, never links,
  and streamed tokens are written as `textContent`, so a half-arrived answer is never
  parsed.

## Drag and drop

`tauri.conf.json` sets `dragDropEnabled: false`, because with Tauri's interception on, the
DOM never sees `drop` on macOS. The price: WebKit's default for an external file drop is to
navigate to the file, which replaces the whole app. So `main.ts` cancels every file drag
first, then decides what to do with it.

Three gestures, each with a keyboard twin (K1):

| Drag | Does | Keyboard twin |
|---|---|---|
| A tree row onto a folder | Moves it | *Move…* in the tree's menu |
| A file from Finder onto the tree | Imports it (`import_file`; the bytes cross IPC as base64) | *Import files…* (`import_path`, from an OS picker) |
| A Similar card onto a line of the note being edited | Types a `[[wikilink]]` at the end of that line, in the editor buffer (`droplink.ts`). Never into a code block. | *Insert link at cursor* in the card menu |

The card drag writes the untyped body link. The card's *Link…* writes the typed frontmatter
one.
