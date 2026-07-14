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
use crate::watch::VaultWatcher;
use crate::{open_vault, AppState};
use b2_core::ingest::ReindexProgress;
use b2_core::vault::{
    EmbedReport, ExplainView, LinkReport, NeighborView, NoteSummary, NoteView, ProjectReport,
    ResourceExplainView, ResourceSummary, SearchResult, SimilarView, WriteReport,
};
use b2_embed::{EmbedConfig, ModelChoice};
use serde::Serialize;
use std::ops::ControlFlow;
use std::path::Path;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

/// The active vault's root + whether semantic ranking is live (real model), for the
/// UI header and honest empty states (mirrors `b2 search`'s "semantic off" caveat).
///
/// `semantic` answers "is the real model installed"; `notes_embedded`/`notes_total`
/// answer the *precise* "how much of this vault is actually embedded" (#26), so the UI
/// can flag search as "keyword-only for now" while a projected vault embeds behind the
/// first tree paint — not just under the fake embedder. A model-free count (the façade's
/// `embed_status` read).
#[derive(Debug, Clone, Serialize)]
pub struct VaultInfo {
    pub root: String,
    pub semantic: bool,
    pub notes_embedded: usize,
    pub notes_total: usize,
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
/// folder, point the app at it (every later command opens over the new root) and
/// **remember** it so the next launch reopens it. Returns the new [`VaultInfo`] on
/// success, or `None` when the user cancels — the UI then leaves the current vault
/// untouched.
///
/// Host-owned by design: vault-root resolution is this crate's job (main.rs), and the
/// picker is an OS concern, so this is a legitimate host responsibility, not engine
/// logic (there is nothing here to push behind the façade). Running the dialog in Rust
/// (not the webview) is also what keeps the webview dialog-permission-free.
///
/// The `persist_last_vault` call lives here in the (untestable) dialog wrapper, not in
/// [`set_vault_root_impl`], so the unit-tested state transition never writes to the real
/// user data dir. It is best-effort: a failed write is logged and swallowed, never
/// blocking the switch the user just made.
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
    let info = set_vault_root_impl(state.inner(), &path)?;
    // Remember it for the next launch (best-effort).
    crate::persist_last_vault(&path);
    // Re-point filesystem auto-reload at the new vault (#14): the old watch is dropped and a
    // fresh one starts, so pulses now reflect the vault the app is on.
    app.state::<VaultWatcher>().watch(&app, &path);
    Ok(Some(info))
}

#[tauri::command(async)]
pub fn read_note(state: State<'_, AppState>, note: String) -> Result<NoteView, CmdError> {
    read_note_impl(state.inner(), &note)
}

#[tauri::command(async)]
pub fn list_notes(state: State<'_, AppState>) -> Result<Vec<NoteSummary>, CmdError> {
    list_notes_impl(state.inner())
}

/// The file tree's resource half (file-type slice 1, spec §6): every inventoried
/// non-`.md` file. The frontend merges this with `list_notes` into one tree — the
/// per-kind composition the locked design prefers over a union type (research §9b #10).
#[tauri::command(async)]
pub fn list_resources(state: State<'_, AppState>) -> Result<Vec<ResourceSummary>, CmdError> {
    let (vault, _) = open_vault(state.inner(), false)?;
    Ok(vault.list_resources()?)
}

/// The fallback card's data: a resource's inventory metadata + backlinks. A pure
/// graph/inventory read, model-free like `explain`.
#[tauri::command(async)]
pub fn explain_resource(
    state: State<'_, AppState>,
    path: String,
) -> Result<ResourceExplainView, CmdError> {
    let (vault, _) = open_vault(state.inner(), false)?;
    Ok(vault.explain_resource(&path)?)
}

/// *Open in system default* on the fallback card — an **OS handoff**, never
/// in-webview execution (spec §6 security posture). Host infrastructure like the
/// folder dialog: the webview holds no opener permission; this command validates
/// the path against the inventory (so only an indexed vault file can be opened)
/// and hands the absolute path to the OS.
#[tauri::command(async)]
pub fn open_resource(state: State<'_, AppState>, path: String) -> Result<(), CmdError> {
    let (vault, _) = open_vault(state.inner(), false)?;
    vault.explain_resource(&path)?; // inventory check: unknown paths refuse, never open
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    tauri_plugin_opener::open_path(root.join(&path), None::<&str>)
        .map_err(|e| CmdError::OpenFailed(e.to_string()))
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

/// The settings picker's model list: every model B2 offers ([`b2_embed::AVAILABLE_MODELS`]),
/// annotated with which is configured now and which are already downloaded. Global
/// (per-machine) config, not per-vault — like `b2 init`, so it needs no vault open.
///
/// Thin like the rest: one `EmbedConfig` read → the shared `model_choices` view →
/// serialize. The registry and the current/installed logic live in `b2-embed`, not here.
#[tauri::command(async)]
pub fn list_models() -> Result<Vec<ModelChoice>, CmdError> {
    Ok(EmbedConfig::load()?.model_choices())
}

/// Persist the chosen embedding model into the shared `config.toml` (the same file the
/// CLI reads), then return the refreshed list. Selecting a *different* model is a model
/// swap: it takes effect only after the model is provisioned (`b2 init`) and the vault
/// is reindexed — the UI surfaces that; this command just records the choice. Refuses an
/// id outside the registry (`EmbedError::UnknownModel`, mapped generic in `error.rs`).
#[tauri::command(async)]
pub fn set_model(model: String) -> Result<Vec<ModelChoice>, CmdError> {
    set_model_impl(&model)
}

/// Provision (download + verify) the **currently-selected** model into the shared cache —
/// the in-app equivalent of `b2 init`, driven from the Settings panel so a freshly-picked
/// model can be installed without dropping to a terminal. Idempotent (an already-present,
/// loadable model is a no-op) and network-bound, so it runs `(async)` off the main thread.
/// Returns the refreshed model list, with the just-installed model's `installed` flag now
/// true. Still thin: it drives [`b2_embed::provision`] — exactly what `b2 init` runs — and
/// reprojects the choices; the download/verify logic lives in `b2-embed`, not here.
#[tauri::command(async)]
pub fn provision_model() -> Result<Vec<ModelChoice>, CmdError> {
    let config = EmbedConfig::load()?;
    // Full progress detail to the server log (repo policy); the webview gets only the
    // generic outcome. The line sink mirrors the CLI's `eprintln!` progress.
    b2_embed::provision(&config, |line| eprintln!("[b2] init: {line}"))?;
    Ok(config.model_choices())
}

/// The shared cache directory where downloaded model files live (each model in its own
/// `<dir>/<sanitized-id>` subfolder) — shown in Settings so the user knows where the
/// (large) files are saved. Per-machine, config-resolved (`EmbedConfig::cache_dir`).
#[tauri::command(async)]
pub fn models_dir() -> Result<String, CmdError> {
    Ok(EmbedConfig::load()?.cache_dir.display().to_string())
}

/// The compute device the real embedder runs on for THIS build — `"Metal"` on a
/// `--features metal` build with a working Apple-Silicon GPU, else `"CPU"` (GH #40). Global,
/// infallible embedder-wiring like [`list_models`] (no vault, no engine logic): it forwards
/// [`b2_embed::active_device_label`] straight through for the Settings badge.
#[tauri::command(async)]
pub fn embed_device() -> &'static str {
    b2_embed::active_device_label()
}

/// One model's cumulative embedding cost, for the Settings pane (`stats.rs`). Flat view
/// over [`crate::stats::ModelStat`] so it crosses IPC as a plain payload.
#[derive(Debug, Clone, Serialize)]
pub struct EmbedStat {
    pub model: String,
    pub total_ms: u64,
    pub chunks: u64,
    pub runs: u64,
}

/// The per-model embedding-time ledger (`stats.rs`) — what the Settings pane renders so a
/// model swap can be judged on real speed. Infallible: no data / an unreadable ledger is
/// an empty list, never an error (the totals are diagnostic, never load-bearing).
#[tauri::command(async)]
pub fn embed_stats() -> Vec<EmbedStat> {
    crate::stats::read_all()
        .into_iter()
        .map(|(model, s)| EmbedStat {
            model,
            total_ms: s.total_ms,
            chunks: s.chunks,
            runs: s.runs,
        })
        .collect()
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

    // Fills missing vectors → needs the real model. `semantic` is false only under
    // `B2_EMBEDDER=fake` (dev/offline): don't attribute fake-embed time to the real model.
    let (vault, semantic) = open_vault(state, true)?;
    // Attribute the time to whatever model this vault embeds with (config.toml / default).
    let model = EmbedConfig::load()
        .map(|c| c.model)
        .unwrap_or_else(|_| b2_embed::DEFAULT_MODEL.to_string());
    // Time the embed pass itself — the clock starts *after* the model load above, so the
    // recorded total is embedding throughput, not one-time setup. `chunks_done` is
    // cumulative, so its last value is this run's chunk count (async-indexing.md §4).
    let start = std::time::Instant::now();
    let mut chunks_this_run = 0u64;
    let report = vault.embed(&mut |p| {
        chunks_this_run = chunks_this_run.max(p.chunks_done as u64);
        // Forward progress to the webview; a send error (the window navigated/closed)
        // is not fatal to the index — keep embedding.
        let _ = on_event.send(p);
        if state.reindex_cancelled() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    // Record the run's cost (best-effort). Skip when nothing embedded (an up-to-date
    // vault) or under the fake embedder, so the ledger stays clean and correctly attributed.
    if semantic && chunks_this_run > 0 {
        crate::stats::record(&model, start.elapsed().as_millis() as u64, chunks_this_run);
    }
    Ok(report)
}

// --- thin impls (Tauri-runtime-free, so the command layer is unit-testable) -------

fn vault_info_impl(state: &AppState) -> Result<VaultInfo, CmdError> {
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    // Model-free read: open the fake vault only to count embedding coverage (#26). The
    // real model is never loaded here — `semantic` stays "is a model installed", while
    // `notes_embedded/total` is the precise fraction the UI flags keyword-only from.
    let (vault, _) = open_vault(state, false)?;
    let status = vault.embed_status()?;
    Ok(VaultInfo {
        root: root.display().to_string(),
        semantic: crate::semantic_available(),
        notes_embedded: status.embedded,
        notes_total: status.total,
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

/// The testable core of `set_model`. `EmbedConfig::set_model` validates the id against
/// the registry *before* any filesystem write, so the unknown-model path is hermetic (no
/// file touched); the real-config write itself is exercised by `b2-embed`'s `write_model`
/// tests against a tempfile, not here (same posture as `persist_last_vault` in main.rs).
///
/// A *changed* model also restarts that model's embed-time ledger ([`stats::reset`]): the
/// swap drops the vault's vectors, so the next reindex re-embeds the whole corpus and the
/// cumulative stat must restart with it rather than stack a second corpus onto the old
/// total. Re-selecting the current model is a no-op that keeps its accumulated history.
fn set_model_impl(model: &str) -> Result<Vec<ModelChoice>, CmdError> {
    let previous = EmbedConfig::load().ok().map(|c| c.model);
    EmbedConfig::set_model(model)?;
    if previous.as_deref() != Some(model) {
        crate::stats::reset(model);
    }
    Ok(EmbedConfig::load()?.model_choices())
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
        // Title is the filename (data-model.md §1); the frontmatter `title:` is inert.
        assert_eq!(note.title.as_deref(), Some("memory"));
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
        assert_eq!(notes[0].title.as_deref(), Some("memory"));
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
    fn vault_info_reports_embedding_coverage() {
        let tmp = tempfile::TempDir::new().unwrap();

        // A fully-indexed vault (setup reindexes with the fake) reads as M/M embedded —
        // the "N/M embedded" honesty signal (#26) surfaced through the command layer.
        let full = tmp.path().join("full");
        golden_indexed(&full);
        let state = AppState::new(Some(full));
        let info = vault_info_impl(&state).unwrap();
        assert_eq!(
            (info.notes_embedded, info.notes_total),
            (2, 2),
            "a fully-indexed vault reads as M/M embedded"
        );

        // A projected-but-unembedded vault reads as 0/M — the command surfaces the precise
        // fraction model-free (it never loads the real model to answer vault_info).
        let projected = tmp.path().join("projected");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
        copy_dir(&src, &projected);
        Vault::open(&projected).unwrap().project(false).unwrap();
        let state = AppState::new(Some(projected));
        let info = vault_info_impl(&state).unwrap();
        assert_eq!(
            (info.notes_embedded, info.notes_total),
            (0, 2),
            "a projected-but-unembedded vault reads as 0/M"
        );
    }

    #[test]
    fn ping_round_trips() {
        assert_eq!(ping(), "pong");
    }

    #[test]
    fn list_models_returns_the_registry() {
        // Global config, no vault needed. Deterministic w.r.t. ambient config only in the
        // ways asserted: the picker offers exactly the registry, by id (the current flag
        // depends on the machine's config.toml and is covered by b2-embed's own tests).
        let choices = list_models().unwrap();
        assert_eq!(choices.len(), b2_embed::AVAILABLE_MODELS.len());
        let ids: Vec<&str> = choices.iter().map(|c| c.id.as_str()).collect();
        for m in b2_embed::AVAILABLE_MODELS {
            assert!(ids.contains(&m.id), "registry model {} is offered", m.id);
        }
    }

    #[test]
    fn embed_device_reports_the_build_device() {
        // Thin passthrough to b2_embed::active_device_label (own tests there). In the default
        // (no `metal` feature) test build it is always "CPU"; a `--features metal` build would
        // report "Metal". Either way it's one of the two labels the badge renders.
        assert!(matches!(embed_device(), "CPU" | "Metal"));
    }

    #[test]
    fn set_model_rejects_unknown_without_writing() {
        // Validation happens before any filesystem write (b2-embed `write_model`), so this
        // touches no real config file — it just proves the command refuses and stays generic.
        let err = set_model_impl("definitely/not-a-real-model").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Embed(b2_embed::EmbedError::UnknownModel(_))
        ));
        assert!(user_message(&err).to_lowercase().contains("settings"));
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
        let msg = "This note changed on disk since it was opened. Reload the note, then reapply your edit.";
        assert_eq!(user_message(&err), msg);

        // …and the frontend's mirror constant (`WRITE_CONFLICT_MESSAGE`) carries the
        // exact same bytes — the recognizer string-matches on it, so a drifted copy
        // would silently demote every conflict to a generic error toast. This assert
        // automates the "change them together" discipline instead of trusting it.
        let api_ts = fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/api.ts"),
        )
        .expect("ui/src/api.ts is part of this repo — the IPC seam the contract pins");
        assert!(
            api_ts.contains(&format!("\"{msg}\"")),
            "ui/src/api.ts WRITE_CONFLICT_MESSAGE must equal the host's conflict message"
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
    fn project_skips_unreadable_files_and_still_reports() {
        // A real vault holds the odd non-UTF-8 file; projecting it must skip that file
        // and index the rest, not fail the whole pass (the "reindex fails on a large
        // vault" bug). The skip flows through the report to the UI, no logic added here.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
        copy_dir(&src, &root);
        fs::write(root.join("bad.md"), [b'#', 0xff, b'\n']).unwrap();
        let state = AppState::new(Some(root));

        let report = project_impl(&state).unwrap();
        assert_eq!(
            report.indexed, 2,
            "the two readable golden notes still project"
        );
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].path, "bad.md");
        // The tree lists the good notes; the bad file is absent, not fatal.
        assert_eq!(list_notes_impl(&state).unwrap().len(), 2);
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
