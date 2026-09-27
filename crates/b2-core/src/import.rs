//! Import an outside file into the vault: a byte-honest copy, authoring nothing, which is
//! what makes it a permitted write (ADR-0004). Unlike [`crate::add`], which authors a note.
//!
//! Vault first: place the file, then project from disk. Model-free: imported chunks wait
//! for the next embed pass. [`import_bytes`] serves a drag-and-drop, [`import_path`] an OS
//! picker.

use crate::error::{Error, Result};
use crate::ingest::{self, ProjectionCtx};
use crate::resource::ResourceClass;
use serde::Serialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Where the imported file landed (L1/L3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub path: String,
    /// Routed as a note rather than a resource, so the editor can open it.
    pub note: bool,
}

/// Import `bytes` into folder `dir` (`""` for the root) as `file_name`. Refuses an invalid
/// destination and an occupied one.
pub fn import_bytes(
    ctx: ProjectionCtx,
    dir: &str,
    file_name: &str,
    bytes: &[u8],
) -> Result<ImportReport> {
    let (rel, abs) = destination_paths(ctx.root, dir, file_name)?;
    place(&rel, &abs, |file| file.write_all(bytes))?;
    project_placed(ctx, rel, &abs)
}

/// Import the file at `source` into folder `dir`, keeping its name, so the adapter holds
/// no logic. A source inside the vault just makes a copy.
pub fn import_path(ctx: ProjectionCtx, dir: &str, source: &Path) -> Result<ImportReport> {
    if source.is_dir() {
        return Err(Error::ImportDestination(format!(
            "{} is a folder; import files",
            source.display()
        )));
    }
    let Some(file_name) = source.file_name().and_then(|n| n.to_str()) else {
        return Err(Error::ImportDestination(format!(
            "{} has no file name",
            source.display()
        )));
    };
    let (rel, abs) = destination_paths(ctx.root, dir, file_name)?;
    // Not `fs::copy`, which would truncate an occupied destination.
    place(&rel, &abs, |file| {
        io::copy(&mut fs::File::open(source)?, file).map(|_| ())
    })?;
    project_placed(ctx, rel, &abs)
}

/// Resolve `(dir, file_name)` into a vault-relative destination. A separator in the name is
/// refused, so a name can't redirect the import. The extension is kept as given.
fn destination(dir: &str, file_name: &str) -> Result<String> {
    let name = file_name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(Error::ImportDestination(format!(
            "'{file_name}' is not a file name"
        )));
    }
    let dir = dir.trim().trim_end_matches('/');
    let joined = if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    };
    crate::pathspec::normalize_rel(&joined).map_err(Error::ImportDestination)
}

/// The validated destination, vault-relative and absolute.
fn destination_paths(vault_root: &Path, dir: &str, file_name: &str) -> Result<(String, PathBuf)> {
    let rel = destination(dir, file_name)?;
    let abs = vault_root.join(&rel);
    Ok((rel, abs))
}

/// [`place_new`] with [`Error::ImportTargetExists`]. The file gets new-file permissions,
/// not the source's mode.
fn place(rel: &str, abs: &Path, fill: impl FnOnce(&mut fs::File) -> io::Result<()>) -> Result<()> {
    place_new(abs, || Error::ImportTargetExists(rel.to_string()), fill)
}

/// Create a new file at `abs` (with parent folders) and fill it, or leave nothing behind.
/// Shared with `add`. `create_new` is the refusal itself, one syscall, so a file appearing
/// concurrently can't be overwritten; `AlreadyExists` maps to `taken`.
pub(crate) fn place_new(
    abs: &Path,
    taken: impl FnOnce() -> Error,
    fill: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<()> {
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = match fs::File::create_new(abs) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(taken()),
        Err(e) => return Err(e.into()),
    };
    if let Err(e) = fill(&mut file) {
        drop(file); // close before unlinking, for every platform
        let _ = fs::remove_file(abs);
        return Err(e.into());
    }
    Ok(())
}

/// Project the placed file, routed as the walk routes it. On failure, B2 removes its own
/// half-finished write (best-effort, never masking the projection error).
fn project_placed(ctx: ProjectionCtx, rel: String, abs: &Path) -> Result<ImportReport> {
    match project_from_disk(ctx, &rel) {
        Ok(note) => Ok(ImportReport { path: rel, note }),
        Err(e) => {
            let _ = fs::remove_file(abs);
            Err(e)
        }
    }
}

/// `true` for a note, `false` for a resource.
fn project_from_disk(ctx: ProjectionCtx, rel: &str) -> Result<bool> {
    match ResourceClass::of_path(rel) {
        None => {
            ingest::project_file(ctx, rel)?;
            Ok(true)
        }
        // `force`: an existing row may describe a file deleted out of band; hash what
        // was actually placed.
        Some(class) => {
            ingest::project_resource_file(ctx.conn, ctx.root, rel, class, true)?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_the_folder_and_keeps_the_extension() {
        assert_eq!(destination("", "notes.pdf").unwrap(), "notes.pdf");
        assert_eq!(destination("papers", "a.pdf").unwrap(), "papers/a.pdf");
        assert_eq!(destination("papers/", " a.md ").unwrap(), "papers/a.md");
    }

    #[test]
    fn a_file_name_is_a_name_never_a_path() {
        assert!(destination("papers", "../../etc/passwd").is_err());
        assert!(destination("papers", "sub/a.pdf").is_err());
        assert!(destination("papers", "sub\\a.pdf").is_err());
        assert!(destination("papers", "   ").is_err());
    }

    #[test]
    fn the_shared_path_rules_still_apply_to_the_pair() {
        assert!(destination("..", "a.pdf").is_err()); // escaping folder
        assert!(destination("/abs", "a.pdf").is_err()); // absolute folder
        assert!(destination("papers", ".hidden.pdf").is_err()); // never indexed
        assert!(destination(".b2", "a.pdf").is_err());
    }
}
