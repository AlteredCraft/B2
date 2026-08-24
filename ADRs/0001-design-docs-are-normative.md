# ADR-0001 — Design docs are normative; the code is a projection of them

- **Status:** Accepted · 2026-06-28
- **Refs:** `docs/design/invariants.md`

## Context

B2's behaviour is easy to drift by accident: a schema tweak or a threshold change can quietly
redefine what the product promises. Prose that merely *describes* the code cannot stop that.

## Decision

- `docs/design/invariants.md` is the **normative register** — one testable claim per entry, cited
  by id (S2, G2, D1…). **On conflict with any other doc or with the code, the register wins** and
  the other side gets fixed.
- `data-model.md` is the *what*, `index-engine.md` the *how*. Code comments cite them by section.
- Changing the register is a deliberate decision (an ADR here + the issue that drove it), never a
  drive-by edit.

## Consequences

- Before changing behaviour, read the relevant doc: the schema must satisfy the data model, never
  the reverse.
- Decision history is the GitHub Issue that drove a verdict plus the commit that shipped it. These
  ADRs record only the *architectural* choices; the backlog stays in Issues, build history in git.
