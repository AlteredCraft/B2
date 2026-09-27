//! The `#[tauri::command]` handlers, B2's IPC surface over the
//! [`Vault`](b2_core::vault::Vault) façade (ADR-0012). Each is deserialize, call one façade
//! method, serialize; any logic belongs in `b2-core`. Views are the CLI's `--json` types.
//!
//! `#[tauri::command(async)]` only moves a command off the main thread so the window never
//! freezes; the bodies stay synchronous (ADR-0011). The `*_impl` split lets commands be
//! tested without a Tauri runtime.

use crate::chat::ChatPrefs;
use crate::error::CmdError;
use crate::watch::VaultWatcher;
use crate::{open_read, open_semantic, open_vault, AppState};
use b2_core::add::AddReport;
use b2_core::ingest::ReindexProgress;
use b2_core::llm::{ChatTurn, LlmProvider};
use b2_core::vault::{
    AnswerView, DeleteReport, DirCreateReport, DirDeleteReport, DirMoveReport, EmbedReport,
    ExplainView, ImportReport, LinkReport, MoveReport, NoteSummary, NoteView, ProjectReport,
    ResourceDeleteReport, ResourceExplainView, ResourceMoveReport, ResourceSummary,
    SearchEvidenceView, SimilarExplainView, SimilarView, Vault, WriteReport,
};
use b2_embed::{EmbedConfig, ModelChoice};
use b2_llm::ChatSetup;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Serialize;
use std::ops::ControlFlow;
use std::path::Path;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

/// The active vault's root, whether the real model is installed, and how much of the
/// vault is embedded (#26), so the UI can say search is keyword-only for now.
#[derive(Debug, Clone, Serialize)]
pub struct VaultInfo {
    pub root: String,
    pub semantic: bool,
    pub notes_embedded: usize,
    pub notes_total: usize,
}

/// One façade call over a fresh read-path vault (the fake embedder, no model load).
fn read_op<T>(
    state: &AppState,
    op: impl FnOnce(&Vault) -> b2_core::Result<T>,
) -> Result<T, CmdError> {
    Ok(op(&open_read(state)?)?)
}

/// [`read_op`] over the real model, for a command that embeds.
fn semantic_op<T>(
    state: &AppState,
    op: impl FnOnce(&Vault) -> b2_core::Result<T>,
) -> Result<T, CmdError> {
    Ok(op(&open_semantic(state)?)?)
}

#[tauri::command(async)]
pub fn vault_info(state: State<'_, AppState>) -> Result<VaultInfo, CmdError> {
    vault_info_impl(state.inner())
}

/// The vault switcher: a native folder picker, then point the app at the pick and
/// remember it. `None` when the user cancels. `persist_last_vault` lives here, not in
/// [`set_vault_root_impl`], so tests never write the real data dir.
///
/// `(async)` is required: `blocking_pick_folder` waits on the main thread, so calling it
/// from there would deadlock.
#[tauri::command(async)]
pub fn choose_vault(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<VaultInfo>, CmdError> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None); // user cancelled
    };
    // A `Url` pick is a mobile content URI; treat it as a cancel.
    let Ok(path) = picked.into_path() else {
        return Ok(None);
    };
    let info = set_vault_root_impl(state.inner(), &path)?;
    crate::persist_last_vault(&path);
    // Re-point filesystem auto-reload at the new vault (#14).
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

/// Every inventoried non-`.md` file, merged by the frontend with `list_notes` into the
/// tree (spec §6).
#[tauri::command(async)]
pub fn list_resources(state: State<'_, AppState>) -> Result<Vec<ResourceSummary>, CmdError> {
    read_op(state.inner(), |v| v.list_resources())
}

/// Every folder in the vault, empty ones included, read live off the filesystem so the
/// tree matches disk.
#[tauri::command(async)]
pub fn list_dirs(state: State<'_, AppState>) -> Result<Vec<String>, CmdError> {
    list_dirs_impl(state.inner())
}

/// Create a folder (⇧⌘N), missing parents included; an occupied target is refused.
/// Touches no index rows.
#[tauri::command(async)]
pub fn create_dir(state: State<'_, AppState>, dir: String) -> Result<DirCreateReport, CmdError> {
    create_dir_impl(state.inner(), &dir)
}

/// The fallback card's data: a resource's inventory metadata and backlinks.
#[tauri::command(async)]
pub fn explain_resource(
    state: State<'_, AppState>,
    path: String,
) -> Result<ResourceExplainView, CmdError> {
    read_op(state.inner(), |v| v.explain_resource(&path))
}

/// A resource's bytes as base64, for the card's viewer (an image, today); the webview
/// makes a `data:` URL of it, which the CSP admits. The façade re-validates the path
/// against the inventory, since the linking note is untrusted (ADR-0016).
#[tauri::command(async)]
pub fn read_resource(state: State<'_, AppState>, path: String) -> Result<String, CmdError> {
    read_op(state.inner(), |v| v.read_resource_bytes(&path)).map(|bytes| BASE64.encode(bytes))
}

/// *Open in system default*: an OS handoff, never in-webview execution (spec §6). Only an
/// inventoried vault file can be opened.
#[tauri::command(async)]
pub fn open_resource(state: State<'_, AppState>, path: String) -> Result<(), CmdError> {
    let abs = read_op(state.inner(), |v| v.resource_path(&path))?;
    tauri_plugin_opener::open_path(abs, None::<&str>)
        .map_err(|e| CmdError::OpenFailed(e.to_string()))
}

/// The URL schemes B2 will hand to the OS. `ui/src/links.ts`'s `externalUrl` routes by the
/// same list, but this one is the authority; change them together.
const OPENABLE_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

/// Is this a link the host will open? A note is untrusted (ADR-0016) and `open` launches
/// whatever app claims the scheme, so this allow-list is the whole security posture.
/// Byte-wise, so a multi-byte prefix can't panic the slice.
fn is_openable_link(url: &str) -> bool {
    if url.chars().any(|c| c.is_ascii_control()) {
        return false;
    }
    let bytes = url.as_bytes();
    OPENABLE_SCHEMES.iter().any(|scheme| {
        // `>` not `>=`: a bare "https://" names nothing to open.
        bytes.len() > scheme.len() && bytes[..scheme.len()].eq_ignore_ascii_case(scheme.as_bytes())
    })
}

/// Open a web link from a note in the system browser; followed in place it would replace
/// the app. Re-checked here because the note is untrusted (ADR-0016).
#[tauri::command(async)]
pub fn open_external(url: String) -> Result<(), CmdError> {
    if !is_openable_link(&url) {
        return Err(CmdError::UnsupportedLink(url));
    }
    tauri_plugin_opener::open_url(url, None::<&str>)
        .map_err(|e| CmdError::OpenFailed(e.to_string()))
}

/// The clipboard's plain text, for ⌘⇧V (`ui/src/paste.ts`). Host-side because WebKit runs
/// no paste for a raw ⌘⇧V and gates `navigator.clipboard` behind a native prompt.
#[tauri::command(async)]
pub fn clipboard_text(app: tauri::AppHandle) -> Result<String, CmdError> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .read_text()
        .map_err(|e| CmdError::ClipboardFailed(e.to_string()))
}

/// Save a note's body. Model-free and outside the embed slot, so saving never waits on a
/// model. A stale `base_revision` surfaces as the stable conflict message the frontend
/// matches (`ui/src/api.ts`).
#[tauri::command(async)]
pub fn write_note(
    state: State<'_, AppState>,
    note: String,
    body: String,
    base_revision: String,
) -> Result<WriteReport, CmdError> {
    write_note_impl(state.inner(), &note, &body, &base_revision)
}

/// Save a note's frontmatter (GH #79), with `write_note`'s posture and conflict contract.
#[tauri::command(async)]
pub fn write_frontmatter(
    state: State<'_, AppState>,
    note: String,
    frontmatter: String,
    base_revision: String,
) -> Result<WriteReport, CmdError> {
    write_frontmatter_impl(state.inner(), &note, &frontmatter, &base_revision)
}

/// Create a new, empty note (⌘N). Model-free like `write_note`: its chunks wait for the
/// next embed pass, since a fake-opened vault must never write vectors.
#[tauri::command(async)]
pub fn create_note(state: State<'_, AppState>, path: String) -> Result<AddReport, CmdError> {
    create_note_impl(state.inner(), &path)
}

/// Import a file from outside the vault into `dir` (`""` for the root): the tree's drop
/// target. `data` is base64, since a dropped file reaches the webview as bytes, not a
/// path. Model-free.
#[tauri::command(async)]
pub fn import_file(
    state: State<'_, AppState>,
    dir: String,
    name: String,
    data: String,
) -> Result<ImportReport, CmdError> {
    import_file_impl(state.inner(), &dir, &name, &data)
}

/// [`import_file`] from a path, for the Import files… picker.
#[tauri::command(async)]
pub fn import_path(
    state: State<'_, AppState>,
    dir: String,
    source: String,
) -> Result<ImportReport, CmdError> {
    import_path_impl(state.inner(), &dir, &source)
}

/// The keyboard half of the drop gesture (K1): a native multi-select picker returning
/// paths for [`import_path`]; empty when cancelled. `(async)` as in `choose_vault`.
#[tauri::command(async)]
pub fn pick_import_files(app: tauri::AppHandle) -> Result<Vec<String>, CmdError> {
    let Some(picked) = app.dialog().file().blocking_pick_files() else {
        return Ok(Vec::new()); // user cancelled
    };
    Ok(picked
        .into_iter()
        // A `Url` pick is a mobile content URI; drop it.
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// Move/rename a note. Opens the real model: rewriting an inbound file's links changes
/// its body, which re-embeds inline.
#[tauri::command(async)]
pub fn move_note(
    state: State<'_, AppState>,
    note: String,
    to: String,
) -> Result<MoveReport, CmdError> {
    move_note_impl(state.inner(), &note, &to)
}

/// [`move_note`] for a resource (L3).
#[tauri::command(async)]
pub fn move_resource(
    state: State<'_, AppState>,
    path: String,
    to: String,
) -> Result<ResourceMoveReport, CmdError> {
    move_resource_impl(state.inner(), &path, &to)
}

/// Move/rename a whole folder: one rename on disk, with inbound links rewritten.
#[tauri::command(async)]
pub fn move_dir(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<DirMoveReport, CmdError> {
    move_dir_impl(state.inner(), &from, &to)
}

/// Delete a note (⌘⌫). Model-free: inbound links dangle, as after an external delete.
#[tauri::command(async)]
pub fn delete_note(state: State<'_, AppState>, note: String) -> Result<DeleteReport, CmdError> {
    delete_note_impl(state.inner(), &note)
}

/// [`delete_note`] for a resource (L3).
#[tauri::command(async)]
pub fn delete_resource(
    state: State<'_, AppState>,
    path: String,
) -> Result<ResourceDeleteReport, CmdError> {
    delete_resource_impl(state.inner(), &path)
}

/// Delete a whole folder and everything in it. The frontend confirms first. Model-free.
#[tauri::command(async)]
pub fn delete_dir(state: State<'_, AppState>, dir: String) -> Result<DirDeleteReport, CmdError> {
    delete_dir_impl(state.inner(), &dir)
}

#[tauri::command(async)]
pub fn similar(
    state: State<'_, AppState>,
    note: String,
    limit: usize,
) -> Result<Vec<SimilarView>, CmdError> {
    read_op(state.inner(), |v| v.similar(&note, limit))
}

/// The Compare view for one Similar card (GH #236), as `b2 similar --explain`. `limit` is
/// the pane's list length, so the rank is the card's.
#[tauri::command(async)]
pub fn explain_similar(
    state: State<'_, AppState>,
    anchor: String,
    candidate: String,
    limit: usize,
) -> Result<SimilarExplainView, CmdError> {
    read_op(state.inner(), |v| {
        v.explain_similar(&anchor, &candidate, limit)
    })
}

/// Hybrid search with its evidence reading (D2, GH #202), as `b2 search`. Returned whole:
/// what to show for each verdict is the frontend's call (E3).
#[tauri::command(async)]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<SearchEvidenceView, CmdError> {
    semantic_op(state.inner(), |v| v.search_evidence(&query, limit))
}

#[tauri::command(async)]
pub fn explain(state: State<'_, AppState>, note: String) -> Result<ExplainView, CmdError> {
    read_op(state.inner(), |v| v.explain(&note))
}

#[tauri::command(async)]
pub fn link(
    state: State<'_, AppState>,
    src: String,
    dst: String,
    relation: String,
    explanation: Option<String>,
) -> Result<LinkReport, CmdError> {
    // Re-projects the source note, so it needs the real model.
    semantic_op(state.inner(), |v| {
        v.link(&src, &dst, &relation, explanation.as_deref())
    })
}

/// The projection pass, the fast model-free half of a reindex: once it returns, the tree
/// and keyword search work, and `embed` follows. Outside the reindex slot: racing a vault
/// switch is harmless, since it idempotently writes the root it captured.
#[tauri::command(async)]
pub fn project(state: State<'_, AppState>) -> Result<ProjectReport, CmdError> {
    project_impl(state.inner())
}

/// The embed pass: fill missing vectors in the background, streaming progress over a
/// [`Channel`] and stopping at the next batch once cancelled.
#[tauri::command(async)]
pub fn embed(
    state: State<'_, AppState>,
    on_event: Channel<ReindexProgress>,
) -> Result<EmbedReport, CmdError> {
    embed_impl(state.inner(), &on_event)
}

/// Ask the in-flight embed to stop at its next batch boundary.
#[tauri::command(async)]
pub fn cancel_reindex(state: State<'_, AppState>) {
    state.reindex.cancel();
}

/// The settings picker's model list, annotated with current and installed. Per-machine,
/// so it needs no vault open.
#[tauri::command(async)]
pub fn list_models() -> Result<Vec<ModelChoice>, CmdError> {
    Ok(EmbedConfig::load()?.model_choices())
}

/// Persist the chosen embedding model to the shared `config.toml` and return the refreshed
/// list. The swap takes effect after provisioning and a reindex.
#[tauri::command(async)]
pub fn set_model(model: String) -> Result<Vec<ModelChoice>, CmdError> {
    set_model_impl(&model)
}

/// Provision the currently selected model: the in-app `b2 init`.
#[tauri::command(async)]
pub fn provision_model() -> Result<Vec<ModelChoice>, CmdError> {
    let config = EmbedConfig::load()?;
    // Progress goes to the host log; the webview gets only the outcome.
    b2_embed::provision(&config, |line| eprintln!("[b2] init: {line}"))?;
    Ok(config.model_choices())
}

/// The shared model cache directory, shown in Settings.
#[tauri::command(async)]
pub fn models_dir() -> Result<String, CmdError> {
    Ok(EmbedConfig::load()?.cache_dir.display().to_string())
}

/// The embedder's compute device, `"Metal"` or `"CPU"`, for the Settings badge (GH #40).
#[tauri::command(async)]
pub fn embed_device() -> &'static str {
    b2_embed::active_device_label()
}

/// The per-model embedding-time ledger (`stats.rs`); empty rather than an error.
#[tauri::command(async)]
pub fn embed_stats() -> Vec<crate::stats::EmbedStat> {
    crate::stats::read_all()
}

/// Every chord the menu bar takes (`menu.rs`, #119), reserved in the UI's keyboard
/// registry (ADR-0017).
#[tauri::command]
pub fn menu_chords() -> Vec<crate::menu::MenuChord> {
    crate::menu::chords()
}

/// Set the window's page zoom, as ⌘+ does in Safari (`ui/src/zoom.ts`). Host-side because
/// a webview can't zoom itself, and CSS scaling would leave the chrome behind. A
/// pass-through: which sizes are allowed is the UI's rule.
#[tauri::command]
pub fn set_zoom(window: tauri::WebviewWindow, factor: f64) -> Result<(), CmdError> {
    window
        .set_zoom(factor)
        .map_err(|e| CmdError::ZoomFailed(e.to_string()))
}

/// Flow ④, one grounded answer behind `Vault::ask`: tokens stream over a [`Channel`] and
/// the [`AnswerView`] is the return. `history` is the pane's; nothing about a chat is
/// stored. Opens the real model, since retrieval embeds the question.
#[tauri::command(async)]
pub fn ask(
    state: State<'_, AppState>,
    question: String,
    history: Vec<ChatTurn>,
    on_event: Channel<String>,
) -> Result<AnswerView, CmdError> {
    let state = state.inner();
    // Wired here and handed to `ask_impl`, so the streaming half is testable against
    // `FakeLlm` without a Tauri runtime.
    let llm = crate::chat::provider(&state.chat_prefs());
    let vault = open_semantic(state)?;
    ask_impl(state, &vault, llm.as_ref(), &question, &history, &|token| {
        // A send error means the window closed; the cancel flag is what stops the stream.
        let _ = on_event.send(token.to_string());
    })
}

/// **Why?** for a Similar card: a grounded, cited explanation from `Vault::why_similar`,
/// delivered as [`ask`] is (same slot, same `cancel_ask`). `limit` is the pane's list
/// length. Model-free: every read is over stored vectors.
#[tauri::command(async)]
pub fn why_similar(
    state: State<'_, AppState>,
    anchor: String,
    candidate: String,
    limit: usize,
    on_event: Channel<String>,
) -> Result<AnswerView, CmdError> {
    let state = state.inner();
    let llm = crate::chat::provider(&state.chat_prefs());
    let vault = open_read(state)?;
    why_similar_impl(
        state,
        &vault,
        llm.as_ref(),
        &anchor,
        &candidate,
        limit,
        &|token| {
            let _ = on_event.send(token.to_string());
        },
    )
}

/// Ask the streaming answer to stop at its next token (Esc). The partial text comes back
/// as an [`AnswerView`] marked `cancelled`, not a failure.
#[tauri::command(async)]
pub fn cancel_ask(state: State<'_, AppState>) {
    state.ask.cancel();
}

/// What the chat surface needs before a question: the endpoint and model in force, Local
/// or Cloud, and for Ollama the setup card's inventory. Infallible by design: "the daemon
/// isn't running" is an answer, not an error.
#[tauri::command(async)]
pub fn chat_setup(state: State<'_, AppState>) -> ChatSetup {
    chat_setup_impl(state.inner())
}

/// Save the chat configuration and re-probe it. The key goes to the Keychain (GH #176);
/// `None` leaves the key in force untouched.
#[tauri::command(async)]
pub fn set_chat_config(
    state: State<'_, AppState>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    max_tool_calls: Option<String>,
) -> ChatSetup {
    {
        // One save at a time (Tauri doesn't serialize `(async)` commands). Not held over
        // the probe below, a network round trip.
        let _saving = state.begin_chat_save();
        let prefs = set_chat_config_impl(
            state.inner(),
            base_url,
            model,
            api_key,
            max_tool_calls,
            &crate::keychain::Keychain,
        );
        // Persisted here so tests never write the real data dir. The Keychain is passed
        // into the core instead, since whether it took the key is state the core records.
        crate::chat::persist_prefs(&prefs);
    }
    chat_setup_impl(state.inner())
}

/// The testable core of `set_chat_config`. Blank is "unset" for the endpoint and model.
/// The key skips `clean`: blank means Remove there (`chat::apply_key`).
fn set_chat_config_impl(
    state: &AppState,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    max_tool_calls: Option<String>,
    keys: &dyn crate::keychain::KeyStore,
) -> ChatPrefs {
    let clean = |v: Option<String>| {
        v.map(|s| s.trim().to_string())
            .filter(|s: &String| !s.is_empty())
    };
    let (api_key, key_remembered) =
        crate::chat::apply_key(&state.chat_prefs(), api_key.as_deref(), keys);
    let prefs = ChatPrefs {
        base_url: clean(base_url),
        model: clean(model),
        max_tool_calls: crate::chat::apply_tool_cap(&state.chat_prefs(), max_tool_calls.as_deref()),
        api_key,
        key_remembered,
    };
    state.set_chat_prefs(prefs.clone());
    prefs
}

/// The testable core of `chat_setup`: the fake's own status, or one probe.
fn chat_setup_impl(state: &AppState) -> ChatSetup {
    let config = state.chat_prefs().config();
    if b2_llm::fake_requested() {
        ChatSetup::fake(&config)
    } else {
        b2_llm::probe_setup(&config)
    }
}

/// The testable core of `ask`. `sink` receives every token, in order, as it arrives.
fn ask_impl(
    state: &AppState,
    vault: &Vault,
    llm: &dyn LlmProvider,
    question: &str,
    history: &[ChatTurn],
    sink: &dyn Fn(&str),
) -> Result<AnswerView, CmdError> {
    stream_answer(state, sink, |on_token| {
        vault.ask(llm, question, history, on_token)
    })
}

/// The testable core of `why_similar`.
fn why_similar_impl(
    state: &AppState,
    vault: &Vault,
    llm: &dyn LlmProvider,
    anchor: &str,
    candidate: &str,
    limit: usize,
    sink: &dyn Fn(&str),
) -> Result<AnswerView, CmdError> {
    stream_answer(state, sink, |on_token| {
        vault.why_similar(llm, anchor, candidate, limit, on_token)
    })
}

/// Claim the answer slot and run `call` with a token callback that feeds `sink` and
/// checks for cancel at every token.
fn stream_answer(
    state: &AppState,
    sink: &dyn Fn(&str),
    call: impl FnOnce(&mut dyn FnMut(&str) -> ControlFlow<()>) -> b2_core::Result<AnswerView>,
) -> Result<AnswerView, CmdError> {
    // Two answers at once would share one cancel flag, and the second claim would
    // un-cancel the first.
    let _held = state.ask.try_claim().ok_or(CmdError::AskInFlight)?;
    Ok(call(&mut |token| {
        sink(token);
        state.ask.flow()
    })?)
}

/// The testable core of `project`, over the fake vault.
fn project_impl(state: &AppState) -> Result<ProjectReport, CmdError> {
    read_op(state, |v| v.project(false))
}

/// The testable core of `embed`.
fn embed_impl(
    state: &AppState,
    on_event: &Channel<ReindexProgress>,
) -> Result<EmbedReport, CmdError> {
    // Refuse a second embed rather than race two writers on one DB.
    let _held = state.reindex.try_claim().ok_or(CmdError::ReindexInFlight)?;

    // `model` is `None` under `B2_EMBEDDER=fake`, so fake time is never recorded.
    let (vault, model) = open_vault(state, true)?;
    // The clock starts after the model load. `chunks_done` is cumulative.
    let start = std::time::Instant::now();
    let mut chunks_this_run = 0u64;
    let report = vault.embed(&mut |p| {
        chunks_this_run = chunks_this_run.max(p.chunks_done as u64);
        // A send error (window closed) doesn't stop the embed.
        let _ = on_event.send(p);
        state.reindex.flow()
    })?;
    // Skip an up-to-date vault or the fake embedder.
    if let Some(model) = model.filter(|_| chunks_this_run > 0) {
        crate::stats::record(&model, start.elapsed().as_millis() as u64, chunks_this_run);
    }
    Ok(report)
}

// --- thin impls (Tauri-runtime-free, so the command layer is unit-testable) -------

fn vault_info_impl(state: &AppState) -> Result<VaultInfo, CmdError> {
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    // Model-free on both halves: this is the first-paint path (#133).
    let status = read_op(state, |v| v.embed_status())?;
    Ok(VaultInfo {
        root: root.display().to_string(),
        semantic: crate::semantic_available(),
        notes_embedded: status.embedded,
        notes_total: status.total,
    })
}

/// The testable core of `choose_vault`. Cancels and waits out any in-flight reindex first,
/// so it can never keep writing the vault the app has left.
fn set_vault_root_impl(state: &AppState, root: &Path) -> Result<VaultInfo, CmdError> {
    state.reindex.cancel_and_wait();
    state.set_root(root);
    vault_info_impl(state)
}

fn read_note_impl(state: &AppState, note: &str) -> Result<NoteView, CmdError> {
    read_op(state, |v| v.read(note))
}

fn list_notes_impl(state: &AppState) -> Result<Vec<NoteSummary>, CmdError> {
    read_op(state, |v| v.list_notes())
}

fn list_dirs_impl(state: &AppState) -> Result<Vec<String>, CmdError> {
    read_op(state, |v| v.list_dirs())
}

fn create_dir_impl(state: &AppState, dir: &str) -> Result<DirCreateReport, CmdError> {
    read_op(state, |v| v.create_dir(dir))
}

fn write_note_impl(
    state: &AppState,
    note: &str,
    body: &str,
    base_revision: &str,
) -> Result<WriteReport, CmdError> {
    read_op(state, |v| v.write(note, body, base_revision))
}

fn write_frontmatter_impl(
    state: &AppState,
    note: &str,
    frontmatter: &str,
    base_revision: &str,
) -> Result<WriteReport, CmdError> {
    read_op(state, |v| {
        v.write_frontmatter(note, frontmatter, base_revision)
    })
}

fn create_note_impl(state: &AppState, path: &str) -> Result<AddReport, CmdError> {
    read_op(state, |v| v.create_note(path))
}

fn import_file_impl(
    state: &AppState,
    dir: &str,
    name: &str,
    data: &str,
) -> Result<ImportReport, CmdError> {
    let bytes = BASE64
        .decode(data)
        .map_err(|e| CmdError::ImportPayload(e.to_string()))?;
    read_op(state, |v| v.import_file(dir, name, &bytes))
}

fn import_path_impl(state: &AppState, dir: &str, source: &str) -> Result<ImportReport, CmdError> {
    read_op(state, |v| v.import_path(dir, Path::new(source)))
}

fn move_note_impl(state: &AppState, note: &str, to: &str) -> Result<MoveReport, CmdError> {
    semantic_op(state, |v| v.move_note(note, to))
}

fn move_resource_impl(
    state: &AppState,
    path: &str,
    to: &str,
) -> Result<ResourceMoveReport, CmdError> {
    semantic_op(state, |v| v.move_resource(path, to))
}

fn move_dir_impl(state: &AppState, from: &str, to: &str) -> Result<DirMoveReport, CmdError> {
    semantic_op(state, |v| v.move_dir(from, to))
}

fn delete_note_impl(state: &AppState, note: &str) -> Result<DeleteReport, CmdError> {
    read_op(state, |v| v.delete_note(note))
}

fn delete_resource_impl(state: &AppState, path: &str) -> Result<ResourceDeleteReport, CmdError> {
    read_op(state, |v| v.delete_resource(path))
}

fn delete_dir_impl(state: &AppState, dir: &str) -> Result<DirDeleteReport, CmdError> {
    read_op(state, |v| v.delete_dir(dir))
}

/// The testable core of `set_model`. A changed model restarts its embed-time ledger
/// ([`stats::reset`]), since the swap re-embeds the whole corpus (ADR-0007).
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
    //! Thin command-layer tests: args resolve, the façade is called, a view comes back.
    //! The façade's own suite covers behavior. Model-free throughout.

    use super::*;
    use crate::error::user_message;
    use crate::keychain::MemoryStore;
    use b2_core::llm::FakeLlm;
    use std::fs;
    use std::path::Path;

    /// Copy the golden vault into `root` and reindex it with the fake embedder.
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
    fn list_dirs_reads_structure_live_including_empty_folders() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        // An external `mkdir` with no reindex after it.
        fs::create_dir_all(root.join("projects/2026")).unwrap();
        let state = AppState::new(Some(root));

        assert_eq!(
            list_dirs_impl(&state).unwrap(),
            vec![
                "concepts",
                "notes",
                "projects",
                "projects/2026",
                "resources"
            ]
        );
    }

    #[test]
    fn create_dir_makes_a_real_folder_and_refuses_occupied_paths() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let report = create_dir_impl(&state, "projects/2026").unwrap();
        assert_eq!(report.dir, "projects/2026");
        assert!(root.join("projects/2026").is_dir());

        let err = create_dir_impl(&state, "projects/2026").unwrap_err();
        assert_eq!(
            user_message(&err),
            "Something already exists at 'projects/2026'. Choose a different folder name."
        );
    }

    #[test]
    fn commands_without_a_vault_are_a_clean_refusal() {
        let state = AppState::new(None);
        let err = read_note_impl(&state, "anything").unwrap_err();
        assert!(matches!(err, CmdError::VaultRequired));
        assert_eq!(
            user_message(&err),
            "No vault open. Launch B2 with a vault path, or set B2_VAULT_PATH to your vault folder."
        );
    }

    #[test]
    fn set_vault_root_switches_the_active_vault() {
        let state = AppState::new(None);
        assert!(matches!(
            list_notes_impl(&state).unwrap_err(),
            CmdError::VaultRequired
        ));

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
        assert!(matches!(
            read_note_impl(&state, "concepts/memory").unwrap_err(),
            CmdError::Core(b2_core::Error::NoteNotFound(_))
        ));
        let notes = list_notes_impl(&state).unwrap();
        let paths: Vec<&str> = notes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(paths, vec!["solo.md"]);
    }

    #[test]
    fn vault_info_reports_embedding_coverage() {
        let tmp = tempfile::TempDir::new().unwrap();

        let full = tmp.path().join("full");
        golden_indexed(&full);
        let state = AppState::new(Some(full));
        let info = vault_info_impl(&state).unwrap();
        assert_eq!(
            (info.notes_embedded, info.notes_total),
            (2, 2),
            "a fully-indexed vault reads as M/M embedded"
        );

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
    fn list_models_returns_the_registry() {
        // The current flag depends on the machine's config.toml, so it isn't asserted.
        let choices = list_models().unwrap();
        assert_eq!(choices.len(), b2_embed::AVAILABLE_MODELS.len());
        let ids: Vec<&str> = choices.iter().map(|c| c.id.as_str()).collect();
        for m in b2_embed::AVAILABLE_MODELS {
            assert!(ids.contains(&m.id), "registry model {} is offered", m.id);
        }
    }

    #[test]
    fn embed_device_reports_the_build_device() {
        assert!(matches!(embed_device(), "CPU" | "Metal"));
    }

    #[test]
    fn set_model_rejects_unknown_without_writing() {
        // Validation precedes any write, so no real config file is touched.
        let err = set_model_impl("definitely/not-a-real-model").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Embed(b2_embed::EmbedError::UnknownModel(_))
        ));
        assert!(user_message(&err).to_lowercase().contains("settings"));
    }

    #[test]
    fn errors_stay_generic_and_leak_no_internals() {
        let msg = user_message(&CmdError::Core(b2_core::Error::NoteNotFound(
            "x/y".to_string(),
        )));
        assert!(msg.contains("Note not found: 'x/y'"));
        assert!(!msg.to_lowercase().contains("sqlite"));
    }

    // --- The host's task-lifecycle bits -------------------------------------------

    #[test]
    fn a_second_embed_is_refused_before_touching_the_model() {
        // Refused before the real model is opened, so this needs no model.
        let state = AppState::new(None);
        let running = state.reindex.try_claim(); // stand in for a running embed
        assert!(running.is_some());
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
    fn create_note_projects_and_lists_model_free() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        // Missing parent folders are created, like `b2 add`.
        let report = create_note_impl(&state, "inbox/idea").unwrap();
        assert_eq!(report.path, "inbox/idea.md");
        assert!(root.join("inbox/idea.md").is_file());

        let notes = list_notes_impl(&state).unwrap();
        assert!(notes.iter().any(|n| n.path == "inbox/idea.md"));
        let note = read_note_impl(&state, "inbox/idea").unwrap();
        assert_eq!(note.body, "");
        assert_eq!(note.path, report.path);
    }

    /// The payload is not UTF-8: the transport must carry a PNG faithfully.
    #[test]
    fn import_file_decodes_the_drop_payload_and_projects() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let bytes: &[u8] = &[0x89, b'P', b'N', b'G', 0xFF, 0x00];
        let report =
            import_file_impl(&state, "resources", "dropped.png", &BASE64.encode(bytes)).unwrap();

        assert_eq!(report.path, "resources/dropped.png");
        assert!(!report.note); // a non-`.md` file is routed as a resource
        assert_eq!(fs::read(root.join("resources/dropped.png")).unwrap(), bytes);
        let listed = read_op(&state, |v| v.list_resources()).unwrap();
        assert!(listed.iter().any(|r| r.path == "resources/dropped.png"));
    }

    #[test]
    fn import_file_refuses_a_payload_that_is_not_base64() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let err = import_file_impl(&state, "resources", "x.png", "not base64!!").unwrap_err();
        assert!(matches!(err, CmdError::ImportPayload(_)));
        assert!(!user_message(&err).contains("base64"), "no internals leak");
        assert!(
            !root.join("resources/x.png").exists(),
            "nothing was written"
        );
    }

    #[test]
    fn import_path_copies_the_picked_file_in() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let pdf: &[u8] = b"%PDF-1.7";
        let source = tmp.path().join("paper.pdf");
        fs::write(&source, pdf).unwrap();

        let report = import_path_impl(&state, "resources", &source.to_string_lossy()).unwrap();

        assert_eq!(report.path, "resources/paper.pdf");
        assert_eq!(fs::read(root.join("resources/paper.pdf")).unwrap(), pdf);
        assert!(source.is_file(), "the picked file is copied, never moved");
    }

    /// Import refusals say "file", not "note": what arrived may be a PDF.
    #[test]
    fn import_refusals_stay_generic_and_actionable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let err =
            import_file_impl(&state, "concepts", "memory.md", &BASE64.encode("x")).unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::ImportTargetExists(_))
        ));
        assert_eq!(
            user_message(&err),
            "A file already exists at 'concepts/memory.md'. Rename it, or drop this one into a different folder."
        );

        // A "name" that is really a path can't escape the folder.
        let err =
            import_file_impl(&state, "notes", "../escaped.png", &BASE64.encode("x")).unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::ImportDestination(_))
        ));
        assert_eq!(
            user_message(&err),
            "That file can't be imported under its own name. Rename it and try again."
        );
    }

    #[test]
    fn create_note_refusals_stay_generic_and_actionable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let err = create_note_impl(&state, "concepts/memory").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::AddTargetExists(_))
        ));
        assert_eq!(
            user_message(&err),
            "A note already exists at 'concepts/memory.md'. Choose a different name, or open that note."
        );

        let err = create_note_impl(&state, "../escape").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::AddDestination(_))
        ));
        assert_eq!(
            user_message(&err),
            "That note name isn't valid. Give a vault-relative name like `notes/new-idea`."
        );
    }

    #[test]
    fn write_frontmatter_saves_through_the_facade_and_chains_revisions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let note = read_note_impl(&state, "concepts/memory").unwrap();
        let body_before = note.body.clone();
        let new_fm = "tags: [edited]\n";
        let report =
            write_frontmatter_impl(&state, "concepts/memory", new_fm, &note.revision).unwrap();
        assert_ne!(report.revision, note.revision);

        let reread = read_note_impl(&state, "concepts/memory").unwrap();
        assert_eq!(reread.frontmatter.as_deref(), Some(new_fm));
        assert_eq!(reread.body, body_before);
        assert_eq!(reread.tags, vec!["edited"]);
        assert_eq!(reread.revision, report.revision);
    }

    /// A `---` line would end the block early and shift bytes into the body; every other
    /// edit saves, since B2 owns no line inside the block (W3, GH #170).
    #[test]
    fn write_frontmatter_refuses_only_the_fence_and_says_so_without_internals() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let note = read_note_impl(&state, "concepts/memory").unwrap();
        let report =
            write_frontmatter_impl(&state, "concepts/memory", "tags: [x]\n", &note.revision)
                .expect("the block is the human's");

        let err = write_frontmatter_impl(
            &state,
            "concepts/memory",
            "tags: [x]\n---\nleak\n",
            &report.revision,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::Frontmatter(_))
        ));
        let msg = user_message(&err);
        assert!(msg.starts_with("Can't save this frontmatter:"), "{msg}");
        assert!(
            !msg.to_lowercase().contains("sqlite"),
            "no internals: {msg}"
        );
    }

    #[test]
    fn write_conflict_is_generic_and_recognizable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let note = read_note_impl(&state, "concepts/memory").unwrap();
        // An external edit lands after the read.
        let abs = root.join("concepts/memory.md");
        fs::write(
            &abs,
            format!("{}\nexternal\n", fs::read_to_string(&abs).unwrap()),
        )
        .unwrap();

        // The frontend string-matches this message to drive its conflict bar.
        let err = write_note_impl(&state, "concepts/memory", "mine", &note.revision).unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::WriteConflict(_))
        ));
        let msg = "This note changed on disk since it was opened. Reload the note, then reapply your edit.";
        assert_eq!(user_message(&err), msg);

        // A drifted `WRITE_CONFLICT_MESSAGE` would silently demote every conflict to a
        // generic error toast.
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
    fn delete_note_removes_it_model_free_and_lists_shrink() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let report = delete_note_impl(&state, "concepts/memory").unwrap();
        assert_eq!(report.path, "concepts/memory.md");
        assert_eq!(
            report.dangled,
            vec!["notes/spaced-repetition.md".to_string()]
        );

        assert!(!root.join("concepts/memory.md").exists());
        let notes = list_notes_impl(&state).unwrap();
        assert!(notes.iter().all(|n| n.path != "concepts/memory.md"));
    }

    #[test]
    fn delete_dir_removes_the_subtree() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        let report = delete_dir_impl(&state, "resources").unwrap();
        assert_eq!(report.deleted_resources, 4);
        assert!(!root.join("resources").exists());
    }

    #[test]
    fn delete_refusals_stay_generic_and_actionable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let err = delete_note_impl(&state, "no/such.md").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::NoteNotFound(_))
        ));
        assert!(user_message(&err).starts_with("Note not found:"));

        let err = delete_dir_impl(&state, "no-such-folder").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::DirNotFound(_))
        ));
        assert!(user_message(&err).starts_with("Folder not found:"));

        let err = delete_resource_impl(&state, "resources/nope.png").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::ResourceNotFound(_))
        ));
        assert!(user_message(&err).starts_with("File not found in the vault:"));
    }

    #[test]
    fn write_note_runs_outside_the_reindex_slot() {
        // A save must not queue behind a long background embed.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        let running = state.reindex.try_claim(); // stand in for an in-flight embed
        assert!(running.is_some());
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
        // Never indexed, so `project` alone makes the tree listable.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
        copy_dir(&src, &root);
        let state = AppState::new(Some(root));

        // A stand-in for an in-flight embed.
        let running = state.reindex.try_claim();
        assert!(running.is_some());
        let report = project_impl(&state).unwrap();
        assert_eq!(report.indexed, 2);

        let notes = list_notes_impl(&state).unwrap();
        assert_eq!(notes.len(), 2);
    }

    #[test]
    fn project_skips_unreadable_files_and_still_reports() {
        // A non-UTF-8 file is skipped and reported, not fatal to the pass.
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
        assert_eq!(list_notes_impl(&state).unwrap().len(), 2);
    }

    /// `open_external`'s allow-list, its whole security posture. Each refusal is a scheme
    /// that would let a `.md` name a program for the OS to launch.
    #[test]
    fn only_web_links_are_openable() {
        for ok in [
            "https://example.com/a?b=1#c",
            "http://example.com",
            "HTTPS://Example.COM", // a scheme is case-insensitive
            "mailto:someone@example.com?subject=hi",
        ] {
            assert!(is_openable_link(ok), "{ok} is a web link");
        }
        for refused in [
            "file:///etc/passwd",          // the local disk, via the note's choice of path
            "javascript:alert(1)",         // the sanitizer drops it; this is the second layer
            "x-b2-evil://run",             // any app that registered a custom scheme
            "vscode://file/etc/passwd",    // ditto, with a real-world registrant
            " https://example.com",        // a leading space is not a scheme
            "https://",                    // a scheme naming nothing
            "https://ok.example\nmailto:", // a control char is smuggling, never a URL
            "",
        ] {
            assert!(!is_openable_link(refused), "{refused:?} is refused");
        }
    }

    #[test]
    fn a_refused_link_says_so_without_echoing_the_url() {
        let err = CmdError::UnsupportedLink("x-b2-evil://run?secret=hunter2".into());
        let msg = user_message(&err);
        assert!(msg.contains("http"), "the message says what B2 does open");
        assert!(
            !msg.contains("x-b2-evil") && !msg.contains("hunter2"),
            "the note's URL stays out of the message"
        );
    }

    #[test]
    fn vault_switch_cancels_and_waits_for_the_inflight_reindex() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        fs::create_dir_all(&root).unwrap();
        let state = AppState::new(Some(root.clone()));

        let held = state.reindex.try_claim().expect("the slot is free");
        assert!(state.reindex.in_flight());

        std::thread::scope(|s| {
            // A stand-in reindex worker that winds down once cancelled.
            s.spawn(|| {
                while !state.reindex.cancelled() {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                drop(held);
            });

            let info = set_vault_root_impl(&state, &root).unwrap();
            assert_eq!(info.root, root.display().to_string());
            assert!(!state.reindex.in_flight());
        });
    }

    // --- chat (flow ④) --------------------------------------------------------------
    //
    // The host owns only the token stream's framing, its guard and its cancel checkpoint;
    // the answer itself is the engine suite's.

    /// A vault whose passages are real, so the fake provider has something to cite.
    fn ask_state(tmp: &tempfile::TempDir) -> (AppState, Vault) {
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let vault = Vault::open(&root).unwrap();
        (AppState::new(Some(root)), vault)
    }

    #[test]
    fn ask_streams_every_token_in_order_and_returns_the_resolved_answer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, vault) = ask_state(&tmp);
        let streamed = std::cell::RefCell::new(Vec::<String>::new());

        let answer = ask_impl(&state, &vault, &FakeLlm, "memory", &[], &|t| {
            streamed.borrow_mut().push(t.to_string())
        })
        .unwrap();

        // The streamed tokens, concatenated, are the returned answer.
        assert_eq!(streamed.borrow().concat(), answer.answer);
        assert!(!answer.cancelled);
        assert!(
            !answer.citations.is_empty(),
            "the fake cites every passage it was handed: {answer:?}"
        );
        assert!(state.ask.try_claim().is_some());
    }

    #[test]
    fn esc_mid_answer_keeps_the_partial_text_and_says_it_was_stopped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, vault) = ask_state(&tmp);
        let streamed = std::cell::RefCell::new(Vec::<String>::new());

        // Esc after the first token: the `cancel_ask` race, made deterministic.
        let answer = ask_impl(&state, &vault, &FakeLlm, "memory", &[], &|t| {
            streamed.borrow_mut().push(t.to_string());
            state.ask.cancel();
        })
        .unwrap();

        assert!(answer.cancelled, "a stopped answer is marked stopped");
        assert_eq!(
            streamed.borrow().len(),
            1,
            "cancellation is token-granular: nothing streams after the break"
        );
        assert_eq!(streamed.borrow().concat(), answer.answer);
    }

    #[test]
    fn a_second_answer_is_refused_while_one_is_streaming() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, vault) = ask_state(&tmp);
        // Stand in for an answer already streaming.
        let streaming = state.ask.try_claim();
        assert!(streaming.is_some());

        let err = ask_impl(&state, &vault, &FakeLlm, "memory", &[], &|_| {}).unwrap_err();
        assert!(matches!(err, CmdError::AskInFlight));
        assert_eq!(
            user_message(&err),
            "B2 is still answering. Wait for it to finish, or press Esc to stop it."
        );
    }

    #[test]
    fn why_similar_streams_like_an_answer_and_shares_its_slot_and_its_esc() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, vault) = ask_state(&tmp);
        // Any two notes do; what it says is `tests/why.rs`'s business.
        let notes = vault.list_notes().unwrap();
        let (anchor, candidate) = (notes[0].path.as_str(), notes[1].path.as_str());
        let streamed = std::cell::RefCell::new(Vec::<String>::new());

        let answer = why_similar_impl(&state, &vault, &FakeLlm, anchor, candidate, 10, &|t| {
            streamed.borrow_mut().push(t.to_string())
        })
        .unwrap();
        assert_eq!(streamed.borrow().concat(), answer.answer);
        assert!(!answer.cancelled);
        for c in &answer.citations {
            assert!(c.path == anchor || c.path == candidate, "{c:?}");
        }

        streamed.borrow_mut().clear();
        let stopped = why_similar_impl(&state, &vault, &FakeLlm, anchor, candidate, 10, &|t| {
            streamed.borrow_mut().push(t.to_string());
            state.ask.cancel();
        })
        .unwrap();
        assert!(stopped.cancelled);
        assert_eq!(streamed.borrow().len(), 1);

        let streaming = state.ask.try_claim();
        assert!(streaming.is_some());
        let err =
            why_similar_impl(&state, &vault, &FakeLlm, anchor, candidate, 10, &|_| {}).unwrap_err();
        assert!(matches!(err, CmdError::AskInFlight));
    }

    #[test]
    fn a_stale_cancel_does_not_stop_the_next_answer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, vault) = ask_state(&tmp);
        state.ask.cancel(); // from a turn that already ended
        let answer = ask_impl(&state, &vault, &FakeLlm, "memory", &[], &|_| {}).unwrap();
        assert!(!answer.cancelled);
    }

    #[test]
    fn the_tool_call_cap_is_set_kept_and_cleared_by_a_save() {
        let state = AppState::new(None);
        let keys = MemoryStore::empty();
        let save = |cap: Option<&str>| {
            set_chat_config_impl(&state, None, None, None, cap.map(str::to_string), &keys);
            state.chat_prefs().config().max_tool_calls
        };
        let default = b2_llm::LlmConfig::from_env().max_tool_calls;
        assert_eq!(save(Some("128")), 128);
        assert_eq!(save(None), 128, "a save that doesn't mention it keeps it");
        assert_eq!(save(Some("0")), 128, "a refused value changes nothing");
        assert_eq!(
            save(Some("")),
            default,
            "blank returns to the shared resolution"
        );
        save(Some("32"));
        let setup = b2_llm::ChatSetup::fake(&state.chat_prefs().config());
        assert_eq!(setup.tool_calls.in_force, 32);
    }

    #[test]
    fn chat_config_layers_over_the_shared_resolution() {
        let state = AppState::new(None);
        let keys = MemoryStore::empty();
        set_chat_config_impl(
            &state,
            Some("http://localhost:1234/v1".into()),
            Some("qwen2.5".into()),
            Some("sk-a-cloud-key".into()),
            None,
            &keys,
        );
        let prefs = state.chat_prefs();
        assert_eq!(prefs.base_url.as_deref(), Some("http://localhost:1234/v1"));
        assert_eq!(prefs.model.as_deref(), Some("qwen2.5"));
        assert_eq!(prefs.api_key.as_deref(), Some("sk-a-cloud-key"));

        // Key absent and model blanked: the key survives, the model falls back.
        set_chat_config_impl(
            &state,
            Some("http://localhost:1234/v1".into()),
            Some("  ".into()),
            None,
            None,
            &keys,
        );
        let prefs = state.chat_prefs();
        assert_eq!(prefs.api_key.as_deref(), Some("sk-a-cloud-key"));
        assert_eq!(prefs.model, None);
        assert_eq!(prefs.config().model, b2_llm::LlmConfig::from_env().model);
    }

    /// GH #176 at the command boundary.
    #[test]
    fn a_saved_key_is_there_at_the_next_launch() {
        let keys = MemoryStore::empty();
        set_chat_config_impl(
            &AppState::new(None),
            Some("https://api.example.com/v1".into()),
            None,
            Some("sk-remember-me".into()),
            None,
            &keys,
        );
        assert_eq!(keys.peek().as_deref(), Some("sk-remember-me"));

        let next_launch = crate::chat::read_prefs_from(None, &keys);
        assert_eq!(next_launch.api_key.as_deref(), Some("sk-remember-me"));
        assert_eq!(next_launch.api_key_source(), b2_llm::ApiKeySource::Stored);
    }

    #[test]
    fn a_blanked_key_clears_it_everywhere() {
        let state = AppState::new(None);
        let keys = MemoryStore::empty();
        set_chat_config_impl(
            &state,
            None,
            None,
            Some("sk-a-cloud-key".into()),
            None,
            &keys,
        );
        assert_eq!(
            state.chat_prefs().api_key.as_deref(),
            Some("sk-a-cloud-key")
        );

        // Absent: untouched.
        set_chat_config_impl(&state, None, None, None, None, &keys);
        assert_eq!(
            state.chat_prefs().api_key.as_deref(),
            Some("sk-a-cloud-key")
        );
        assert_eq!(keys.peek().as_deref(), Some("sk-a-cloud-key"));

        // Blank (the Remove button): gone from both. `B2_LLM_API_KEY` is beyond its reach.
        set_chat_config_impl(&state, None, None, Some("   ".into()), None, &keys);
        assert_eq!(state.chat_prefs().api_key, None);
        assert_eq!(
            keys.peek(),
            None,
            "a removed key must not return next launch"
        );
    }

    /// Also pinned in `b2-llm` on the view type; this is the command boundary.
    #[test]
    fn the_chat_setup_view_never_carries_the_key() {
        let state = AppState::new(None);
        set_chat_config_impl(
            &state,
            // `.invalid` is reserved (RFC 2606), so the probe fails at once.
            Some("http://b2-no-such-host.invalid:11434/v1".into()),
            Some("llama3.2".into()),
            Some("sk-live-must-not-cross".into()),
            None,
            &MemoryStore::empty(),
        );
        let setup = chat_setup_impl(&state);
        let json = serde_json::to_string(&setup).unwrap();
        assert!(!json.contains("sk-live-must-not-cross"), "{json}");
        // The source depends on the developer's env, so only assert it isn't "none".
        assert!(json.contains("\"api_key_source\":"), "{json}");
        assert!(!json.contains("\"api_key_source\":\"none\""), "{json}");
    }

    #[test]
    fn a_refused_store_leaves_the_key_in_force_for_the_session() {
        let state = AppState::new(None);
        let keys = MemoryStore::refusing();
        set_chat_config_impl(
            &state,
            Some("https://api.example.com/v1".into()),
            None,
            Some("sk-typed-just-now".into()),
            None,
            &keys,
        );
        let prefs = state.chat_prefs();
        assert_eq!(prefs.api_key.as_deref(), Some("sk-typed-just-now"));
        assert_eq!(keys.peek(), None, "a refused write stores nothing");
        assert_eq!(prefs.api_key_source(), b2_llm::ApiKeySource::Session);
    }
}
