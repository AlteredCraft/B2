//! Move / rename a note and repair inbound links.
//!
//! **A move is where "rename keeps every backlink resolving" is earned.** Identity is
//! the vault-relative path (ADR-0003), so a move changes the moved note's identity, and
//! this module makes that a re-key rather than a break. Two halves, both bounded by the
//! moved note's backlink count: the human-facing copy — the inline `[[oldpath|alias]]`
//! text in every file linking *at* it, rewritten in place — and the index, one
//! [`db::repoint_note_path`] whose `ON UPDATE CASCADE` FKs carry chunks, aliases,
//! centroid and outbound edges atomically, then a re-projection of the inbound sources
//! so their `edges.dst_path` (no FK — it must be free to dangle) points at the new path.
//! The moved note's **vectors are not touched at all**: content-addressed (ADR-0006),
//! they belong to the chunk text, which a move does not change.
//!
//! It is **Markdown-first**: rewrite the inbound text, move the file, *then* re-project
//! from the now-current Markdown, so a crash mid-move leaves the Markdown correct and a
//! `b2 reindex` recovers. And bounded, not a scan: [`db::inbound_edge_targets`] names
//! exactly the files to touch, so the cost is O(inbound links).

use crate::db;
use crate::error::{Error, Result};
use crate::ingest::{self, EmbedCtx};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// What [`move_note`] did: the note's old and new vault-relative paths, the inbound
/// files whose link text was rewritten, and the total number of `[[…]]` targets
/// repaired across them. `to` is the note's identity after the move (L1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoveReport {
    pub from: String,
    pub to: String,
    /// Vault-relative paths of the inbound files whose link text was rewritten
    /// (sorted, deduped). Empty when nothing linked to the moved note.
    pub rewrote: Vec<String>,
    /// Total individual `[[…]]` link targets rewritten across `rewrote`.
    pub links_rewritten: usize,
}

/// Move the note at `old_rel` to `new_rel_input`, rewriting every inbound
/// `[[oldpath|alias]]` link and re-keying the index. `old_rel` is the note's current
/// path (as the façade resolved it); `new_rel_input` is the raw destination the user
/// gave (a `.md` suffix is optional).
///
/// Re-projection **re-embeds the inbound files** — their bodies changed — so the caller
/// must open the vault with the embedder the index was built with. The *moved* note
/// re-embeds nothing (ADR-0006). Errors with [`Error::MoveDestination`] for an invalid
/// destination and [`Error::MoveTargetExists`] rather than clobber.
pub fn move_note(ctx: EmbedCtx, old_rel: &str, new_rel_input: &str) -> Result<MoveReport> {
    let (conn, root) = (ctx.proj.conn, ctx.proj.root);
    let new_rel = normalize_dest(new_rel_input)?;
    refuse_same_path(&new_rel, old_rel, "note")?;
    let old_abs = root.join(old_rel);
    let new_abs = root.join(&new_rel);
    refuse_occupied(&old_abs, &new_abs, &new_rel)?;

    // The graph names the bounded inbound set: for each active edge pointing at the
    // moved note, its source file and the exact link text (`dst_path_raw`) written
    // there. Group by file into a target→replacement map, each link keeping its own
    // `.md`-or-not convention ([`wiki_replacement`]).
    let mut by_file = ByFile::new();
    for e in db::inbound_edge_targets(conn, old_rel)? {
        let replacement = wiki_replacement(&new_rel, &e.dst_raw);
        by_file
            .entry(e.src_path)
            .or_default()
            .insert(e.dst_raw, replacement);
    }
    // Every inbound source is re-projected below, whether or not its *text* changed:
    // its `edges.dst_path` names the old path and carries no FK to cascade (it must
    // be free to be NULL — the dangling case, G5). The two sets differ for a
    // Markdown-form link at a note (`[x](notes/a.md)`), which resolves as an edge but
    // is not a form `move_note` rewrites — its edge must still re-resolve, or it
    // would keep naming a path that no longer exists.
    let inbound: Vec<String> = by_file.keys().cloned().collect();

    // 1. Markdown first: rewrite inbound link text in place — the `[[…]]` form, which
    //    is the one a note move repairs (the `[…](…)` pass is the resource move's).
    //    A self-link (the moved note links to itself) is rewritten here at its old
    //    path, before the move.
    let (rewrote, links_rewritten) = rewrite_inbound(root, &by_file, &ByFile::new())?;

    // 2. Move the file on disk (creating any missing parent directories).
    rename_with_parents(&old_abs, &new_abs)?;

    // 3. Re-key the index before anything re-projects, so path-based link resolution
    //    is independent of re-projection order (the same reason full ingest is
    //    two-phase). One statement; the FK cascades carry the note's chunks,
    //    aliases, centroid and outbound edges with it.
    db::repoint_note_path(conn, old_rel, &new_rel)?;

    // 4. Re-project from the now-current Markdown: the moved note (refreshing its
    //    filename-derived title and mtime), then every inbound source so its edges
    //    re-resolve at the new path.
    ingest::ingest_file(ctx, &new_rel)?;
    for src_path in &inbound {
        if src_path == old_rel {
            continue; // the moved note itself (a self-link) — already re-projected
        }
        ingest::ingest_file(ctx, src_path)?;
    }

    Ok(MoveReport {
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

/// What [`move_resource`] did — the resource sibling of [`MoveReport`]. Since
/// GH #170 the two carry the same fields, both arms being path-keyed
/// (data-model.md §10); they stay separate types because the reports are
/// separate contracts, not because the shapes diverge.
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

/// Move the resource at `old_rel` to `new_rel_input` — the note move minus the identity
/// step: rewrite every inbound link's authored text (both syntaxes, each keeping its own
/// relative-vs-root convention), move the file, update the inventory, re-project the
/// touched notes. B2 never touches the resource's bytes; the move is path-only. Errors
/// mirror [`move_note`] (the façade owns [`Error::ResourceNotFound`]).
pub fn move_resource(
    ctx: EmbedCtx,
    old_rel: &str,
    new_rel_input: &str,
) -> Result<ResourceMoveReport> {
    let (conn, root) = (ctx.proj.conn, ctx.proj.root);
    let new_rel = crate::pathspec::normalize_rel(new_rel_input).map_err(Error::MoveDestination)?;
    refuse_same_path(&new_rel, old_rel, "resource")?;
    let old_abs = root.join(old_rel);
    let new_abs = root.join(&new_rel);
    refuse_occupied(&old_abs, &new_abs, &new_rel)?;

    // The graph names the bounded inbound set; each authored target is rewritten in
    // its own convention, fragment intact ([`resource_replacement`]).
    let mut by_file = ByFile::new();
    for e in db::inbound_resource_edge_targets(conn, old_rel)? {
        let replacement =
            resource_replacement(&e.dst_raw, old_rel, &new_rel, &parent_dir(&e.src_path));
        by_file
            .entry(e.src_path)
            .or_default()
            .insert(e.dst_raw, replacement);
    }

    // 1. Markdown first: rewrite inbound link text in place, both syntaxes — a
    //    resource is linked as `![[img.png]]` *or* `![](img.png)`, so the one map
    //    feeds both passes.
    let (rewrote, links_rewritten) = rewrite_inbound(root, &by_file, &by_file)?;

    // 2. Move the file on disk (creating any missing parent directories).
    rename_with_parents(&old_abs, &new_abs)?;

    // 3. Update the inventory: same bytes at a new path (the hash is untouched;
    //    class re-derives from the new extension), then drop the old row — its
    //    inbound edges re-dangle (ON DELETE SET NULL) until the re-projection
    //    below re-resolves them at the new path.
    repoint_resource_row(conn, old_rel, &new_rel, &new_abs)?;

    // 4. Re-project the rewritten notes from the now-current Markdown (their
    //    changed chunks re-embed inline, exactly like a note move's inbound set).
    for src_path in &rewrote {
        ingest::ingest_file(ctx, src_path)?;
    }

    Ok(ResourceMoveReport {
        from: old_rel.to_string(),
        to: new_rel,
        rewrote,
        links_rewritten,
    })
}

// --- the mechanics every move shares (GH #134) --------------------------------
//
// A move is always the same four steps — refuse a bad destination, rewrite the inbound
// files' link text, rename on disk, re-project — differing only in what counts as a
// replacement. Composed rather than folded into one `preflight` because the refusal
// *order* is not uniform, and that precedence is part of each op's contract.

/// Each inbound file's authored-target → replacement map, keyed by the file's
/// vault-relative path. `BTreeMap` throughout, so the rewrite order — and the
/// `rewrote` list every report carries — is sorted and deterministic.
type ByFile = BTreeMap<String, BTreeMap<String, String>>;

/// A vault-relative file's directory (`""` at the vault root) — the base a
/// note-relative Markdown target is resolved and re-relativized against.
fn parent_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

/// Refuse a destination equal to the source. `subject` names the thing being
/// moved, so the message reads in the user's own nouns ("… is the note's current
/// path").
fn refuse_same_path(new_rel: &str, old_rel: &str, subject: &str) -> Result<()> {
    if new_rel == old_rel {
        return Err(Error::MoveDestination(format!(
            "{new_rel} is the {subject}'s current path"
        )));
    }
    Ok(())
}

/// Refuse an occupied destination rather than clobber it (the vault never
/// overwrites, data-model.md §1) — with the case-only-rename carve-out: on a
/// case-insensitive filesystem the destination "exists" because it *is* the
/// source ([`is_same_dirent`]).
fn refuse_occupied(old_abs: &Path, new_abs: &Path, new_rel: &str) -> Result<()> {
    if new_abs.exists() && !is_same_dirent(old_abs, new_abs) {
        return Err(Error::MoveTargetExists(new_rel.to_string()));
    }
    Ok(())
}

/// Move one directory entry on disk, creating any missing destination parents
/// first — the note, resource, and whole-folder moves all rename exactly once.
fn rename_with_parents(old_abs: &Path, new_abs: &Path) -> Result<()> {
    if let Some(parent) = new_abs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(old_abs, new_abs)?;
    Ok(())
}

/// Markdown first, for every move: rewrite the inbound files' authored link text in
/// place. `wiki` holds each file's `[[…]]` replacements and `md` its `[…](…)` ones; a
/// file whose passes change nothing is left untouched *and* unreported, which is how a
/// relative link between two co-moved files stays a no-op. Returns the rewritten paths
/// (sorted, deduped) and the total targets replaced.
fn rewrite_inbound(vault_root: &Path, wiki: &ByFile, md: &ByFile) -> Result<(Vec<String>, usize)> {
    let none = BTreeMap::new();
    let mut rewrote = Vec::new();
    let mut links_rewritten = 0usize;
    let touched: BTreeSet<&str> = wiki.keys().chain(md.keys()).map(String::as_str).collect();
    for src_path in touched {
        let abs = vault_root.join(src_path);
        let raw = fs::read_to_string(&abs)?;
        let (pass1, n1) = rewrite_links(&raw, wiki.get(src_path).unwrap_or(&none));
        let (pass2, n2) = rewrite_md_targets(&pass1, md.get(src_path).unwrap_or(&none));
        if n1 + n2 > 0 {
            fs::write(&abs, pass2)?;
            rewrote.push(src_path.to_string());
            links_rewritten += n1 + n2;
        }
    }
    Ok((rewrote, links_rewritten))
}

/// The replacement for a wikilink at a note now living at `new_path`, preserving
/// the link's own `.md`-or-not convention (Obsidian omits the extension; an
/// authored `.md` is kept). Shared by the note move and the folder move, which
/// differ only in where `new_path` comes from.
fn wiki_replacement(new_path: &str, authored: &str) -> String {
    if authored.ends_with(".md") {
        new_path.to_string()
    } else {
        new_path.strip_suffix(".md").unwrap_or(new_path).to_string()
    }
}

/// The replacement for a link at a resource moving `old_path` → `new_path`, as
/// authored from a note in `src_dir` (its directory **after** the move, so a link
/// between two co-moved files computes to itself and is skipped). Two things
/// survive: the authored convention — a vault-root target stays vault-root,
/// anything else is re-relativized against `src_dir` — and a `#fragment` suffix,
/// carried through untouched. Shared by the resource move and the folder move.
fn resource_replacement(authored: &str, old_path: &str, new_path: &str, src_dir: &str) -> String {
    let (base, fragment) = match authored.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (authored, None),
    };
    let new_base = if base.trim() == old_path {
        new_path.to_string() // authored vault-root — keep it vault-root
    } else {
        relativize(src_dir, new_path) // authored note-relative — keep it relative
    };
    match fragment {
        Some(f) => format!("{new_base}#{f}"),
        None => new_base,
    }
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
    let detail = db::resource_detail(conn, old_rel)?
        .ok_or_else(|| Error::ResourceNotFound(old_rel.to_string()))?;
    let mtime = fs::metadata(new_abs)
        .ok()
        .as_ref()
        .and_then(ingest::unix_mtime);
    let class = crate::resource::ResourceClass::of_path(new_rel)
        .map(|c| c.as_str().to_string())
        .unwrap_or_else(|| "binary".to_string());
    db::upsert_resource(
        conn,
        &db::ResourceRow {
            path: new_rel,
            class: &class,
            size: detail.size,
            mtime,
            content_hash: &detail.content_hash,
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

/// Move/rename the whole directory `from_input` to `to_input`. One `fs::rename` moves
/// the directory — so **unindexed** files inside travel too — after every inbound link
/// at the moved set is rewritten, exactly as the per-file moves do:
///
/// - wikilinks are vault-root-anchored, so links *between* co-moved notes are rewritten
///   just like links from outside the set;
/// - note-relative Markdown targets between co-moved files survive unchanged (a computed
///   replacement equal to the authored text is skipped);
/// - after the rename every moved note's `notes.path` is repointed **first**, then each
///   moved file re-projects — so path-based link resolution never depends on
///   re-projection order, the same reason full ingest is two-phase.
///
/// Re-projection re-embeds only genuinely rewritten bodies, but still requires the real
/// embedder. Errors: [`Error::DirNotFound`], [`Error::MoveDestination`] (including a
/// destination inside the moved folder), [`Error::MoveTargetExists`] rather than merge
/// (with the case-only-rename carve-out on case-insensitive filesystems).
pub fn move_dir(ctx: EmbedCtx, from_input: &str, to_input: &str) -> Result<DirMoveReport> {
    let (conn, root) = (ctx.proj.conn, ctx.proj.root);
    let from = crate::pathspec::normalize_rel_dir(from_input).map_err(Error::MoveDestination)?;
    let to = crate::pathspec::normalize_rel_dir(to_input).map_err(Error::MoveDestination)?;
    refuse_same_path(&to, &from, "folder")?;
    if to.strip_prefix(&from).is_some_and(|r| r.starts_with('/')) {
        return Err(Error::MoveDestination(format!(
            "{to} is inside the folder being moved"
        )));
    }
    let old_abs = root.join(&from);
    let new_abs = root.join(&to);
    if !old_abs.is_dir() {
        return Err(Error::DirNotFound(from));
    }
    refuse_occupied(&old_abs, &new_abs, &to)?;

    let moved_notes = db::notes_under_dir(conn, &from)?;
    let moved_resources = db::resources_under_dir(conn, &from)?;

    // Build each inbound file's target->replacement maps. Wikilink note targets are
    // vault-root-anchored; Markdown resource targets are convention-preserving,
    // relativized against the source's **post-move** directory so inside-to-inside
    // relative links become no-ops. Two maps per file because the two syntaxes rewrite
    // through different passes.
    let mut wiki_by_file = ByFile::new();
    let mut md_by_file = ByFile::new();

    // Every inbound source of a moved note re-projects, rewritten or not — same
    // reason as `move_note`: its `edges.dst_path` names a path that is about to
    // change and no FK cascades it.
    let mut inbound_notes: BTreeSet<String> = BTreeSet::new();
    for old_path in &moved_notes {
        let new_path = remap_prefix(old_path, &from, &to);
        for e in db::inbound_edge_targets(conn, old_path)? {
            inbound_notes.insert(e.src_path.clone());
            let replacement = wiki_replacement(&new_path, &e.dst_raw);
            if replacement != e.dst_raw {
                wiki_by_file
                    .entry(e.src_path)
                    .or_default()
                    .insert(e.dst_raw, replacement);
            }
        }
    }
    for old_path in &moved_resources {
        let new_path = remap_prefix(old_path, &from, &to);
        for e in db::inbound_resource_edge_targets(conn, old_path)? {
            // The source's directory *after* the move — sources inside the moved
            // set remap; outside sources keep their dir.
            let src_dir_after = parent_dir(&remap_prefix(&e.src_path, &from, &to));
            let replacement = resource_replacement(&e.dst_raw, old_path, &new_path, &src_dir_after);
            if replacement != e.dst_raw {
                wiki_by_file
                    .entry(e.src_path.clone())
                    .or_default()
                    .insert(e.dst_raw.clone(), replacement.clone());
                md_by_file
                    .entry(e.src_path)
                    .or_default()
                    .insert(e.dst_raw, replacement);
            }
        }
    }

    // 1. Markdown first: rewrite each inbound file in place at its pre-move path.
    let (rewrote_old_paths, links_rewritten) = rewrite_inbound(root, &wiki_by_file, &md_by_file)?;

    // 2. One rename moves the whole directory (unindexed files travel for free),
    //    creating any missing destination parents.
    rename_with_parents(&old_abs, &new_abs)?;

    // 3. Repoint the resolver before any re-projection: every moved note's path
    //    (old and new sets are disjoint — the destination didn't exist — so the
    //    UNIQUE(path) constraint can't trip), then every moved resource's
    //    inventory row (so resource links resolve at their new paths too).
    for old_path in &moved_notes {
        db::repoint_note_path(conn, old_path, &remap_prefix(old_path, &from, &to))?;
    }
    for old_path in &moved_resources {
        let new_path = remap_prefix(old_path, &from, &to);
        repoint_resource_row(conn, old_path, &new_path, &root.join(&new_path))?;
    }

    // 4. Re-project from the now-current Markdown: every moved note (refreshes
    //    the filename-derived title, mtime, and its outbound edges — an unchanged
    //    body reuses its vectors), then every touched file outside the moved
    //    set (moved ones were just re-projected at their new paths). "Touched" is
    //    the union of the rewritten files and the inbound sources of moved notes,
    //    which differ for a Markdown-form link at a note (see `move_note`).
    let moved_note_old_paths: BTreeSet<&str> = moved_notes.iter().map(String::as_str).collect();
    for old_path in &moved_notes {
        let new_path = remap_prefix(old_path, &from, &to);
        ingest::ingest_file(ctx, &new_path)?;
    }
    let touched: BTreeSet<&str> = rewrote_old_paths
        .iter()
        .map(String::as_str)
        .chain(inbound_notes.iter().map(String::as_str))
        .collect();
    for src_path in touched {
        if moved_note_old_paths.contains(src_path) {
            continue;
        }
        ingest::ingest_file(ctx, src_path)?;
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

    // --- the replacement rules, direct (GH #134) ------------------------------
    //
    // Both were computed inline in two places each before the extraction, so the
    // only cover they had was through a whole-vault move; the `#fragment` rule had
    // none at all. They are pure functions, so they get pinned here.

    #[test]
    fn a_wikilink_keeps_its_own_md_convention() {
        // Obsidian's bare form stays bare; an authored `.md` keeps its `.md`. Both
        // land on the same note — the convention is the *link's*, not the vault's.
        assert_eq!(
            wiki_replacement("archive/memory.md", "concepts/memory"),
            "archive/memory"
        );
        assert_eq!(
            wiki_replacement("archive/memory.md", "concepts/memory.md"),
            "archive/memory.md"
        );
    }

    #[test]
    fn a_resource_link_keeps_its_convention_and_its_fragment() {
        // Authored vault-root (the target equals the resource's vault path) → stays
        // vault-root, whoever links it.
        assert_eq!(
            resource_replacement(
                "assets/plan.pdf",
                "assets/plan.pdf",
                "docs/plan.pdf",
                "notes"
            ),
            "docs/plan.pdf"
        );
        // Authored note-relative → re-relativized against the linking note's dir.
        assert_eq!(
            resource_replacement(
                "../assets/plan.pdf",
                "assets/plan.pdf",
                "docs/plan.pdf",
                "notes"
            ),
            "../docs/plan.pdf"
        );
        // A `#fragment` survives verbatim on both routes — it addresses a place
        // *inside* the resource, which a move never touches.
        assert_eq!(
            resource_replacement(
                "assets/plan.pdf#page=3",
                "assets/plan.pdf",
                "docs/plan.pdf",
                "notes"
            ),
            "docs/plan.pdf#page=3"
        );
        assert_eq!(
            resource_replacement(
                "../assets/plan.pdf#page=3",
                "assets/plan.pdf",
                "docs/plan.pdf",
                "notes"
            ),
            "../docs/plan.pdf#page=3"
        );
    }

    #[test]
    fn a_relative_link_between_co_moved_files_computes_to_itself() {
        // The folder move passes the source's **post-move** directory, so a link from
        // `dir/note.md` to `dir/img.png` is unchanged by moving `dir/` — and an
        // unchanged replacement is what `move_dir` skips, leaving the file untouched.
        assert_eq!(
            resource_replacement("img.png", "dir/img.png", "moved/img.png", "moved"),
            "img.png"
        );
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
