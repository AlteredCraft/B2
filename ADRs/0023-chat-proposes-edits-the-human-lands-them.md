# ADR-0023 — Chat proposes edits; the human lands them, and every landed edit is reversible

- **Status:** Accepted · 2026-11-08
- **Refs:** invariants W1–W5, S4, M1, E3, E5 · ADR-0004, ADR-0005, ADR-0012, ADR-0016, ADR-0022 ·
  GH #253 · [docs/proposals/2026-11-08-chat-edits-notes.md](../docs/proposals/2026-11-08-chat-edits-notes.md)

## Context

Chat was built as a reader: it answered from the notes and never touched them, and W2 said so
("B2 never authors the note body"). Daily use asked for more. "Clean up this doc" (GH #253) is a
request for an editing partner, and a partner who can only describe an edit in prose is one I then
have to retype. The pivot is for chat to work the notes with me.

The reason W2 existed still stands: the vault is mine, and B2 must never change it behind my back.
So the question was how a model can author bytes without B2 making an unbidden write. The answer
is to separate *proposing* from *landing*.

Git was considered as the undo store and rejected: the vault happens to be a git repo, but git is
per commit, not per edit; one commit per accepted edit would have B2 writing into `.git`, sweeping
in my own unsaved typing, and handling dirty trees and rebases. B2 keeps no dependency on git.

## Decision

- **The model proposes; it never writes.** The one edit-shaped tool, `b2_edit`, is
  `{note, find, replace}`: replace one exact, unique occurrence of `find` in the note body. It
  returns a diff or an `error:` result (no match, several matches, cap hit) and touches no file.
  Within a turn, proposals compose on a per-conversation working copy the model's reads see.
- **Landing is my command.** An accepted proposal is written by `Vault::apply_edits` through the
  existing guarded save (`save_guarded`): revision-checked, body-only, frontmatter untouched,
  re-projected model-free. An outside change since the proposal fails as `WriteConflict`. W1 holds
  literally: until I accept, nothing has changed.
- **Review-first by default, opt-out per conversation.** Every proposal waits for Apply unless I
  switch the conversation to *apply as you go*. That switch is one gesture, session-scoped, and
  never a persisted default.
- **Every landed edit is recorded and reversible, one edit at a time.** The record holds the hunk,
  the before and after bytes and both revisions. Undo restores the before bytes when the file is
  unchanged, else reverse-applies the hunk if its text is still unique, else refuses and shows the
  prior text. Never a guess, never a merge.
- **The journal is disposable cache under `.b2/edits/`.** It is B2-held copies of my own bytes;
  deleting it loses undo history and nothing else. S4 is clarified: the vault is still the only
  truth, and the journal is a cache like the index.
- **Hunks, not whole-note rewrites.** A whole-note rewrite from a small model drops paragraphs
  silently; a hunk can only change what it names, yields a precise diff, and reverses exactly.
- **Every edit is visible.** Each proposal renders as a diff card with a status (pending, applied,
  undone, conflict); landed edits appear in a Changes list with Undo; the answer can never claim an
  edit the ledger does not show (ADR-0022's `tools` principle, extended). Diff text is model
  output and renders through the escaping path (E5).
- **No delete, move or rename from chat.** W4 is unchanged. Frontmatter and resources are out of
  the tool's reach.
- **The loop and the writes live in `b2-core`** behind façade methods; the CLI and desktop are
  dumb adapters over the same proposals, apply and undo (E3, ADR-0012).

## Consequences

- W2 and W3 are amended (see `invariants.md`): B2 may carry model-drafted body bytes, but lands
  only bytes I accepted, and the on-command write list gains *apply an accepted edit* and *undo an
  applied edit*.
- How well a given model drives `b2_edit` is a quality question for the harness (ADR-0013): clean
  match rate, preservation of links and headings, judged quality, honesty when nothing needs
  changing. The eval decides whether edit mode is on by default for the shipped model.
- Phase 1 keeps the journal in memory; durability under `.b2/edits/` and the opt-out toggle follow
  once undo is proven on the in-memory form.
- `FakeLlm` scripts an edit call so the propose, apply and undo paths are covered model-free (E2).
