//! The host's per-machine state files — the last opened vault, the chat settings, the
//! embed-time ledger. All live under `<data-dir>/b2/` (macOS:
//! `~/Library/Application Support/b2/`, Linux: `~/.local/share/b2/`), the same vendor dir
//! `b2-embed` keeps its model cache in, and all are **best-effort**: remembering something
//! must never fail the action the user just took. Never vault or index state.

use std::path::{Path, PathBuf};

/// `<data-dir>/b2/<name>`, or `None` when the platform has no data dir — in which case
/// the caller simply doesn't remember.
pub fn path(name: &str) -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("b2").join(name))
}

/// Write `contents` to `file`, creating its parent dir first — the testable core every
/// state file's writer shares.
pub fn write(file: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, contents)
}

/// Run `update` against the state file `name`, best-effort: no data dir, or a failed
/// write, is logged to stderr (`could not {what}: …`) and swallowed.
pub fn update(name: &str, what: &str, update: impl FnOnce(&Path) -> std::io::Result<()>) {
    let Some(file) = path(name) else {
        eprintln!("[b2] could not {what}: no platform data directory");
        return;
    };
    if let Err(e) = update(&file) {
        eprintln!("[b2] could not {what}: {e}");
    }
}
