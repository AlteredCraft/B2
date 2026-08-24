//! Bring an *outside* file into the vault — CRUD's **import** arm, and the kernel
//! behind the desktop's drag-from-Finder-onto-the-tree gesture.
//!
//! What separates this from [`crate::add`]: `add` **authors** a new document, while an
//! import is a **byte-honest copy** of a file the human already has. B2 writes the bytes
//! it was handed and authors nothing, so a dropped `.md` keeps its own frontmatter
//! verbatim and a dropped PDF keeps its bytes. That is what makes it a permitted write
//! (ADR-0004): placing a file is the same category of act as moving or deleting one.
//!
//! **Vault first**, the order `add`/`mv`/`link` also write in: place the file, then
//! project *from disk*, so the index is derived from what actually landed. **Model-free**
//! (a [`ProjectionCtx`], like `create_note`): an imported note's chunks join the pending
//! set for the next embed pass, so importing works with no model provisioned.
//!
//! Two entry points because the two gestures arrive differently: a file dropped on the
//! webview is **bytes**, a file chosen in an OS picker is a **path**.

use crate::error::{Error, Result};
use crate::ingest::{self, ProjectionCtx};
use crate::resource::ResourceClass;
use serde::Serialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// What an import did: where the file landed — which, for a note and a resource
/// alike, is the whole of what arrived (invariants L1/L3: both are path-keyed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub path: String,
    /// Whether the arriving file was routed as a **note** (a `.md`, projected with
    /// chunks/FTS/edges) rather than a resource (one inventory row). The adapters
    /// use it to decide whether the drop is openable in the editor.
    pub note: bool,
}

/// Import `bytes` into the vault folder `dir` (`""` for the root) under `file_name` —
/// the drag-and-drop arm, where the OS hands the page content rather than a path.
/// Refuses [`Error::ImportDestination`] for a name/folder pair that isn't a valid
/// vault-relative path and [`Error::ImportTargetExists`] rather than clobber.
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

/// Import the file at `source` into the vault folder `dir`, keeping its name — the
/// OS-picker arm, and the reason it exists rather than `import_bytes(fs::read(source)?)`
/// at the call site: the adapter then holds no logic and the bytes never round-trip
/// through it. Same refusals as [`import_bytes`], plus [`Error::ImportDestination`] for a
/// source that is a folder or has no file name. A source *inside* the vault is a
/// duplicate, not an error — a copy landing at a free path is simply a second note.
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
    // Streamed rather than `fs::copy`, so the destination is reserved by the same
    // create-new open every import goes through — `fs::copy` would truncate whatever
    // is there.
    place(&rel, &abs, |file| {
        io::copy(&mut fs::File::open(source)?, file).map(|_| ())
    })?;
    project_placed(ctx, rel, &abs)
}

/// Resolve `(dir, file_name)` into the vault-relative destination, refusing anything
/// that isn't one: a `file_name` carrying a path separator (a *name* free to carry `../`
/// would silently redirect the import), and via [`crate::pathspec::normalize_rel`] an
/// empty, absolute, vault-escaping or dot-prefixed result. The extension is left exactly
/// as given — it decides note vs resource, and B2 does not rename the human's file.
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

/// The validated destination as both halves the rest of the op needs: the
/// vault-relative path (what the index and the report speak in) and its absolute twin
/// (what the filesystem does).
fn destination_paths(vault_root: &Path, dir: &str, file_name: &str) -> Result<(String, PathBuf)> {
    let rel = destination(dir, file_name)?;
    let abs = vault_root.join(&rel);
    Ok((rel, abs))
}

/// Reserve the destination and fill it — or leave nothing behind.
///
/// **`create_new` is the refusal**, not a check before one: "does it exist" and "claim
/// it" are a single syscall, so a file appearing in between — another window, a sync
/// client, the CLI — cannot be overwritten. That is also why `import_path` streams
/// instead of calling `fs::copy`, which would truncate an occupied destination. An
/// [`io::ErrorKind::AlreadyExists`] *is* [`Error::ImportTargetExists`], so the race and
/// the ordinary "that name is taken" reach the user as one message.
///
/// A `fill` that fails partway takes the reserved file with it: half a file is not an
/// import. The destination is a file B2 created, so it carries ordinary new-file
/// permissions rather than the source's mode — byte-honesty is about content.
fn place(rel: &str, abs: &Path, fill: impl FnOnce(&mut fs::File) -> io::Result<()>) -> Result<()> {
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = match fs::File::create_new(abs) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(Error::ImportTargetExists(rel.to_string()))
        }
        Err(e) => return Err(e.into()),
    };
    if let Err(e) = fill(&mut file) {
        drop(file); // close before unlinking, so every platform agrees what happens
        let _ = fs::remove_file(abs);
        return Err(e.into());
    }
    Ok(())
}

/// Project the just-placed file from disk, routing on its extension exactly as the vault
/// walk does: `.md` is a note (chunks, FTS, edges), everything else a resource (one
/// inventory row). B2 adds nothing to either.
///
/// **On a refusal the placed file is removed again** — B2 undoing its own half-finished
/// write, not deleting vault material: the file becomes vault material only if this
/// returns `Ok`. The removal is best-effort and never masks the projection error, which
/// is the actionable one; a leftover file is then just an unindexed file, the state a
/// Finder copy produces and the next whole-vault pass picks up.
fn project_placed(ctx: ProjectionCtx, rel: String, abs: &Path) -> Result<ImportReport> {
    match project_from_disk(ctx, &rel) {
        Ok(note) => Ok(ImportReport { path: rel, note }),
        Err(e) => {
            let _ = fs::remove_file(abs);
            Err(e)
        }
    }
}

/// The routing itself: `true` for the note arm, `false` for a resource.
fn project_from_disk(ctx: ProjectionCtx, rel: &str) -> Result<bool> {
    match ResourceClass::of_path(rel) {
        None => {
            ingest::project_file(ctx, rel)?;
            Ok(true)
        }
        // `force`: the walk may skip a file whose `(size, mtime)` is unchanged, but an
        // import never may. The row it would be trusting can describe a file that was
        // deleted out of band and not yet pruned, and this one was written moments ago —
        // so a same-size replacement inside the same second would keep a `content_hash`
        // for bytes that no longer exist. Hash what was actually placed.
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
        // The whole point of the separator refusal: neither of these may relocate the
        // import out of the folder the human dropped on.
        assert!(destination("papers", "../../etc/passwd").is_err());
        assert!(destination("papers", "sub/a.pdf").is_err());
        assert!(destination("papers", "sub\\a.pdf").is_err());
        assert!(destination("papers", "   ").is_err());
    }

    #[test]
    fn the_shared_path_rules_still_apply_to_the_pair() {
        assert!(destination("..", "a.pdf").is_err()); // escaping folder
        assert!(destination("/abs", "a.pdf").is_err()); // absolute folder
        assert!(destination("papers", ".hidden.pdf").is_err()); // never indexed, so never written
        assert!(destination(".b2", "a.pdf").is_err());
    }
}
