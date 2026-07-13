//! Native filesystem watch → **auto-reload on external edits** (desktop-ui-mvp.md §5,
//! Step 5, [#14](https://github.com/AlteredCraft/B2/issues/14)). B2's premise is that the
//! vault is *also* edited outside the app (Obsidian/vim, a `git pull`), so the window has
//! to notice "the files changed under me" and reconcile — replacing the editing spec's
//! "stale until you try to save" conflict bar (desktop-editing.md §5) with live
//! reconciliation. The conflict bar remains the fallback for the one case this can't cover
//! safely: an external edit to the note you are *actively typing in* (never clobber a live
//! buffer).
//!
//! **Still a dumb adapter (this crate's charter).** The watcher holds **no engine logic**:
//! it watches the vault directory, coalesces a burst of raw OS events into one **debounced
//! `vault-changed` pulse**, and emits it to the webview. The frontend reconciles by calling
//! the *existing* façade ops (`list_notes` for the tree, `read_note`/`similar`/`explain`
//! for the open note) — exactly the "*how the window drives* the façade stays here; *what*
//! it computes stays in the core" line. This is host **infrastructure**, the same class as
//! the background-reindex task lifecycle (main.rs) and the OS folder picker.
//!
//! **Security: the webview gets no filesystem permission.** The watch runs entirely in the
//! Rust host (like the dialog), and the pulse is a bare signal carrying no paths — so this
//! adds nothing to the webview's least-privilege capability set (capabilities/default.json;
//! listening for a host event is covered by `core:default`). The frontend never touches the
//! disk; it re-reads through the façade, which keeps `index = projection of (Markdown)`
//! honest and path/`b2id` resolution centralized.

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// The event name the host emits and the webview listens for. **Pinned** to the mirror
/// constant in `ui/src/api.ts` by `vault_changed_event_matches_the_frontend` — change both
/// together (the same "one contract, one string" discipline as the write-conflict message).
pub const VAULT_CHANGED_EVENT: &str = "vault-changed";

/// How long the debounce thread waits for the event stream to go quiet before emitting one
/// pulse. A single editor save or a `git pull` fires a *burst* of OS events; coalescing on
/// the trailing edge turns the burst into exactly one reconcile. Short enough to feel
/// immediate, long enough to swallow a burst.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Managed state holding the active watcher. Replacing or dropping it stops the previous
/// watch (its channel sender drops, so the debounce thread's `recv` ends and the thread
/// exits) — how a vault switch re-points the watch and app shutdown tears it down.
///
/// Kept **out of [`AppState`](crate::AppState)** on purpose: the watcher pulls in `notify`
/// and a live OS handle, while `AppState` is the pure, unit-tested root+reindex state
/// machine. Wiring the two together would drag a filesystem handle into those hermetic
/// tests for no benefit — so the watcher is its own Tauri-managed state, started from the
/// setup hook and the `choose_vault` command (both of which have an `AppHandle`).
#[derive(Default)]
pub struct VaultWatcher(Mutex<Option<RecommendedWatcher>>);

impl VaultWatcher {
    /// Point the watch at `root` (recursively), replacing any previous watch. Best-effort
    /// host state, exactly like remembering the last vault: a platform that can't watch
    /// (permissions, an unsupported backend) logs to stderr and runs on — auto-reload is a
    /// convenience over the always-present conflict-bar fallback, never a hard dependency,
    /// so it must not fail a launch or a vault switch.
    pub fn watch(&self, app: &AppHandle, root: &Path) {
        // Drop the old watcher first so only one watch is ever live (its sender drops →
        // its debounce thread ends).
        *self.lock() = None;
        match start(app.clone(), root) {
            Ok(watcher) => *self.lock() = Some(watcher),
            Err(e) => eprintln!("[b2] filesystem auto-reload unavailable for {root:?}: {e}"),
        }
    }

    /// Recover the inner value rather than panic if the lock is ever poisoned — the
    /// critical section is a single store/drop that can't panic, so poisoning is
    /// effectively impossible, but the no-`unwrap` rule holds regardless (main.rs mirrors
    /// this on its root mutex).
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<RecommendedWatcher>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Build a recursive watcher on `root` and spawn its debounce thread. The `notify` callback
/// runs on `notify`'s own thread and does the minimum — forward each raw event into a
/// channel; all coalescing happens in [`debounce_loop`], off that thread.
fn start(app: AppHandle, root: &Path) -> notify::Result<RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        // A send error means the debounce thread is gone (we're shutting this watch down);
        // nothing to do but drop the event.
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    // Canonicalize once so event paths (which the OS reports canonicalized — e.g.
    // `/private/var` for a `/var` root on macOS) strip cleanly against it. A root
    // that can't canonicalize falls back to itself; the filter fails open anyway.
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    std::thread::spawn(move || debounce_loop(rx, app, canonical_root));
    Ok(watcher)
}

/// Coalesce bursts of raw filesystem events into one `vault-changed` pulse per quiet period.
/// Blocks for the first event of a burst, then drains until [`DEBOUNCE`] passes with no new
/// event, emitting a single pulse iff the burst touched a **vault member** — any file the
/// walk would see (index churn under `.b2/`, `.git/` internals, and other dot-prefixed
/// paths are filtered out — see [`touches_vault`]). Ends when the watcher (and thus the
/// sending half) is dropped.
fn debounce_loop(rx: mpsc::Receiver<notify::Result<Event>>, app: AppHandle, root: PathBuf) {
    loop {
        // Block for the first event of a fresh burst; a receive error means the watcher was
        // dropped (vault switch / shutdown) — stop the thread.
        let mut relevant = match rx.recv() {
            Ok(ev) => event_touches_vault(&root, &ev),
            Err(_) => return,
        };
        // Drain the rest of the burst until the stream goes quiet for DEBOUNCE.
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

/// Emit one `vault-changed` pulse when the burst mattered. A send error (the window closed)
/// is not fatal — the app is going away anyway.
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

/// Whether a filesystem event touches a **vault member** — the watch mirror of the
/// walk's routing rule (b2-core `collect_vault_files`, file-type slice 1 spec §6):
/// any path with **no dot-prefixed component below the vault root** counts, so a
/// Finder-dropped PNG pulses like a note edit, while the self-inflicted noise stays
/// filtered — the index (`.b2/` sqlite, rewritten continuously by a save's trailing
/// embed), `.git/` internals on a pull, `.obsidian/` workspace churn.
///
/// The dot rule needs *vault-relative* components (a dot-dir **above** the root —
/// `~/.config/vaults/…` — must not mute everything), so each path is stripped
/// against the pre-canonicalized root. The former `.md`-allowlist avoided that
/// stripping; the price of covering resources is taking it on — mitigated by
/// canonicalizing the root once at watch start, and by **failing open**: a path
/// that won't strip (an unexpected mount/symlink shape) counts as relevant, costing
/// at most one extra debounced pulse (a cheap re-list), never a silently dead reload.
fn touches_vault(root: &Path, paths: &[PathBuf]) -> bool {
    paths.iter().any(|p| match p.strip_prefix(root) {
        Ok(rel) => !rel
            .components()
            .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.'))),
        Err(_) => true, // fail open — see above
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_member_paths_are_relevant_dot_prefixed_churn_is_not() {
        let root = Path::new("/v");
        // A note edit anywhere in the tree is relevant — and, since file-type slice 1,
        // so is any resource the walk would inventory (a Finder-dropped PNG must pulse).
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
        // …but the disposable sqlite index (the trailing-embed write storm) is not…
        assert!(!touches_vault(root, &[PathBuf::from("/v/.b2/b2.sqlite")]));
        assert!(!touches_vault(
            root,
            &[PathBuf::from("/v/.b2/b2.sqlite-wal")]
        ));
        assert!(!touches_vault(
            root,
            &[PathBuf::from("/v/.b2/b2.sqlite-shm")]
        ));
        // …nor `.git` internals from a pull, `.obsidian/` churn, or dotfiles.
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
        // A dot-dir ABOVE the root must not mute the vault it contains…
        let dotted_root = Path::new("/home/u/.config/vaults/v");
        assert!(touches_vault(
            dotted_root,
            &[PathBuf::from("/home/u/.config/vaults/v/notes/a.md")]
        ));
        assert!(!touches_vault(
            dotted_root,
            &[PathBuf::from("/home/u/.config/vaults/v/.b2/b2.sqlite")]
        ));
        // …and a path that doesn't strip against the root counts as relevant (fail
        // open): one extra debounced pulse beats a silently dead auto-reload.
        assert!(touches_vault(
            Path::new("/v"),
            &[PathBuf::from("/elsewhere/x.bin")]
        ));
    }

    #[test]
    fn a_burst_touching_any_vault_member_is_relevant() {
        // A rename fires create+remove; a pull touches many files at once. As long as one
        // path in the coalesced burst is a vault member, the burst earns a single pulse.
        let burst = [
            PathBuf::from("/v/.b2/b2.sqlite-wal"),
            PathBuf::from("/v/.git/ORIG_HEAD"),
            PathBuf::from("/v/concepts/memory.md"),
        ];
        assert!(touches_vault(Path::new("/v"), &burst));
    }

    #[test]
    fn vault_changed_event_matches_the_frontend() {
        // The webview listens for this exact string (ui/src/api.ts `VAULT_CHANGED_EVENT`).
        // Pin it here so a rename on either side can't silently break auto-reload — the
        // same cross-language "change them together" guard as the write-conflict message.
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
