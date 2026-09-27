//! Filesystem watch for auto-reload on external edits (#14). Coalesces a burst of OS events
//! into one debounced `vault-changed` event; the frontend reconciles through existing façade
//! ops (ADR-0012). The conflict bar still covers an external edit to the note being typed in.
//!
//! The event carries no paths, so the webview still needs no filesystem permission.

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// The event the webview listens for; must match `ui/src/api.ts` (pinned by a test).
pub const VAULT_CHANGED_EVENT: &str = "vault-changed";

/// Quiet period before one event is emitted for a burst (a save or a `git pull`).
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Managed state holding the active watcher; replacing or dropping it stops the previous
/// watch. Kept out of [`AppState`](crate::AppState) so its tests stay free of OS handles.
#[derive(Default)]
pub struct VaultWatcher(Mutex<Option<RecommendedWatcher>>);

impl VaultWatcher {
    /// Point the watch at `root`, replacing any previous watch. Best-effort: a failure logs
    /// to stderr and never fails a launch or vault switch.
    pub fn watch(&self, app: &AppHandle, root: &Path) {
        // Drop the old watcher first so only one watch is ever live.
        *self.lock() = None;
        match start(app.clone(), root) {
            Ok(watcher) => *self.lock() = Some(watcher),
            Err(e) => eprintln!("[b2] filesystem auto-reload unavailable for {root:?}: {e}"),
        }
    }

    /// See [`lock_recover`](crate::lock_recover).
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<RecommendedWatcher>> {
        crate::lock_recover(&self.0)
    }
}

/// Build a recursive watcher on `root` and spawn its debounce thread.
fn start(app: AppHandle, root: &Path) -> notify::Result<RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    // Event paths arrive canonicalized (macOS `/private/var` for `/var`), so strip against
    // a canonical root.
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    std::thread::spawn(move || debounce_loop(rx, app, canonical_root));
    Ok(watcher)
}

/// Emit one `vault-changed` per burst that touched a vault member ([`touches_vault`]).
/// Ends when the watcher is dropped.
fn debounce_loop(rx: mpsc::Receiver<notify::Result<Event>>, app: AppHandle, root: PathBuf) {
    loop {
        let mut relevant = match rx.recv() {
            Ok(ev) => event_touches_vault(&root, &ev),
            Err(_) => return,
        };
        loop {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(ev) => relevant |= event_touches_vault(&root, &ev),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    emit_if(&app, relevant);
                    return;
                }
            }
        }
        emit_if(&app, relevant);
    }
}

fn emit_if(app: &AppHandle, relevant: bool) {
    if relevant {
        let _ = app.emit(VAULT_CHANGED_EVENT, ());
    }
}

fn event_touches_vault(root: &Path, ev: &notify::Result<Event>) -> bool {
    match ev {
        Ok(e) => touches_vault(root, &e.paths),
        Err(_) => false,
    }
}

/// Whether an event touches a vault member, mirroring the walk's rule (b2-core
/// `collect_vault_files`): no dot-prefixed component below the root. Filters out `.b2/`,
/// `.git/` and `.obsidian/` churn.
///
/// Components are vault-relative, so a dot-dir above the root doesn't mute the vault. A path
/// that won't strip fails open: one extra reload beats a silently dead one.
fn touches_vault(root: &Path, paths: &[PathBuf]) -> bool {
    paths.iter().any(|p| match p.strip_prefix(root) {
        Ok(rel) => !rel
            .components()
            .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.'))),
        Err(_) => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_member_paths_are_relevant_dot_prefixed_churn_is_not() {
        let root = Path::new("/v");
        assert!(touches_vault(
            root,
            &[PathBuf::from("/v/notes/spaced-repetition.md")]
        ));
        assert!(touches_vault(root, &[PathBuf::from("/v/memory.MD")]));
        assert!(touches_vault(
            root,
            &[PathBuf::from("/v/assets/diagram.png")]
        ));
        assert!(touches_vault(root, &[PathBuf::from("/v/no-extension")]));
        assert!(!touches_vault(root, &[PathBuf::from("/v/.b2/b2.sqlite")]));
        assert!(!touches_vault(
            root,
            &[PathBuf::from("/v/.b2/b2.sqlite-wal")]
        ));
        assert!(!touches_vault(
            root,
            &[PathBuf::from("/v/.b2/b2.sqlite-shm")]
        ));
        assert!(!touches_vault(root, &[PathBuf::from("/v/.git/index")]));
        assert!(!touches_vault(
            root,
            &[PathBuf::from("/v/.obsidian/workspace.json")]
        ));
        assert!(!touches_vault(root, &[PathBuf::from("/v/notes/.DS_Store")]));
        assert!(!touches_vault(root, &[]));
    }

    #[test]
    fn the_dot_rule_is_vault_relative_and_fails_open() {
        let dotted_root = Path::new("/home/u/.config/vaults/v");
        assert!(touches_vault(
            dotted_root,
            &[PathBuf::from("/home/u/.config/vaults/v/notes/a.md")]
        ));
        assert!(!touches_vault(
            dotted_root,
            &[PathBuf::from("/home/u/.config/vaults/v/.b2/b2.sqlite")]
        ));
        // A path outside the root fails open.
        assert!(touches_vault(
            Path::new("/v"),
            &[PathBuf::from("/elsewhere/x.bin")]
        ));
    }

    #[test]
    fn a_burst_touching_any_vault_member_is_relevant() {
        let burst = [
            PathBuf::from("/v/.b2/b2.sqlite-wal"),
            PathBuf::from("/v/.git/ORIG_HEAD"),
            PathBuf::from("/v/concepts/memory.md"),
        ];
        assert!(touches_vault(Path::new("/v"), &burst));
    }

    #[test]
    fn vault_changed_event_matches_the_frontend() {
        let api_ts = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/api.ts"),
        )
        .expect("ui/src/api.ts is part of this repo — the IPC seam this event crosses");
        assert!(
            api_ts.contains(&format!("\"{VAULT_CHANGED_EVENT}\"")),
            "ui/src/api.ts VAULT_CHANGED_EVENT must equal the host's `{VAULT_CHANGED_EVENT}`"
        );
    }
}
