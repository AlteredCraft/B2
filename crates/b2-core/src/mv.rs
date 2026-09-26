//! Move / rename a note, a resource or a folder, and repair inbound links.
//!
//! **A move is where "rename keeps every backlink resolving" is earned.** Identity is
//! the vault-relative path (ADR-0003, L1/L3), so a move changes the moved member's
//! identity, and this module makes that a re-key rather than a break. Two halves, both
//! bounded by the moved set's backlink count: the human-facing copy — the link text in
//! every file linking *at* a moved member, rewritten in place — and the index: each moved
//! note re-keyed by one [`db::repoint_note_path`] whose `ON UPDATE CASCADE` FKs carry
//! chunks, aliases, centroid and outbound edges atomically, each moved resource by one
//! [`db::repoint_resource`], then a re-projection of the inbound sources so their edges
//! (`edges.dst_path` has no FK — it must be free to dangle) point at the new paths. A
//! moved note's **vectors are not touched at all**: content-addressed (ADR-0006), they
//! belong to the chunk text, which a move does not change.
//!
//! **One pipeline for every move.** `move_note`, `move_resource` and `move_dir` only
//! validate and build a [`MoveSet`] — the one rename, and the indexed members travelling
//! with it — and [`execute`] runs it. It is **Markdown-first**: rewrite the inbound text,
//! rename on disk, *then* re-project from the now-current Markdown. And bounded, not a
//! scan: [`db::inbound_edges_of`] names exactly the files to touch, so the cost is
//! O(inbound links). The link text is found by [`crate::link`]'s own scanner, so a move
//! rewrites exactly the links ingest projected.
//!
//! **The vault half is all or nothing** (GH #230). Reindexing can't repair rewritten
//! link text (it projects whatever the Markdown says), so a failed move must leave every
//! file as it was. Three layers: the destination is checked before anything is written
//! ([`refuse_occupied`], [`refuse_file_ancestor`]); every rewrite is read and computed
//! in memory before the first write ([`plan_inbound`]); and the writes, the folders the
//! move creates and the rename run as one [`commit`] that undoes whatever it did if a
//! later step fails. Once the rename lands the vault is final: a failure re-keying or
//! re-projecting the index leaves correct Markdown, which `b2 reindex` projects.
//!
//! No process can undo a crash, so the one unguarded window is between the first
//! rewrite and the rename: the rewritten links already name the destination, the note
//! is still at its source. Re-running the same move finishes it (the rewrites are
//! already done, so only the rename is left).

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

/// What [`move_note`] did: the note's old and new vault-relative paths, the inbound
/// files whose link text was rewritten, and the total number of `[[…]]` targets
/// repaired across them. `to` is the note's identity after the move (L1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoveReport {
    pub from: String,
    pub to: String,
    /// Post-move vault-relative paths of the files whose link text was rewritten
    /// (sorted, deduped) — the moved note itself under `to`, when it links to itself.
    /// Empty when nothing linked to the moved note.
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
///
/// Only the `[[…]]` form is rewritten at a note. A Markdown-form link at one
/// (`[x](notes/a.md)`) keeps its text, but its source is re-projected like every inbound
/// source, so the edge dangles exactly as a rebuild would project it rather than keep
/// naming a path that no longer exists.
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
/// inbound notes. B2 never touches the resource's bytes; the move is path-only. Errors
/// mirror [`move_note`] (the façade owns [`Error::ResourceNotFound`]), plus
/// [`Error::MoveDestination`] for a `.md` destination: that path names a note, so the
/// file would stop being this resource (and a rebuild would index it as a note).
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

    // Every indexed member under the folder travels to the same place under `to`. A
    // resource keeps its file name, so its class (from the new path) is its old one.
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
// A move is always the same steps — refuse a bad destination, rewrite the inbound files'
// link text, rename on disk, re-key, re-project — whatever travels. Each op's own
// refusals (the destination's shape, a folder moved into itself) run before `execute`,
// whose shared refusals (an occupied destination, a file where a folder must be) follow:
// that precedence is part of each op's contract.

/// Everything one move carries: the one rename that moves it on disk (a file, or the
/// folder holding every member), and the **indexed** members travelling with it.
#[derive(Debug, Default)]
struct MoveSet {
    /// The rename's vault-relative source and destination.
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
    /// A set renaming `from` → `to` with no members yet.
    fn rename(from: &str, to: &str) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            ..Self::default()
        }
    }

    /// Where the note at `path` is after the move — its new path if it travels,
    /// otherwise `path` itself.
    fn after<'a>(&'a self, path: &'a str) -> &'a str {
        match self
            .notes
            .binary_search_by(|(old, _)| old.as_str().cmp(path))
        {
            Ok(i) => &self.notes[i].1,
            Err(_) => path,
        }
    }

    /// Whether the note at `path` travels with this set.
    fn moves_note(&self, path: &str) -> bool {
        self.notes
            .binary_search_by(|(old, _)| old.as_str().cmp(path))
            .is_ok()
    }
}

/// What [`execute`] did: the rewritten files at their post-move paths (sorted), and
/// how many link targets it rewrote across them.
#[derive(Debug)]
struct Moved {
    rewrote: Vec<String>,
    links_rewritten: usize,
}

/// Run one move: refuse an occupied or unreachable destination, plan every inbound
/// rewrite, commit the rewrites and the rename (all or nothing), re-key every moved
/// member, then re-project from the now-current Markdown.
fn execute(ctx: EmbedCtx, set: &MoveSet) -> Result<Moved> {
    let (conn, root) = (ctx.proj.conn, ctx.proj.root);
    let old_abs = root.join(&set.from);
    let new_abs = root.join(&set.to);
    refuse_occupied(&old_abs, &new_abs, &set.to)?;
    refuse_file_ancestor(root, &set.to)?;

    // The graph names the bounded inbound set: for each edge pointing at a moved
    // member, its source file and the exact link text (`dst_path_raw`) written there.
    // Group by file into per-syntax target→replacement maps. A note is rewritten in
    // its `[[…]]` form only, each link keeping its own `.md`-or-not convention; a
    // resource is linked as `![[img.png]]` *or* `![](img.png)`, so its replacement
    // feeds both, re-relativized against the source's **post-move** folder so a
    // relative link between two co-moved files computes to itself and is skipped.
    let note_paths: Vec<&str> = set.notes.iter().map(|(old, _)| old.as_str()).collect();
    let resource_paths: Vec<&str> = set.resources.iter().map(|r| r.from.as_str()).collect();
    let (mut wiki, mut md) = (ByFile::new(), ByFile::new());
    // Every inbound source is re-projected below, whether or not its *text* changes:
    // its edges name paths that are about to change, and `edges.dst_path` carries no FK
    // to cascade (it must be free to be NULL — the dangling case, G5).
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

    // 1. Markdown first: rewrite inbound link text in place at the pre-move paths (a
    //    self-link or a co-moved linker included), then one rename moves the note,
    //    resource or folder — unindexed files in a folder travel for free — all or
    //    nothing.
    let plan = plan_inbound(root, &wiki, &md)?;
    commit(root, &plan.rewrites, &old_abs, &new_abs)?;

    // 2. Re-key the index before anything re-projects, so path-based link resolution
    //    is independent of re-projection order (the same reason full ingest is
    //    two-phase). Old and new paths are disjoint — the destination didn't exist — so
    //    the UNIQUE(path) constraints can't trip.
    for (old, new) in &set.notes {
        db::repoint_note_path(conn, old, new)?;
    }
    for r in &set.resources {
        let mtime = fs::metadata(root.join(&r.to))
            .ok()
            .as_ref()
            .and_then(ingest::unix_mtime);
        if !db::repoint_resource(conn, &r.from, &r.to, r.class.as_str(), mtime)? {
            // Not inventoried after all (an out-of-band change the index hasn't seen):
            // inventory the file where it now is, as a reindex would.
            ingest::project_resource_file(conn, root, &r.to, r.class, true)?;
        }
    }

    // 3. Re-project from the now-current Markdown: every moved note at its new path
    //    (refreshing its filename-derived title, mtime and outbound edges — an
    //    unchanged body reuses its vectors), then every inbound source that stayed put.
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

/// Each inbound file's authored-target → replacement map, keyed by the file's
/// vault-relative path. `BTreeMap` throughout, so the rewrite order — and the
/// `rewrote` list every report carries — is sorted and deterministic.
type ByFile = BTreeMap<String, Targets>;

/// One file's authored-target → replacement map for one link syntax.
type Targets = BTreeMap<String, String>;

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

/// Refuse a destination beneath a regular file (`blocked/a.md` when `blocked` is a
/// file): no rename can land there, and finding out *after* the inbound rewrites is
/// exactly the broken-links failure GH #230 reported. Walks the destination's
/// ancestors from the vault root down and stops at the first missing one — the
/// move creates everything below it.
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

/// One inbound file's planned rewrite: the bytes it holds now and the bytes it will
/// hold. `original` is kept so [`commit`] can put the file back.
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

/// Markdown first, for every move: read each inbound file and compute its rewritten
/// link text, writing nothing. `wiki` holds each file's `[[…]]` replacements and `md`
/// its `[…](…)` ones; a file whose rewrite changes nothing is left out of the plan. A
/// file that can't be read fails the move here, while the vault is still untouched.
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

/// Rewrite every link in `raw` whose trimmed target is a key of its syntax's map —
/// `wiki` for `[[…]]`/`![[…]]`, `md` for `[…](…)`/`![…](…)` — to that key's replacement.
/// Only the target token changes: every other byte (the brackets, the `|alias`, the
/// link text, whitespace around the target) is preserved, and a target merely sharing a
/// prefix with a key is never touched. The links are the ones [`link::link_spans`]
/// finds — the scanner ingest projects edges from — so a move rewrites exactly the links
/// that point at it. Returns the rewritten text and the count of targets replaced.
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

/// One vault change [`commit`] made, and so must undo if a later step fails.
#[derive(Debug)]
enum Done {
    /// `rewrites[i]` was opened for writing (and so truncated).
    Wrote(usize),
    /// A destination folder that did not exist before the move.
    CreatedDir(PathBuf),
}

/// Apply a move's vault writes as one unit: write every planned rewrite, create any
/// missing destination folders, then rename `old_abs` → `new_abs` (the one step that
/// moves the note, resource or folder). If any step fails, undo the earlier ones in
/// reverse and return the failure; the rename is last, so once it succeeds there is
/// nothing left to undo. A filesystem has no transaction, so an undo step can fail
/// too — then [`Error::MoveIncomplete`] names the files still holding a rewrite, and
/// carries the failure that started the undo, rather than pretending the vault is whole.
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
    // The missing ancestors, outermost first, each created and recorded on its own so
    // the undo removes exactly the folders this move made.
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

/// [`commit`]'s undo: reverse `done`, restoring each rewritten file's original bytes
/// and removing each folder the move created. Returns the vault-relative paths of the
/// files that could not be restored. A folder that won't go is only logged: it is
/// empty, so no authored content is lost with it.
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

/// Split an authored link target at its first `#` into the path and the fragment —
/// which addresses a place *inside* the target, so a move carries it through verbatim.
fn split_fragment(authored: &str) -> (&str, Option<&str>) {
    match authored.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (authored, None),
    }
}

/// `base` with the authored `#fragment` (if any) put back on.
fn with_fragment(base: &str, fragment: Option<&str>) -> String {
    match fragment {
        Some(f) => format!("{base}#{f}"),
        None => base.to_string(),
    }
}

/// The replacement for a wikilink at a note now living at `new_path`, preserving the
/// link's own `.md`-or-not convention (Obsidian omits the extension; an authored `.md`
/// is kept) and any `#heading` fragment. Only a lowercase `.md` is dropped: that is the
/// suffix the resolver's `+ ".md"` ladder adds back, so any other spelling stays whole.
fn wiki_replacement(new_path: &str, authored: &str) -> String {
    let (base, fragment) = split_fragment(authored);
    let new_base = if base.ends_with(".md") {
        new_path
    } else {
        new_path.strip_suffix(".md").unwrap_or(new_path)
    };
    with_fragment(new_base, fragment)
}

/// The replacement for a link at a resource moving `old_path` → `new_path`, as
/// authored from a note in `src_dir` (its directory **after** the move, so a link
/// between two co-moved files computes to itself and is skipped). Two things
/// survive: the authored convention — a vault-root target stays vault-root,
/// anything else is re-relativized against `src_dir` — and a `#fragment` suffix,
/// carried through untouched.
fn resource_replacement(authored: &str, old_path: &str, new_path: &str, src_dir: &str) -> String {
    let (base, fragment) = split_fragment(authored);
    let new_base = if base.trim() == old_path {
        new_path.to_string() // authored vault-root — keep it vault-root
    } else {
        pathspec::relativize(src_dir, new_path) // authored note-relative — keep it relative
    };
    with_fragment(&new_base, fragment)
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

    /// Each syntax is rewritten from its own map, and a Markdown target keeps the
    /// whitespace and link text around it.
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
        // A map for one syntax never touches the other's links.
        let (out, n) = rewrite_targets("![alt](img.png)", &wiki, &Targets::new());
        assert_eq!((out.as_str(), n), ("![alt](img.png)", 0));
    }

    /// The divergence one grammar closes: a stray `[[` on one line used to pair with
    /// the next line's `]]` in the move's whole-file scan, hiding a link ingest had
    /// projected — so the backlink dangled after the move.
    #[test]
    fn a_stray_open_bracket_does_not_hide_the_next_lines_link() {
        let t = targets(&[("old", "new")]);
        let (out, n) = rewrite_links("broken [[x\nsee [[old]]\n", &t);
        assert_eq!(out, "broken [[x\nsee [[new]]\n");
        assert_eq!(n, 1);
    }

    // --- the replacement rules, direct (GH #134) ------------------------------
    //
    // Pure functions, so they get pinned here rather than only through a whole-vault
    // move.

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

    /// A `[[note#heading]]` link resolves to the note (the fragment is stripped for
    /// the lookup only), so its move keeps the heading it addresses.
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
        // unchanged replacement is what the pipeline skips, leaving the file untouched.
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
    // The façade suite (tests/mv.rs) drives a failed rename end to end. These pin the
    // two failures it can't reach deterministically: a write that fails after an
    // earlier one landed, and an undo that itself fails.

    /// A planned rewrite of `rel` under `root`, from whatever is on disk now.
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
