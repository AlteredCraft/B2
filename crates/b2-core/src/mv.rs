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
pub fn move_note(
    conn: &Connection,
    idgen: &dyn IdGen,
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
    if new_abs.exists() {
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
    ingest::ingest_file(conn, vault_root, &new_rel, idgen, embedder)?;
    for src_path in &rewrote {
        if src_path == old_rel {
            continue; // the moved note itself (a self-link) — already re-projected
        }
        ingest::ingest_file(conn, vault_root, src_path, idgen, embedder)?;
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
    if new_abs.exists() {
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
    let (_, size, _, content_hash) =
        db::resource_detail(conn, old_rel)?.ok_or_else(|| Error::ResourceNotFound(
            old_rel.to_string(),
        ))?;
    let mtime = fs::metadata(&new_abs)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let class = crate::resource::ResourceClass::of_path(&new_rel)
        .map(|c| c.as_str().to_string())
        .unwrap_or_else(|| "binary".to_string());
    db::upsert_resource(
        conn,
        &db::ResourceRow {
            path: &new_rel,
            class: &class,
            size,
            mtime,
            content_hash: &content_hash,
        },
    )?;
    conn.execute("DELETE FROM resources WHERE path = ?1", [old_rel])?;

    // 4. Re-project the rewritten notes from the now-current Markdown (their
    //    changed chunks re-embed inline, exactly like a note move's inbound set).
    for src_path in &rewrote {
        ingest::ingest_file(conn, vault_root, src_path, idgen, embedder)?;
    }

    Ok(ResourceMoveReport {
        from: old_rel.to_string(),
        to: new_rel,
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
