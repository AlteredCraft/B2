//! Delete a note / resource / folder — the destructive complement of [`crate::mv`].
//!
//! A delete is "a move minus the destination": the file(s) leave the disk, the
//! projection rows leave the index, and the inbound links that pointed at the
//! deleted target **dangle** — they are never rewritten (there is nothing to
//! rewrite them *to*), exactly the state an external `rm` plus a full reindex
//! produces. That equivalence is the correctness bar (`incremental ≡ full
//! rebuild`, index-engine.md §8): after every op here, the index is byte-identical
//! to a from-scratch rebuild of the now-current Markdown.
//!
//! The single-note projection paths never prune (ingest.rs #31 is whole-vault
//! only), so each op drops its rows directly, then re-projects the **surviving**
//! inbound files: their edges re-derive against the pruned tables, so a link at
//! the dead target re-keys to its raw-path (dangling) edge id — the same id a
//! rebuild derives. Bodies are untouched, so re-projection re-chunks nothing and
//! the ops are **model-free** (the `create_note`/`write` posture, not `mv`'s).

use crate::db;
use crate::error::{Error, Result};
use crate::ingest::{self, ProjectionCtx};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// What [`delete_note`] did: the deleted note's path — its identity (L1) — and the
/// surviving files whose links at it now dangle (sorted, deduped; empty when nothing
/// linked here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeleteReport {
    pub path: String,
    /// Vault-relative paths of the files whose links at the deleted note now
    /// dangle. They are re-projected, never rewritten — the link text stays as
    /// authored and surfaces as an unresolved link (GH #12).
    pub dangled: Vec<String>,
}

/// What [`delete_resource`] did — the resource sibling of [`DeleteReport`]; the two
/// now carry the same fields, since notes and resources share one identity model
/// (L3, data-model.md §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceDeleteReport {
    pub path: String,
    /// See [`DeleteReport::dangled`].
    pub dangled: Vec<String>,
}

/// What [`delete_dir`] did: the deleted folder, how many **indexed**
/// notes/resources died with it (unindexed files inside are removed too — the
/// whole directory goes), and the surviving files whose links now dangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirDeleteReport {
    pub dir: String,
    pub deleted_notes: usize,
    pub deleted_resources: usize,
    /// See [`DeleteReport::dangled`] — only files *outside* the deleted folder
    /// (a linker inside it died with the folder).
    pub dangled: Vec<String>,
}

/// Remove one file, tolerating a file already gone: an external delete that raced
/// us leaves exactly the state we are reconciling toward, so the projection
/// cleanup must still run rather than abort.
fn remove_file_if_present(abs: &Path) -> Result<()> {
    match fs::remove_file(abs) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Re-project each surviving inbound file (bodies unchanged, so nothing
/// re-chunks and no vector is touched): their edges re-derive against the
/// pruned tables, re-dangling the links that pointed at the deleted target with
/// the raw-path-keyed edge ids a full rebuild would derive.
fn reproject_dangled(ctx: ProjectionCtx, dangled: &BTreeSet<String>) -> Result<()> {
    for src in dangled {
        ingest::project_file(ctx, src)?;
    }
    Ok(())
}

/// Delete the note at `rel`: file off disk, projection rows off the index
/// (chunks/FTS/centroid/aliases/outbound edges cascade with the `notes` row, as in
/// whole-vault pruning), then re-project the inbound linkers so their edges
/// re-dangle. The caller (the façade) resolved the ref.
///
/// The note's chunk **vectors** deliberately do not cascade — they are
/// content-addressed and may be shared (M4), so the whole-vault pass collects them
/// when nothing references them ([`db::prune_orphan_vectors`]).
pub fn delete_note(ctx: ProjectionCtx, rel: &str) -> Result<DeleteReport> {
    let (conn, root) = (ctx.conn, ctx.root);
    // The graph names the bounded inbound set before the rows go. A self-link's
    // source is the note itself — it dies with the file, so it is not re-projected.
    let dangled: BTreeSet<String> = db::inbound_edge_targets(conn, rel)?
        .into_iter()
        .map(|e| e.src_path)
        .filter(|p| p != rel)
        .collect();

    remove_file_if_present(&root.join(rel))?;
    conn.execute("DELETE FROM notes WHERE path = ?1", [rel])?;
    reproject_dangled(ctx, &dangled)?;

    Ok(DeleteReport {
        path: rel.to_string(),
        dangled: dangled.into_iter().collect(),
    })
}

/// Delete the resource at `rel` — the note delete minus the identity step: file
/// off disk, inventory row off the index (inbound edges' `dst_resource_path` is
/// `ON DELETE SET NULL`), then re-project the inbound linkers so their edges
/// re-key to the raw-path (dangling) ids a rebuild derives.
pub fn delete_resource(ctx: ProjectionCtx, rel: &str) -> Result<ResourceDeleteReport> {
    let (conn, root) = (ctx.conn, ctx.root);
    db::resource_detail(conn, rel)?.ok_or_else(|| Error::ResourceNotFound(rel.to_string()))?;
    let dangled: BTreeSet<String> = db::inbound_resource_edge_targets(conn, rel)?
        .into_iter()
        .map(|e| e.src_path)
        .collect();

    remove_file_if_present(&root.join(rel))?;
    conn.execute("DELETE FROM resources WHERE path = ?1", [rel])?;
    reproject_dangled(ctx, &dangled)?;

    Ok(ResourceDeleteReport {
        path: rel.to_string(),
        dangled: dangled.into_iter().collect(),
    })
}

/// Delete the whole folder `dir_input` (vault-relative; a trailing `/` is
/// tolerated): one `fs::remove_dir_all` — so **unindexed** files inside go too —
/// then every contained note's and resource's rows, then re-projection of the
/// surviving linkers *outside* the folder. Errors with [`Error::DirNotFound`]
/// for a missing (or invalid) source folder.
pub fn delete_dir(ctx: ProjectionCtx, dir_input: &str) -> Result<DirDeleteReport> {
    let (conn, root) = (ctx.conn, ctx.root);
    // The UI only sends tree-derived paths, so an invalid input (empty, absolute,
    // escaping, a dotfolder) is refused as "no such folder" rather than growing a
    // delete-specific destination error.
    let dir = crate::pathspec::normalize_rel_dir(dir_input)
        .map_err(|_| Error::DirNotFound(dir_input.trim().to_string()))?;
    let abs = root.join(&dir);
    if !abs.is_dir() {
        return Err(Error::DirNotFound(dir));
    }

    let notes = db::notes_under_dir(conn, &dir)?;
    let resources = db::resources_under_dir(conn, &dir)?;

    // Inbound linkers that survive the delete — sources inside the folder die
    // with it and must not be re-projected (their files are gone).
    let prefix = format!("{dir}/");
    let mut dangled: BTreeSet<String> = BTreeSet::new();
    for note_path in &notes {
        for e in db::inbound_edge_targets(conn, note_path)? {
            if !e.src_path.starts_with(&prefix) {
                dangled.insert(e.src_path);
            }
        }
    }
    for path in &resources {
        for e in db::inbound_resource_edge_targets(conn, path)? {
            if !e.src_path.starts_with(&prefix) {
                dangled.insert(e.src_path);
            }
        }
    }

    fs::remove_dir_all(&abs)?;
    for note_path in &notes {
        conn.execute("DELETE FROM notes WHERE path = ?1", [note_path])?;
    }
    for path in &resources {
        conn.execute("DELETE FROM resources WHERE path = ?1", [path])?;
    }
    reproject_dangled(ctx, &dangled)?;

    Ok(DirDeleteReport {
        dir,
        deleted_notes: notes.len(),
        deleted_resources: resources.len(),
        dangled: dangled.into_iter().collect(),
    })
}
