//! The `#[tauri::command]` handlers — B2's IPC surface, and the frontend's mirror of
//! the [`Vault`](b2_core::vault::Vault) façade (specs/completed/desktop-ui-mvp.md §3). Each
//! handler is **deserialize → call one façade method → serialize**: no branch, no
//! loop, no rule. If a handler ever needs one, that logic belongs behind the façade
//! in `b2-core` (add a façade op, not host logic) — that is the whole discipline that
//! keeps the GUI and CLI from drifting. The façade already returns `Serialize` views
//! (the CLI's `--json` types), so Tauri hands them to the webview directly — the IPC
//! contract is nearly free (no parallel DTO layer).
//!
//! Every data command is `#[tauri::command(async)]` so Tauri runs it **off the main
//! thread** — a slow `search` (model load) or `embed` (embedding) never freezes the
//! window. The bodies stay fully synchronous (no `async`/`tokio` in our code, per the
//! repo's no-speculative-async rule); `(async)` is only the "don't block the UI" knob.
//!
//! The thin `*_impl` split lets the command layer be unit-tested against a real vault
//! without a Tauri runtime (the `State` wrapper is only in the one-line `#[command]`).

use crate::error::CmdError;
use crate::{open_vault, AppState};
use b2_core::ingest::ReindexProgress;
use b2_core::vault::{
    EmbedReport, ExplainView, LinkReport, NeighborView, NoteSummary, NoteView, ProjectReport,
    SearchResult, SimilarView, WriteReport,
};
use serde::Serialize;
use std::ops::ControlFlow;
use std::path::Path;
use tauri::ipc::Channel;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

/// The active vault's root + whether semantic ranking is live (real model), for the
/// UI header and honest empty states (mirrors `b2 search`'s "semantic off" caveat).
#[derive(Debug, Clone, Serialize)]
pub struct VaultInfo {
    pub root: String,
    pub semantic: bool,
}

/// Step 0's seam-proving command: the frontend `invoke('ping')` round-trips this to
/// confirm the Rust↔JS bridge before any real surface exists.
#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

#[tauri::command(async)]
pub fn vault_info(state: State<'_, AppState>) -> Result<VaultInfo, CmdError> {
    vault_info_impl(state.inner())
}

/// The in-app vault switcher: open a **native folder picker** and, if the user picks a
/// folder, point the app at it (every later command opens over the new root). Returns
/// the new [`VaultInfo`] on success, or `None` when the user cancels — the UI then
/// leaves the current vault untouched.
///
/// Host-owned by design: vault-root resolution is this crate's job (main.rs), and the
/// picker is an OS concern, so this is a legitimate host responsibility, not engine
/// logic (there is nothing here to push behind the façade). Running the dialog in Rust
/// (not the webview) is also what keeps the webview dialog-permission-free.
///
/// `(async)` runs this off the main thread, which is *required*: `blocking_pick_folder`
/// waits on the main thread to show the panel, so calling it from the main thread would
/// deadlock.
#[tauri::command(async)]
pub fn choose_vault(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<VaultInfo>, CmdError> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None); // user cancelled
    };
    // On desktop a folder pick is always a real filesystem path (`Url` is a mobile
    // content URI); if it somehow isn't, treat it as a cancel rather than error out.
    let Ok(path) = picked.into_path() else {
        return Ok(None);
    };
    Ok(Some(set_vault_root_impl(state.inner(), &path)?))
}

#[tauri::command(async)]
pub fn read_note(state: State<'_, AppState>, note: String) -> Result<NoteView, CmdError> {
    read_note_impl(state.inner(), &note)
}

#[tauri::command(async)]
pub fn list_notes(state: State<'_, AppState>) -> Result<Vec<NoteSummary>, CmdError> {
    list_notes_impl(state.inner())
}

/// Save a note's body — the editing surface's one write (desktop-editing.md §5).
/// **Model-free** like `project`: `Vault::write` splices the body and re-projects
/// without touching vectors, so this opens the fake vault (no model load; saving
/// works with nothing provisioned) and runs **outside** the single-in-flight embed
/// slot (short, and safe against a racing vault switch for the same
/// captured-root reason as `project`). A stale `base_revision` surfaces as the
/// **stable** conflict message the frontend recognizes to drive its conflict bar —
/// change it in `error.rs` and `ui/src/api.ts` together.
#[tauri::command(async)]
pub fn write_note(
    state: State<'_, AppState>,
    note: String,
    body: String,
    base_revision: String,
) -> Result<WriteReport, CmdError> {
    write_note_impl(state.inner(), &note, &body, &base_revision)
}

#[tauri::command(async)]
pub fn similar(
    state: State<'_, AppState>,
    note: String,
    limit: usize,
) -> Result<Vec<SimilarView>, CmdError> {
    let (vault, _) = open_vault(state.inner(), false)?;
    Ok(vault.similar(&note, limit)?)
}

#[tauri::command(async)]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<SearchResult>, CmdError> {
    // Semantic: the query is embedded, so this opens the real model (fail-fast if absent).
    let (vault, _) = open_vault(state.inner(), true)?;
    Ok(vault.search(&query, limit)?)
}

#[tauri::command(async)]
pub fn neighbors(state: State<'_, AppState>, note: String) -> Result<Vec<NeighborView>, CmdError> {
    let (vault, _) = open_vault(state.inner(), false)?;
    Ok(vault.neighbors(&note)?)
}

#[tauri::command(async)]
pub fn explain(state: State<'_, AppState>, note: String) -> Result<ExplainView, CmdError> {
    let (vault, _) = open_vault(state.inner(), false)?;
    Ok(vault.explain(&note)?)
}

#[tauri::command(async)]
pub fn link(
    state: State<'_, AppState>,
    src: String,
    dst: String,
    relation: String,
    explanation: Option<String>,
) -> Result<LinkReport, CmdError> {
    // Re-projects the source note → opens the same real model the index was built with.
    let (vault, _) = open_vault(state.inner(), true)?;
    Ok(vault.link(&src, &dst, &relation, explanation.as_deref())?)
}

/// The **projection pass** — the fast, model-free half of a reindex
/// (projection-embedding-split.md §6). One façade call over the **fake** vault (no
/// model load on the first-paint path), so the moment it returns the file tree can
/// repopulate and keyword search answers; `embed` then streams behind it. Fast and
/// synchronous-feeling; nothing to stream, nothing to cancel.
///
/// Deliberately **outside** the single-in-flight reindex slot: the slot exists to
/// protect the long, vector-writing embed pass, and a `project` racing a vault
/// switch is harmless — it writes the `.b2/` of the root it captured at dispatch,
/// idempotently, never the new vault's (§6 "why leaving `project` outside the slot
/// is safe").
#[tauri::command(async)]
pub fn project(state: State<'_, AppState>) -> Result<ProjectReport, CmdError> {
    project_impl(state.inner())
}

/// The **embed pass** — fill the missing vectors as an **observable, cancellable
/// background action** (async-indexing.md §4). Tauri runs the `(async)` body on a
/// worker thread, so the window stays live; progress streams to the webview over
/// `on_event` (a typed, per-invocation [`Channel`]), and the closure returns
/// `ControlFlow::Break` once the shared cancel flag is set — the one cancel
/// checkpoint the core exposes. This is the old fused `reindex` command minus the
/// projection it no longer does; the guard/cancel machinery attaches here.
///
/// Still a dumb adapter: the body is "claim the slot → open one vault → call one
/// façade op → serialize," with progress forwarded and a flag consulted. Task
/// spawn/track/cancel + IPC streaming are host infrastructure (same class as the root
/// `Mutex` and the OS dialog), not engine logic.
#[tauri::command(async)]
pub fn embed(
    state: State<'_, AppState>,
    on_event: Channel<ReindexProgress>,
) -> Result<EmbedReport, CmdError> {
    embed_impl(state.inner(), &on_event)
}

/// Ask the in-flight embed to stop at its next batch boundary. Runs on a *different*
/// worker thread than `embed`, so it observes/sets the shared flag concurrently; the
/// embed closure sees it and breaks cooperatively — no thread-killing, no torn
/// writes (async-indexing.md §4/§5.6). A no-op if nothing is running.
#[tauri::command(async)]
pub fn cancel_reindex(state: State<'_, AppState>) {
    state.request_reindex_cancel();
}

/// Releases the single-in-flight reindex slot on drop, so it is freed on **every**
/// exit path — normal return, an early `?` (e.g. model-not-provisioned), or a panic.
struct ReindexGuard<'a>(&'a AppState);
impl Drop for ReindexGuard<'_> {
    fn drop(&mut self) {
        self.0.finish_reindex();
    }
}

/// The testable core of `project`: one façade call over the fake vault (projection
/// is model-free by construction — it never touches the embedding space).
fn project_impl(state: &AppState) -> Result<ProjectReport, CmdError> {
    let (vault, _) = open_vault(state, false)?;
    Ok(vault.project(false)?)
}

/// The testable core of `embed`, split from the Tauri `State` wrapper. Guards
/// single-in-flight, arms the cancel flag, opens the real-model vault, and streams
/// progress while consulting the cancel flag at each batch.
fn embed_impl(
    state: &AppState,
    on_event: &Channel<ReindexProgress>,
) -> Result<EmbedReport, CmdError> {
    // Single-in-flight: refuse a second embed rather than race two writers on one DB
    // (async-indexing.md §5.4). The UI also disables the button, so this is rarely hit.
    if !state.try_start_reindex() {
        return Err(CmdError::ReindexInFlight);
    }
    let _guard = ReindexGuard(state);
    // Clear any stale cancel now that *this* run owns the slot (a prior switch/cancel
    // must not kill a fresh embed).
    state.arm_reindex();

    // Fills missing vectors → needs the real model.
    let (vault, _) = open_vault(state, true)?;
    Ok(vault.embed(&mut |p| {
        // Forward progress to the webview; a send error (the window navigated/closed)
        // is not fatal to the index — keep embedding.
        let _ = on_event.send(p);
        if state.reindex_cancelled() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?)
}

// --- thin impls (Tauri-runtime-free, so the command layer is unit-testable) -------

fn vault_info_impl(state: &AppState) -> Result<VaultInfo, CmdError> {
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    Ok(VaultInfo {
        root: root.display().to_string(),
        semantic: crate::semantic_available(),
    })
}

/// Set the active vault root and report the resulting [`VaultInfo`] — the testable core
/// of `choose_vault`, split off from the (untestable) OS dialog. The picker only yields
/// existing directories, so no validation is needed here; the switch takes effect for
/// every subsequent command via [`AppState::current_root`].
///
/// **Cancels any in-flight reindex first**, waiting for it to wind down before
/// repointing the root, so a reindex can never keep writing the vault the app has left
/// (async-indexing.md §4/§5.4).
fn set_vault_root_impl(state: &AppState, root: &Path) -> Result<VaultInfo, CmdError> {
    state.cancel_and_wait_for_reindex();
    state.set_root(root);
    vault_info_impl(state)
}

fn read_note_impl(state: &AppState, note: &str) -> Result<NoteView, CmdError> {
    let (vault, _) = open_vault(state, false)?;
    Ok(vault.read(note)?)
}

fn list_notes_impl(state: &AppState) -> Result<Vec<NoteSummary>, CmdError> {
    let (vault, _) = open_vault(state, false)?;
    Ok(vault.list_notes()?)
}

fn write_note_impl(
    state: &AppState,
    note: &str,
    body: &str,
    base_revision: &str,
) -> Result<WriteReport, CmdError> {
    let (vault, _) = open_vault(state, false)?;
    Ok(vault.write(note, body, base_revision)?)
}

#[cfg(test)]
mod tests {
    //! Thin command-layer tests: args resolve → the façade is called → a view comes
    //! back (specs/completed/desktop-ui-mvp.md §7 — "thinness *is* the test strategy"; the
    //! façade's own suite covers behavior). Model-free: read-path commands open with
    //! the fake, and setup reindexes with the fake directly, so no model is needed.

    use super::*;
    use crate::error::user_message;
    use b2_core::vault::Vault;
    use std::fs;
    use std::path::Path;

    /// Copy the committed golden vault into `dst` (never mutate the repo fixtures),
    /// then reindex it with the fake embedder so the read path has an index to resolve.
    fn golden_indexed(root: &Path) {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
        copy_dir(&src, root);
        Vault::open(root).unwrap().reindex().unwrap();
    }

    fn copy_dir(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir(&from, &to);
            } else {
                fs::copy(&from, &to).unwrap();
            }
        }
    }

    #[test]
    fn read_note_resolves_and_calls_the_facade() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let note = read_note_impl(&state, "concepts/memory").unwrap();
        assert_eq!(note.title.as_deref(), Some("Human memory"));
        assert!(note.body.contains("The brain encodes"));
    }

    #[test]
    fn list_notes_returns_the_vault_listing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let notes = list_notes_impl(&state).unwrap();
        let paths: Vec<&str> = notes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["concepts/memory.md", "notes/spaced-repetition.md"]
        );
        assert_eq!(notes[0].title.as_deref(), Some("Human memory"));
    }

    #[test]
    fn commands_without_a_vault_are_a_clean_refusal() {
        let state = AppState::new(None);
        let err = read_note_impl(&state, "anything").unwrap_err();
        assert!(matches!(err, CmdError::VaultRequired));
        // …surfaced to the webview as an actionable, no-internals message.
        assert_eq!(
            user_message(&err),
            "No vault open. Launch B2 with a vault path, or set B2_VAULT_PATH to your vault folder."
        );
    }

    #[test]
    fn set_vault_root_switches_the_active_vault() {
        // Start with no vault (the actionable-refusal state)…
        let state = AppState::new(None);
        assert!(matches!(
            list_notes_impl(&state).unwrap_err(),
            CmdError::VaultRequired
        ));

        // …then point it at a real vault: the switch reports the new root, and every
        // later command resolves against it (proves `set_root` takes effect downstream).
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let info = set_vault_root_impl(&state, &root).unwrap();
        assert_eq!(info.root, root.display().to_string());

        let notes = list_notes_impl(&state).unwrap();
        let paths: Vec<&str> = notes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["concepts/memory.md", "notes/spaced-repetition.md"]
        );
    }

    #[test]
    fn switching_vaults_repoints_subsequent_reads() {
        // Two distinct vaults; switching from one to the other must change what reads
        // resolve — the whole point of a runtime-swappable root.
        let tmp = tempfile::TempDir::new().unwrap();
        let first = tmp.path().join("first");
        golden_indexed(&first);
        let state = AppState::new(Some(first.clone()));
        assert!(read_note_impl(&state, "concepts/memory").is_ok());

        let second = tmp.path().join("second");
        fs::create_dir_all(&second).unwrap();
        fs::write(second.join("solo.md"), "# Solo\n\nOnly note here.\n").unwrap();
        Vault::open(&second).unwrap().reindex().unwrap();

        set_vault_root_impl(&state, &second).unwrap();
        // The first vault's note is gone from the newly-active vault…
        assert!(matches!(
            read_note_impl(&state, "concepts/memory").unwrap_err(),
            CmdError::Core(b2_core::Error::NoteNotFound(_))
        ));
        // …and the second vault's note resolves.
        let notes = list_notes_impl(&state).unwrap();
        let paths: Vec<&str> = notes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(paths, vec!["solo.md"]);
    }

    #[test]
    fn ping_round_trips() {
        assert_eq!(ping(), "pong");
    }

    #[test]
    fn errors_stay_generic_and_leak_no_internals() {
        // A missing note is actionable, and never exposes sqlite/io detail.
        let msg = user_message(&CmdError::Core(b2_core::Error::NoteNotFound(
            "x/y".to_string(),
        )));
        assert!(msg.contains("Note not found: 'x/y'"));
        assert!(!msg.to_lowercase().contains("sqlite"));
    }

    // --- async-indexing: the host's task-lifecycle bits (§4) ----------------------
    //
    // Thin host-infrastructure tests: the guard + cancel state machine and
    // switch-cancels-first, all model-free (no reindex actually runs — the core's own
    // suite covers the cancel *behavior*; here we prove the host's control bits).

    #[test]
    fn reindex_slot_is_single_in_flight() {
        let state = AppState::new(None);
        assert!(state.try_start_reindex(), "first claim wins the slot");
        assert!(state.reindex_in_flight());
        assert!(
            !state.try_start_reindex(),
            "second claim is refused while running"
        );
        state.finish_reindex();
        assert!(!state.reindex_in_flight());
        assert!(state.try_start_reindex(), "slot is reusable once released");
    }

    #[test]
    fn a_second_embed_is_refused_before_touching_the_model() {
        // With the slot already held, `embed_impl` must refuse *before* opening the
        // real-model vault — so this needs no model and can't hang on one.
        let state = AppState::new(None);
        assert!(state.try_start_reindex()); // stand in for a running embed
        let channel = Channel::<ReindexProgress>::new(|_| Ok(()));
        let err = embed_impl(&state, &channel).unwrap_err();
        assert!(matches!(err, CmdError::ReindexInFlight));
        assert_eq!(
            user_message(&err),
            "A reindex is already in progress. Please wait for it to finish."
        );
    }

    #[test]
    fn write_note_saves_through_the_facade_and_chains_revisions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        // Save based on the read's revision; the returned revision chains the next.
        let note = read_note_impl(&state, "concepts/memory").unwrap();
        let report = write_note_impl(
            &state,
            "concepts/memory",
            "An edited body.\n",
            &note.revision,
        )
        .unwrap();
        assert_ne!(report.revision, note.revision);
        let reread = read_note_impl(&state, "concepts/memory").unwrap();
        assert_eq!(reread.body, "An edited body.\n");
        assert_eq!(reread.revision, report.revision);
        write_note_impl(&state, "concepts/memory", "Again.\n", &report.revision).unwrap();
    }

    #[test]
    fn write_conflict_is_generic_and_recognizable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let note = read_note_impl(&state, "concepts/memory").unwrap();
        // An external edit lands after the read…
        let abs = root.join("concepts/memory.md");
        fs::write(
            &abs,
            format!("{}\nexternal\n", fs::read_to_string(&abs).unwrap()),
        )
        .unwrap();

        // …so the stale save is refused with the STABLE message the frontend
        // string-matches to drive its conflict bar (desktop-editing.md §5) — keep
        // this assertion in lockstep with ui/src/api.ts.
        let err = write_note_impl(&state, "concepts/memory", "mine", &note.revision).unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::WriteConflict(_))
        ));
        assert_eq!(
            user_message(&err),
            "This note changed on disk since it was opened. Reload the note, then reapply your edit."
        );
    }

    #[test]
    fn write_note_runs_outside_the_reindex_slot() {
        // Like `project`, a save is deliberately unguarded by the embed slot
        // (desktop-editing.md §5): short, model-free, must not queue behind a
        // long-running background embed.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        assert!(state.try_start_reindex()); // stand in for an in-flight embed
        let note = read_note_impl(&state, "concepts/memory").unwrap();
        write_note_impl(
            &state,
            "concepts/memory",
            "Saved mid-embed.\n",
            &note.revision,
        )
        .unwrap();
        assert_eq!(
            read_note_impl(&state, "concepts/memory").unwrap().body,
            "Saved mid-embed.\n"
        );
    }

    #[test]
    fn project_is_model_free_and_runs_outside_the_reindex_slot() {
        // A fresh (never-indexed) vault copy — no reindex in the setup, so this also
        // proves `project` alone is what makes the tree listable.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
        copy_dir(&src, &root);
        let state = AppState::new(Some(root));

        // Hold the slot (a stand-in for an in-flight embed): `project` is deliberately
        // unguarded (projection-embedding-split.md §6) and must still run.
        assert!(state.try_start_reindex());
        let report = project_impl(&state).unwrap();
        assert_eq!(report.indexed, 2);
        assert_eq!(report.stamped, 0, "golden notes already carry b2ids");

        // The tree is live off projection alone — no model, no vectors.
        let notes = list_notes_impl(&state).unwrap();
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn arm_clears_a_stale_cancel_but_request_sets_it() {
        let state = AppState::new(None);
        state.request_reindex_cancel();
        assert!(state.reindex_cancelled());
        // A fresh run arming the slot clears a cancel left by a prior switch/cancel…
        state.arm_reindex();
        assert!(!state.reindex_cancelled());
        // …and a new cancel request is then observable again.
        state.request_reindex_cancel();
        assert!(state.reindex_cancelled());
    }

    #[test]
    fn vault_switch_cancels_and_waits_for_the_inflight_reindex() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        fs::create_dir_all(&root).unwrap();
        let state = AppState::new(Some(root.clone()));

        // Simulate a reindex holding the slot.
        assert!(state.try_start_reindex());
        assert!(state.reindex_in_flight());

        std::thread::scope(|s| {
            // A stand-in reindex worker: spin until asked to cancel, then wind down —
            // exactly what the real embed-loop closure does at a batch boundary.
            s.spawn(|| {
                while !state.reindex_cancelled() {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                state.finish_reindex();
            });

            // Switching vaults must request cancel AND block until the worker released
            // the slot, *before* it repoints the root and reports the new info.
            let info = set_vault_root_impl(&state, &root).unwrap();
            assert_eq!(info.root, root.display().to_string());
            // If the switch returned, the in-flight run has already wound down.
            assert!(!state.reindex_in_flight());
        });
    }
}
