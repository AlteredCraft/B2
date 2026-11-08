# Implementation plan: chat edits notes, Phase 1 (GH #255)

The build order for ADR-0023's Phase 1, one PR per child issue, each shippable alone.
Written against the code as of `f9b34d5`. Design: [the proposal](2026-11-08-chat-edits-notes.md).

## The one structural decision the code forces

`Vault` is opened fresh for every desktop command (`open_semantic(state)` in
`commands.rs`) and the core has no clock (E1). So the per-conversation state (the working
copy, pending proposals, the undo journal) cannot live inside `Vault`. It is a value type the
core defines and the **adapter owns**: `b2_core::edit::EditSession`. The desktop holds one in
`AppState` behind a mutex and hands `&mut` to each façade call; the CLI holds one as a local
in `cmd_chat`. All logic stays in the core (E3); the adapter only stores the value and passes
in the time. "New conversation" drops the session, and with it the pending proposals (not the
landed edits: those are on disk, and in Phase 1 their undo records go with the session, which
the pane says plainly).

## PR 1: focus note (#253)

The smallest change, and every later PR needs it.

- `vault/chat.rs`: `ask` gains `focus: Option<&str>`. When set, resolve it, take its passages
  (`db::note_chunk_ids` + `chunk_detail`, as `tool_read` does) up to `FOCUS_PASSAGES` (new
  constant, default 8, in `chat.rs`'s constants block), number them first, fill the rest from
  `search_chunks` minus duplicates. `chat::build_request` gains the focus path and appends one
  sentence to the system prompt: "The user is viewing `<path>`; 'this note' or 'this doc' means
  it." `condense_query` gets the same sentence.
- If the cap trimmed the note, the system message says so in one line (limits are loud).
- `commands.rs` `ask` takes `focus: Option<String>`; `api.ts`, `main.ts` `sendChat` passes
  `state.current?.path` when the open document is a note. The pane shows an "about: title"
  chip beside the composer, so what the model was told is visible (`chatview.ts`).
- CLI: `b2 ask --note PATH`, `b2 chat --note PATH` (`args.rs`, `chat.rs`).
- Tests: `tests/ask.rs` pins that no focus is byte-identical to today's request (compare
  `ChatRequest` through a recording `LlmProvider`); with focus, passage 1 is the focus note and
  the prompt names the path; a focus of more than `FOCUS_PASSAGES` passages adds the trim line.
  `quickstart.md`: the flag. Eval case lands with PR 6.

## PR 2: `edit.rs`, the pure half (#258)

No façade, no IO. Everything here runs against strings.

```rust
pub struct Hunk { pub find: String, pub replace: String }
pub enum HunkError { NoMatch, Ambiguous(usize), Empty }
pub fn apply_hunk(body: &str, h: &Hunk) -> Result<(String, Span), HunkError>
pub fn reverse(h: &Hunk) -> Hunk
pub fn unified_diff(before: &str, after: &str, path: &str) -> String   // `similar` crate
pub struct EditProposal { id, path, hunk, line: usize, diff: String, base_revision: String, status: EditStatus }
pub enum EditStatus { Pending, Applied, Undone, Conflict }
pub struct EditRecord { proposal: EditProposal, before: String, after: String, revision_before, revision_after, applied_at: String }
pub struct EditSession { working: BTreeMap<String, String>, proposals: Vec<EditProposal>, journal: Vec<EditRecord>, max_edits_per_turn: usize }
```

- `EditSession::propose(path, disk_body, disk_revision, hunk) -> Result<&EditProposal, HunkError>`
  applies the hunk to the working copy (disk body on first touch), records the proposal, and
  returns it. `pending_for(path)`, `working_body(path)`. Cap: the N+1th `propose` in a turn
  returns `HunkError::Cap(n)`; `begin_turn()` resets the count.
- `Empty` refuses `find == ""` (it would match everywhere) and `find == replace`.
- Body only by construction: callers pass `ParsedNote::body()`, never the raw file.
- `chat.rs`: `TOOL_EDIT = "b2_edit"`, its `ToolSpec` (`note` defaults to the focus note;
  `find` and `replace` required), `TOOL_SEARCH = "b2_search"` spec, `EDIT_AGENT_SYSTEM_PROMPT`.
- `llm.rs` `FakeLlm`: on an agent request that offers `b2_edit`, has at least one passage and
  no `b2_edit` exchange yet, emit one call with `find` = the first line of passage 1 and
  `replace` = that line plus ` (edited)`. Deterministic, so the suite covers the loop (E2).
- `Cargo.toml` (`b2-core`): add `similar` for the diff text. No other dependency.
- Tests, `tests/edit.rs`: apply, reverse round trip (`proptest` over bodies and hunks, as
  `props.rs` does), ambiguity count, no match, empty, frontmatter never in reach, cap fires on
  the N+1th call and not the Nth (the gate-falsification habit), diff text names the path.

## PR 3: the façade (#256)

`vault/edits.rs`, new, plus additions to `vault/chat.rs`.

- `Vault::ask_edit(llm, question, history, focus: Option<&str>, session: &mut EditSession, on_token) -> AnswerView`:
  the ADR-0022 loop generalised. Extract the loop from `why_similar` into a private
  `run_agent_turn(req, tools, desk, run_tool: impl FnMut(..))` so the two turns share it, then
  `why_similar` is the first caller and `ask_edit` the second. Tools: `b2_read` (reads the
  **working copy** when the session has one for that note), `b2_search` (`search_chunks` at
  `ASK_PASSAGES`, numbered on the ledger), `b2_neighbors`, `b2_edit`. Round 1 is not hidden
  here: this turn's text is the model's running commentary, so stream from round 1 and let
  the last round offer no tools, as today.
- Degrade: a provider that refuses tools gets `ask` with focus and `edits: []`.
- `AnswerView` gains `edits: Vec<EditProposal>` (serde-skipped when empty, so `why`'s JSON is
  unchanged).
- `Vault::apply_edits(session, path, ids: &[String], base_revision, now: &str) -> ApplyReport`:
  resolve, read disk, check revision (else `WriteConflict`, proposals → `Conflict`), apply the
  named pending hunks in proposal order to the **disk** body (a hunk that no longer matches →
  `Conflict`, nothing written), then `save_guarded` with the new body. Record one `EditRecord`
  per hunk with before/after bytes and both revisions, status `Applied`. Drop the working
  copy for that path (disk is the truth again).
- `Vault::undo_edit(session, id, now) -> UndoReport`: the three rules. Rule 3 is
  `Error::UndoRefused { path }` and the report carries `prior_text` for the UI to show.
  Status → `Undone`; a redo is `propose` + `apply` of the recorded hunk, nothing new.
- `Vault::edit_journal(session) -> Vec<EditRecord>` is a borrow, not a façade op; the adapters
  serialise the session's view directly.
- Errors: `Error::UndoRefused`, `Error::EditLimit { limit }` (never degraded, like
  `ToolCallLimit`). Both mapped in `b2-cli/error.rs` and `b2-desktop/error.rs` with generic,
  actionable text.
- Tests, `tests/ask_edit.rs` on a golden-vault copy with `FakeLlm`: the turn yields one pending
  proposal and the vault bytes are unchanged (hash the tree before and after); `apply_edits`
  lands exactly the hunk and nothing else (diff the file); apply against a changed file is
  `WriteConflict` and writes nothing; undo rules 1, 2, 3 each with a test named for the rule;
  a provider that refuses tools degrades to `ask` with `edits: []`; the edit cap fires.
- Docs: `index-engine.md` §6 gets the `ask_edit` tool table beside `why_similar`'s.

## PR 4: desktop (#259)

- `AppState`: `edits: Mutex<EditSession>`; `new_conversation` IPC (or the existing "new")
  replaces it. Cap read from Settings as `max_tool_calls` is (`chat.rs` prefs, `apply_tool_cap`
  pattern), env `B2_LLM_MAX_EDITS_PER_TURN`.
- Commands: `ask` gains `focus`, `edit: bool`; new `apply_edits(path, ids, base_revision)`,
  `undo_edit(id)`, `list_edits()`. Each is parse, lock, one façade call, return. The clock is
  the host's (`chrono`-free: `SystemTime` to RFC 3339 via `time`, already in the tree? check
  `Cargo.lock`; else pass seconds).
- `ui/src/types.ts`: `EditProposal`, `EditRecord`, `AnswerView.edits`.
- `ui/src/edits.ts` (pure, tested): proposal rows (`chat:edit:<turn>:<id>` keys, so they join
  `chatRows` and inherit the keyboard walk), buttons by status, grouping of the journal by
  note, and `applyAllIds(turn)`.
- `chatview.ts`: a card per proposal under its turn: title, path, diff in a `<pre>` with
  per-line `del`/`ins` classes computed from the diff text (every line through `escapeHtml`),
  status chip, *Apply* / *Skip* buttons, *Apply all* on the turn. A Changes section above the
  log when the journal is non-empty: grouped by note, newest first, *Undo* per row, a line
  "Undo history lasts for this conversation" (Phase 1 honesty).
- `main.ts`: apply → `api.applyEdits(path, ids, revision)` where `revision` is
  `state.current.revision` if the note is open, else a fresh `api.read`'s; on success chain
  the revision as a save does and re-read the note (the `afterSave` path at ~1159); on
  `WriteConflict` mark the cards *conflict* and flash the existing message. Undo mirrors it. A
  dirty editor buffer on the same note: flush first (as the link path at ~2196 does).
- Chat pane: an "Edit mode" toggle next to "new" (off → plain `ask`), the "about:" chip
  from PR 1, and the intro text loses "Nothing here is written to your notes" in favour of
  "Edits are proposed as diffs and land only when you apply them."
- `bindings.ts`: chords for Apply, Skip, Apply all, Undo on the focused row; the sheet test
  enforces documentation. Focus restored by row key across the repaint.
- Settings → Chat: "Edits per reply" field, the `toolCapInput` three-state pattern.
- Verify: `ui/` suite; the real `ui/` in Chrome against a mocked `__TAURI_INTERNALS__` fed
  `--json` proposals; then `make app` with `B2_LLM=fake` end to end.

## PR 5: CLI (#260)

- `args.rs`: `--edit` on `Ask` and `Chat`; `Edits { List | Apply { ids } | Undo { id } }`.
- `chat.rs`: `cmd_ask` with `--edit` prints the answer, then each proposal's diff; in a TTY,
  `apply? [y/n/a]` per proposal, reading stdin; `--json` emits the `AnswerView` and exits.
  `cmd_chat` keeps one `EditSession` for the REPL and adds `/apply`, `/apply all`, `/undo ID`,
  `/edits`. `b2 edits …` outside a REPL reports "no edits in this session" until Phase 2.
- Writes require an explicit vault, as `write.rs` commands do (`require_vault`).
- Tests, `crates/b2-cli/tests/cli.rs` with `B2_LLM=fake`: `ask --edit --json` emits one pending
  proposal and leaves the vault tree's hash unchanged; piping `y` applies exactly the hunk.
- `quickstart.md`: the flags and `B2_LLM_MAX_EDITS_PER_TURN`.

## PR 6: eval (#257)

- Corpus prompts: ten "clean up / tighten / fix headings" tasks on existing eval notes, each
  with the expected preserved set (wikilinks, headings, frontmatter keys) and a short rubric.
- Harness: run `b2 ask --edit --json --note` per task per model tier, apply all proposals to a
  temp copy, score: clean match rate, preservation, judged quality, honesty (two tasks with
  nothing to fix must yield zero proposals). Plus the #253 focus case.
- Record the run in `docs/evals.md`; the result decides the default for edit mode.

## Order and gates

| PR | Issue | Gate before merge |
|---|---|---|
| 1 | #253 | `make ci`; no-focus request byte-identical; "clean up this doc" answers about the open note in the app |
| 2 | #258 | `make ci`; cap and ambiguity tests fail on a broken input first |
| 3 | #256 | `make ci`; vault-hash-unchanged test; undo rules 1 to 3 |
| 4 | #259 | `make ci`; browser mock run; `make app` with the fake provider end to end |
| 5 | #260 | `make ci`; CLI fake-provider tests |
| 6 | #257 | harness run recorded; default-mode decision written into #255 |

PRs 2 and 5's arg parsing can proceed in parallel with 1; 3 needs 1 and 2; 4 and 5 need 3.
