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

use crate::chunk::ChunkConfig;
use crate::db;
use crate::error::{Error, Result};
use crate::id::IdGen;
use crate::ingest;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// What [`delete_note`] did: the deleted note's identity, and the surviving files
/// whose links at it now dangle (sorted, deduped; empty when nothing linked here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeleteReport {
    pub b2id: String,
    pub path: String,
    /// Vault-relative paths of the files whose links at the deleted note now
    /// dangle. They are re-projected, never rewritten — the link text stays as
    /// authored and surfaces as an unresolved link (GH #12).
    pub dangled: Vec<String>,
}

/// What [`delete_resource`] did — the resource sibling of [`DeleteReport`],
/// minus the identity field (a resource has no `b2id`; data-model.md §10).
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
fn reproject_dangled(
    conn: &Connection,
    vault_root: &Path,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    dangled: &BTreeSet<String>,
) -> Result<()> {
    for src in dangled {
        ingest::project_file(conn, vault_root, src, idgen, cfg)?;
    }
    Ok(())
}

/// Delete the note `b2id` (currently at `rel`): file off disk, projection rows
/// off the index (chunks/FTS/vectors/centroid/aliases/outbound edges cascade
/// with the `notes` row, as in whole-vault pruning), then re-project the inbound
/// linkers so their edges re-dangle. The caller (the façade) resolved the ref.
pub fn delete_note(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    vault_root: &Path,
    b2id: &str,
    rel: &str,
) -> Result<DeleteReport> {
    // The graph names the bounded inbound set before the rows go. A self-link's
    // source is the note itself — it dies with the file, so it is not re-projected.
    let dangled: BTreeSet<String> = db::inbound_edge_targets(conn, b2id)?
        .into_iter()
        .map(|(_src_id, src_path, _raw)| src_path)
        .filter(|p| p != rel)
        .collect();

    remove_file_if_present(&vault_root.join(rel))?;
    conn.execute("DELETE FROM notes WHERE b2id = ?1", [b2id])?;
    reproject_dangled(conn, vault_root, idgen, cfg, &dangled)?;

    Ok(DeleteReport {
        b2id: b2id.to_string(),
        path: rel.to_string(),
        dangled: dangled.into_iter().collect(),
    })
}

/// Delete the resource at `rel` — the note delete minus the identity step: file
/// off disk, inventory row off the index (inbound edges' `dst_resource_path` is
/// `ON DELETE SET NULL`), then re-project the inbound linkers so their edges
/// re-key to the raw-path (dangling) ids a rebuild derives.
pub fn delete_resource(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    vault_root: &Path,
    rel: &str,
) -> Result<ResourceDeleteReport> {
    db::resource_detail(conn, rel)?.ok_or_else(|| Error::ResourceNotFound(rel.to_string()))?;
    let dangled: BTreeSet<String> = db::inbound_resource_edge_targets(conn, rel)?
        .into_iter()
        .map(|(_src_id, src_path, _raw)| src_path)
        .collect();

    remove_file_if_present(&vault_root.join(rel))?;
    conn.execute("DELETE FROM resources WHERE path = ?1", [rel])?;
    reproject_dangled(conn, vault_root, idgen, cfg, &dangled)?;

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
pub fn delete_dir(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    vault_root: &Path,
    dir_input: &str,
) -> Result<DirDeleteReport> {
    // The UI only sends tree-derived paths, so an invalid input (empty, absolute,
    // escaping, a dotfolder) is refused as "no such folder" rather than growing a
    // delete-specific destination error.
    let dir = crate::pathspec::normalize_rel_dir(dir_input)
        .map_err(|_| Error::DirNotFound(dir_input.trim().to_string()))?;
    let abs = vault_root.join(&dir);
    if !abs.is_dir() {
        return Err(Error::DirNotFound(dir));
    }

    let notes = db::notes_under_dir(conn, &dir)?;
    let resources = db::resources_under_dir(conn, &dir)?;

    // Inbound linkers that survive the delete — sources inside the folder die
    // with it and must not be re-projected (their files are gone).
    let prefix = format!("{dir}/");
    let mut dangled: BTreeSet<String> = BTreeSet::new();
    for (b2id, _path) in &notes {
        for (_src_id, src_path, _raw) in db::inbound_edge_targets(conn, b2id)? {
            if !src_path.starts_with(&prefix) {
                dangled.insert(src_path);
            }
        }
    }
    for path in &resources {
        for (_src_id, src_path, _raw) in db::inbound_resource_edge_targets(conn, path)? {
            if !src_path.starts_with(&prefix) {
                dangled.insert(src_path);
            }
        }
    }

    fs::remove_dir_all(&abs)?;
    for (b2id, _path) in &notes {
        conn.execute("DELETE FROM notes WHERE b2id = ?1", [b2id])?;
    }
    for path in &resources {
        conn.execute("DELETE FROM resources WHERE path = ?1", [path])?;
    }
    reproject_dangled(conn, vault_root, idgen, cfg, &dangled)?;

    Ok(DirDeleteReport {
        dir,
        deleted_notes: notes.len(),
        deleted_resources: resources.len(),
        dangled: dangled.into_iter().collect(),
    })
}
