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
