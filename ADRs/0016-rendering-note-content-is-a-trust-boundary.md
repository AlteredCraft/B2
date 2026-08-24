# ADR-0016 — Rendering note content is a trust boundary

- **Status:** Accepted · 2026-07-26
- **Refs:** invariants E5 · GH #77 · `ui/src/sanitize.ts`, `ui/src/links.ts`

## Context

Authorship is not trust. A `.md` can come from a shared vault, a download, or a web clip — and model
output is the same class of input. The webview *is* the application, so injected markup or a hostile
link scheme is not a rendering bug, it is code execution.

## Decision

- **One** Markdown→HTML seam (`renderMarkdown`), which sanitizes its output — DOMPurify wired as
  `marked`'s `postprocess` hook, so every call site is covered *by construction*.
- Every value B2 itself interpolates into chrome goes through `escapeHtml`.
- The webview CSP (`default-src 'self'`, no inline scripts) is a **second, independent layer** — never
  the sole guard.
- A note's link **never navigates the webview**: `http`/`https`/`mailto` are an OS handoff performed
  host-side behind a scheme allow-list the host re-checks; every other scheme is **refused**, not
  handed to whatever app registered it.

## Consequences

- Any new render path must go through the one seam, or it is a hole.
- Model output is rendered under exactly the same rules as note content.
