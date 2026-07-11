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
    std::thread::spawn(move || debounce_loop(rx, app));
    Ok(watcher)
}

/// Coalesce bursts of raw filesystem events into one `vault-changed` pulse per quiet period.
/// Blocks for the first event of a burst, then drains until [`DEBOUNCE`] passes with no new
/// event, emitting a single pulse iff the burst touched a Markdown note (index churn under
/// `.b2/` is sqlite, never `.md`, so it's filtered out — see [`touches_markdown`]). Ends when
/// the watcher (and thus the sending half) is dropped.
fn debounce_loop(rx: mpsc::Receiver<notify::Result<Event>>, app: AppHandle) {
    loop {
        // Block for the first event of a fresh burst; a receive error means the watcher was
        // dropped (vault switch / shutdown) — stop the thread.
        let mut relevant = match rx.recv() {
            Ok(ev) => event_touches_markdown(&ev),
            Err(_) => return,
        };
        // Drain the rest of the burst until the stream goes quiet for DEBOUNCE.
        loop {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(ev) => relevant |= event_touches_markdown(&ev),
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

fn event_touches_markdown(ev: &notify::Result<Event>) -> bool {
    match ev {
        Ok(e) => touches_markdown(&e.paths),
        Err(_) => false,
    }
}

/// Whether a filesystem event touches a Markdown note — the one filter that keeps the watch
/// from firing on its own index writes. The disposable index lives entirely under `<vault>/
/// .b2/` and is **only** sqlite files (`b2.sqlite`, `-wal`, `-shm`); a save's trailing embed
/// rewrites them continuously, so reacting to them would be a self-inflicted pulse storm.
/// Filtering on the `.md` extension alone excludes every one of them — and `.git/` internals
/// on a `git pull`, and image/attachment writes — while still catching the note adds,
/// removes, renames, and body edits that actually change what B2 projects. Cheaper and more
/// robust than stripping the vault root off each path (no canonicalization pitfalls), because
/// nothing B2 writes to the index ever carries a `.md` extension.
fn touches_markdown(paths: &[PathBuf]) -> bool {
    paths.iter().any(|p| {
        p.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_paths_are_relevant_index_and_dotdir_churn_is_not() {
        // A note edit anywhere in the tree is relevant (extension match is case-insensitive)…
        assert!(touches_markdown(&[PathBuf::from(
            "/v/notes/spaced-repetition.md"
        )]));
        assert!(touches_markdown(&[PathBuf::from("/v/memory.MD")]));
        // …but the disposable sqlite index (the trailing-embed write storm) is not…
        assert!(!touches_markdown(&[PathBuf::from("/v/.b2/b2.sqlite")]));
        assert!(!touches_markdown(&[PathBuf::from("/v/.b2/b2.sqlite-wal")]));
        assert!(!touches_markdown(&[PathBuf::from("/v/.b2/b2.sqlite-shm")]));
        // …nor `.git` internals from a pull, nor attachments.
        assert!(!touches_markdown(&[PathBuf::from("/v/.git/index")]));
        assert!(!touches_markdown(&[PathBuf::from("/v/assets/diagram.png")]));
        assert!(!touches_markdown(&[PathBuf::from("/v/no-extension")]));
        assert!(!touches_markdown(&[]));
    }

    #[test]
    fn a_burst_touching_any_markdown_file_is_relevant() {
        // A rename fires create+remove; a pull touches many files at once. As long as one
        // path in the coalesced burst is a note, the burst earns a single pulse.
        let burst = [
            PathBuf::from("/v/.b2/b2.sqlite-wal"),
            PathBuf::from("/v/.git/ORIG_HEAD"),
            PathBuf::from("/v/concepts/memory.md"),
        ];
        assert!(touches_markdown(&burst));
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
