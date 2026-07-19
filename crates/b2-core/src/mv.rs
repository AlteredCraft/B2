//! Move / rename a note and repair inbound links (user-stories.md Story 1).
//!
//! The typed graph keys every edge by `b2id`, never by path, so a move **never
//! breaks the graph** — the target's `b2id` is untouched and every b2id-keyed edge
//! stays valid the instant the index learns the new path. What a move *does* make
//! stale is the human-facing convenience copy: the inline `[[oldpath|alias]]` text
//! in the files that link *at* the moved note. This module rewrites exactly those.
//!
//! It is **Markdown-first** (like [`crate::vault::Vault::link`]): rewrite
//! the inbound files' text, move the file on disk, *then* re-project the index from
//! the now-current Markdown. The disposable index is rebuilt from the source of
//! truth; a crash mid-move leaves the Markdown correct and a `b2 reindex` recovers.
//!
//! A move is fully reconstructible from Markdown — files sit at their new paths with
//! their `b2id`s intact — so nothing durable is recorded; the index re-derives from
//! Markdown (`index = projection of (Markdown)`, CLAUDE.md, the core invariant).
//!
//! Bounded, not a scan: [`db::inbound_edge_targets`] reads the materialized graph
//! to name *exactly* the inbound files and link strings to touch (index-engine.md
//! §8), so the cost is O(inbound links), never O(vault).

use crate::chunk::ChunkConfig;
use crate::db;
use crate::embed::Embedder;
use crate::error::{Error, Result};
use crate::id::IdGen;
use crate::ingest;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// What [`move_note`] did: the note that moved (by `b2id`), its old and new
/// vault-relative paths, the inbound files whose link text was rewritten, and the
/// total number of `[[…]]` targets repaired across them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoveReport {
    pub b2id: String,
    pub from: String,
    pub to: String,
    /// Vault-relative paths of the inbound files whose link text was rewritten
    /// (sorted, deduped). Empty when nothing linked to the moved note.
    pub rewrote: Vec<String>,
    /// Total individual `[[…]]` link targets rewritten across `rewrote`.
    pub links_rewritten: usize,
}

/// Move the note `b2id` (currently at `old_rel`) to `new_rel_input`, rewriting
/// every inbound `[[oldpath|alias]]` link to the new path and re-projecting the
/// index. `old_rel` is the note's current vault-relative path (as the façade
/// resolved it); `new_rel_input` is the raw destination the user gave (a `.md`
/// suffix is optional and added if missing).
///
/// Re-projection **re-embeds the inbound files** (their bodies changed — the link
/// text moved), so the caller must open the vault with the same embedder the index
/// was built with (the CLI loads the real model for `mv`, as for `reindex`).
///
/// Errors with [`Error::MoveDestination`] for an invalid destination (empty,
/// absolute, escaping the vault, or equal to the source) and
/// [`Error::MoveTargetExists`] rather than clobber an existing file.
#[allow(clippy::too_many_arguments)]
pub fn move_note(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    embedder: &dyn Embedder,
    vault_root: &Path,
    b2id: &str,
    old_rel: &str,
    new_rel_input: &str,
) -> Result<MoveReport> {
    let new_rel = normalize_dest(new_rel_input)?;
    if new_rel == old_rel {
        return Err(Error::MoveDestination(format!(
            "{new_rel} is the note's current path"
        )));
    }
    let old_abs = vault_root.join(old_rel);
    let new_abs = vault_root.join(&new_rel);
    if new_abs.exists() && !is_same_dirent(&old_abs, &new_abs) {
        return Err(Error::MoveTargetExists(new_rel));
    }

    // The graph names the bounded inbound set: for each active edge pointing at the
    // moved note, its source file and the exact link text (`dst_path_raw`) written
    // there. Group by file into a target→replacement map, preserving each link's
    // own `.md`-or-not convention (Obsidian omits `.md`; a stored `.md` is kept).
    let new_rel_no_md = new_rel.strip_suffix(".md").unwrap_or(&new_rel).to_string();
    let mut by_file: BTreeMap<String, (String, BTreeMap<String, String>)> = BTreeMap::new();
    for (src_id, src_path, dst_raw) in db::inbound_edge_targets(conn, b2id)? {
        let replacement = if dst_raw.ends_with(".md") {
            new_rel.clone()
        } else {
            new_rel_no_md.clone()
        };
        by_file
            .entry(src_path)
            .or_insert_with(|| (src_id, BTreeMap::new()))
            .1
            .insert(dst_raw, replacement);
    }

    // 1. Markdown first: rewrite inbound link text in place. A self-link (the moved
    //    note links to itself) is rewritten here at its old path, before the move.
    let mut rewrote = Vec::new();
    let mut links_rewritten = 0usize;
    for (src_path, (_src_id, targets)) in &by_file {
        let abs = vault_root.join(src_path);
        let raw = fs::read_to_string(&abs)?;
        let (new_raw, n) = rewrite_links(&raw, targets);
        if n > 0 {
            fs::write(&abs, new_raw)?;
            rewrote.push(src_path.clone());
            links_rewritten += n;
        }
    }

    // 2. Move the file on disk (creating any missing parent directories).
    if let Some(parent) = new_abs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&old_abs, &new_abs)?;

    // 3. Re-project from the now-current Markdown. The moved note goes first so its
    //    `notes.path` is current before inbound files re-resolve their links to it.
    //    (An unchanged body means the moved note reuses its vectors — no re-embed.)
    ingest::ingest_file(conn, vault_root, &new_rel, idgen, cfg, embedder)?;
    for src_path in &rewrote {
        if src_path == old_rel {
            continue; // the moved note itself (a self-link) — already re-projected
        }
        ingest::ingest_file(conn, vault_root, src_path, idgen, cfg, embedder)?;
    }

    Ok(MoveReport {
        b2id: b2id.to_string(),
        from: old_rel.to_string(),
        to: new_rel,
        rewrote,
        links_rewritten,
    })
}

/// Normalize + validate a move destination into a vault-relative `.md` path,
/// mapping any rejection ([`crate::pathspec::normalize_rel_md`] — empty, absolute,
/// or vault-escaping) onto [`Error::MoveDestination`]. The "onto its current path"
/// and "onto an existing file" checks stay in [`move_note`], which alone knows the
/// source path and can read the disk.
fn normalize_dest(input: &str) -> Result<String> {
    crate::pathspec::normalize_rel_md(input).map_err(Error::MoveDestination)
}

/// What [`move_resource`] did — the resource sibling of [`MoveReport`], minus the
/// identity field (a resource has no `b2id` to carry; data-model.md §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceMoveReport {
    pub from: String,
    pub to: String,
    /// Vault-relative paths of the inbound notes whose link text was rewritten
    /// (sorted, deduped). Empty when nothing linked to the moved resource.
    pub rewrote: Vec<String>,
    /// Total individual link targets rewritten across `rewrote`.
    pub links_rewritten: usize,
}

/// Move the resource at `old_rel` to `new_rel_input` — the note move minus the
/// identity step (slice-1 spec §4): rewrite every inbound link's authored text
/// (wikilink and Markdown forms alike, each keeping its own relative-vs-root
/// convention), move the file, update the inventory, and re-project the touched
/// notes (their bodies changed, so their chunks re-embed through the usual flow).
/// B2 never touches the resource's bytes — the move is path-only.
///
/// Errors mirror [`move_note`]: [`Error::MoveDestination`] /
/// [`Error::MoveTargetExists`]; the caller resolved `old_rel` against the
/// inventory first ([`Error::ResourceNotFound`] lives in the façade).
pub fn move_resource(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    embedder: &dyn Embedder,
    vault_root: &Path,
    old_rel: &str,
    new_rel_input: &str,
) -> Result<ResourceMoveReport> {
    let new_rel = crate::pathspec::normalize_rel(new_rel_input).map_err(Error::MoveDestination)?;
    if new_rel == old_rel {
        return Err(Error::MoveDestination(format!(
            "{new_rel} is the resource's current path"
        )));
    }
    let old_abs = vault_root.join(old_rel);
    let new_abs = vault_root.join(&new_rel);
    if new_abs.exists() && !is_same_dirent(&old_abs, &new_abs) {
        return Err(Error::MoveTargetExists(new_rel));
    }

    // The graph names the bounded inbound set. Each authored target is rewritten
    // in its own convention: a note-relative Markdown target stays note-relative
    // (re-relativized against its note's directory), a vault-root target stays
    // vault-root; a `#fragment` suffix survives untouched.
    let mut by_file: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (_src_id, src_path, dst_raw) in db::inbound_resource_edge_targets(conn, old_rel)? {
        let src_dir = src_path
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default();
        let (base, fragment) = match dst_raw.split_once('#') {
            Some((b, f)) => (b, Some(f)),
            None => (dst_raw.as_str(), None),
        };
        let new_base = if base.trim() == old_rel {
            new_rel.clone() // authored vault-root — keep it vault-root
        } else {
            relativize(&src_dir, &new_rel) // authored note-relative — keep it relative
        };
        let replacement = match fragment {
            Some(f) => format!("{new_base}#{f}"),
            None => new_base,
        };
        by_file
            .entry(src_path)
            .or_default()
            .insert(dst_raw, replacement);
    }

    // 1. Markdown first: rewrite inbound link text in place, both syntaxes.
    let mut rewrote = Vec::new();
    let mut links_rewritten = 0usize;
    for (src_path, targets) in &by_file {
        let abs = vault_root.join(src_path);
        let raw = fs::read_to_string(&abs)?;
        let (pass1, n1) = rewrite_links(&raw, targets);
        let (pass2, n2) = rewrite_md_targets(&pass1, targets);
        if n1 + n2 > 0 {
            fs::write(&abs, pass2)?;
            rewrote.push(src_path.clone());
            links_rewritten += n1 + n2;
        }
    }

    // 2. Move the file on disk (creating any missing parent directories).
    if let Some(parent) = new_abs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&old_abs, &new_abs)?;

    // 3. Update the inventory: same bytes at a new path (the hash is untouched;
    //    class re-derives from the new extension), then drop the old row — its
    //    inbound edges re-dangle (ON DELETE SET NULL) until the re-projection
    //    below re-resolves them at the new path.
    repoint_resource_row(conn, old_rel, &new_rel, &new_abs)?;

    // 4. Re-project the rewritten notes from the now-current Markdown (their
    //    changed chunks re-embed inline, exactly like a note move's inbound set).
    for src_path in &rewrote {
        ingest::ingest_file(conn, vault_root, src_path, idgen, cfg, embedder)?;
    }

    Ok(ResourceMoveReport {
        from: old_rel.to_string(),
        to: new_rel,
        rewrote,
        links_rewritten,
    })
}

/// Whether `a` and `b` name the **same directory entry** on disk — true only on
/// a case-insensitive filesystem (APFS default) for a case-only rename, where
/// `Path::exists` on the destination false-positives against the source itself.
/// `fs::canonicalize` returns the on-disk-case path, so the two canonicalize
/// equal iff they are one entry; any error (e.g. the path doesn't exist) means
/// "not the same entry" and the ordinary target-exists refusal stands.
fn is_same_dirent(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// Repoint one inventory row from `old_rel` to `new_rel` (whose file now sits at
/// `new_abs`): upsert the new path with the same bytes' hash (class re-derives
/// from the new extension), then drop the old row — its inbound edges re-dangle
/// (ON DELETE SET NULL) until the caller's re-projection re-resolves them.
/// Shared by [`move_resource`] and [`move_dir`].
fn repoint_resource_row(
    conn: &Connection,
    old_rel: &str,
    new_rel: &str,
    new_abs: &Path,
) -> Result<()> {
    let (_, size, _, content_hash) = db::resource_detail(conn, old_rel)?
        .ok_or_else(|| Error::ResourceNotFound(old_rel.to_string()))?;
    let mtime = fs::metadata(new_abs)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let class = crate::resource::ResourceClass::of_path(new_rel)
        .map(|c| c.as_str().to_string())
        .unwrap_or_else(|| "binary".to_string());
    db::upsert_resource(
        conn,
        &db::ResourceRow {
            path: new_rel,
            class: &class,
            size,
            mtime,
            content_hash: &content_hash,
        },
    )?;
    conn.execute("DELETE FROM resources WHERE path = ?1", [old_rel])?;
    Ok(())
}

/// What [`move_dir`] did: the folder's old and new vault-relative paths, how many
/// **indexed** notes/resources travelled (unindexed files travel too — the whole
/// directory is renamed — but only indexed rows are counted), the files whose
/// link text was rewritten (reported at their **post-move** paths, sorted), and
/// the total link targets repaired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirMoveReport {
    pub from: String,
    pub to: String,
    pub moved_notes: usize,
    pub moved_resources: usize,
    /// Post-move vault-relative paths of the files whose link text was rewritten
    /// (sorted, deduped). Empty when no links referenced the moved set.
    pub rewrote: Vec<String>,
    /// Total individual link targets rewritten across `rewrote`.
    pub links_rewritten: usize,
}

/// Map `path` through the `from/ → to/` prefix, or return it unchanged when it
/// is outside the moved subtree.
fn remap_prefix(path: &str, from: &str, to: &str) -> String {
    match path.strip_prefix(from).and_then(|r| r.strip_prefix('/')) {
        Some(rest) => format!("{to}/{rest}"),
        None => path.to_string(),
    }
}

/// Move/rename the whole directory `from_input` to `to_input` (both
/// vault-relative; a trailing `/` is tolerated). One `fs::rename` moves the
/// directory — so **unindexed** files inside travel too — after every inbound
/// link at the moved set is rewritten, exactly as [`move_note`]/[`move_resource`]
/// do per file:
///
/// - wikilinks are vault-root-anchored, so links *between* co-moved notes are
///   rewritten just like links from outside the set;
/// - note-relative Markdown targets between co-moved files survive unchanged (a
///   computed replacement equal to the authored text is skipped, so those files
///   are not rewritten at all);
/// - after the rename, every moved note's `notes.path` is repointed **first**
///   ([`db::repoint_note_path`]), then each moved/rewritten file re-projects —
///   so path-based link resolution never depends on re-projection order (the
///   same reason full ingest is two-phase).
///
/// Re-projection **re-embeds** only genuinely rewritten bodies (unchanged bodies
/// reuse their vectors), but that still requires the caller to open the vault
/// with the real embedder — same posture as [`move_note`].
///
/// Errors: [`Error::DirNotFound`] for a missing source directory,
/// [`Error::MoveDestination`] for an invalid destination (including one inside
/// the moved folder), [`Error::MoveTargetExists`] rather than merge into an
/// existing entry (with the case-only-rename carve-out on case-insensitive
/// filesystems).
#[allow(clippy::too_many_arguments)]
pub fn move_dir(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    embedder: &dyn Embedder,
    vault_root: &Path,
    from_input: &str,
    to_input: &str,
) -> Result<DirMoveReport> {
    let from = crate::pathspec::normalize_rel_dir(from_input).map_err(Error::MoveDestination)?;
    let to = crate::pathspec::normalize_rel_dir(to_input).map_err(Error::MoveDestination)?;
    if to == from {
        return Err(Error::MoveDestination(format!(
            "{to} is the folder's current path"
        )));
    }
    if to.strip_prefix(&from).is_some_and(|r| r.starts_with('/')) {
        return Err(Error::MoveDestination(format!(
            "{to} is inside the folder being moved"
        )));
    }
    let old_abs = vault_root.join(&from);
    let new_abs = vault_root.join(&to);
    if !old_abs.is_dir() {
        return Err(Error::DirNotFound(from));
    }
    if new_abs.exists() && !is_same_dirent(&old_abs, &new_abs) {
        return Err(Error::MoveTargetExists(to));
    }

    let moved_notes = db::notes_under_dir(conn, &from)?;
    let moved_resources = db::resources_under_dir(conn, &from)?;

    // Build each inbound file's target→replacement maps. Wikilink note targets
    // are vault-root-anchored (one replacement regardless of source); Markdown
    // resource targets are convention-preserving, relativized against the
    // source's **post-move** directory so inside↔inside relative links become
    // no-ops (and are skipped rather than rewritten). Two maps per file because
    // the two syntaxes rewrite through different passes, mirroring
    // `move_note` (wikilinks only) and `move_resource` (both).
    let mut wiki_by_file: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut md_by_file: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

    for (b2id, old_path) in &moved_notes {
        let new_path = remap_prefix(old_path, &from, &to);
        let new_path_no_md = new_path
            .strip_suffix(".md")
            .unwrap_or(&new_path)
            .to_string();
        for (_src_id, src_path, dst_raw) in db::inbound_edge_targets(conn, b2id)? {
            let replacement = if dst_raw.ends_with(".md") {
                new_path.clone()
            } else {
                new_path_no_md.clone()
            };
            if replacement != dst_raw {
                wiki_by_file
                    .entry(src_path)
                    .or_default()
                    .insert(dst_raw, replacement);
            }
        }
    }
    for old_path in &moved_resources {
        let new_path = remap_prefix(old_path, &from, &to);
        for (_src_id, src_path, dst_raw) in db::inbound_resource_edge_targets(conn, old_path)? {
            // The source's directory *after* the move — sources inside the moved
            // set remap; outside sources keep their dir.
            let src_dir_after = {
                let src_after = remap_prefix(&src_path, &from, &to);
                src_after
                    .rsplit_once('/')
                    .map(|(dir, _)| dir.to_string())
                    .unwrap_or_default()
            };
            let (base, fragment) = match dst_raw.split_once('#') {
                Some((b, f)) => (b, Some(f)),
                None => (dst_raw.as_str(), None),
            };
            let new_base = if base.trim() == old_path.as_str() {
                new_path.clone() // authored vault-root — keep it vault-root
            } else {
                relativize(&src_dir_after, &new_path) // authored note-relative
            };
            let replacement = match fragment {
                Some(f) => format!("{new_base}#{f}"),
                None => new_base,
            };
            if replacement != dst_raw {
                wiki_by_file
                    .entry(src_path.clone())
                    .or_default()
                    .insert(dst_raw.clone(), replacement.clone());
                md_by_file
                    .entry(src_path)
                    .or_default()
                    .insert(dst_raw, replacement);
            }
        }
    }

    // 1. Markdown first: rewrite each inbound file in place at its pre-move path.
    let empty = BTreeMap::new();
    let mut rewrote_old_paths = Vec::new();
    let mut links_rewritten = 0usize;
    let touched: std::collections::BTreeSet<String> = wiki_by_file
        .keys()
        .chain(md_by_file.keys())
        .cloned()
        .collect();
    for src_path in &touched {
        let abs = vault_root.join(src_path);
        let raw = fs::read_to_string(&abs)?;
        let (pass1, n1) = rewrite_links(&raw, wiki_by_file.get(src_path).unwrap_or(&empty));
        let (pass2, n2) = rewrite_md_targets(&pass1, md_by_file.get(src_path).unwrap_or(&empty));
        if n1 + n2 > 0 {
            fs::write(&abs, pass2)?;
            rewrote_old_paths.push(src_path.clone());
            links_rewritten += n1 + n2;
        }
    }

    // 2. One rename moves the whole directory (unindexed files travel for free),
    //    creating any missing destination parents.
    if let Some(parent) = new_abs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&old_abs, &new_abs)?;

    // 3. Repoint the resolver before any re-projection: every moved note's path
    //    (old and new sets are disjoint — the destination didn't exist — so the
    //    UNIQUE(path) constraint can't trip), then every moved resource's
    //    inventory row (so resource links resolve at their new paths too).
    for (b2id, old_path) in &moved_notes {
        db::repoint_note_path(conn, b2id, &remap_prefix(old_path, &from, &to))?;
    }
    for old_path in &moved_resources {
        let new_path = remap_prefix(old_path, &from, &to);
        repoint_resource_row(conn, old_path, &new_path, &vault_root.join(&new_path))?;
    }

    // 4. Re-project from the now-current Markdown: every moved note (refreshes
    //    the filename-derived title, mtime, and its outbound edges — an unchanged
    //    body reuses its vectors), then every rewritten file outside the moved
    //    set (moved ones were just re-projected at their new paths).
    let moved_note_old_paths: std::collections::BTreeSet<&str> =
        moved_notes.iter().map(|(_, p)| p.as_str()).collect();
    for (_b2id, old_path) in &moved_notes {
        let new_path = remap_prefix(old_path, &from, &to);
        ingest::ingest_file(conn, vault_root, &new_path, idgen, cfg, embedder)?;
    }
    for src_path in &rewrote_old_paths {
        if moved_note_old_paths.contains(src_path.as_str()) {
            continue;
        }
        ingest::ingest_file(conn, vault_root, src_path, idgen, cfg, embedder)?;
    }

    let mut rewrote: Vec<String> = rewrote_old_paths
        .iter()
        .map(|p| remap_prefix(p, &from, &to))
        .collect();
    rewrote.sort();

    Ok(DirMoveReport {
        from,
        to,
        moved_notes: moved_notes.len(),
        moved_resources: moved_resources.len(),
        rewrote,
        links_rewritten,
    })
}

/// The relative path from `base_dir` (a vault-relative directory, `""` = root)
/// to `to_path` (a vault-relative file): shared prefix dropped, one `..` per
/// remaining base segment — the inverse of resolution's note-relative join.
fn relativize(base_dir: &str, to_path: &str) -> String {
    let base: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    let to: Vec<&str> = to_path.split('/').collect();
    let shared = base
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<&str> = Vec::with_capacity(base.len() - shared + to.len() - shared);
    out.extend(std::iter::repeat_n("..", base.len() - shared));
    out.extend(&to[shared..]);
    out.join("/")
}

/// Rewrite every Markdown-form target (`[text](target)` / `![alt](target)`)
/// whose *trimmed* target is a key in `targets` — the `[…](…)` sibling of
/// [`rewrite_links`], same contract: only the target token changes, every other
/// byte (text, whitespace, the `](` frame) is preserved.
fn rewrite_md_targets(raw: &str, targets: &BTreeMap<String, String>) -> (String, usize) {
    let mut out = String::with_capacity(raw.len());
    let mut count = 0usize;
    let mut rest = raw;
    while let Some(open) = rest.find("](") {
        out.push_str(&rest[..open + 2]);
        let after = &rest[open + 2..];
        let Some(close) = after.find(')') else {
            out.push_str(after);
            return (out, count);
        };
        let inner = &after[..close];
        match targets.get(inner.trim()) {
            Some(replacement) => {
                let lead = inner.len() - inner.trim_start().len();
                let trail = inner.len() - inner.trim_end().len();
                out.push_str(&inner[..lead]);
                out.push_str(replacement);
                out.push_str(&inner[inner.len() - trail..]);
                count += 1;
            }
            None => out.push_str(inner),
        }
        out.push(')');
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    (out, count)
}

/// Rewrite every wikilink whose *trimmed* target is a key in `targets` to that
/// key's replacement, preserving all other bytes — surrounding whitespace inside
/// the brackets, the `|alias`, and the `[[`/`]]` themselves. The match is bounded
/// to the target token (up to `|` or `]]`), so moving `foo` never touches a
/// `[[foo-bar]]` that merely shares its prefix. Returns the rewritten text and the
/// count of targets replaced.
fn rewrite_links(raw: &str, targets: &BTreeMap<String, String>) -> (String, usize) {
    let mut out = String::with_capacity(raw.len());
    let mut count = 0usize;
    let mut rest = raw;
    while let Some(open) = rest.find("[[") {
        out.push_str(&rest[..open + 2]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else {
            out.push_str(after);
            return (out, count);
        };
        let inner = &after[..close];
        // Split off the display alias (kept verbatim, including its leading `|`).
        let (path_part, alias_part) = match inner.find('|') {
            Some(i) => (&inner[..i], Some(&inner[i..])),
            None => (inner, None),
        };
        match targets.get(path_part.trim()) {
            Some(replacement) => {
                // Preserve the path's own surrounding whitespace; swap only the
                // trimmed target so every other byte is identical.
                let lead = path_part.len() - path_part.trim_start().len();
                let trail = path_part.len() - path_part.trim_end().len();
                out.push_str(&path_part[..lead]);
                out.push_str(replacement);
                out.push_str(&path_part[path_part.len() - trail..]);
                count += 1;
            }
            None => out.push_str(path_part),
        }
        if let Some(alias) = alias_part {
            out.push_str(alias);
        }
        out.push_str("]]");
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn rewrites_the_target_and_keeps_the_alias() {
        let t = targets(&[("concepts/memory", "concepts/human-memory")]);
        let (out, n) = rewrite_links("see [[concepts/memory|Human memory]] here", &t);
        assert_eq!(out, "see [[concepts/human-memory|Human memory]] here");
        assert_eq!(n, 1);
    }

    #[test]
    fn rewrites_a_bare_link_with_no_alias() {
        let t = targets(&[("concepts/memory", "concepts/human-memory")]);
        let (out, n) = rewrite_links("[[concepts/memory]]", &t);
        assert_eq!(out, "[[concepts/human-memory]]");
        assert_eq!(n, 1);
    }

    #[test]
    fn a_prefix_sharing_sibling_is_never_touched() {
        // Moving `concepts/memory` must not corrupt `concepts/memory-palace`.
        let t = targets(&[("concepts/memory", "concepts/human-memory")]);
        let (out, n) = rewrite_links(
            "[[concepts/memory-palace|MP]] and [[concepts/memory|M]]",
            &t,
        );
        assert_eq!(
            out,
            "[[concepts/memory-palace|MP]] and [[concepts/human-memory|M]]"
        );
        assert_eq!(n, 1);
    }

    #[test]
    fn surrounding_whitespace_inside_the_brackets_is_preserved() {
        let t = targets(&[("concepts/memory", "concepts/human-memory")]);
        let (out, n) = rewrite_links("[[ concepts/memory | Mem ]]", &t);
        assert_eq!(
            out, "[[ concepts/human-memory | Mem ]]",
            "only the target token changes"
        );
        assert_eq!(n, 1);
    }

    #[test]
    fn each_link_keeps_its_own_md_convention() {
        // The `.md`-bearing and bare forms map to their matching replacements.
        let t = targets(&[
            ("concepts/memory", "concepts/human-memory"),
            ("concepts/memory.md", "concepts/human-memory.md"),
        ]);
        let (out, n) = rewrite_links("[[concepts/memory]] [[concepts/memory.md|M]]", &t);
        assert_eq!(
            out,
            "[[concepts/human-memory]] [[concepts/human-memory.md|M]]"
        );
        assert_eq!(n, 2);
    }

    #[test]
    fn text_with_no_matching_link_is_returned_verbatim() {
        let t = targets(&[("concepts/memory", "concepts/human-memory")]);
        let raw = "no links here, and an [[unrelated|note]] plus a stray [[ bracket";
        let (out, n) = rewrite_links(raw, &t);
        assert_eq!(out, raw);
        assert_eq!(n, 0);
    }
}
