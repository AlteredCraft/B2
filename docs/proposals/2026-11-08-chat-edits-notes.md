# Proposal: chat becomes an editor of the notes

- **Status:** Proposal, 2026-11-08. Decisions taken 2026-11-08 (see the end); nothing built yet.
- **Refs:** invariants W1 to W5, S4, M1, E3, E5 · ADR-0004, ADR-0005, ADR-0012, ADR-0016,
  ADR-0022 · GH #253 (chat doesn't know which note is open)

## The pivot in one paragraph

Today chat is a reader. It answers from the notes and never touches them (W2: "B2 never
authors the note body"). The pivot: in chat, the model may **propose edits** to notes, and
work with me across several turns to clean up, restructure or rewrite them. Three things are
non-negotiable: I can **see** every edit before or after it lands, as a diff against the
note; every landed edit is **undoable**, one at a time, exactly; and nothing lands that I did
not **accept**. The human stays the judge. This keeps the spirit of W1 (no unbidden writes)
while changing the letter of W2 and W3, so it needs an ADR, not just code.

## What changes in the rules

| Rule today | Rule after | Why |
|---|---|---|
| W2: B2 never authors the body. | B2 never **lands** body bytes I did not accept. The model may draft them; the draft is shown as a diff; only my accept writes it. | The model is an author now, but the write is still the mechanics of my command (accept). |
| W3: closed list of on-command writes. | Add two entries: *apply an accepted edit* (a guarded save of the accepted bytes) and *undo an applied edit* (a guarded save of the recorded prior bytes). | Both go through the existing `save_guarded` path, so the revision guard and re-projection are unchanged. |
| S4: no durable B2-derived state outside Markdown. | Clarify: an **edit journal** under `.b2/` (before and after bytes of each landed edit) is disposable cache, like the index. Deleting it loses undo history and nothing else. | Undo that survives a restart needs the prior bytes stored somewhere. The vault stays the truth. |
| W4: never delete, move or archive on its own. | Unchanged. The model gets no delete or move tool in this proposal. | Reversibility of a delete is a different problem; defer. |

Everything else holds: identity is the path (L1), the index is a projection (S2), the model
is behind the seam (M1), model output is untrusted (E5), no engine logic in adapters (E3).

## How it works

### The model proposes; B2 holds; I accept

The agent turn already exists (ADR-0022, `why_similar`). This proposal gives `ask` the same
loop with a wider tool set, and adds the first tool that is not a read:

| Tool | Kind | What it does |
|---|---|---|
| `b2_read` | read | A note's passages (exists). |
| `b2_search` | read | Hybrid search, so the model can find the notes to work on. |
| `b2_neighbors` | read | A note's links (exists). |
| `b2_edit` | **propose** | `{note, find, replace}`: replace one exact, unique occurrence of `find` in the note body with `replace`. Returns the resulting diff, or an error result ("no match" / "matched 3 times, be more specific"). |

`b2_edit` does **not** write to disk. The core keeps a per-conversation **working copy** of
each touched note (disk bytes plus the accepted and pending edits, in order). The model's
reads see the working copy, so a five-edit cleanup composes correctly within one turn. Each
successful `b2_edit` call becomes a **proposal**: note path, the hunk (find, replace, line
numbers), the base revision, a status (`pending`).

The turn's answer returns the proposals beside the text and citations (`AnswerView.edits`).
Nothing has touched the vault yet (W1 holds exactly, at this point).

**Accept** is a façade call: `Vault::apply_edits(note, edits, base_revision)`. It applies
the hunks to the current disk bytes and saves through `save_guarded`, so an outside change
to the file since the proposal (a typed edit, a `git pull`) fails with the existing
`WriteConflict`, shown as "this note changed since the edit was proposed; ask again".
Frontmatter is never in the hunk's reach: `b2_edit` works on the body only, the way
`Vault::write` does. On success the edit is recorded in the journal with before and after
bytes and marked `applied`.

### Why find/replace hunks and not whole-note rewrites

- A whole-note rewrite from a small local model silently drops paragraphs. A hunk can only
  change what it names.
- A hunk makes a precise diff for free, and precise undo is the reverse hunk.
- A hunk on a long note costs a few hundred tokens, not the whole note twice.
- The uniqueness rule turns a vague edit into an error the model can fix, not a wrong edit.

This is the shape every coding agent converged on, for the same reasons. Whole-note
replacement can be a later tool for "rewrite this short note", guarded by a size cap.

### Two modes, one default

- **Review first (default).** Every proposal waits for me. A turn's card shows the diffs
  with *Apply*, *Apply all*, *Skip*. Nothing lands until I click or press the key.
- **Apply as you go (per conversation, opt-in).** Edits land as the model makes them, each
  shown as a diff the moment it lands, each undoable. For "clean up these twelve meeting
  notes the same way" after I've seen the first two.

Review-first is the default. Opting out must be one gesture: a toggle in the chat pane,
and *Apply all* on a card also offers "and keep applying this conversation". The toggle is
session-scoped and never becomes a persisted default. Phase 1 ships review-first; the
toggle lands in Phase 2 once undo is durable. A cap on edits per turn (configurable, loud,
in Settings) bounds both modes.

### Transparency: where I see it

1. **In the answer.** Each proposal is a card in the transcript: note title and path, a
   unified diff (removed in red, added in green), status chip (*pending*, *applied*,
   *undone*, *conflict*). The card is an in-app button that opens the note at the hunk.
   Diff text is model output: rendered through `escapeHtml`, never `innerHTML` (E5).
2. **In the editor.** After an edit lands in the open note, the changed lines carry a
   gutter mark and a soft highlight until I next type in them. CodeMirror decorations; no
   bytes injected into the note (W2 for B2 syntax still holds).
3. **In a Changes list.** A section at the top of the chat pane: every edit landed this
   conversation, newest first, grouped by note, each with *Undo*. The journal view.
4. **Honest answers.** If the model asked for an edit the core refused (no match, cap hit),
   the card says so. If the model wrote "I've cleaned it up" and made no `b2_edit` call, the
   card list is empty and says "no edits were proposed", so the text can't overstate what
   happened (ADR-0022's `tools` principle, extended).

### Undo: precise, per edit

Each applied edit records: path, the hunk, the full before bytes, the full after bytes,
revision before and after, the turn it came from. Undo of one edit:

1. If the file still hashes to the edit's after-revision: write the before bytes
   (`save_guarded`). Exact.
2. Else, if the edit's `replace` text still occurs exactly once: apply the reverse hunk
   (`replace` → `find`). Exact for that hunk, even after I typed elsewhere.
3. Else: refuse with "the edited text has changed since; open the note to fix by hand",
   and offer the before bytes in a read-only view to copy from. Never a guess.

Also *Undo all from this answer* and *Undo all in this note this session*, which are the
per-edit undo run newest-first with the same three rules. Redo is re-applying the recorded
hunk under the same rules.

Phase 1 keeps the journal in memory (lost on quit). Phase 2 writes it under `.b2/edits/`
so undo survives a restart, with a size cap and age limit, both in Settings. B2 does not
use git for this, even when the vault is a git repo: git is per commit, not per edit, and
one commit per accepted edit would mean B2 writing into `.git`, sweeping in your own
unsaved typing, and handling dirty trees and rebases. A B2-owned folder of before and after
bytes is smaller and has no dependency. Git stays your backstop, untouched by B2.

### CLI parity (E3)

The desktop and the CLI are the two dumb adapters, so the façade grows and both call it:

- `b2 ask --edit "clean up this note" --note PATH` runs the agent turn and prints the
  proposals as diffs; in a TTY it asks `apply? [y/n/a]`; with `--json` it emits them and
  exits, so an agent can read them.
- `b2 edits apply PROPOSAL`, `b2 edits list`, `b2 edits undo ID`, all with `--json`.

### Focus note (#253) is the first step

"Clean up this doc" needs chat to know what "this doc" is. #253's shape (an optional focus
path on `ask`, its passages numbered first, one sentence in the prompt, a chip in the pane)
is unchanged and becomes step one of phase 1. The agent turn makes the focus note the
default `note` argument of every tool, the way the why-turn defaults to its pair.

## Where the code goes

| Piece | Crate | Note |
|---|---|---|
| `b2_edit` tool spec, prompt, hunk apply/reverse, working copy, proposal and journal types | `b2-core` (`chat.rs`, new `edit.rs`) | Pure, deterministic, tested against `FakeLlm`. `FakeLlm` gains a scripted edit call so the loop, accept and undo are covered model-free (E2). |
| `Vault::ask_agent` (or `ask` with an `edit: bool`), `Vault::apply_edits`, `Vault::undo_edit`, `Vault::edit_journal` | `b2-core` (`vault/chat.rs`, `vault/edits.rs`) | Writes go through `save_guarded`. |
| View types: `EditProposal`, `EditRecord`, `AnswerView.edits` | `b2-core` views, reused as IPC and `--json` | One contract, two adapters. |
| IPC commands: `ask` gains `edit`/`focus`, plus `apply_edits`, `undo_edit`, `list_edits` | `b2-desktop` | Parse, call, return. |
| Diff rendering, proposal cards, Changes list, editor highlights, keys for Apply/Undo | `ui/` (`chat.ts`, `chatview.ts`, new `edits.ts`) | Pure logic in tested modules; K1 keyboard-complete. |
| `--edit`, `b2 edits` | `b2-cli` | |
| Eval cases | `docs/evals` harness | See below. |

## Measure before trusting it (ADR-0013)

Whether gemma4:e2b can drive `b2_edit` well is a quality question, so it is answered by the
harness, not by intuition. New cases on the eval corpus:

- **Clean match rate.** Of the edits a model proposes on a set of "clean up" prompts, how
  many apply without a uniqueness or no-match error.
- **Preservation.** After the edits, every `[[wikilink]]`, heading and frontmatter key the
  note had is still present unless the prompt asked to remove it.
- **Judged quality.** A model judge scores the cleaned note against the prompt, as the
  harness scores answers today.
- **Honesty.** A prompt the model cannot satisfy (nothing to clean) yields no proposals and
  an answer that says so.

If the default model scores poorly, review-first still makes the feature safe, but the eval
decides whether edit mode is on by default or behind the model tiers from PR #250.

## Phases

1. **Decide.** ADR amending W2/W3 and clarifying S4. Update `invariants.md`,
   `architecture.md` ("the write discipline"), `index-engine.md` §6.
2. **Phase 1: propose and accept.** #253 focus note; agent `ask` with `b2_read`,
   `b2_search`, `b2_neighbors`, `b2_edit`; proposal cards with diffs; Apply/Apply all/Skip;
   in-memory journal; per-edit Undo in the Changes list; CLI `--edit` and `--json`; `FakeLlm`
   coverage; first eval cases.
3. **Phase 2: durable and visible.** Journal under `.b2/edits/`; undo across restarts;
   editor gutter marks for landed edits; Undo all from this answer; the *apply as you go*
   toggle; Settings for caps.
4. **Phase 3: wider authoring.** `b2_create_note`; whole-note
   rewrite tool for short notes; multi-note batch prompts; eval results drive the default.

## Decisions taken (2026-11-08)

1. **Default mode:** review-first, with a one-gesture opt-out to *apply as you go* for the
   rest of the conversation.
2. **The journal** lives under `.b2/edits/` as disposable cache. S4 is clarified, not
   changed. Git is not used.
3. **Edit grain:** find/replace hunks with the uniqueness rule.

## Out of scope here

Deleting, moving or renaming notes from chat (W4 unchanged). Editing frontmatter from chat
(typed links already have `link`). Editing resources. A persisted chat transcript (S4).
