//! Move / rename a note, a resource or a folder, and repair inbound links.
//!
//! Identity is the path (ADR-0003, L1/L3), so a move re-keys: it rewrites the link text in
//! every file linking at a moved member, repoints the index rows (`ON UPDATE CASCADE`
//! carries chunks, centroid and outbound edges), and re-projects the inbound sources,
//! whose `edges.dst_path` has no FK. Vectors are untouched (ADR-0006).
//!
//! Every move builds a [`MoveSet`] and runs [`execute`]: Markdown first, then re-project.
//! Cost is O(inbound links) via [`db::inbound_edges_of`]. Known gap: rewrites are keyed by
//! a link's written text, so two same-text links resolving to different files in one
//! inbound file are both rewritten.
//!
//! The vault half is all or nothing (GH #230), since a reindex can't repair link text: the
//! destination is checked and every rewrite computed before the first write, then
//! [`commit`] undoes itself on failure. A crash between the first rewrite and the rename
//! is the one unguarded window; re-running the move finishes it.

use crate::db::{self, InboundTarget};
use crate::error::{Error, Result};
use crate::ingest::{self, EmbedCtx};
use crate::link::{self, LinkForm};
use crate::pathspec;
use crate::resource::ResourceClass;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What [`move_note`] did. `to` is the note's identity after the move (L1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoveReport {
    pub from: String,
    pub to: String,
    /// Post-move paths of the files whose link text was rewritten (sorted, deduped),
    /// including the moved note itself when it self-links.
    pub rewrote: Vec<String>,
    /// Total individual `[[…]]` link targets rewritten across `rewrote`.
    pub links_rewritten: usize,
}

/// Move the note at `old_rel` to `new_rel_input` (`.md` optional), rewriting inbound
/// `[[…]]` links and re-keying the index. Inbound files re-embed, so the caller needs the
/// index's embedder. A Markdown-form link at a note keeps its text; its source is still
/// re-projected, so its edge matches what a rebuild would project.
pub fn move_note(ctx: EmbedCtx, old_rel: &str, new_rel_input: &str) -> Result<MoveReport> {
    let new_rel = pathspec::normalize_rel_md(new_rel_input).map_err(Error::MoveDestination)?;
    refuse_same_path(&new_rel, old_rel, "note")?;
    let set = MoveSet {
        notes: vec![(old_rel.to_string(), new_rel.clone())],
        ..MoveSet::rename(old_rel, &new_rel)
    };
    let moved = execute(ctx, &set)?;
    Ok(MoveReport {
        from: old_rel.to_string(),
        to: new_rel,
        rewrote: moved.rewrote,
        links_rewritten: moved.links_rewritten,
    })
}

/// What [`move_resource`] did. Same fields as [`MoveReport`] (GH #170), kept separate as
/// a separate contract (data-model.md §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceMoveReport {
    pub from: String,
    pub to: String,
    /// Inbound notes whose link text was rewritten (sorted, deduped).
    pub rewrote: Vec<String>,
    /// Total individual link targets rewritten across `rewrote`.
    pub links_rewritten: usize,
}

/// Move the resource at `old_rel` to `new_rel_input`, rewriting inbound links in both
/// syntaxes (each keeping its relative-vs-root convention). Path-only; the bytes are never
/// touched. A `.md` destination is refused: a rebuild would index it as a note.
pub fn move_resource(
    ctx: EmbedCtx,
    old_rel: &str,
    new_rel_input: &str,
) -> Result<ResourceMoveReport> {
    let new_rel = pathspec::normalize_rel(new_rel_input).map_err(Error::MoveDestination)?;
    refuse_same_path(&new_rel, old_rel, "resource")?;
    let Some(class) = ResourceClass::of_path(&new_rel) else {
        return Err(Error::MoveDestination(format!(
            "{new_rel} is a note path; a resource keeps a non-.md extension"
        )));
    };
    let set = MoveSet {
        resources: vec![ResourceMove {
            from: old_rel.to_string(),
            to: new_rel.clone(),
            class,
        }],
        ..MoveSet::rename(old_rel, &new_rel)
    };
    let moved = execute(ctx, &set)?;
    Ok(ResourceMoveReport {
        from: old_rel.to_string(),
        to: new_rel,
        rewrote: moved.rewrote,
        links_rewritten: moved.links_rewritten,
    })
}

/// What [`move_dir`] did. The counts are of indexed members; unindexed files travel too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirMoveReport {
    pub from: String,
    pub to: String,
    pub moved_notes: usize,
    pub moved_resources: usize,
    /// Post-move paths of the files whose link text was rewritten (sorted, deduped).
    pub rewrote: Vec<String>,
    /// Total individual link targets rewritten across `rewrote`.
    pub links_rewritten: usize,
}

/// Move/rename the directory `from_input` to `to_input` with one `fs::rename`, after
/// rewriting inbound links as the per-file moves do. Wikilinks between co-moved notes are
/// rewritten (they are vault-root); note-relative Markdown links between them compute to
/// themselves and are skipped. Requires the real embedder.
pub fn move_dir(ctx: EmbedCtx, from_input: &str, to_input: &str) -> Result<DirMoveReport> {
    let conn = ctx.proj.conn;
    let from = pathspec::normalize_rel_dir(from_input).map_err(Error::MoveDestination)?;
    let to = pathspec::normalize_rel_dir(to_input).map_err(Error::MoveDestination)?;
    refuse_same_path(&to, &from, "folder")?;
    if pathspec::is_under(&to, &from) {
        return Err(Error::MoveDestination(format!(
            "{to} is inside the folder being moved"
        )));
    }
    if !ctx.proj.root.join(&from).is_dir() {
        return Err(Error::DirNotFound(from));
    }

    // A resource keeps its file name, so its class from the new path is its old one.
    let rebased = |old: String| pathspec::rebase(&old, &from, &to).map(|new| (old, new));
    let mut notes: Vec<(String, String)> = db::notes_under_dir(conn, &from)?
        .into_iter()
        .filter_map(&rebased)
        .collect();
    notes.sort(); // `MoveSet::notes` is looked up by old path
    let resources: Vec<ResourceMove> = db::resources_under_dir(conn, &from)?
        .into_iter()
        .filter_map(&rebased)
        .filter_map(|(from, to)| {
            let class = ResourceClass::of_path(&to)?;
            Some(ResourceMove { from, to, class })
        })
        .collect();
    let set = MoveSet {
        notes,
        resources,
        ..MoveSet::rename(&from, &to)
    };
    let moved = execute(ctx, &set)?;

    Ok(DirMoveReport {
        moved_notes: set.notes.len(),
        moved_resources: set.resources.len(),
        from,
        to,
        rewrote: moved.rewrote,
        links_rewritten: moved.links_rewritten,
    })
}

// --- the one move pipeline (GH #134) ---------------------------------------------
//
// Each op's own refusals run before `execute`'s shared ones; that precedence is part of
// each op's contract.

/// One move: the single on-disk rename, and the indexed members travelling with it.
#[derive(Debug, Default)]
struct MoveSet {
    from: String,
    to: String,
    /// Each moved note's `(old path, new path)`, sorted by old path.
    notes: Vec<(String, String)>,
    /// Each moved resource, with its class at the new path.
    resources: Vec<ResourceMove>,
}

/// One resource travelling with a [`MoveSet`].
#[derive(Debug)]
struct ResourceMove {
    from: String,
    to: String,
    class: ResourceClass,
}

impl MoveSet {
    fn rename(from: &str, to: &str) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            ..Self::default()
        }
    }

    /// Where the note at `path` is after the move.
    fn after<'a>(&'a self, path: &'a str) -> &'a str {
        match self
            .notes
            .binary_search_by(|(old, _)| old.as_str().cmp(path))
        {
            Ok(i) => &self.notes[i].1,
            Err(_) => path,
        }
    }

    fn moves_note(&self, path: &str) -> bool {
        self.notes
            .binary_search_by(|(old, _)| old.as_str().cmp(path))
            .is_ok()
    }
}

/// What [`execute`] did: rewritten files at post-move paths (sorted), and the link count.
#[derive(Debug)]
struct Moved {
    rewrote: Vec<String>,
    links_rewritten: usize,
}

/// Run one move: refuse, plan, commit (all or nothing), re-key, re-project.
fn execute(ctx: EmbedCtx, set: &MoveSet) -> Result<Moved> {
    let (conn, root) = (ctx.proj.conn, ctx.proj.root);
    let old_abs = root.join(&set.from);
    let new_abs = root.join(&set.to);
    refuse_occupied(&old_abs, &new_abs, &set.to)?;
    refuse_file_ancestor(root, &set.to)?;

    // Per-file, per-syntax target→replacement maps. A note is rewritten in its `[[…]]`
    // form only; a resource in both, re-relativized against the source's post-move folder
    // so a link between co-moved files computes to itself.
    let note_paths: Vec<&str> = set.notes.iter().map(|(old, _)| old.as_str()).collect();
    let resource_paths: Vec<&str> = set.resources.iter().map(|r| r.from.as_str()).collect();
    let (mut wiki, mut md) = (ByFile::new(), ByFile::new());
    // Every inbound source is re-projected, text changed or not: `edges.dst_path` has no
    // FK to cascade (it must be free to dangle, G5).
    let mut sources = BTreeSet::new();
    for (target, e) in db::inbound_edges_of(conn, &note_paths, &resource_paths)? {
        let (replacement, both_forms) = match target {
            InboundTarget::Note(i) => (wiki_replacement(&set.notes[i].1, &e.dst_raw), false),
            InboundTarget::Resource(i) => {
                let r = &set.resources[i];
                let src_dir = pathspec::parent_dir(set.after(&e.src_path));
                let replacement = resource_replacement(&e.dst_raw, &r.from, &r.to, src_dir);
                (replacement, true)
            }
        };
        if replacement != e.dst_raw {
            if both_forms {
                md.entry(e.src_path.clone())
                    .or_default()
                    .insert(e.dst_raw.clone(), replacement.clone());
            }
            wiki.entry(e.src_path.clone())
                .or_default()
                .insert(e.dst_raw, replacement);
        }
        sources.insert(e.src_path);
    }

    // 1. Markdown first: rewrite inbound links at the pre-move paths, then rename.
    let plan = plan_inbound(root, &wiki, &md)?;
    commit(root, &plan.rewrites, &old_abs, &new_abs)?;

    // 2. Re-key before re-projecting, so resolution is order-independent. Old and new
    //    paths are disjoint, so UNIQUE(path) can't trip.
    for (old, new) in &set.notes {
        db::repoint_note_path(conn, old, new)?;
    }
    for r in &set.resources {
        let mtime = fs::metadata(root.join(&r.to))
            .ok()
            .as_ref()
            .and_then(ingest::unix_mtime);
        if !db::repoint_resource(conn, &r.from, &r.to, r.class.as_str(), mtime)? {
            // Not inventoried (an out-of-band change): inventory it where it now is.
            ingest::project_resource_file(conn, root, &r.to, r.class, true)?;
        }
    }

    // 3. Re-project moved notes at their new paths, then inbound sources that stayed.
    for (_, new) in &set.notes {
        ingest::ingest_file(ctx, new)?;
    }
    for src in sources.iter().filter(|src| !set.moves_note(src)) {
        ingest::ingest_file(ctx, src)?;
    }

    let mut rewrote: Vec<String> = plan
        .rewrites
        .iter()
        .map(|r| set.after(&r.rel).to_string())
        .collect();
    rewrote.sort();
    Ok(Moved {
        rewrote,
        links_rewritten: plan.links_rewritten,
    })
}

/// Each inbound file's target → replacement map. `BTreeMap` keeps the order deterministic.
type ByFile = BTreeMap<String, Targets>;

/// One file's authored-target → replacement map for one link syntax.
type Targets = BTreeMap<String, String>;

/// Refuse a destination equal to the source. `subject` names the thing moved, for the message.
fn refuse_same_path(new_rel: &str, old_rel: &str, subject: &str) -> Result<()> {
    if new_rel == old_rel {
        return Err(Error::MoveDestination(format!(
            "{new_rel} is the {subject}'s current path"
        )));
    }
    Ok(())
}

/// Refuse an occupied destination rather than clobber it (data-model.md §1), except a
/// case-only rename on a case-insensitive filesystem ([`is_same_dirent`]).
fn refuse_occupied(old_abs: &Path, new_abs: &Path, new_rel: &str) -> Result<()> {
    if new_abs.exists() && !is_same_dirent(old_abs, new_abs) {
        return Err(Error::MoveTargetExists(new_rel.to_string()));
    }
    Ok(())
}

/// Refuse a destination beneath a regular file, before any rewrite (GH #230). Stops at the
/// first missing ancestor; the move creates everything below it.
fn refuse_file_ancestor(vault_root: &Path, new_rel: &str) -> Result<()> {
    for (i, _) in new_rel.match_indices('/') {
        let dir = &new_rel[..i];
        match fs::metadata(vault_root.join(dir)) {
            Ok(meta) if !meta.is_dir() => {
                return Err(Error::MoveDestination(format!(
                    "{dir} is a file, not a folder"
                )))
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(())
}

/// One inbound file's planned rewrite. `original` lets [`commit`] put the file back.
#[derive(Debug)]
struct Rewrite {
    rel: String,
    abs: PathBuf,
    original: String,
    rewritten: String,
}

/// Every inbound rewrite a move will make, computed before the first write.
#[derive(Debug)]
struct Plan {
    /// Sorted by path; only files whose passes change something.
    rewrites: Vec<Rewrite>,
    links_rewritten: usize,
}

/// Read each inbound file and compute its rewrite, writing nothing. An unreadable file
/// fails the move here, while the vault is untouched.
fn plan_inbound(vault_root: &Path, wiki: &ByFile, md: &ByFile) -> Result<Plan> {
    let none = Targets::new();
    let mut rewrites = Vec::new();
    let mut links_rewritten = 0usize;
    let touched: BTreeSet<&str> = wiki.keys().chain(md.keys()).map(String::as_str).collect();
    for src_path in touched {
        let abs = vault_root.join(src_path);
        let original = fs::read_to_string(&abs)?;
        let (rewritten, n) = rewrite_targets(
            &original,
            wiki.get(src_path).unwrap_or(&none),
            md.get(src_path).unwrap_or(&none),
        );
        if n > 0 {
            rewrites.push(Rewrite {
                rel: src_path.to_string(),
                abs,
                original,
                rewritten,
            });
            links_rewritten += n;
        }
    }
    Ok(Plan {
        rewrites,
        links_rewritten,
    })
}

/// Replace each link target in `raw` found in its syntax's map (`wiki` for `[[…]]`, `md`
/// for `[…](…)`). Only the target token changes. Uses [`link::link_spans`], the scanner
/// ingest uses. Returns the text and the count replaced.
fn rewrite_targets(raw: &str, wiki: &Targets, md: &Targets) -> (String, usize) {
    let mut out = String::with_capacity(raw.len());
    let (mut copied, mut count) = (0usize, 0usize);
    for span in link::link_spans(raw) {
        let targets = match span.form {
            LinkForm::Wiki => wiki,
            LinkForm::Markdown => md,
        };
        let Some(replacement) = targets.get(&raw[span.target.clone()]) else {
            continue;
        };
        out.push_str(&raw[copied..span.target.start]);
        out.push_str(replacement);
        copied = span.target.end;
        count += 1;
    }
    out.push_str(&raw[copied..]);
    (out, count)
}

/// One vault change [`commit`] must undo if a later step fails.
#[derive(Debug)]
enum Done {
    /// `rewrites[i]` was opened for writing (and so truncated).
    Wrote(usize),
    /// A destination folder that did not exist before the move.
    CreatedDir(PathBuf),
}

/// Apply a move's vault writes as one unit: rewrites, missing folders, then the rename
/// (last, so success leaves nothing to undo). On failure, undo in reverse. If the undo
/// fails too, [`Error::MoveIncomplete`] names the files still holding a rewrite.
fn commit(vault_root: &Path, rewrites: &[Rewrite], old_abs: &Path, new_abs: &Path) -> Result<()> {
    let mut done = Vec::new();
    let Err(err) = apply(rewrites, old_abs, new_abs, &mut done) else {
        return Ok(());
    };
    let unrestored = undo(vault_root, rewrites, &done);
    if unrestored.is_empty() {
        return Err(err);
    }
    Err(Error::MoveIncomplete {
        paths: unrestored,
        source: Box::new(err),
    })
}

/// [`commit`]'s forward half, recording each change in `done` as it happens.
fn apply(rewrites: &[Rewrite], old_abs: &Path, new_abs: &Path, done: &mut Vec<Done>) -> Result<()> {
    for (i, r) in rewrites.iter().enumerate() {
        // Open (which truncates) before recording: a file that won't open is untouched.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&r.abs)?;
        done.push(Done::Wrote(i));
        file.write_all(r.rewritten.as_bytes())?;
    }
    // Created outermost first, each recorded so the undo removes exactly these.
    let mut missing: Vec<&Path> = new_abs
        .ancestors()
        .skip(1)
        .take_while(|dir| !dir.exists())
        .collect();
    missing.reverse();
    for dir in missing {
        fs::create_dir(dir)?;
        done.push(Done::CreatedDir(dir.to_path_buf()));
    }
    fs::rename(old_abs, new_abs)?;
    Ok(())
}

/// [`commit`]'s undo. Returns the files that could not be restored; a folder that won't
/// go is only logged, as it is empty.
fn undo(vault_root: &Path, rewrites: &[Rewrite], done: &[Done]) -> Vec<String> {
    let mut unrestored = Vec::new();
    for step in done.iter().rev() {
        match step {
            Done::Wrote(i) => {
                let Some(r) = rewrites.get(*i) else { continue };
                if fs::write(&r.abs, r.original.as_bytes()).is_err() {
                    unrestored.push(r.rel.clone());
                }
            }
            Done::CreatedDir(dir) => {
                if let Err(e) = fs::remove_dir(dir) {
                    let rel = dir.strip_prefix(vault_root).unwrap_or(dir);
                    tracing::warn!(
                        target: "b2::mv",
                        folder = %rel.display(),
                        error = %e,
                        "could not remove a folder created by a failed move"
                    );
                }
            }
        }
    }
    unrestored.sort();
    unrestored
}

/// Split a link target at its first `#`. A move carries the fragment through verbatim.
fn split_fragment(authored: &str) -> (&str, Option<&str>) {
    match authored.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (authored, None),
    }
}

fn with_fragment(base: &str, fragment: Option<&str>) -> String {
    match fragment {
        Some(f) => format!("{base}#{f}"),
        None => base.to_string(),
    }
}

/// The new wikilink target for a note at `new_path`, keeping the link's `.md`-or-not
/// convention and fragment. Only a lowercase `.md` is dropped, the suffix the resolver
/// adds back.
fn wiki_replacement(new_path: &str, authored: &str) -> String {
    let (base, fragment) = split_fragment(authored);
    let new_base = if base.ends_with(".md") {
        new_path
    } else {
        new_path.strip_suffix(".md").unwrap_or(new_path)
    };
    with_fragment(new_base, fragment)
}

/// The new target for a link at a moved resource, from a note in `src_dir` (post-move).
/// A vault-root target stays vault-root; anything else is re-relativized. Keeps the fragment.
fn resource_replacement(authored: &str, old_path: &str, new_path: &str, src_dir: &str) -> String {
    let (base, fragment) = split_fragment(authored);
    let new_base = if base.trim() == old_path {
        new_path.to_string()
    } else {
        pathspec::relativize(src_dir, new_path)
    };
    with_fragment(&new_base, fragment)
}

/// Whether `a` and `b` are the same directory entry: a case-only rename on a
/// case-insensitive filesystem (APFS default). Any canonicalize error means "not the same".
fn is_same_dirent(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(pairs: &[(&str, &str)]) -> Targets {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// The note move's rewrite: `[[…]]` targets only.
    fn rewrite_links(raw: &str, t: &Targets) -> (String, usize) {
        rewrite_targets(raw, t, &Targets::new())
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
    fn each_syntax_is_rewritten_from_its_own_map() {
        let wiki = targets(&[("img.png", "media/img.png")]);
        let md = targets(&[("img.png", "../media/img.png")]);
        let (out, n) = rewrite_targets("![[img.png|cap]] and ![alt]( img.png )\n", &wiki, &md);
        assert_eq!(
            out,
            "![[media/img.png|cap]] and ![alt]( ../media/img.png )\n"
        );
        assert_eq!(n, 2);
        let (out, n) = rewrite_targets("![alt](img.png)", &wiki, &Targets::new());
        assert_eq!((out.as_str(), n), ("![alt](img.png)", 0));
    }

    /// The move uses ingest's scanner, so a stray `[[` can't hide the next line's link.
    #[test]
    fn a_stray_open_bracket_does_not_hide_the_next_lines_link() {
        let t = targets(&[("old", "new")]);
        let (out, n) = rewrite_links("broken [[x\nsee [[old]]\n", &t);
        assert_eq!(out, "broken [[x\nsee [[new]]\n");
        assert_eq!(n, 1);
    }

    // --- the replacement rules, direct (GH #134) ------------------------------

    #[test]
    fn a_wikilink_keeps_its_own_md_convention() {
        // The convention is the link's, not the vault's.
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
    fn a_wikilink_keeps_its_heading_fragment() {
        assert_eq!(
            wiki_replacement("archive/memory.md", "concepts/memory#Recall"),
            "archive/memory#Recall"
        );
        assert_eq!(
            wiki_replacement("archive/memory.md", "concepts/memory.md#Recall"),
            "archive/memory.md#Recall"
        );
    }

    #[test]
    fn a_resource_link_keeps_its_convention_and_its_fragment() {
        // Authored vault-root stays vault-root.
        assert_eq!(
            resource_replacement(
                "assets/plan.pdf",
                "assets/plan.pdf",
                "docs/plan.pdf",
                "notes"
            ),
            "docs/plan.pdf"
        );
        // Authored note-relative is re-relativized.
        assert_eq!(
            resource_replacement(
                "../assets/plan.pdf",
                "assets/plan.pdf",
                "docs/plan.pdf",
                "notes"
            ),
            "../docs/plan.pdf"
        );
        // A `#fragment` survives on both routes.
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
        // `src_dir` is the post-move directory, so the link is unchanged and skipped.
        assert_eq!(
            resource_replacement("img.png", "dir/img.png", "moved/img.png", "moved"),
            "img.png"
        );
    }

    #[test]
    fn a_set_maps_its_moved_notes_and_leaves_every_other_path() {
        let set = MoveSet {
            notes: vec![
                ("docs/a.md".to_string(), "media/a.md".to_string()),
                ("docs/b.md".to_string(), "media/b.md".to_string()),
            ],
            ..MoveSet::rename("docs", "media")
        };
        assert_eq!(set.after("docs/b.md"), "media/b.md");
        assert_eq!(set.after("hub.md"), "hub.md");
        assert!(set.moves_note("docs/a.md") && !set.moves_note("media/a.md"));
    }

    // --- the all-or-nothing commit (GH #230) ------------------------------------
    //
    // Failures tests/mv.rs can't reach deterministically.

    fn planned(root: &Path, rel: &str, rewritten: &str) -> Rewrite {
        let abs = root.join(rel);
        Rewrite {
            rel: rel.to_string(),
            original: fs::read_to_string(&abs).unwrap_or_default(),
            abs,
            rewritten: rewritten.to_string(),
        }
    }

    #[test]
    fn a_write_failing_after_another_landed_restores_the_first() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("a.md"), "See [[old]].\n").unwrap();
        fs::write(root.join("old.md"), "Target.\n").unwrap();
        // `b.md` is a folder, so opening it for writing fails — after `a.md` is written.
        fs::create_dir(root.join("b.md")).unwrap();
        let rewrites = vec![
            planned(root, "a.md", "See [[new]].\n"),
            planned(root, "b.md", "unused"),
        ];

        let mut done = Vec::new();
        let err = apply(
            &rewrites,
            &root.join("old.md"),
            &root.join("new.md"),
            &mut done,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err:?}");
        assert_eq!(
            fs::read_to_string(root.join("a.md")).unwrap(),
            "See [[new]].\n",
            "the first rewrite landed before the failure"
        );

        assert!(undo(root, &rewrites, &done).is_empty());
        assert_eq!(
            fs::read_to_string(root.join("a.md")).unwrap(),
            "See [[old]].\n"
        );
        assert!(root.join("old.md").exists() && !root.join("new.md").exists());
    }

    #[test]
    fn a_failed_rename_undoes_every_write_and_created_folder() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("a.md"), "See [[old]].\n").unwrap();
        fs::write(root.join("b.md"), "Also [[old|O]].\n").unwrap();
        let rewrites = vec![
            planned(root, "a.md", "See [[x/y/new]].\n"),
            planned(root, "b.md", "Also [[x/y/new|O]].\n"),
        ];

        // No `old.md` on disk: both writes and both folders land, then the rename fails.
        let err = commit(
            root,
            &rewrites,
            &root.join("old.md"),
            &root.join("x/y/new.md"),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err:?}");
        assert_eq!(
            fs::read_to_string(root.join("a.md")).unwrap(),
            "See [[old]].\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("b.md")).unwrap(),
            "Also [[old|O]].\n"
        );
        assert!(!root.join("x").exists(), "the created folders are removed");
    }

    #[test]
    fn an_undo_that_cannot_restore_a_file_names_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        // A "rewritten" path that is now a folder: writing its original back fails.
        fs::create_dir(root.join("gone.md")).unwrap();
        let rewrites = vec![Rewrite {
            rel: "gone.md".to_string(),
            abs: root.join("gone.md"),
            original: "See [[old]].\n".to_string(),
            rewritten: "See [[new]].\n".to_string(),
        }];
        assert_eq!(
            undo(root, &rewrites, &[Done::Wrote(0)]),
            vec!["gone.md".to_string()]
        );
    }

    #[test]
    fn a_file_ancestor_is_refused_and_a_missing_one_is_not() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("dir")).unwrap();
        fs::write(root.join("dir/file"), "x").unwrap();
        assert!(matches!(
            refuse_file_ancestor(root, "dir/file/a.md"),
            Err(Error::MoveDestination(m)) if m == "dir/file is a file, not a folder"
        ));
        assert!(refuse_file_ancestor(root, "dir/new/deeper/a.md").is_ok());
        assert!(refuse_file_ancestor(root, "a.md").is_ok());
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
