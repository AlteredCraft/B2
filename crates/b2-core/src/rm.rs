//! Delete a note, resource or folder: the files leave the disk, the rows leave the
//! index, and inbound links dangle, never rewritten. The result must equal an external
//! `rm` plus a full reindex (S3).
//!
//! Every delete runs [`delete_set`], which re-projects the surviving linkers. Bodies are
//! untouched, so the ops are model-free.

use crate::db;
use crate::error::{Error, Result};
use crate::ingest::{self, ProjectionCtx};
use crate::pathspec;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// What [`delete_note`] did: the deleted path (L1) and the files whose links now dangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeleteReport {
    pub path: String,
    /// Files whose links at the deleted note now dangle, sorted. Re-projected, never
    /// rewritten (GH #12).
    pub dangled: Vec<String>,
}

/// What [`delete_resource`] did (L3, data-model.md §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceDeleteReport {
    pub path: String,
    /// See [`DeleteReport::dangled`].
    pub dangled: Vec<String>,
}

/// What [`delete_dir`] did: the folder, how many indexed notes and resources went with
/// it, and the files whose links now dangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirDeleteReport {
    pub dir: String,
    pub deleted_notes: usize,
    pub deleted_resources: usize,
    /// See [`DeleteReport::dangled`]; only files outside the folder.
    pub dangled: Vec<String>,
}

/// Delete the note at `rel` (already resolved). Vectors stay for the whole-vault pass
/// to collect, as they may be shared (ADR-0006).
pub fn delete_note(ctx: ProjectionCtx, rel: &str) -> Result<DeleteReport> {
    let dangled = delete_set(ctx, &[rel], &[], || {
        remove_file_if_present(&ctx.root.join(rel))
    })?;
    Ok(DeleteReport {
        path: rel.to_string(),
        dangled,
    })
}

/// Delete the resource at `rel` (already inventory-checked).
pub fn delete_resource(ctx: ProjectionCtx, rel: &str) -> Result<ResourceDeleteReport> {
    let dangled = delete_set(ctx, &[], &[rel], || {
        remove_file_if_present(&ctx.root.join(rel))
    })?;
    Ok(ResourceDeleteReport {
        path: rel.to_string(),
        dangled,
    })
}

/// Delete the folder `dir_input` (trailing `/` tolerated) and everything in it,
/// unindexed files included. [`Error::DirNotFound`] for a missing or invalid folder.
pub fn delete_dir(ctx: ProjectionCtx, dir_input: &str) -> Result<DirDeleteReport> {
    // The UI only sends tree-derived paths, so invalid input reads as "no such folder".
    let dir = pathspec::normalize_rel_dir(dir_input)
        .map_err(|_| Error::DirNotFound(dir_input.trim().to_string()))?;
    let abs = ctx.root.join(&dir);
    if !abs.is_dir() {
        return Err(Error::DirNotFound(dir));
    }

    let notes = db::notes_under_dir(ctx.conn, &dir)?;
    let resources = db::resources_under_dir(ctx.conn, &dir)?;
    let dangled = delete_set(
        ctx,
        &notes.iter().map(String::as_str).collect::<Vec<_>>(),
        &resources.iter().map(String::as_str).collect::<Vec<_>>(),
        || Ok(fs::remove_dir_all(&abs)?),
    )?;

    Ok(DirDeleteReport {
        dir,
        deleted_notes: notes.len(),
        deleted_resources: resources.len(),
        dangled,
    })
}

/// The delete pipeline: `remove` takes the files off disk, the rows go, and surviving
/// inbound linkers re-project so their links dangle. Returns the survivors, sorted.
fn delete_set(
    ctx: ProjectionCtx,
    notes: &[&str],
    resources: &[&str],
    remove: impl FnOnce() -> Result<()>,
) -> Result<Vec<String>> {
    let conn = ctx.conn;
    // The graph names the bounded inbound set before the rows go.
    let dying: HashSet<&str> = notes.iter().copied().collect();
    let dangled: Vec<String> = db::inbound_sources(conn, notes, resources)?
        .into_iter()
        .filter(|src| !dying.contains(src.as_str()))
        .collect();

    remove()?;
    for path in notes {
        db::delete_note_row(conn, path)?;
    }
    for path in resources {
        db::delete_resource_row(conn, path)?;
    }
    // Bodies unchanged, so only edges re-derive, with the ids a full rebuild would give.
    for src in &dangled {
        ingest::project_file(ctx, src)?;
    }
    Ok(dangled)
}

/// Remove one file, tolerating one already gone, so the index cleanup still runs.
fn remove_file_if_present(abs: &Path) -> Result<()> {
    match fs::remove_file(abs) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
