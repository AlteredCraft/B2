//! The `#[tauri::command]` handlers — B2's IPC surface, and the frontend's mirror of
//! the [`Vault`](b2_core::vault::Vault) façade (crates/b2-desktop/CLAUDE.md). Each
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

use crate::chat::ChatPrefs;
use crate::error::CmdError;
use crate::watch::VaultWatcher;
use crate::{open_read, open_semantic, open_vault, AppState, AskGuard, ReindexGuard};
use b2_core::add::AddReport;
use b2_core::ingest::ReindexProgress;
use b2_core::llm::{ChatTurn, LlmProvider};
use b2_core::vault::{
    AnswerView, DeleteReport, DirCreateReport, DirDeleteReport, DirMoveReport, EmbedReport,
    ExplainView, ImportReport, LinkReport, MoveReport, NeighborView, NoteSummary, NoteView,
    ProjectReport, ResourceDeleteReport, ResourceExplainView, ResourceMoveReport, ResourceSummary,
    SearchResult, SimilarView, Vault, WriteReport,
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
    let vault = open_read(state.inner())?;
    Ok(vault.list_resources()?)
}

/// The file tree's structure half: every folder in the vault, **empty ones
/// included**, read live off the filesystem (never the index) so the tree stays
/// one-to-one with disk. The frontend composes this with `list_notes` +
/// `list_resources` into one tree.
#[tauri::command(async)]
pub fn list_dirs(state: State<'_, AppState>) -> Result<Vec<String>, CmdError> {
    list_dirs_impl(state.inner())
}

/// Create a folder — the file tree's New-folder action (⇧⌘N). A real on-disk
/// create, missing parents included, occupied targets refused (a folder is
/// user-authored vault structure, immediately visible to Finder/CLI/sync),
/// touching no index rows. Model-free and index-free.
#[tauri::command(async)]
pub fn create_dir(state: State<'_, AppState>, dir: String) -> Result<DirCreateReport, CmdError> {
    create_dir_impl(state.inner(), &dir)
}

/// The fallback card's data: a resource's inventory metadata + backlinks. A pure
/// graph/inventory read, model-free like `explain`.
#[tauri::command(async)]
pub fn explain_resource(
    state: State<'_, AppState>,
    path: String,
) -> Result<ResourceExplainView, CmdError> {
    let vault = open_read(state.inner())?;
    Ok(vault.explain_resource(&path)?)
}

/// *Open in system default* on the fallback card — an **OS handoff**, never
/// in-webview execution (spec §6 security posture). Host infrastructure like the
/// folder dialog: the webview holds no opener permission; this command validates
/// the path against the inventory (so only an indexed vault file can be opened)
/// and hands the absolute path to the OS.
#[tauri::command(async)]
pub fn open_resource(state: State<'_, AppState>, path: String) -> Result<(), CmdError> {
    let vault = open_read(state.inner())?;
    vault.explain_resource(&path)?; // inventory check: unknown paths refuse, never open
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    tauri_plugin_opener::open_path(root.join(&path), None::<&str>)
        .map_err(|e| CmdError::OpenFailed(e.to_string()))
}

/// The three URL schemes B2 will hand to the OS, lowercase, each including the
/// punctuation that ends it. The frontend's `externalUrl` (`ui/src/links.ts`) holds the
/// same list because it is what *routes* a click here; this one is what **refuses**, and
/// it is the authority — change them together, the way `WRITE_CONFLICT_MESSAGE` and
/// `VAULT_CHANGED_EVENT` are changed on both sides of the seam.
const OPENABLE_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

/// Is this a link the host will open in the user's browser or mail app?
///
/// A note is **untrusted input** (invariant E5): its links are authored by whoever wrote
/// the file, and `open` on macOS launches whatever app has registered the scheme — so an
/// unfiltered handoff turns a `.md` into "run the thing this URL names". The allow-list
/// is therefore the whole of the security posture here, and it is deliberately three
/// schemes wide: the web, and email.
///
/// Byte-wise rather than `&url[..n]`, so a URL beginning mid-UTF-8 can't panic the slice;
/// control characters are refused outright (a `\n` in an href is smuggling, never a URL).
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

/// *Open a web link in the system default browser* — [`open_resource`]'s sibling for the
/// links **inside** a note, and the same OS handoff rather than in-webview navigation.
///
/// Following a `https://…` in place would replace the whole app with a web page in a
/// window that has no back button, no address bar and no way home; the webview holds no
/// opener permission, so the frontend routes the click here instead
/// (`ui/src/links.ts` + the click delegation in `main.ts`). Vault-free — nothing here
/// touches the index — so it takes no `State`, like `clipboard_text`.
///
/// The frontend has already decided this href is a web link; this re-checks, because the
/// frontend's copy of the rule is *routing* and the note that authored the href is not
/// trusted (the `open_resource` posture: the caller passes, the host validates).
#[tauri::command(async)]
pub fn open_external(url: String) -> Result<(), CmdError> {
    if !is_openable_link(&url) {
        return Err(CmdError::UnsupportedLink(url));
    }
    tauri_plugin_opener::open_url(url, None::<&str>)
        .map_err(|e| CmdError::OpenFailed(e.to_string()))
}

/// The clipboard's plain-text flavor — what the editor's ⌘⇧V pastes (paste as plain
/// text, the escape hatch from the rich paste in `ui/src/paste.ts`). Host infrastructure
/// like the folder dialog and [`open_resource`], and host-side *necessarily*: WebKit runs
/// no editing command for a raw ⌘⇧V, and it gates a programmatic `navigator.clipboard`
/// read behind a native confirmation — so the webview cannot do this itself. Touches no
/// vault; an empty or non-text clipboard reads as "".
#[tauri::command(async)]
pub fn clipboard_text(app: tauri::AppHandle) -> Result<String, CmdError> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .read_text()
        .map_err(|e| CmdError::ClipboardFailed(e.to_string()))
}

/// Save a note's body — the editing surface's body write (crates/b2-desktop/CLAUDE.md;
/// [`write_frontmatter`] is its frontmatter sibling).
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

/// Save a note's frontmatter — the drawer's write op (GH #79), `write_note`'s
/// frontmatter sibling with the identical posture: **model-free** (an unchanged
/// body keeps its vectors, so no model is ever needed), outside the embed slot,
/// and guarded by the same `base_revision` conflict contract the frontend
/// recognizes. The fence refusal — the one thing the façade still checks, because a
/// stray `---` would shift bytes into the body — lives behind
/// `Vault::write_frontmatter`, not here. B2 owns no line *inside* the block: the
/// `b2id` guard went with the stamp (GH #170).
#[tauri::command(async)]
pub fn write_frontmatter(
    state: State<'_, AppState>,
    note: String,
    frontmatter: String,
    base_revision: String,
) -> Result<WriteReport, CmdError> {
    write_frontmatter_impl(state.inner(), &note, &frontmatter, &base_revision)
}

/// Create a new, empty note — the file tree's New-note action (and ⌘N). **Model-free**
/// like `write_note`/`project`: `Vault::create_note` writes the file and projects it
/// without touching vectors, so creating works with no model provisioned, and it runs
/// outside the single-in-flight embed slot (short, and a fake-opened vault must never
/// write into a real-model embedding space). The note's chunks join the DB-derived
/// pending set for the next embed pass — and an empty body has nothing to embed anyway.
/// Missing parent folders are created, mirroring `b2 add`; an *empty* folder is
/// [`create_dir`]'s job.
#[tauri::command(async)]
pub fn create_note(state: State<'_, AppState>, path: String) -> Result<AddReport, CmdError> {
    create_note_impl(state.inner(), &path)
}

/// Import a file from **outside** the vault into the folder `dir` (`""` for the root)
/// — the file tree's drop target (a drag from Finder) and its Import files… action.
/// `data` is the file's bytes, base64-encoded: a file dropped on a webview arrives as
/// *content*, not a path (the OS hands WebKit the bytes), and Tauri's JSON IPC carries
/// no byte array cheaply. Decoding it is **transport**, not logic — the façade op takes
/// the bytes and does the placing, the projecting, and every refusal.
///
/// **Model-free** (the `create_note`/`write_note` posture): `Vault::import_file` writes
/// the file and projects it without touching vectors, so importing works with no model
/// provisioned and runs outside the single-in-flight embed slot. An imported note's
/// chunks join the DB-derived pending set for the next embed pass.
#[tauri::command(async)]
pub fn import_file(
    state: State<'_, AppState>,
    dir: String,
    name: String,
    data: String,
) -> Result<ImportReport, CmdError> {
    import_file_impl(state.inner(), &dir, &name, &data)
}

/// [`import_file`] from a path instead of bytes — what the Import files… picker's
/// selections come back as. Same façade op, same posture; the bytes never round-trip
/// through the webview.
#[tauri::command(async)]
pub fn import_path(
    state: State<'_, AppState>,
    dir: String,
    source: String,
) -> Result<ImportReport, CmdError> {
    import_path_impl(state.inner(), &dir, &source)
}

/// The keyboard half of the drop gesture (K1): open a **native multi-select file
/// picker** and return what was chosen, as absolute paths for [`import_path`] to place.
/// An empty list means the user cancelled.
///
/// Host-owned for `choose_vault`'s reason — a picker is an OS concern with nothing to
/// push behind the façade, and running it in Rust is what keeps the webview
/// dialog-permission-free. It deliberately imports *nothing*: the choosing is the OS's
/// job, the placing is the façade's, and this command only carries the answer between
/// them. `(async)` is required, as there: `blocking_pick_files` waits on the main
/// thread to show the panel.
#[tauri::command(async)]
pub fn pick_import_files(app: tauri::AppHandle) -> Result<Vec<String>, CmdError> {
    let Some(picked) = app.dialog().file().blocking_pick_files() else {
        return Ok(Vec::new()); // user cancelled
    };
    Ok(picked
        .into_iter()
        // On desktop a file pick is always a real filesystem path (`Url` is a mobile
        // content URI); anything else is dropped rather than surfaced as an error.
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// Move/rename a note — the tree's Rename / Move… / drag-drop action. Opens the
/// **real** model (like `search`/`link`): rewriting an inbound file's link text
/// changes its body, and the re-projection re-embeds it inline.
#[tauri::command(async)]
pub fn move_note(
    state: State<'_, AppState>,
    note: String,
    to: String,
) -> Result<MoveReport, CmdError> {
    move_note_impl(state.inner(), &note, &to)
}

/// [`move_note`]'s resource sibling — same posture, same path-keyed identity (L3).
#[tauri::command(async)]
pub fn move_resource(
    state: State<'_, AppState>,
    path: String,
    to: String,
) -> Result<ResourceMoveReport, CmdError> {
    move_resource_impl(state.inner(), &path, &to)
}

/// Move/rename a whole folder — one rename on disk (unindexed files travel too),
/// inbound links across and within the moved set rewritten, index re-projected.
#[tauri::command(async)]
pub fn move_dir(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<DirMoveReport, CmdError> {
    move_dir_impl(state.inner(), &from, &to)
}

/// Delete a note — the tree's Delete action (context menu, ⌘⌫). **Model-free**
/// (the `create_note`/`write_note` posture): no body changes, so nothing
/// re-embeds — inbound links simply dangle, exactly as an external delete plus a
/// reindex would leave them.
#[tauri::command(async)]
pub fn delete_note(state: State<'_, AppState>, note: String) -> Result<DeleteReport, CmdError> {
    delete_note_impl(state.inner(), &note)
}

/// [`delete_note`]'s resource sibling — same posture, same path-keyed identity (L3).
#[tauri::command(async)]
pub fn delete_resource(
    state: State<'_, AppState>,
    path: String,
) -> Result<ResourceDeleteReport, CmdError> {
    delete_resource_impl(state.inner(), &path)
}

/// Delete a whole folder and everything inside it (one `remove_dir_all` — unindexed
/// files go too). The frontend confirms first; the host just executes. Model-free.
#[tauri::command(async)]
pub fn delete_dir(state: State<'_, AppState>, dir: String) -> Result<DirDeleteReport, CmdError> {
    delete_dir_impl(state.inner(), &dir)
}

#[tauri::command(async)]
pub fn similar(
    state: State<'_, AppState>,
    note: String,
    limit: usize,
    no_floor: Option<bool>,
) -> Result<Vec<SimilarView>, CmdError> {
    // `no_floor` is the pane's explicit "show the raw nearest anyway" (GH #150) —
    // the GUI sibling of `b2 similar --no-floor`, routed to the façade's own raw
    // method so the choice lives in the core, not here.
    let vault = open_read(state.inner())?;
    Ok(if no_floor.unwrap_or(false) {
        vault.similar_raw(&note, limit)?
    } else {
        vault.similar(&note, limit)?
    })
}

#[tauri::command(async)]
pub fn search(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<SearchResult>, CmdError> {
    // Semantic: the query is embedded, so this opens the real model (fail-fast if absent).
    let vault = open_semantic(state.inner())?;
    Ok(vault.search(&query, limit)?)
}

#[tauri::command(async)]
pub fn neighbors(state: State<'_, AppState>, note: String) -> Result<Vec<NeighborView>, CmdError> {
    let vault = open_read(state.inner())?;
    Ok(vault.neighbors(&note)?)
}

#[tauri::command(async)]
pub fn explain(state: State<'_, AppState>, note: String) -> Result<ExplainView, CmdError> {
    let vault = open_read(state.inner())?;
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
    let vault = open_semantic(state.inner())?;
    Ok(vault.link(&src, &dst, &relation, explanation.as_deref())?)
}

/// The **projection pass** — the fast, model-free half of a reindex
/// (index-engine.md). One façade call over the **fake** vault (no
/// model load on the first-paint path), so the moment it returns the file tree can
/// repopulate and keyword search answers; `embed` then streams behind it. Fast and
/// synchronous-feeling; nothing to stream, nothing to cancel.
///
/// Deliberately **outside** the single-in-flight reindex slot: the slot exists to
/// protect the long, vector-writing embed pass, and a `project` racing a vault
/// switch is harmless — it writes the `.b2/` of the root it captured at dispatch,
/// idempotently, never the new vault's ("why leaving `project` outside the slot
/// is safe").
#[tauri::command(async)]
pub fn project(state: State<'_, AppState>) -> Result<ProjectReport, CmdError> {
    project_impl(state.inner())
}

/// The **embed pass** — fill the missing vectors as an **observable, cancellable
/// background action**. Tauri runs the `(async)` body on a
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
/// writes. A no-op if nothing is running.
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

impl From<(String, crate::stats::ModelStat)> for EmbedStat {
    fn from((model, s): (String, crate::stats::ModelStat)) -> Self {
        Self {
            model,
            total_ms: s.total_ms,
            chunks: s.chunks,
            runs: s.runs,
        }
    }
}

/// The per-model embedding-time ledger (`stats.rs`) — what the Settings pane renders so a
/// model swap can be judged on real speed. Infallible: no data / an unreadable ledger is
/// an empty list, never an error (the totals are diagnostic, never load-bearing).
#[tauri::command(async)]
pub fn embed_stats() -> Vec<EmbedStat> {
    crate::stats::read_all()
        .into_iter()
        .map(EmbedStat::from)
        .collect()
}

/// Every chord the app's **menu bar** takes, in menu order (`menu.rs`, #119). The UI
/// folds these into its keyboard registry as reserved chords: the reference sheet lists
/// them, and the conflict check can finally see the one set of chords it was blind to
/// — AppKit dispatches a menu key equivalent before the webview receives the key at all,
/// so no amount of watching `keydown` would have found them.
///
/// Static data, so infallible and vault-free — the shape of [`embed_device`], not of a
/// façade call.
#[tauri::command]
pub fn menu_chords() -> Vec<crate::menu::MenuChord> {
    crate::menu::chords()
}

/// **Flow ④ — one grounded answer** (GH #151/#153, cut here as GH #155): condense →
/// retrieve → assemble → stream → cite, all of it behind `Vault::ask`. The host's whole
/// contribution is the shape of the *delivery*: Tauri runs the `(async)` body on a worker
/// thread (the cancellable-reindex-task precedent), tokens stream to the webview over a
/// typed per-invocation [`Channel`] as they arrive, and the resolved [`AnswerView`] is the
/// command's return — the same two facts `b2 ask --json` emits as a JSONL event stream,
/// which is the standing convention (the view types are the adapters' shared contract).
///
/// `history` is the **caller's**: session-only (S4), held in the pane's own state and
/// handed back turn by turn, exactly as `b2 chat` holds a `Vec<ChatTurn>`. Nothing about a
/// chat is stored here or anywhere else — no `meta` row, no cache, no transcript.
///
/// Opens the **real-model** vault: retrieval embeds the question for the vector half, like
/// `search` (and degrades to BM25-only on an unembedded vault, M4 — chat keeps working).
#[tauri::command(async)]
pub fn ask(
    state: State<'_, AppState>,
    question: String,
    history: Vec<ChatTurn>,
    on_event: Channel<String>,
) -> Result<AnswerView, CmdError> {
    let state = state.inner();
    // Pick the provider (`B2_LLM=fake` or the configured endpoint) and open the vault
    // before claiming the answer slot: both are the wiring `ask_impl` is handed, which is
    // what makes the streaming half — the guard, the sink, the cancel checkpoint —
    // testable against `FakeLlm` and a fake-embedder vault with no Tauri runtime and no
    // environment to set.
    let llm = crate::chat::provider(&state.chat_prefs());
    let vault = open_semantic(state)?;
    ask_impl(state, &vault, llm.as_ref(), &question, &history, &|token| {
        // A send error means the window navigated or closed; the stream is then
        // pointless but not broken — the cancel flag is what actually stops it.
        let _ = on_event.send(token.to_string());
    })
}

/// Ask the streaming answer to stop at its next token — the chat pane's Esc. Runs on a
/// *different* worker thread than `ask`, so it sets the shared flag while that one reads
/// it; the token callback sees it and breaks cooperatively. The partial text is not
/// discarded: it comes back as an [`AnswerView`] marked `cancelled`, which is what lets the
/// pane render a stopped answer honestly rather than as a failure. A no-op if nothing is
/// streaming.
#[tauri::command(async)]
pub fn cancel_ask(state: State<'_, AppState>) {
    state.request_ask_cancel();
}

/// What the chat surface needs to draw itself before a question is asked: the endpoint and
/// model in force, whether that is the **Local** or the **Cloud models** configuration, and
/// — when the runtime is Ollama — the native inventory behind the setup card (GH #151's
/// deliberately Ollama-native onboarding corner: is the daemon up, what is installed, what
/// would you pull on a machine this size).
///
/// Thin like `list_models`: this is provider wiring, not a vault op, so the one call is
/// into `b2-llm` (which owns the probe, the `/api/tags` read and the tier heuristic) and
/// the rest is serialization. Network-bound, hence `(async)`.
///
/// Infallible **by design** — "the daemon isn't running" is the answer the card is asking
/// for, not an error to raise (`b2_llm::probe_setup`). And it never carries the API key:
/// the view reports only that one is configured.
#[tauri::command(async)]
pub fn chat_setup(state: State<'_, AppState>) -> ChatSetup {
    chat_setup_impl(state.inner())
}

/// Save the chat configuration and re-probe it — the Settings section's one write.
///
/// **Adapter state, never vault or index state** (GH #151): the endpoint and model persist
/// beside the remembered vault, so a chat model swap costs no reindex (contrast M2). The
/// key goes to the platform's own encrypted store instead (GH #176 — `keychain.rs` says
/// why, and what happens when that store says no); `None` leaves whatever is already in
/// force, so re-saving the endpoint doesn't silently clear a key the user typed a minute
/// ago.
#[tauri::command(async)]
pub fn set_chat_config(
    state: State<'_, AppState>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
) -> ChatSetup {
    let prefs = set_chat_config_impl(
        state.inner(),
        base_url,
        model,
        api_key,
        &crate::keychain::Keychain,
    );
    // Persisting lives in the command wrapper, not in the state transition, so the
    // unit-tested core never writes to the real user data dir (`choose_vault`'s split).
    // The Keychain is the exception and has to be: whether the store took the key
    // decides how long it lasts, and the state transition is what records that — so the
    // store crosses in as a parameter, and tests pass an in-memory one.
    crate::chat::persist_prefs(&prefs);
    chat_setup_impl(state.inner())
}

/// The testable core of `set_chat_config`: normalize the three fields into
/// [`ChatPrefs`] and install them. Blank is "unset" for the endpoint and the model
/// (the `LlmConfig::from_env` rule — an emptied field returns to the
/// environment/default rather than configuring an unusable endpoint).
///
/// The key does not go through `clean`, and must not: blank is a *distinct* input
/// there rather than the absence of one. `chat::apply_key` owns that three-state
/// rule and the [`KeyStore`](crate::keychain::KeyStore) write it implies.
fn set_chat_config_impl(
    state: &AppState,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
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
        api_key,
        key_remembered,
    };
    state.set_chat_prefs(prefs.clone());
    prefs
}

/// The testable core of `chat_setup`: the fake provider's own status when
/// `B2_LLM=fake` is in force (never overstate what answered — the CLI prints the
/// same note), else one probe of the configured endpoint.
fn chat_setup_impl(state: &AppState) -> ChatSetup {
    let config = state.chat_prefs().config();
    if crate::chat::use_fake_llm() {
        ChatSetup::fake(&config)
    } else {
        b2_llm::probe_setup(&config)
    }
}

/// The testable core of `ask`, split from the Tauri `Channel` wrapper so the whole
/// streaming path — the single-in-flight guard, the token sink, the cancel checkpoint —
/// is exercised without a Tauri runtime. `sink` is the framing the host owns: every token
/// the seam delivers, in order, as it arrives.
fn ask_impl(
    state: &AppState,
    vault: &Vault,
    llm: &dyn LlmProvider,
    question: &str,
    history: &[ChatTurn],
    sink: &dyn Fn(&str),
) -> Result<AnswerView, CmdError> {
    // Single-in-flight: two answers at once would share one cancel flag, so the second
    // one's `arm` would quietly un-cancel the first. The pane already refuses a second
    // turn while one is streaming, so this is the belt-and-suspenders half.
    if !state.try_start_ask() {
        return Err(CmdError::AskInFlight);
    }
    let _guard = AskGuard(state);
    // Clear any stale cancel now that *this* answer owns the slot (the Esc that stopped
    // the previous turn must not stop this one before its first token).
    state.arm_ask();

    Ok(vault.ask(llm, question, history, &mut |token| {
        sink(token);
        if state.ask_cancelled() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?)
}

/// The testable core of `project`: one façade call over the fake vault (projection
/// is model-free by construction — it never touches the embedding space).
fn project_impl(state: &AppState) -> Result<ProjectReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.project(false)?)
}

/// The testable core of `embed`, split from the Tauri `State` wrapper. Guards
/// single-in-flight, arms the cancel flag, opens the real-model vault, and streams
/// progress while consulting the cancel flag at each batch.
fn embed_impl(
    state: &AppState,
    on_event: &Channel<ReindexProgress>,
) -> Result<EmbedReport, CmdError> {
    // Single-in-flight: refuse a second embed rather than race two writers on one DB.
    // The UI also disables the button, so this is rarely hit.
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
    // cumulative, so its last value is this run's chunk count.
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
    // Model-free read, on both halves — this sits on the first-paint path (#133). The
    // vault opens with the fake purely to count embedding coverage (#26), and
    // `semantic` is a file *probe* (`semantic_available`), not a model load: it stays
    // "is a model installed", while `notes_embedded/total` is the precise fraction the
    // UI flags keyword-only from.
    let vault = open_read(state)?;
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
/// repointing the root, so a reindex can never keep writing the vault the app has left.
fn set_vault_root_impl(state: &AppState, root: &Path) -> Result<VaultInfo, CmdError> {
    state.cancel_and_wait_for_reindex();
    state.set_root(root);
    vault_info_impl(state)
}

fn read_note_impl(state: &AppState, note: &str) -> Result<NoteView, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.read(note)?)
}

fn list_notes_impl(state: &AppState) -> Result<Vec<NoteSummary>, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.list_notes()?)
}

fn list_dirs_impl(state: &AppState) -> Result<Vec<String>, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.list_dirs()?)
}

fn create_dir_impl(state: &AppState, dir: &str) -> Result<DirCreateReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.create_dir(dir)?)
}

fn write_note_impl(
    state: &AppState,
    note: &str,
    body: &str,
    base_revision: &str,
) -> Result<WriteReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.write(note, body, base_revision)?)
}

fn write_frontmatter_impl(
    state: &AppState,
    note: &str,
    frontmatter: &str,
    base_revision: &str,
) -> Result<WriteReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.write_frontmatter(note, frontmatter, base_revision)?)
}

fn create_note_impl(state: &AppState, path: &str) -> Result<AddReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.create_note(path)?)
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
    let vault = open_read(state)?;
    Ok(vault.import_file(dir, name, &bytes)?)
}

fn import_path_impl(state: &AppState, dir: &str, source: &str) -> Result<ImportReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.import_path(dir, Path::new(source))?)
}

fn move_note_impl(state: &AppState, note: &str, to: &str) -> Result<MoveReport, CmdError> {
    let vault = open_semantic(state)?;
    Ok(vault.move_note(note, to)?)
}

fn move_resource_impl(
    state: &AppState,
    path: &str,
    to: &str,
) -> Result<ResourceMoveReport, CmdError> {
    let vault = open_semantic(state)?;
    Ok(vault.move_resource(path, to)?)
}

fn move_dir_impl(state: &AppState, from: &str, to: &str) -> Result<DirMoveReport, CmdError> {
    let vault = open_semantic(state)?;
    Ok(vault.move_dir(from, to)?)
}

fn delete_note_impl(state: &AppState, note: &str) -> Result<DeleteReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.delete_note(note)?)
}

fn delete_resource_impl(state: &AppState, path: &str) -> Result<ResourceDeleteReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.delete_resource(path)?)
}

fn delete_dir_impl(state: &AppState, dir: &str) -> Result<DirDeleteReport, CmdError> {
    let vault = open_read(state)?;
    Ok(vault.delete_dir(dir)?)
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
    //! back (crates/b2-desktop/CLAUDE.md — "thinness *is* the test strategy"; the
    //! façade's own suite covers behavior). Model-free: read-path commands open with
    //! the fake, and setup reindexes with the fake directly, so no model is needed.

    use super::*;
    use crate::error::user_message;
    use crate::keychain::MemoryStore;
    use b2_core::llm::FakeLlm;
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
    fn list_dirs_reads_structure_live_including_empty_folders() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        // An external `mkdir` (Finder, CLI) — no reindex follows, and none is
        // needed: structure is read off the filesystem, so the tree can't go stale.
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

        // Occupied → the actionable, no-internals refusal.
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

    // --- The host's task-lifecycle bits -------------------------------------------
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
    fn create_note_projects_and_lists_model_free() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root.clone()));

        // Missing parent folders are created on the way, like `b2 add`.
        let report = create_note_impl(&state, "inbox/idea").unwrap();
        assert_eq!(report.path, "inbox/idea.md");
        assert!(root.join("inbox/idea.md").is_file());

        // Immediately in the tree and readable (projected, no model touched).
        let notes = list_notes_impl(&state).unwrap();
        assert!(notes.iter().any(|n| n.path == "inbox/idea.md"));
        let note = read_note_impl(&state, "inbox/idea").unwrap();
        assert_eq!(note.body, "");
        assert_eq!(note.path, report.path);
    }

    /// The drop path end to end at this layer: base64 in → the façade places and
    /// projects → the tree lists it. The payload is deliberately **not** UTF-8, since
    /// the transport exists to carry a PNG as faithfully as a `.md`.
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
        let listed = open_read(&state).unwrap().list_resources().unwrap();
        assert!(listed.iter().any(|r| r.path == "resources/dropped.png"));
    }

    /// A payload the frontend's encoder mangled is the transport's own failure, so it
    /// never reaches the façade and never leaks the decoder's detail to the webview.
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

    /// The picker path: a chosen absolute path is copied in, source left alone.
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

    /// The import refusals speak of a *file*, not a note: what arrived may be a PDF.
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

        // A "name" that is really a path can't redirect the import out of the folder.
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

        // Clobber refusal names the path and the way out.
        let err = create_note_impl(&state, "concepts/memory").unwrap_err();
        assert!(matches!(
            err,
            CmdError::Core(b2_core::Error::AddTargetExists(_))
        ));
        assert_eq!(
            user_message(&err),
            "A note already exists at 'concepts/memory.md'. Choose a different name, or open that note."
        );

        // An invalid destination is refused with actionable phrasing, no internals.
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

        // Save a new block based on the read's revision; the returned revision chains
        // the next save, and a fresh read round-trips the block verbatim with the body
        // untouched.
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

    /// The drawer's one refusal, and the shape of everything it no longer refuses.
    /// A `---` line would end the block early and shift bytes into the *body*, which
    /// is not this op's to change; every other edit saves, because since GH #170 B2
    /// owns no line inside the block (W3).
    #[test]
    fn write_frontmatter_refuses_only_the_fence_and_says_so_without_internals() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        let state = AppState::new(Some(root));

        // Wholesale replacement — the edit that used to be an identity refusal — saves.
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
        // An external edit lands after the read…
        let abs = root.join("concepts/memory.md");
        fs::write(
            &abs,
            format!("{}\nexternal\n", fs::read_to_string(&abs).unwrap()),
        )
        .unwrap();

        // …so the stale save is refused with the STABLE message the frontend
        // string-matches to drive its conflict bar (crates/b2-desktop/CLAUDE.md) — keep
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
        // Like `project`, a save is deliberately unguarded by the embed slot
        // (crates/b2-desktop/CLAUDE.md): short, model-free, must not queue behind a
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
        // unguarded (index-engine.md) and must still run.
        assert!(state.try_start_reindex());
        let report = project_impl(&state).unwrap();
        assert_eq!(report.indexed, 2);

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

    /// `open_external`'s allow-list — the whole of its security posture, so it is pinned
    /// on both sides. The refusals matter more than the acceptances: each one is a scheme
    /// that would otherwise let a `.md` name a program for the OS to launch.
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

    /// The refusal reaches the webview as a generic sentence, with the note-authored URL
    /// left server-side — the same "detail is logged, never sent" rule every other arm of
    /// `user_message` follows.
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

    // --- chat (flow ④, GH #155) -----------------------------------------------------
    //
    // The host owns exactly one thing here, and it is what these cover: the **framing of
    // the token stream** — every token, in order, as it arrives — plus the guard and the
    // cancel checkpoint around it. Everything the answer *is* (condense, retrieve, prompt,
    // citations) belongs to `Vault::ask` and is covered by the engine suite against the
    // same `FakeLlm` used here.

    /// A vault whose passages are real, so the fake provider has something to cite.
    fn ask_state(tmp: &tempfile::TempDir) -> (AppState, Vault) {
        let root = tmp.path().join("vault");
        golden_indexed(&root);
        // The fake-embedder vault the read path uses everywhere in this suite: `ask`'s
        // retrieval degrades to the same hybrid search the rest of the app runs on.
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

        // The framing contract: the tokens the webview saw, concatenated, ARE the answer
        // the command returned. A pane that renders the stream and then the final view
        // must never see the two disagree.
        assert_eq!(streamed.borrow().concat(), answer.answer);
        assert!(!answer.cancelled);
        assert!(
            !answer.citations.is_empty(),
            "the fake cites every passage it was handed: {answer:?}"
        );
        // The slot is released on the way out, so the next turn can claim it.
        assert!(state.try_start_ask());
    }

    #[test]
    fn esc_mid_answer_keeps_the_partial_text_and_says_it_was_stopped() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, vault) = ask_state(&tmp);
        let streamed = std::cell::RefCell::new(Vec::<String>::new());

        // The pane's Esc, arriving after the first token — `cancel_ask` sets the same flag
        // from another thread; setting it inside the sink is that race, made deterministic.
        let answer = ask_impl(&state, &vault, &FakeLlm, "memory", &[], &|t| {
            streamed.borrow_mut().push(t.to_string());
            state.request_ask_cancel();
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
        // Stand in for an answer already streaming (the real one holds the slot for the
        // duration of its call).
        assert!(state.try_start_ask());

        let err = ask_impl(&state, &vault, &FakeLlm, "memory", &[], &|_| {}).unwrap_err();
        assert!(matches!(err, CmdError::AskInFlight));
        assert_eq!(
            user_message(&err),
            "B2 is still answering. Wait for it to finish, or press Esc to stop it."
        );
    }

    /// A stale cancel must not kill the *next* answer: `arm_ask` clears it once the fresh
    /// turn owns the slot, which is the whole reason the flag is armed rather than reset
    /// by whoever set it.
    #[test]
    fn a_stale_cancel_does_not_stop_the_next_answer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (state, vault) = ask_state(&tmp);
        state.request_ask_cancel(); // …from a turn that already ended
        let answer = ask_impl(&state, &vault, &FakeLlm, "memory", &[], &|_| {}).unwrap();
        assert!(!answer.cancelled);
    }

    /// Chat settings are adapter state: setting them changes what the next ask resolves,
    /// and blank means "unset" (back to the environment/default) rather than an unusable
    /// endpoint. The key is kept when a save doesn't mention it — re-saving the endpoint
    /// must not silently sign you out of a cloud provider.
    #[test]
    fn chat_config_layers_over_the_shared_resolution() {
        let state = AppState::new(None);
        let keys = MemoryStore::empty();
        set_chat_config_impl(
            &state,
            Some("http://localhost:1234/v1".into()),
            Some("qwen2.5".into()),
            Some("sk-a-cloud-key".into()),
            &keys,
        );
        let prefs = state.chat_prefs();
        assert_eq!(prefs.base_url.as_deref(), Some("http://localhost:1234/v1"));
        assert_eq!(prefs.model.as_deref(), Some("qwen2.5"));
        assert_eq!(prefs.api_key.as_deref(), Some("sk-a-cloud-key"));

        // Re-save with the key absent (the field the user didn't retype) and a blanked
        // model: the key survives, the model falls back to the shared default.
        set_chat_config_impl(
            &state,
            Some("http://localhost:1234/v1".into()),
            Some("  ".into()),
            None,
            &keys,
        );
        let prefs = state.chat_prefs();
        assert_eq!(prefs.api_key.as_deref(), Some("sk-a-cloud-key"));
        assert_eq!(prefs.model, None);
        assert_eq!(prefs.config().model, b2_llm::LlmConfig::from_env().model);
    }

    /// The #176 behavior at the command boundary: a saved key goes into the store, so
    /// the *next launch* — a fresh `AppState` reading from that same store — starts
    /// with it already in force.
    #[test]
    fn a_saved_key_is_there_at_the_next_launch() {
        let keys = MemoryStore::empty();
        set_chat_config_impl(
            &AppState::new(None),
            Some("https://api.example.com/v1".into()),
            None,
            Some("sk-remember-me".into()),
            &keys,
        );
        assert_eq!(keys.peek().as_deref(), Some("sk-remember-me"));

        let next_launch = crate::chat::read_prefs_from(None, &keys);
        assert_eq!(next_launch.api_key.as_deref(), Some("sk-remember-me"));
        assert_eq!(next_launch.api_key_source(), b2_llm::ApiKeySource::Stored);
    }

    /// The key's third state, and the reason it has one: **blank clears**. Without
    /// it a key set once could never be removed through Settings — and repointing
    /// the endpoint would then send the first provider's bearer token to the
    /// second, which is the privacy failure, not just the annoyance. Since #176 the
    /// removal has to reach the store too, or the key would simply return.
    #[test]
    fn a_blanked_key_clears_it_everywhere() {
        let state = AppState::new(None);
        let keys = MemoryStore::empty();
        set_chat_config_impl(&state, None, None, Some("sk-a-cloud-key".into()), &keys);
        assert_eq!(
            state.chat_prefs().api_key.as_deref(),
            Some("sk-a-cloud-key")
        );

        // Absent: untouched, so the key stands — in memory and in the store.
        set_chat_config_impl(&state, None, None, None, &keys);
        assert_eq!(
            state.chat_prefs().api_key.as_deref(),
            Some("sk-a-cloud-key")
        );
        assert_eq!(keys.peek().as_deref(), Some("sk-a-cloud-key"));

        // Blank (what the UI's Remove sends): gone from both. `config()` then resolves
        // the key from the environment alone — `B2_LLM_API_KEY` is the user's own
        // configuration, and Settings never had the standing to clear that.
        set_chat_config_impl(&state, None, None, Some("   ".into()), &keys);
        assert_eq!(state.chat_prefs().api_key, None);
        assert_eq!(
            keys.peek(),
            None,
            "a removed key must not return next launch"
        );
    }

    /// The status the setup card renders never carries the key — only where the key in
    /// force came from. (Pinned in `b2-llm` too, on the view type itself; this is the
    /// command boundary.)
    #[test]
    fn the_chat_setup_view_never_carries_the_key() {
        let state = AppState::new(None);
        set_chat_config_impl(
            &state,
            // `.invalid` is reserved (RFC 2606), so the probe fails immediately and this
            // never meets a model server a developer happens to be running.
            Some("http://b2-no-such-host.invalid:11434/v1".into()),
            Some("llama3.2".into()),
            Some("sk-live-must-not-cross".into()),
            &MemoryStore::empty(),
        );
        let setup = chat_setup_impl(&state);
        let json = serde_json::to_string(&setup).unwrap();
        assert!(!json.contains("sk-live-must-not-cross"), "{json}");
        // *That* a key is configured still crosses; which source answered depends on
        // whether the developer's own environment has one, so this asserts the part
        // that holds either way.
        assert!(json.contains("\"api_key_source\":"), "{json}");
        assert!(!json.contains("\"api_key_source\":\"none\""), "{json}");
    }

    /// A Keychain that refuses must not break the save. The key the user typed is in
    /// force for this run, nothing was stored, and the configuration says `session` so
    /// the Settings copy can be honest about how long that lasts (GH #176's fallback
    /// path — the convenience degrades, chat does not).
    #[test]
    fn a_refused_store_leaves_the_key_in_force_for_the_session() {
        let state = AppState::new(None);
        let keys = MemoryStore::refusing();
        set_chat_config_impl(
            &state,
            Some("https://api.example.com/v1".into()),
            None,
            Some("sk-typed-just-now".into()),
            &keys,
        );
        let prefs = state.chat_prefs();
        assert_eq!(prefs.api_key.as_deref(), Some("sk-typed-just-now"));
        assert_eq!(keys.peek(), None, "a refused write stores nothing");
        assert_eq!(prefs.api_key_source(), b2_llm::ApiKeySource::Session);
    }
}
