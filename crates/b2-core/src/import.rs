//! Bring an *outside* file into the vault — note/resource CRUD's **import** arm, and
//! the kernel behind the desktop's drag-a-file-from-Finder-onto-the-tree gesture.
//!
//! What separates this from [`crate::add`]: `add` **authors** a new document (B2 mints
//! the frontmatter and owns every byte of it), while an import is a **byte-honest
//! copy** of a file the human already has. B2 writes the bytes it was handed and
//! authors nothing — the posture [`crate::vault::Vault::write`] takes with a body edit
//! — so a dropped `.md` keeps its own frontmatter verbatim and a dropped PDF keeps its
//! bytes. That is what makes this a permitted write: invariants.md W3 enumerates
//! "create/move/delete of notes, resources, and folders on explicit command", and
//! data-model.md §10's asymmetry is about *authoring* a resource's content, which this
//! never does — placing one is the same category of act as moving or deleting it.
//!
//! **Vault first**, the same order `add`/`mv`/`link` write in: place the file, then
//! project *from disk* — so the index is derived from what actually landed, never from
//! what the caller said it was writing.
//!
//! **Model-free** (a [`ProjectionCtx`], like `create_note`): an imported note is
//! projected — tree, keyword search, graph — with no embedder touched, and its chunks
//! join the DB-derived missing-vector set for the next embed pass to fill. An imported
//! resource joins the inventory exactly as the walk would have inventoried it. So
//! importing works with no model provisioned, and a fake-opened vault can never write
//! foreign vectors into a real-model embedding space.
//!
//! Two entry points because the two gestures arrive differently: a file dropped on the
//! webview is **bytes** (the OS hands the page content, not a path), while a file
//! chosen in an OS picker is a **path**. Both share one destination rule and one
//! projection.

use crate::error::{Error, Result};
use crate::ingest::{self, ProjectionCtx};
use crate::resource::ResourceClass;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// What an import did: where the file landed, and — for a note — the `b2id` the
/// projection stamped or read back. `None` for a resource, which has no identity to
/// stamp (invariants.md L3: resources are path-keyed peers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub path: String,
    pub b2id: Option<String>,
}

/// Import `bytes` into the vault folder `dir` (`""` for the root) under `file_name`.
/// The drag-and-drop arm: a file dropped on a webview arrives as content, so the
/// bytes *are* the source.
///
/// Refuses [`Error::ImportDestination`] for a name/folder pair that isn't a valid
/// vault-relative path (see [`destination`]) and [`Error::ImportTargetExists`] rather
/// than clobber a file already sitting there (the vault never overwrites,
/// data-model.md §1).
pub fn import_bytes(
    ctx: ProjectionCtx,
    dir: &str,
    file_name: &str,
    bytes: &[u8],
) -> Result<ImportReport> {
    let (rel, abs) = place(ctx.root, dir, file_name)?;
    fs::write(&abs, bytes)?;
    project_placed(ctx, rel, &abs)
}

/// Import the file at `source` (an absolute path outside — or inside — the vault)
/// into the vault folder `dir`, keeping its name. The OS-picker arm, and the reason
/// it exists rather than being `import_bytes(fs::read(source)?)` at the call site:
/// the adapter then holds no logic, and the bytes never round-trip through it.
///
/// Same refusals as [`import_bytes`], plus [`Error::ImportDestination`] for a source
/// that is a folder or has no file name. A source *inside* the vault is a duplicate,
/// not an error — and if it is a note, the copy carries the original's `b2id`, which
/// [`project_placed`]'s guard refuses rather than let it steal the identity.
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
    let (rel, abs) = place(ctx.root, dir, file_name)?;
    fs::copy(source, &abs)?;
    project_placed(ctx, rel, &abs)
}

/// Resolve `(dir, file_name)` into the vault-relative destination path, refusing
/// anything that isn't one: a `file_name` carrying a path separator (a *name* is what
/// a dropped file has — a name free to carry `../` or `a/b` would silently redirect
/// the import out of the folder the human aimed at), and, via
/// [`crate::pathspec::normalize_rel`], an empty, absolute, vault-escaping, or
/// dot-prefixed result. The extension is left exactly as given: it is what decides
/// note vs resource, and B2 does not get to rename the human's file.
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

/// Validate the destination, refuse an occupied one, and make sure its folder exists
/// (mirroring `add`'s parent creation — an import into a folder the tree shows always
/// finds it there, but a nested `dir` typed by a future caller shouldn't fail on it).
/// Returns the vault-relative path and its absolute twin. Writes nothing itself: the
/// caller supplies the bytes, so each arm keeps its own one-line write.
fn place(vault_root: &Path, dir: &str, file_name: &str) -> Result<(String, PathBuf)> {
    let rel = destination(dir, file_name)?;
    let abs = vault_root.join(&rel);
    if abs.exists() {
        return Err(Error::ImportTargetExists(rel));
    }
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok((rel, abs))
}

/// Project the just-placed file from disk, routing on its extension exactly as the
/// vault walk does ([`ResourceClass::of_path`]): `.md` is a note (chunks, FTS, edges,
/// a stamped `b2id`), everything else is a resource (one inventory row).
///
/// A note gets the single-note collision guard first (GH #81). An *arriving* file is
/// precisely the case that guard exists for: a copied note carries the original's
/// `b2id`, and projecting it would silently transfer that identity — and every inbound
/// edge — to the copy.
///
/// **On any refusal the placed file is removed again.** That is B2 undoing its own
/// half-finished write, not deleting vault material (W4): the file becomes vault
/// material only if this returns `Ok`. Leaving it behind would strand bytes the tree
/// can't show and the next pass could only re-refuse.
fn project_placed(ctx: ProjectionCtx, rel: String, abs: &Path) -> Result<ImportReport> {
    match project_from_disk(ctx, &rel) {
        Ok(b2id) => Ok(ImportReport { path: rel, b2id }),
        Err(e) => {
            let _ = fs::remove_file(abs);
            Err(e)
        }
    }
}

/// The routing itself: the note arm's `b2id`, or `None` for a resource.
fn project_from_disk(ctx: ProjectionCtx, rel: &str) -> Result<Option<String>> {
    match ResourceClass::of_path(rel) {
        None => {
            ingest::guard_single_note_collision(ctx.conn, ctx.root, rel)?;
            Ok(Some(ingest::project_file(ctx, rel)?.b2id))
        }
        Some(class) => {
            ingest::project_resource_file(ctx.conn, ctx.root, rel, class)?;
            Ok(None)
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
