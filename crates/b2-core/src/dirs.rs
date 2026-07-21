//! Folder-structure ops — the structure half of "the vault directory is the source
//! of truth": Markdown files carry content, the directory tree carries **structure**
//! (data-model.md §1), and a folder — empty or not — is user-authored vault material
//! exactly like a note. Folders are never projected into the index (nothing to
//! chunk, embed, or link), so both ops here read/write the filesystem directly:
//! the walk *is* the projection, and a listing can never go stale against disk.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::{Error, Result};

/// `b2 --json` / IPC view of a folder creation: the normalized vault-relative path.
#[derive(Debug, Clone, Serialize)]
pub struct DirCreateReport {
    pub dir: String,
}

/// Every folder under `vault_root` (empty ones included), vault-relative with `/`
/// separators and no trailing slash, sorted. Dot-prefixed directories (`.b2/`,
/// `.git/`, `.obsidian/`) are skipped — the same routing rule as the ingest walk
/// (`collect_vault_files`), so the tree and the index agree on what a vault
/// member is.
pub fn list_dirs(vault_root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    collect_dirs(vault_root, vault_root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_dirs(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let is_dotdir = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if is_dotdir {
            continue;
        }
        // `path` was produced by walking `root`, so `strip_prefix` cannot fail;
        // handle it gracefully anyway rather than panic on the invariant.
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        out.push(rel.to_string_lossy().replace('\\', "/"));
        collect_dirs(root, &path, out)?;
    }
    Ok(())
}

/// Create the folder `dir_input` (vault-relative; a trailing `/` is tolerated),
/// missing parents included — `mkdir -p`, matching `create_note`'s parent
/// creation and the UI's nested-name input. Errors with
/// [`Error::DirDestination`] for an invalid path and [`Error::DirTargetExists`]
/// when anything (file or folder) already sits there.
pub fn create_dir(vault_root: &Path, dir_input: &str) -> Result<DirCreateReport> {
    let dir = crate::pathspec::normalize_rel_dir(dir_input).map_err(Error::DirDestination)?;
    let abs = vault_root.join(&dir);
    if abs.exists() {
        return Err(Error::DirTargetExists(dir));
    }
    fs::create_dir_all(&abs)?;
    Ok(DirCreateReport { dir })
}
