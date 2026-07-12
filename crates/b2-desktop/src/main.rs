//! `b2-desktop` — the Tauri host, B2's **second dumb adapter** over the
//! [`Vault`](b2_core::vault::Vault) façade (the GUI sibling of `b2-cli`). It holds
//! **no engine logic**: each `#[tauri::command]` deserializes its args, calls one
//! façade method, and serializes the result (specs/completed/desktop-ui-mvp.md §3). The rules
//! that keep it a *dumb* adapter live in this crate's charter, `CLAUDE.md`.
//!
//! Two things this file owns, both mirroring the CLI:
//!   * **Vault root resolution** — an explicit launch arg (the CLI's positional), else
//!     the **last vault the user opened** (persisted across launches, see
//!     [`read_last_vault`]), else `$B2_VAULT_PATH` (the CLI's `-C` / env). Seeded once
//!     at startup into [`AppState`] and thereafter **swappable at runtime** by the
//!     in-app vault picker (`choose_vault`), which also **remembers** the pick so the
//!     next launch reopens it. Every command opens a *fresh* vault from the current
//!     root, exactly as the one-process-per-command CLI does. (The remembered choice is
//!     desktop-only state — a long-lived window's "reopen what I had"; the stateless CLI
//!     has no equivalent, so this is a legitimate host responsibility, not engine logic.)
//!   * **Embedder wiring** — pure reads open with the deterministic fake; anything
//!     that embeds a query or writes vectors (`search` / `link` / `embed`) opens the
//!     real [`LocalEmbedder`] and **fails fast** with "run `b2 init`" if it's absent.
//!     `project` — the model-free half of a reindex (projection-embedding-split.md
//!     §6) — opens the fake, so the first tree paint never waits on a model load.
//!     `B2_EMBEDDER=fake` forces the fake everywhere (offline/dev mode).

// This binary is desktop-only (no mobile entry point), so a plain `main` suffices.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;
mod logging;
mod watch;

use b2_core::embed::Embedder;
use b2_core::vault::Vault;
use b2_embed::{EmbedConfig, LocalEmbedder};
use error::CmdError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Manager;
use watch::VaultWatcher;

/// How often [`AppState::cancel_and_wait_for_reindex`] re-asserts the cancel flag and
/// re-checks whether the in-flight reindex has wound down. Short enough to feel
/// instant on a vault switch, long enough not to spin hot.
const CANCEL_POLL: Duration = Duration::from_millis(25);

/// The host's shared state: the active vault root plus the background-reindex control
/// bits (async-indexing.md §4). Resolved once at startup, then **swappable at runtime**
/// by the in-app vault picker (`choose_vault`) — so the root sits behind a [`Mutex`].
/// Every command still opens its own short-lived [`Vault`] over the *current* root
/// (SQLite WAL permits concurrent readers + one writer), the faithful mirror of the CLI
/// opening a fresh vault per invocation. `None` means no vault is configured; commands
/// then return an actionable [`CmdError::VaultRequired`].
///
/// The reindex bits are host **infrastructure**, not engine logic: *how the window
/// drives and interrupts* the one façade op stays here; *what* to embed stays in the
/// core (the charter's line). `reindex_running` is a single-in-flight guard for the
/// long, vector-writing **embed** pass (the fast, model-free `project` command runs
/// outside it by design — projection-embedding-split.md §6); a running embed checks
/// `reindex_cancel` at each batch boundary (via the closure it passes to
/// `Vault::embed`) and stops cooperatively when it is set.
pub struct AppState {
    root: Mutex<Option<PathBuf>>,
    /// Set by `cancel_reindex` (and a vault switch); the running reindex closure
    /// observes it at each batch boundary and returns `ControlFlow::Break`.
    reindex_cancel: AtomicBool,
    /// `true` while a reindex is in flight — the single-in-flight guard (a second
    /// `reindex` is refused; see [`AppState::try_start_reindex`]).
    reindex_running: AtomicBool,
}

impl AppState {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root: Mutex::new(root),
            reindex_cancel: AtomicBool::new(false),
            reindex_running: AtomicBool::new(false),
        }
    }

    /// The current vault root, cloned out so the lock is **not** held while a command
    /// opens a vault (which may load the model — slow). `None` when unconfigured.
    pub fn current_root(&self) -> Option<PathBuf> {
        self.lock_root().clone()
    }

    /// Point the app at a new vault root (the vault switcher). Takes effect for every
    /// subsequent command, since each opens a fresh vault over `current_root`.
    pub fn set_root(&self, root: &Path) {
        *self.lock_root() = Some(root.to_path_buf());
    }

    /// Claim the single reindex slot. Returns `true` if this call won the slot (the
    /// caller must release it via [`finish_reindex`](Self::finish_reindex)), `false`
    /// if a reindex is already in flight — the belt-and-suspenders half of the
    /// single-in-flight guard (the UI also disables the button).
    pub fn try_start_reindex(&self) -> bool {
        self.reindex_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Release the reindex slot (always, even on error — see the RAII guard in
    /// `commands.rs`). Idempotent.
    pub fn finish_reindex(&self) {
        self.reindex_running.store(false, Ordering::SeqCst);
    }

    /// Whether a reindex is currently in flight.
    pub fn reindex_in_flight(&self) -> bool {
        self.reindex_running.load(Ordering::SeqCst)
    }

    /// Clear the cancel flag — called once a fresh reindex has claimed the slot, so a
    /// stale cancel from a previous run (or a prior vault switch) can't kill it.
    pub fn arm_reindex(&self) {
        self.reindex_cancel.store(false, Ordering::SeqCst);
    }

    /// Whether the running reindex has been asked to stop (checked at each batch).
    pub fn reindex_cancelled(&self) -> bool {
        self.reindex_cancel.load(Ordering::SeqCst)
    }

    /// Signal the running reindex to stop at its next batch boundary (the
    /// `cancel_reindex` command). Cooperative — never a thread kill, so no torn writes
    /// (async-indexing.md §5.6). A no-op if nothing is running.
    pub fn request_reindex_cancel(&self) {
        self.reindex_cancel.store(true, Ordering::SeqCst);
    }

    /// Cancel any in-flight reindex and **block until it winds down** — used before a
    /// vault switch so a reindex can never keep writing the vault the app has left
    /// (async-indexing.md §4/§5.4). Re-asserts the cancel flag on every poll so it wins
    /// even against a reindex that armed (cleared) it a moment after starting; returns
    /// immediately when nothing is running.
    pub fn cancel_and_wait_for_reindex(&self) {
        loop {
            self.request_reindex_cancel();
            if !self.reindex_in_flight() {
                return;
            }
            std::thread::sleep(CANCEL_POLL);
        }
    }

    /// The critical sections here are a single clone or store — neither can panic —
    /// so the lock can never be poisoned; recover the inner value rather than unwrap
    /// (the no-panic rule) if a poison ever somehow occurs.
    fn lock_root(&self) -> std::sync::MutexGuard<'_, Option<PathBuf>> {
        self.root
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Whether the deterministic fake embedder is forced (`B2_EMBEDDER=fake`) — the CLI's
/// offline/dev switch, honored identically so the two adapters behave the same.
fn use_fake_embedder() -> bool {
    matches!(std::env::var("B2_EMBEDDER").ok().as_deref(), Some("fake"))
}

/// Open a fresh vault over the configured root with the right embedder — the desktop
/// mirror of the CLI's `open_vault`. `needs_semantic` commands (`search`/`link`/
/// `embed`) load the real [`LocalEmbedder`] (fail-fast "run `b2 init`" if absent);
/// pure reads — and `project`, which never touches vectors — use the fake. Returns
/// the vault and whether its embedder is semantic (used only for honest UI). Errors
/// with [`CmdError::VaultRequired`] if no vault is set.
pub fn open_vault(state: &AppState, needs_semantic: bool) -> Result<(Vault, bool), CmdError> {
    let root = state.current_root().ok_or(CmdError::VaultRequired)?;
    if needs_semantic && !use_fake_embedder() {
        let config = EmbedConfig::load()?;
        let embedder = LocalEmbedder::load(&config)?;
        let vault = Vault::open_with_embedder(&root, Box::new(embedder) as Box<dyn Embedder>)?;
        Ok((vault, true))
    } else {
        Ok((Vault::open(&root)?, false))
    }
}

/// Whether the real (semantic) embedder is available right now — mirrors the CLI:
/// false under `B2_EMBEDDER=fake`, or if the model isn't provisioned yet. Used by
/// `vault_info` to tell the UI whether semantic ranking is live, so the app can be
/// honest (never overstate the fake), exactly as `b2 search` is.
pub fn semantic_available() -> bool {
    if use_fake_embedder() {
        return false;
    }
    match EmbedConfig::load() {
        Ok(config) => LocalEmbedder::load(&config).is_ok(),
        Err(_) => false,
    }
}

/// Resolve the vault root once at startup. Precedence, most-explicit first:
///   1. an explicit **launch argument** (the CLI's positional) — a per-launch override;
///   2. the **last vault the user opened** via the picker, remembered across launches
///      ([`read_last_vault`]) — so the app reopens what was open when it was last closed;
///   3. `$B2_VAULT_PATH` (the CLI's `-C` / env) — the first-run / never-picked default.
///
/// The remembered choice deliberately beats `$B2_VAULT_PATH`: on a GUI the picker is the
/// primary way you choose a vault, and "remember my choice" is the expected behavior; the
/// env var seeds the *first* run, and a launch arg remains the escape hatch to force a
/// specific vault for one session without disturbing the remembered pick. A leading-`-`
/// first arg is ignored so a macOS `-psn_…` Finder argument is never mistaken for a path.
fn resolve_root() -> Option<PathBuf> {
    let arg = std::env::args()
        .nth(1)
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from);
    let env = std::env::var("B2_VAULT_PATH").ok().map(PathBuf::from);
    pick_root(arg, read_last_vault(), env)
}

/// The pure precedence rule behind [`resolve_root`], split out so it is unit-testable
/// without touching process args, the environment, or the filesystem: launch arg, then
/// the remembered pick, then the env fallback.
fn pick_root(
    arg: Option<PathBuf>,
    remembered: Option<PathBuf>,
    env: Option<PathBuf>,
) -> Option<PathBuf> {
    arg.or(remembered).or(env)
}

/// Path of the "last opened vault" state file: `<data-dir>/b2/last-vault` (macOS:
/// `~/Library/Application Support/b2/…`, Linux: `~/.local/share/b2/…`) — the same
/// `dirs`-resolved `b2/` vendor dir b2-embed uses for its model cache. `None` only if the
/// platform has no data dir, in which case remembering is silently skipped.
fn last_vault_file() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("b2").join("last-vault"))
}

/// The remembered vault root from the last picker choice, or `None` if there is none,
/// the file is unreadable/empty, **or the remembered directory no longer exists** (moved
/// or deleted). Falling through on a stale entry lets startup drop back to the env
/// default rather than opening a vault whose every command would then error.
fn read_last_vault() -> Option<PathBuf> {
    read_last_vault_from(&last_vault_file()?)
}

/// [`read_last_vault`] against an explicit file path — the testable core (a tempfile
/// stands in for the real state file). Strips only trailing newline(s), so a path is
/// preserved verbatim, and requires the target to be an existing directory.
fn read_last_vault_from(file: &Path) -> Option<PathBuf> {
    let contents = std::fs::read_to_string(file).ok()?;
    let path = PathBuf::from(contents.trim_end_matches(['\n', '\r']));
    path.is_dir().then_some(path)
}

/// Remember `root` as the last opened vault so the next launch reopens it. **Best-effort
/// host state**: a write failure (no data dir, unwritable disk) is logged to stderr and
/// swallowed — remembering must never fail the vault switch the user just made. Called
/// only from the `choose_vault` command wrapper (an explicit user pick), never from the
/// unit-tested state transition, so tests don't touch the real data dir.
fn persist_last_vault(root: &Path) {
    let Some(file) = last_vault_file() else {
        eprintln!("[b2] could not remember last vault: no platform data directory");
        return;
    };
    if let Err(e) = persist_last_vault_to(&file, root) {
        eprintln!("[b2] could not remember last vault: {e}");
    }
}

/// [`persist_last_vault`] against an explicit file path — the testable core. Creates the
/// parent dir if needed and writes the path as the file's sole line.
fn persist_last_vault_to(file: &Path, root: &Path) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, root.to_string_lossy().as_bytes())
}

fn main() {
    // Opt-in structured logging (B2_LOG/B2_DEBUG/B2_LOG_FILE), the GUI mirror of the
    // CLI. Bind the guard for the whole run: it owns the non-blocking writer thread's
    // flush-on-drop, and `.run()` below blocks until the app exits, so `_guard` lives
    // exactly as long as the app does. `None` (no logging requested) is a plain no-op.
    let _guard = logging::init_logging();
    let state = AppState::new(resolve_root());
    tauri::Builder::default()
        // The dialog plugin backs the native folder picker for `choose_vault`. It is
        // driven host-side only; the webview gets no dialog permission (capabilities/
        // default.json), so it can never open a dialog itself.
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        // Filesystem auto-reload (#14 / desktop-ui-mvp §5): its own managed state so the
        // pure `AppState` machine stays free of an OS watch handle. Started below once the
        // app handle exists, and re-pointed on a vault switch (`choose_vault`).
        .manage(VaultWatcher::default())
        .setup(|app| {
            // Watch the startup vault, if one resolved. Best-effort: `watch` swallows and
            // logs a failure, so a platform without a watch backend still launches (the
            // conflict bar remains the fallback). No vault configured → nothing to watch;
            // the first `choose_vault` starts it.
            if let Some(root) = app.state::<AppState>().current_root() {
                app.state::<VaultWatcher>().watch(app.handle(), &root);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::vault_info,
            commands::choose_vault,
            commands::read_note,
            commands::list_notes,
            commands::write_note,
            commands::similar,
            commands::search,
            commands::neighbors,
            commands::explain,
            commands::link,
            commands::project,
            commands::embed,
            commands::cancel_reindex,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the B2 desktop app");
}

#[cfg(test)]
mod tests {
    //! Host-side vault-resolution tests: the pure precedence rule and the "last opened
    //! vault" state file's persist/read round-trip. All hermetic — `pick_root` touches
    //! nothing global, and the file helpers run against a tempdir, never the real data
    //! dir (which only the `choose_vault` wrapper writes, in production).

    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> Option<PathBuf> {
        Some(PathBuf::from(s))
    }

    #[test]
    fn pick_root_precedence_arg_then_remembered_then_env() {
        // (arg, remembered, env) → the chosen root. Arg wins outright; the remembered
        // pick beats the env fallback; env is used only when nothing more explicit is set.
        let cases = [
            (p("/arg"), p("/mem"), p("/env"), p("/arg")),
            (None, p("/mem"), p("/env"), p("/mem")),
            (None, None, p("/env"), p("/env")),
            (p("/arg"), None, None, p("/arg")),
            (p("/arg"), None, p("/env"), p("/arg")),
            (None, p("/mem"), None, p("/mem")),
            (None, None, None, None),
        ];
        for (arg, remembered, env, want) in cases {
            assert_eq!(
                pick_root(arg.clone(), remembered.clone(), env.clone()),
                want,
                "pick_root({arg:?}, {remembered:?}, {env:?})"
            );
        }
    }

    #[test]
    fn persist_then_read_round_trips_an_existing_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join("my vault"); // a space proves we don't trim it
        std::fs::create_dir_all(&vault).unwrap();
        // The state file sits under a not-yet-created subdir — persist must `mkdir -p`.
        let file = tmp.path().join("state/b2/last-vault");

        persist_last_vault_to(&file, &vault).unwrap();
        assert_eq!(read_last_vault_from(&file), Some(vault));
    }

    #[test]
    fn read_last_vault_ignores_a_stale_or_missing_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("last-vault");
        // A remembered vault that has since been deleted must not be reopened.
        std::fs::write(&file, tmp.path().join("gone").to_string_lossy().as_bytes()).unwrap();
        assert_eq!(read_last_vault_from(&file), None);
    }

    #[test]
    fn read_last_vault_ignores_missing_or_empty_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        // No file at all.
        assert_eq!(read_last_vault_from(&tmp.path().join("absent")), None);
        // An empty file (whitespace only) is not a path.
        let empty = tmp.path().join("empty");
        std::fs::write(&empty, "\n").unwrap();
        assert_eq!(read_last_vault_from(&empty), None);
    }

    #[test]
    fn read_last_vault_strips_only_trailing_newlines() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let file = tmp.path().join("last-vault");
        // A trailing newline (as `persist` never writes, but a hand-edit might) is fine.
        std::fs::write(&file, format!("{}\n", vault.display())).unwrap();
        assert_eq!(read_last_vault_from(&file), Some(vault));
    }
}
