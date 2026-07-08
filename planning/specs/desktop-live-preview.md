---
title: "B2 — Live-preview decorations: a document feel over the byte-honest buffer"
type: note
tags: [b2, ui, desktop, editor, codemirror, live-preview, decorations, spec]
created: 2026-07-08
status: draft
---

# B2 — Live-preview decorations: a document feel over the byte-honest buffer

> **The build spec for [#30](https://github.com/AlteredCraft/B2/issues/30)** — the follow-on
> [completed/desktop-editing.md](completed/desktop-editing.md) §9 deferred. Edit mode today shows raw
> Markdown with syntax colors; this doc upgrades it to **live preview**: markup concealed away from
> the cursor, content styled in place, markup revealed exactly where you're working — the
> "byte-honest buffer that *renders* like a document" the MVP's editor-substrate decision promised
> ([completed/desktop-ui-mvp.md](completed/desktop-ui-mvp.md) §1.2).
>
> **This doc owns:** the decoration engine (`ui/src/livepreview.ts` — the wikilink tree extension,
> the ViewPlugin, the reveal policy); its integration into the Step-3 editor (the Compartment, the
> `</>` source toggle, mod-click navigation, styles); and the build order. **It does not own:** the
> save path, the conflict guard, or any façade/host surface
> ([completed/desktop-editing.md](completed/desktop-editing.md) — untouched end to end: **pure
> frontend, no Rust changes**); the reading view (unchanged — see §3 *Role*).

## 0. Scope & ground rules

Live preview is a **view transform, never a text transform**. Decorations conceal and style; the
document the editor holds is byte-for-byte the buffer the save chain splices — nothing here may
change bytes, intercept the save path, or alter what `Cmd+S` writes. Held fixed from the prior
specs: the reading view stays the default/discovery surface; the render carve-out and the save
chain (desktop-editing.md §6) are load-bearing and unchanged; `ui/` stays vanilla TS with **zero
new dependencies** and no test runner (verification = `npx tsc --noEmit`, `npm run build`, the
dogfood checklist in §7).

**In scope:** the v1 construct cut (§3); hybrid reveal; Cmd/Ctrl+click wikilink follow; the
`</>`-toggled source mode inside the editor; reading-view-matched typography while decorated.
**Out of scope (§8):** tables, images, checkbox interactivity, transclusion/embeds, one-surface
convergence, dark-tuned syntax colors for source mode.

## 1. The problem, grounded in the code

| Layer | What exists today | The gap |
|---|---|---|
| **Editor** ([`main.ts`](../../ui/src/main.ts) `mountEditor`) | CM6 with `markdown()`, history, default keymaps, line wrapping, `defaultHighlightStyle` — raw Markdown, colored. | Editing feels like *source code*: `**`, `[[`, `##` noise everywhere, monospace, no document rhythm. The reading view has the document feel; the editor doesn't. |
| **Syntax tree** (`@codemirror/lang-markdown` → `@lezer/markdown`) | Already parses the buffer incrementally — every decoration target (headings, emphasis, links…) is a tree node we currently use only for colors. | Two gaps: the default `markdown()` base is CommonMark-only (no `Strikethrough` node), and `[[wikilinks]]` — the vault's most important construct — aren't in the tree at all. |
| **Reading view** ([`render.ts`](../../ui/src/render.ts)) | `marked` + the wikilink tokenizer + `.note-body` typography; click-to-navigate wikilinks (the discovery surface). | None — it stays as is. But it sets the bar: mode-switching must not jar (same heading scale, same wikilink look, same code chips). |
| **Chrome** ([`main.ts`](../../ui/src/main.ts) editor bar, [`state.ts`](../../ui/src/state.ts) `sourceOpen`) | The reading view's `</>` raw toggle is sticky state; the editor bar (imperative, carve-out-owned) has only *Editing · path* + Done. | No way to see exact bytes while editing once decorations land; the editor bar is where that toggle belongs (§3 *Escape hatch*). |

**Root cause, in one line:** everything needed to render-in-place is already parsed and already
styled *somewhere* — the buffer's tree and the reading view's CSS have simply never been joined.

## 2. The enabling insights

1. **The tree is already there.** `syntaxTree(state)` over `view.visibleRanges` gives every
   construct in the viewport for free — the decoration engine is a fold over nodes we already
   parse, so cost scales with the *viewport*, not the note.
2. **Conceal is a decoration, not an edit.** `Decoration.replace`/`mark`/`line` change what the DOM
   shows, never what `state.doc` holds — byte-honesty is preserved *by construction*, exactly as
   the splice preserves frontmatter. The save chain literally cannot observe this feature.
3. **The wikilink grammar already exists twice-over.** The reading view's `marked` tokenizer
   (`render.ts`) defines B2's wikilink syntax; a ~20-line `@lezer/markdown` inline parser is its
   twin, giving the tree a `Wikilink` node so *one* engine decorates everything uniformly.
4. **One CM6 constraint shapes the cut.** View plugins may not provide *block-structure* decorations
   (block widgets, replacements spanning line breaks) — those need a StateField. Every v1 construct
   decorates **within** lines (marks, inline replaces, line classes), so the whole engine stays one
   ViewPlugin — and that same constraint is why tables/images defer so cleanly (§8).

## 3. Decisions locked (2026-07-08)

| Concern | Locked choice | Rejected — and why |
|---|---|---|
| **Role** | **Edit-mode upgrade only.** The `marked` reading view stays the default and the discovery surface; Edit now toggles into a live-preview editor instead of a raw one. | **One surface now** (editor becomes the note pane) — forces the deferred hard problems (images, tables, click-vs-cursor) into scope and regresses *reading* until they all land — the exact trap desktop-editing.md §3 rejected. Recorded as a possible future decision; nothing in this slice forecloses it. |
| **v1 construct cut** | Headings (conceal `#`s, reading-view scale) · bold/italic/strikethrough (conceal markers) · inline code (conceal backticks, chip style) · Markdown links (show text, conceal `(url)`) · **wikilinks** (Lezer extension; conceal brackets/alias pipe, accent style) · blockquote (conceal-less `>` styling + border) · bullet `-`/`*` → `•` · horizontal rules → a rule widget · fenced code (block background; **fences stay visible** — concealing them hides the language tag for little gain). | **Tables / images / interactive checkboxes** — block widgets or new write paths; each is its own effort (§8). **Leaner inline-only cut** — leaves the editor half-raw; the block styling is where most of the document feel lives. |
| **Reveal policy** | **Hybrid.** Block markers (heading `#`s, `>`, bullets, HR) reveal when the **cursor's line** is theirs — you need the marker visible to change a level or outdent. Inline markup (`**`, `` ` ``, `[[ ]]`, `(url)`) reveals only when the **selection touches the element's span**. Same machinery either way: every conceal carries a reveal range. | **Line-based everywhere** — typing anywhere on a line flashes every concealed marker on it; noisy. **Element-based everywhere** — quiet, but editing a heading level means landing the cursor *inside* a concealed marker. |
| **Wikilink follow** | **Cmd/Ctrl+click** follows, through the existing `openNote` path (flush → conflict-guard → leave edit mode → open in reading view). Plain click places the cursor, as an editor must. | **No follow while editing** — zero code, but breaks the check-that-note flow mid-edit, and mod-click is deep muscle memory (Obsidian, IDEs). |
| **Escape hatch** | **Reuse `sourceOpen`.** The editor bar gains the same `</>` toggle; on = decorations off (raw + syntax colors, monospace — exactly today's editor) via a **Compartment** reconfigure, no remount, cursor/undo intact. One sticky "show me raw" preference shared with the reading view — entering edit mode while reading raw starts raw, coherently. | **A separate editor-only mode flag** — a second toggle state for the same intent. **No escape hatch** — nothing to reach for when markup misbehaves or exact bytes matter. |
| **Substrate** | **Hand-rolled**: one module, `ui/src/livepreview.ts` — the wikilink Lezer extension + a ViewPlugin folding the visible tree + selection into a `DecorationSet`. Zero new deps (everything ships with `@codemirror/lang-markdown`). | **Adopt a community package** (ixora, rich-markdoc, HyperMD) — variously unmaintained, theme-opinionated, or CM5-era; none know our wikilink syntax or reveal policy, so adoption means forking; a new dep under the CSP for negative benefit. |

**Typography follows the decision, not a new one:** while decorated, the editor drops monospace for
the reading view's voice (16px proportional, `--reading` measure, `.note-body` heading scale) —
"document feel" *is* the motivation, and Obsidian does the same. Source mode (`</>` on) keeps
today's monospace. Code spans/fences stay monospace in both.

## 4. The decoration engine — `ui/src/livepreview.ts`

- **The wikilink tree extension.** A `MarkdownConfig` defining a `Wikilink` inline node (parser:
  scan `[[` → `]]`, same grammar as the `render.ts` tokenizer — target, optional `|label`).
  `mountEditor` switches to `markdown({ base: markdownLanguage, extensions: [wikilink] })` —
  `markdownLanguage` pins **GFM** (the default base is CommonMark-only; without it there is no
  `Strikethrough` node), matching the reading view's `gfm: true`.
- **The ViewPlugin.** Recomputes on `docChanged || selectionSet || viewportChanged`: iterate
  `syntaxTree` over `view.visibleRanges`; per node emit **style** decorations unconditionally
  (`Decoration.mark` for emphasis/code/links/wikilinks, `Decoration.line` for heading scale,
  blockquote border, fence background) and **conceal** decorations (`Decoration.replace`, or a
  widget for `•`/HR) *only when their reveal range doesn't intersect the selection* — the hybrid
  test: marker's line for block nodes, element span for inline. Multi-range selections check every
  range. All decorations are line-local (insight 4), so the plugin is legal and the engine is one
  pure function of `(tree, selection, viewport)`.
- **No atomic ranges — deliberately.** Arrow keys must be able to step *into* markup (the reveal
  makes it visible as the cursor arrives). Skipping concealed ranges atomically would make markers
  uneditable; Obsidian behaves the same.
- **Selection is byte-honest too.** Selecting across a concealed region selects the underlying
  markup bytes (copy/cut carry them) — correct, and stated so the dogfood doesn't misread it.
- **Navigation.** Wikilink marks carry `data-target`; a `mousedown` DOM handler with
  `metaKey/ctrlKey` on such a span resolves the target and calls `openNote` (which flushes via
  `closeEditor` — Step 3's machinery, untouched). Plain clicks fall through to CM.
- **Degradation.** A construct the walker doesn't recognize renders undecorated raw text — never an
  error, never a changed byte. Source mode is the universal out.

## 5. Integration — the editor mount, the toggle, the styles

- **[`main.ts`](../../ui/src/main.ts)** — `mountEditor` wraps the live-preview extension (and the
  proportional-font theme class) in a `Compartment`, initialized from `!state.sourceOpen`; the
  editor bar gains the `</>` toggle (same `data-toggle-source` delegation), whose handler — while
  editing — flips `state.sourceOpen`, dispatches the compartment reconfigure, and repaints the bar
  button (`paintEditor`-style, never a pane rebuild). Reading-view behavior of the toggle is
  unchanged.
- **[`state.ts`](../../ui/src/state.ts)** — no new state. `sourceOpen` gains the second consumer it
  was designed sticky for; timers/flags stay module-local.
- **[`style.css`](../../ui/style.css)** — a `.lp-*` block mirroring `.note-body`'s scale: headings,
  strong/em, the code chip, the dashed-accent wikilink, blockquote border, fence background, the HR
  widget; plus the decorated-mode proportional font on `.editor-host`. Light/dark via the existing
  CSS vars — no highlight-theme dependency.
- **Save chain, carve-out, conflict bar** — untouched. Decorations never produce doc changes, so
  the autosave `updateListener` cannot fire from them (§7 invariant 1 pins this).

## 6. Build order

Pure `ui/`; each step leaves the editor shippable. Verification per step: `npx tsc --noEmit` +
`npm run build` + targeted dogfood.

1. **The tree knows the vault** — the wikilink `MarkdownConfig`; `markdown({ base:
   markdownLanguage, extensions: [wikilink] })`. Editor behavior unchanged (colors only); prove
   `Strikethrough` and `Wikilink` nodes exist (temporary tree dump in dev).
2. **The engine, proven on inline** — the ViewPlugin skeleton (viewport walk, reveal test,
   selection intersection) decorating emphasis, strikethrough, inline code with hybrid reveal.
   The pattern every other construct reuses.
3. **The full cut** — headings, links, wikilinks (+ `data-target` and the mod-click handler),
   blockquote, bullets, HR widget, fence background. The proportional-font theme + the `.lp-*`
   styles land here (the feel arrives with the block constructs).
4. **The escape hatch** — the Compartment, the editor-bar `</>` toggle, sticky-state wiring, and
   the §7 dogfood pass end to end.

## 7. Correctness invariants & definition of done

1. **Zero byte drift.** For any editing session, the doc CM holds (and the save chain splices) is
   identical with decorations on, off, or toggled mid-session — live preview cannot appear in any
   diff.
2. **Reveal is total.** Every concealed byte is reachable: cursor on the construct (per the hybrid
   policy) reveals it; `</>` reveals everything. No byte is editable-only-blind.
3. **Step 3's guarantees hold verbatim.** Autosave cadence, single-flight chain, conflict bar,
   flush points, and the render carve-out behave identically under decorations.

**Done when:** `npx tsc --noEmit` and `npm run build` clean; `cargo test -p b2-desktop` untouched
and green (no Rust changes); and the dogfood passes — markup conceals/reveals per policy with no
character-eating at boundaries; undo/redo and paste behave; typing stays smooth on the vault's
largest note (viewport-scoped engine); Cmd/Ctrl+click on a wikilink flushes and navigates (plain
click just places the cursor); `</>` flips raw and back without losing cursor or history; a
conflict mid-decorated-edit shows the bar and both actions work; the reading view and discovery
panes are pixel-unchanged; mode-switching read ↔ edit doesn't jar (typography parity).

## 8. Open questions / deferred

- **One-surface convergence** — if live preview proves good enough, does the editor become the
  note pane? A future decision with its own bar to clear (images, tables, click semantics at
  parity with the reading view); deliberately not foreclosed here.
- **Tables & images** — block-widget territory (StateField, asset-protocol work for vault-relative
  images — the reading view shares that gap). Each its own slice.
- **Interactive task checkboxes** — a widget that *writes* (`- [ ]` ↔ `- [x]`) is a new buffer
  mutation path; decide deliberately, not as a rendering side-effect.
- **Source-mode syntax colors in dark theme** — `defaultHighlightStyle` is light-tuned; tolerated
  in the raw escape hatch, revisit if it grates.

## 9. Docs to mirror

- [tasks.md](../tasks.md) — Active item 1 (#30) points here. *(Done alongside this doc.)*
- [completed/desktop-editing.md](completed/desktop-editing.md) §9 — the live-preview deferral gains
  a "specced →" pointer. *(Done alongside this doc.)*
- [completed/desktop-ui-mvp.md](completed/desktop-ui-mvp.md) §1.2 — the document-feel rationale
  gains its execution pointer. *(Done alongside this doc.)*
- [#30](https://github.com/AlteredCraft/B2/issues/30) — link this spec when it lands on main.
