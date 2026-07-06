//! The `#[tauri::command]` handlers — B2's IPC surface, and the frontend's mirror of
//! the [`Vault`](b2_core::vault::Vault) façade (specs/desktop-ui-mvp.md §3). Each
//! handler is **deserialize → call one façade method → serialize**: no branch, no
//! loop, no rule. If a handler ever needs one, that logic belongs behind the façade
//! in `b2-core` (add a façade op, not host logic) — that is the whole discipline that
//! keeps the GUI and CLI from drifting. The façade already returns `Serialize` views
//! (the CLI's `--json` types), so Tauri hands them to the webview directly — the IPC
//! contract is nearly free (no parallel DTO layer).
//!
//! Every data command is `#[tauri::command(async)]` so Tauri runs it **off the main
//! thread** — a slow `search` (model load) or `reindex` (embedding) never freezes the
//! window. The bodies stay fully synchronous (no `async`/`tokio` in our code, per the
//! repo's no-speculative-async rule); `(async)` is only the "don't block the UI" knob.
//!
//! The thin `*_impl` split lets the command layer be unit-tested against a real vault
//! without a Tauri runtime (the `State` wrapper is only in the one-line `#[command]`).

use crate::error::CmdError;
use crate::{open_vault, AppState};
use b2_core::vault::{
    ExplainView, LinkReport, NeighborView, NoteSummary, NoteView, ReindexReport, SearchResult,
    SimilarView,
};
use serde::Serialize;
use std::path::Path;
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

#[tauri::command(async)]
pub fn reindex(state: State<'_, AppState>) -> Result<ReindexReport, CmdError> {
    // Embeds changed chunks → needs the real model.
    let (vault, _) = open_vault(state.inner(), true)?;
    Ok(vault.reindex()?)
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
fn set_vault_root_impl(state: &AppState, root: &Path) -> Result<VaultInfo, CmdError> {
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

#[cfg(test)]
mod tests {
    //! Thin command-layer tests: args resolve → the façade is called → a view comes
    //! back (specs/desktop-ui-mvp.md §7 — "thinness *is* the test strategy"; the
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
}
