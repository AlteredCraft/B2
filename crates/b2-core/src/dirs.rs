//! Folder ops. A folder, empty or not, is vault material like a note (data-model.md §1),
//! but is never indexed, so these read and write the filesystem directly.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::{Error, Result};

/// `b2 --json` / IPC view of a folder creation: the normalized vault-relative path.
#[derive(Debug, Clone, Serialize)]
pub struct DirCreateReport {
    pub dir: String,
}

/// Every folder under `vault_root`, sorted, skipping dot-prefixed ones as the ingest walk
/// does (GH #136). `is_dir()` follows symlinks as that walk does; change both together.
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
        if crate::pathspec::is_hidden(&path) {
            continue;
        }
        // Cannot fail under `root`; skip rather than panic.
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        out.push(rel.to_string_lossy().replace('\\', "/"));
        collect_dirs(root, &path, out)?;
    }
    Ok(())
}

/// Create the folder `dir_input` with missing parents. Unlike `mkdir -p`, an occupied
/// target is refused.
pub fn create_dir(vault_root: &Path, dir_input: &str) -> Result<DirCreateReport> {
    let dir = crate::pathspec::normalize_rel_dir(dir_input).map_err(Error::DirDestination)?;
    let abs = vault_root.join(&dir);
    if abs.exists() {
        return Err(Error::DirTargetExists(dir));
    }
    fs::create_dir_all(&abs)?;
    Ok(DirCreateReport { dir })
}
