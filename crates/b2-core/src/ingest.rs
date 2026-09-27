//! Ingest (flow ①): parse the vault and project it into `notes`, `chunks` (+FTS) and
//! `edges`, keyed by vault-relative path (ADR-0003). Writes nothing to the vault (ADR-0004).
//!
//! Two passes: [`project_vault`] (model-free; notes and chunks first, then edges, so link
//! resolution never depends on file order) and [`embed_vault`] (fills chunks lacking a
//! vector, a pending set derived from the DB). [`ingest_file`] re-projects one note.

use crate::chunk::{chunk_body, ChunkConfig};
use crate::db::{self, EdgeRow, NoteRow};
use crate::embed::Embedder;
use crate::error::{Error, Result};
use crate::note;
use crate::resource::ResourceClass;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

/// Chunks per forward pass. Larger batches pad short chunks to the longest; measured, 16
/// beat 8 and 32. Also the reindex cancel granularity.
const EMBED_BATCH: usize = 16;

/// What a projection needs: connection, vault root and chunking policy (GH #134). It holds
/// no embedder, so an op taking it cannot embed: that keeps `rm`/`create_note`/`write`
/// model-free. A short-lived `Copy` view, like `NoteRow`.
#[derive(Clone, Copy)]
pub struct ProjectionCtx<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) root: &'a Path,
    pub(crate) cfg: &'a ChunkConfig,
}

impl<'a> ProjectionCtx<'a> {
    /// `cfg` is the vault's one chunking policy, never re-defaulted per call, so
    /// `incremental ≡ full rebuild` holds.
    pub fn new(conn: &'a Connection, root: &'a Path, cfg: &'a ChunkConfig) -> Self {
        Self { conn, root, cfg }
    }
}

/// A [`ProjectionCtx`] plus the embedder, for ops that re-embed what they touch (GH #134).
#[derive(Clone, Copy)]
pub struct EmbedCtx<'a> {
    pub(crate) proj: ProjectionCtx<'a>,
    pub(crate) embedder: &'a dyn Embedder,
}

impl<'a> EmbedCtx<'a> {
    /// Add an embedder to a projection context.
    pub fn new(proj: ProjectionCtx<'a>, embedder: &'a dyn Embedder) -> Self {
        Self { proj, embedder }
    }
}

/// A file's mtime as Unix seconds, or `None` when the platform can't supply one.
pub(crate) fn unix_mtime(meta: &fs::Metadata) -> Option<i64> {
    let modified = meta.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since_epoch.as_secs() as i64)
}

/// One projected note: the body for edge derivation, and `(text_hash, text)` pairs still
/// needing a vector.
struct ProjectedNote {
    body: String,
    relations: Vec<String>,
    pending: Vec<(String, String)>,
}

/// A [`plan_reindex`] preview (`reindex --dry-run`), decided read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Notes a real reindex would project.
    pub notes: usize,
    /// …of which this many would be (re)embedded (changed, fresh, or forced).
    pub would_embed: usize,
}

/// When [`project_note_and_chunks`] re-cuts a note's chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rechunk {
    /// The projection pass: re-chunk a new or body-changed note. Reads only `notes`, never
    /// vector state (missing vectors are [`embed_vault`]'s job).
    IfBodyChanged,
    /// A forced projection (`reindex --force`): re-chunk every note.
    Always,
    /// The inline path ([`ingest_file`]): also re-chunk a note left mid-embed, and hand
    /// back the chunks still missing a vector.
    IfBodyChangedOrUnembedded,
}

impl Rechunk {
    fn forced(force: bool) -> Self {
        if force {
            Self::Always
        } else {
            Self::IfBodyChanged
        }
    }
}

/// The incremental "did the body change?" key, shared with the dry-run so they can't drift.
fn body_hash(body: &str) -> String {
    blake3::hash(body.as_bytes()).to_hex().to_string()
}

/// Embed-phase progress, reported per batch. Counts cover only the notes that (re)embed
/// this run, not the whole vault. Field names are the JSON keys the desktop frontend reads.
#[derive(Debug, Clone, Serialize)]
pub struct ReindexProgress {
    /// Vault-relative path of the note currently embedding.
    pub note_path: String,
    /// Chunks in the current note.
    pub note_chunks: usize,
    /// How many notes have begun embedding so far (1-based)…
    pub notes_embedded: usize,
    /// …out of this many notes that need (re)embedding this run.
    pub notes_to_embed: usize,
    /// Chunks embedded so far, cumulative across every note this run.
    pub chunks_done: usize,
}

/// Project one note's row and chunks: everything derivable without resolving links.
/// Unless [`Rechunk::Always`], an unchanged body keeps its chunks (`incremental ≡ full
/// rebuild`: they are what a fresh projection would produce). `pending` is filled only for
/// [`Rechunk::IfBodyChangedOrUnembedded`].
fn project_note_and_chunks(
    ctx: ProjectionCtx,
    rel_path: &str,
    when: Rechunk,
) -> Result<ProjectedNote> {
    let ProjectionCtx { conn, root, cfg } = ctx;
    let abs = root.join(rel_path);
    let raw = fs::read_to_string(&abs)?;
    let parsed = note::parse(&raw);

    let body = parsed.body().to_string();
    let body_hash = body_hash(&body);
    let mtime = fs::metadata(&abs).ok().as_ref().and_then(unix_mtime);

    // Decide before the upsert overwrites `body_hash`. The inline path's caller ensured
    // the embedding space, hence `space_exists = true`.
    let rechunk = match when {
        Rechunk::Always => true,
        Rechunk::IfBodyChanged => {
            db::note_body_hash(conn, rel_path)?.as_deref() != Some(body_hash.as_str())
        }
        Rechunk::IfBodyChangedOrUnembedded => {
            would_reembed(conn, rel_path, &body_hash, false, true)?
        }
    };

    let fields = parsed.fields();
    // The display title is the filename; a frontmatter `title:` is inert.
    let title = note::display_title(rel_path);
    db::upsert_note(
        conn,
        &NoteRow {
            path: rel_path,
            title: Some(title.as_str()),
            created: fields.created.as_deref(),
            body_hash: &body_hash,
            mtime,
        },
    )?;

    let relations = fields.relations.clone();

    // Vectors are content-addressed (ADR-0006), so re-chunking unchanged text (a move)
    // yields nothing pending.
    let pending = if rechunk {
        let chunks = chunk_body(&body, cfg);
        db::replace_chunks(conn, rel_path, &chunks)?;
        if when == Rechunk::IfBodyChangedOrUnembedded {
            pending_for_note(conn, rel_path)?
        } else {
            // The embed pass derives its own pending set from the DB.
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(ProjectedNote {
        body,
        relations,
        pending,
    })
}

/// One note's chunks still lacking a vector. Its own query, not a filter over the whole
/// vault, because `move_dir` calls it per note. Requires the embedding space.
fn pending_for_note(conn: &Connection, note_path: &str) -> Result<Vec<(String, String)>> {
    Ok(db::note_chunks_missing_vectors(conn, note_path)?
        .into_iter()
        .map(|c| (c.text_hash, c.text))
        .collect())
}

/// Whether a note would be (re)embedded: under `force`, with no embedding space yet, when
/// the body hash differs, or when not fully embedded. Not used by [`project_vault`], which
/// never reads vector state. `space_exists` avoids querying a table that doesn't exist.
fn would_reembed(
    conn: &Connection,
    note_path: &str,
    body_hash: &str,
    force: bool,
    space_exists: bool,
) -> Result<bool> {
    if force || !space_exists {
        return Ok(true);
    }
    let unchanged = db::note_body_hash(conn, note_path)?.as_deref() == Some(body_hash)
        && db::note_fully_embedded(conn, note_path)?;
    Ok(!unchanged)
}

/// The result of embedding one note's pending chunks.
struct NoteEmbedOutcome {
    /// `on_batch` returned [`ControlFlow::Break`]; stop starting new notes.
    cancelled: bool,
    /// Every pending chunk got a vector, even if the cancel landed on the final batch.
    completed: bool,
}

/// Embed a note's pending pairs in batches of [`EMBED_BATCH`], calling `on_batch` per
/// batch for progress and cancel. The cancel check runs after a batch is written, so a
/// cancel never tears one. A finished note gets its centroid refreshed here, where vectors
/// are written (ADR-0006), even when `pending` is empty, which heals a missing centroid.
fn embed_pending(
    conn: &Connection,
    embedder: &dyn Embedder,
    note_path: &str,
    pending: &[(String, String)],
    mut on_batch: impl FnMut(usize) -> ControlFlow<()>,
) -> Result<NoteEmbedOutcome> {
    let total = pending.len();
    let mut done = 0usize;
    let mut cancelled = false;
    for batch in pending.chunks(EMBED_BATCH) {
        let texts: Vec<&str> = batch.iter().map(|(_, t)| t.as_str()).collect();
        let vectors = embedder.embed_batch(&texts)?;
        for ((hash, _), v) in batch.iter().zip(&vectors) {
            db::set_vector(conn, hash, v)?;
        }
        done += batch.len();
        if on_batch(batch.len()).is_break() {
            cancelled = true;
            break;
        }
    }
    let completed = done == total;
    if completed {
        db::refresh_note_centroid(conn, note_path)?;
    }
    Ok(NoteEmbedOutcome {
        cancelled,
        completed,
    })
}

/// Derive and project a note's authored edges: body links (`origin=inline`, untyped
/// `references`) plus frontmatter `b2_relations:` (`origin=frontmatter`, the typed home).
/// On overlap the frontmatter entry wins, since only it can carry an explanation
/// (ADR-0010). Occurrence is counted per `(target, type)` over the kept set.
fn project_edges(
    conn: &Connection,
    src_path: &str,
    body: &str,
    relations: &[String],
) -> Result<()> {
    let mut staged: Vec<(crate::link::ParsedLink, &'static str)> = Vec::new();
    for link in crate::link::parse_links(body) {
        staged.push((link, "inline"));
    }
    for spec in relations {
        if let Some(link) = crate::link::parse_relation(spec) {
            staged.push((link, "frontmatter"));
        }
    }

    // The base for a Markdown-form relative target.
    let src_dir = crate::pathspec::parent_dir(src_path);

    let mut fm_keys: HashSet<(String, String)> = HashSet::new();
    let mut resolved = Vec::with_capacity(staged.len());
    for (link, origin) in staged {
        let (dst_path, dst_resource_path) = resolve_target(conn, src_dir, &link)?;
        let target_key = dst_path
            .clone()
            .or_else(|| dst_resource_path.clone())
            .unwrap_or_else(|| link.target_path.clone());
        if origin == "frontmatter" {
            fm_keys.insert((target_key.clone(), link.edge_type.clone()));
        }
        resolved.push((link, origin, dst_path, dst_resource_path, target_key));
    }

    let mut occ: HashMap<(String, String), i64> = HashMap::new();
    let mut rows = Vec::with_capacity(resolved.len());
    for (link, origin, dst_path, dst_resource_path, target_key) in resolved {
        let key = (target_key.clone(), link.edge_type.clone());
        if origin == "inline" && fm_keys.contains(&key) {
            continue; // frontmatter wins — it alone can carry the explanation
        }
        let occurrence_index = *occ.get(&key).unwrap_or(&0);
        occ.insert(key, occurrence_index + 1);

        rows.push(EdgeRow {
            id: derive_edge_id(src_path, &target_key, &link.edge_type, occurrence_index),
            src_path: src_path.to_string(),
            dst_path,
            dst_resource_path,
            dst_path_raw: link.target_path.clone(),
            r#type: link.edge_type.clone(),
            origin: origin.to_string(),
            explanation: link.explanation.clone(),
            embed: link.embed,
            caption: link.caption.clone(),
            occurrence_index,
        });
    }

    db::replace_authored_edges(conn, src_path, &rows)
}

/// Resolve a link to `(dst_path, dst_resource_path)`: at most one is `Some`, both `None`
/// means dangling. A `#fragment` is stripped for the lookup only.
fn resolve_target(
    conn: &Connection,
    src_dir: &str,
    link: &crate::link::ParsedLink,
) -> Result<(Option<String>, Option<String>)> {
    let lookup = link
        .target_path
        .split('#')
        .next()
        .unwrap_or_default()
        .trim();
    if lookup.is_empty() {
        return Ok((None, None)); // fragment-only wikilink — dangling
    }

    // A Markdown-form target tries note-relative first, then vault-root; wikilinks are
    // vault-root only.
    let mut candidates: Vec<String> = Vec::with_capacity(2);
    if link.md_form {
        if let Some(joined) = crate::pathspec::join_relative(src_dir, lookup) {
            candidates.push(joined);
        }
    }
    if !candidates.iter().any(|c| c == lookup) {
        candidates.push(lookup.to_string());
    }

    // Extension-only kind dispatch, shared with the adapters: non-`md` means resource.
    let is_resource = crate::resource::doc_kind(lookup) == crate::resource::DocKind::Resource;
    for candidate in &candidates {
        if is_resource {
            if let Some(path) = db::resolve_resource_target(conn, candidate)? {
                return Ok((None, Some(path)));
            }
        } else if let Some(note_path) = db::resolve_link_target(conn, candidate)? {
            return Ok((Some(note_path), None));
        }
    }
    Ok((None, None))
}

/// Deterministic edge id from its identity tuple (ADR-0010): stable across re-index, not
/// across a move, since both ends are paths.
fn derive_edge_id(src_path: &str, target_key: &str, edge_type: &str, occurrence: i64) -> String {
    let mut h = blake3::Hasher::new();
    for part in [src_path, target_key, edge_type] {
        h.update(part.as_bytes());
        h.update(b"\x1f"); // unit separator — avoids field-boundary collisions
    }
    h.update(occurrence.to_string().as_bytes());
    h.finalize().to_hex()[..32].to_string()
}

/// Ingest a single note against an already-built index: note, chunks, vectors, edges.
pub fn ingest_file(ctx: EmbedCtx, rel_path: &str) -> Result<()> {
    let EmbedCtx { proj, embedder } = ctx;
    let conn = proj.conn;
    db::ensure_embedding_space(conn, embedder.model_id(), embedder.dim())?;
    // A frontmatter-only edit re-projects without re-embedding; a note left mid-embed
    // re-chunks and re-embeds here.
    let p = project_note_and_chunks(proj, rel_path, Rechunk::IfBodyChangedOrUnembedded)?;
    embed_pending(conn, embedder, rel_path, &p.pending, |_| {
        ControlFlow::Continue(())
    })?;
    project_edges(conn, rel_path, &p.body, &p.relations)?;
    Ok(())
}

/// [`project_vault`] then [`embed_vault`] with the default [`ChunkConfig`] and no
/// progress. A convenience for the test suite.
pub fn ingest_vault(conn: &Connection, vault_root: &Path, embedder: &dyn Embedder) -> Result<()> {
    let cfg = ChunkConfig::default();
    project_vault(ProjectionCtx::new(conn, vault_root, &cfg), false)?;
    embed_vault(conn, embedder, &mut |_| ControlFlow::Continue(()))?;
    Ok(())
}

/// Re-project a single note model-free (what `Vault::write` runs). New chunks join the
/// DB-derived pending set for a later embed pass, so saving needs no embedder.
pub fn project_file(ctx: ProjectionCtx, rel_path: &str) -> Result<()> {
    let p = project_note_and_chunks(ctx, rel_path, Rechunk::IfBodyChanged)?;
    project_edges(ctx.conn, rel_path, &p.body, &p.relations)
}

/// A vault file the projection pass could not read and skipped. `reason` describes the
/// file, never a B2 internal, so it is safe to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedNote {
    pub path: String,
    pub reason: String,
}

fn skipped(path: &str, err: &std::io::Error) -> SkippedNote {
    SkippedNote {
        path: path.to_string(),
        reason: skip_reason(err),
    }
}

/// A short, user-facing reason for an I/O error on one file, free of OS jargon.
fn skip_reason(err: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::InvalidData => "not valid UTF-8 text".to_string(),
        ErrorKind::PermissionDenied => "permission denied".to_string(),
        ErrorKind::NotFound => "file no longer exists".to_string(),
        _ => "could not be read".to_string(),
    }
}

/// The result of [`project_vault`]: projected note paths in sorted walk order, plus
/// files skipped as unreadable.
#[derive(Debug, Clone)]
pub struct ProjectOutcome {
    pub notes: Vec<String>,
    pub skipped: Vec<SkippedNote>,
    /// Rows pruned for notes deleted outside b2 (#31).
    pub notes_pruned: usize,
    /// Resources inventoried (unchanged included), and stale rows pruned.
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// The result of a (possibly cancelled) [`embed_vault`].
#[derive(Debug, Clone)]
pub struct EmbedOutcome {
    /// Notes that fully embedded this run, in path order. Already-complete notes are
    /// not listed.
    pub embedded: Vec<String>,
    /// The pass stopped early because `on_progress` returned [`ControlFlow::Break`].
    pub cancelled: bool,
}

/// The projection pass: notes, chunks and FTS, then edges, with no embedder, so an
/// unembedded index already serves keyword search and the graph. Unless `force`, only new
/// or body-changed notes re-chunk. Writes nothing to the vault (ADR-0004).
pub fn project_vault(ctx: ProjectionCtx, force: bool) -> Result<ProjectOutcome> {
    let ProjectionCtx { conn, root, .. } = ctx;
    let VaultWalk {
        notes: rel_paths,
        resources: resource_files,
    } = walk_vault(root)?;

    // Phase 1: notes and chunks, filling the resolver so phase 2 is order-independent.
    let mut staged = Vec::with_capacity(rel_paths.len());
    let mut skipped = Vec::new();
    for rel in &rel_paths {
        match project_note_and_chunks(ctx, rel, Rechunk::forced(force)) {
            Ok(p) => staged.push((rel.clone(), p.body, p.relations)),
            // An unreadable note is skipped, not fatal; a DB error still aborts. The read
            // fails before any upsert, so no partial row is written.
            Err(Error::Io(e)) => skipped.push(self::skipped(rel, &e)),
            Err(other) => return Err(other),
        }
    }

    // Prune notes the walk did not meet (#31); a seen-but-unreadable file is kept. Before
    // phase 2 so links at a deleted note re-dangle, as in a full rebuild.
    let seen: HashSet<&str> = staged
        .iter()
        .map(|(path, ..)| path.as_str())
        .chain(skipped.iter().map(|s| s.path.as_str()))
        .collect();
    let notes_pruned = db::prune_notes_except(conn, &seen)?;

    // Content-addressed vectors (ADR-0006) outlive their chunk rows; collect the
    // unreferenced ones now the chunk set is final.
    if db::embedding_space_exists(conn)? {
        db::prune_orphan_vectors(conn)?;
    }

    // Resources before phase 2, which resolves `![[img.png]]` against them (spec §3).
    let (resources_indexed, resources_pruned, mut resource_skips) =
        project_resources(conn, root, &resource_files)?;
    skipped.append(&mut resource_skips);

    // Phase 2: edges. A link at a skipped note stays unresolved.
    let mut notes = Vec::with_capacity(staged.len());
    for (path, body, relations) in staged {
        project_edges(conn, &path, &body, &relations)?;
        notes.push(path);
    }
    tracing::debug!(
        target: "b2::ingest",
        notes = notes.len(),
        skipped = skipped.len(),
        notes_pruned,
        resources = resources_indexed,
        resources_pruned,
        force,
        "projection pass complete"
    );
    Ok(ProjectOutcome {
        notes,
        skipped,
        notes_pruned,
        resources_indexed,
        resources_pruned,
    })
}

/// The embed pass: fill a vector for every chunk lacking one (after a model swap, all of
/// them). `on_progress` fires per batch; [`ControlFlow::Break`] cancels. No `force`: it only
/// fills what's missing, so an interruption heals on the next call.
pub fn embed_vault(
    conn: &Connection,
    embedder: &dyn Embedder,
    on_progress: &mut dyn FnMut(ReindexProgress) -> ControlFlow<()>,
) -> Result<EmbedOutcome> {
    db::ensure_embedding_space(conn, embedder.model_id(), embedder.dim())?;

    // Group pending chunks by note. Each hash is embedded once per run (ADR-0006); a note
    // whose chunks were all claimed earlier finds its vectors stored and still refreshes
    // its centroid.
    type PendingNote = (String, Vec<(String, String)>);
    let mut by_note: Vec<PendingNote> = Vec::new();
    let mut claimed: HashSet<String> = HashSet::new();
    for c in db::chunks_missing_vectors(conn)? {
        let fresh = claimed.insert(c.text_hash.clone());
        let pair = fresh.then_some((c.text_hash, c.text));
        match by_note.last_mut() {
            Some((last, pending)) if *last == c.note_path => pending.extend(pair),
            _ => by_note.push((c.note_path, pair.into_iter().collect())),
        }
    }

    let notes_to_embed = by_note.len();
    tracing::debug!(
        target: "b2::ingest",
        notes_to_embed,
        pending_chunks = by_note.iter().map(|(_, p)| p.len()).sum::<usize>(),
        "embed pass starting (DB-derived pending set)"
    );
    let mut embedded = Vec::new();
    let mut chunks_done = 0usize;
    let mut cancelled = false;
    for (i, (path, pending)) in by_note.iter().enumerate() {
        // Per-note span, for timing the slowest step.
        let _note_span = tracing::debug_span!(
            target: "b2::ingest", "embed_note",
            path = path.as_str(), chunks = pending.len()
        )
        .entered();
        let notes_embedded = i + 1; // 1-based
        let note_chunks = pending.len();
        let outcome = embed_pending(conn, embedder, path, pending, |n| {
            chunks_done += n;
            on_progress(ReindexProgress {
                note_path: path.clone(),
                note_chunks,
                notes_embedded,
                notes_to_embed,
                chunks_done,
            })
        })?;
        if outcome.completed {
            embedded.push(path.clone());
        }
        if outcome.cancelled {
            cancelled = true;
            break;
        }
    }
    // A re-cut but unchanged note skips the loop above, yet `replace_chunks` dropped its
    // centroid; without one it vanishes from discovery (S3). Safe after a cancel: only
    // fully embedded notes are offered.
    for note_path in db::notes_missing_centroids(conn)? {
        db::refresh_note_centroid(conn, &note_path)?;
    }
    tracing::debug!(
        target: "b2::ingest",
        notes_embedded = embedded.len(),
        chunks_embedded = chunks_done,
        cancelled,
        "embed pass complete"
    );
    Ok(EmbedOutcome {
        embedded,
        cancelled,
    })
}

/// Read-only preview of a reindex (`reindex --dry-run`): per note, would a real run
/// (re)embed it? Reads the stored vectors, so it does not detect a pending model swap.
pub fn plan_reindex(conn: &Connection, vault_root: &Path, force: bool) -> Result<Plan> {
    let space_exists = db::embedding_space_exists(conn)?;
    let rel_paths = walk_vault(vault_root)?.notes;
    let mut plan = Plan {
        notes: 0,
        would_embed: 0,
    };
    for rel in rel_paths {
        // A real reindex skips an unreadable file too.
        let raw = match fs::read_to_string(vault_root.join(&rel)) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let body_hash = body_hash(note::parse(&raw).body());
        plan.notes += 1;
        if would_reembed(conn, &rel, &body_hash, force, space_exists)? {
            plan.would_embed += 1;
        }
    }
    Ok(plan)
}

/// What one vault walk found, each list sorted for a deterministic order.
struct VaultWalk {
    notes: Vec<String>,
    resources: Vec<(String, ResourceClass)>,
}

/// The vault walk both whole-vault passes share.
fn walk_vault(root: &Path) -> Result<VaultWalk> {
    let mut notes = Vec::new();
    let mut resources = Vec::new();
    collect_vault_files(root, root, &mut notes, &mut resources)?;
    notes.sort();
    resources.sort_by(|a, b| a.0.cmp(&b.0)); // paths are unique — a total order
    Ok(VaultWalk { notes, resources })
}

/// Walk the vault, routing `.md` (case-insensitive) to `notes` and everything else to
/// `resources` (ADR-0002). Dot-prefixed entries, files and folders alike, are skipped
/// (GH #136).
fn collect_vault_files(
    root: &Path,
    dir: &Path,
    notes: &mut Vec<String>,
    resources: &mut Vec<(String, ResourceClass)>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if crate::pathspec::is_hidden(&path) {
            continue;
        }
        if path.is_dir() {
            collect_vault_files(root, &path, notes, resources)?;
            continue;
        }
        // Cannot fail under `root`; skip rather than panic.
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        match ResourceClass::of_path(&rel) {
            None => notes.push(rel),
            Some(class) => resources.push((rel, class)),
        }
    }
    Ok(())
}

/// The resource inventory pass: inventory every walked resource, then prune rows the walk
/// no longer saw (inbound edges re-dangle via `ON DELETE SET NULL`). An unreadable file is
/// skipped and its prior row kept. Returns `(indexed, pruned, skipped)`.
fn project_resources(
    conn: &Connection,
    vault_root: &Path,
    resources: &[(String, ResourceClass)],
) -> Result<(usize, usize, Vec<SkippedNote>)> {
    let mut skipped = Vec::new();
    let mut seen: HashSet<String> = HashSet::with_capacity(resources.len());
    let mut indexed = 0;
    for (rel, class) in resources {
        // Seen, so never pruned, even if reading it fails.
        seen.insert(rel.clone());
        match project_resource_file(conn, vault_root, rel, *class, false) {
            Ok(()) => indexed += 1,
            // Only a filesystem failure is recoverable; anything else aborts.
            Err(Error::Io(e)) => skipped.push(self::skipped(rel, &e)),
            Err(e) => return Err(e),
        }
    }
    let pruned = db::prune_resources_except(conn, &seen)?;
    Ok((indexed, pruned, skipped))
}

/// Inventory one resource: skip on an unchanged `(size, mtime)`, else blake3 and upsert.
/// `force` bypasses the stat check: an import just created the file, so an existing row
/// describes different bytes. I/O errors return as [`Error::Io`] for the caller to judge.
pub(crate) fn project_resource_file(
    conn: &Connection,
    vault_root: &Path,
    rel: &str,
    class: ResourceClass,
    force: bool,
) -> Result<()> {
    let abs = vault_root.join(rel);
    let meta = fs::metadata(&abs)?;
    let size = meta.len() as i64;
    let mtime = unix_mtime(&meta);
    if !force && db::resource_stat(conn, rel)? == Some((size, mtime)) {
        return Ok(()); // unchanged
    }
    let bytes = fs::read(&abs)?;
    let content_hash = blake3::hash(&bytes).to_hex().to_string();
    db::upsert_resource(
        conn,
        &db::ResourceRow {
            path: rel,
            class: class.as_str(),
            size,
            mtime,
            content_hash: &content_hash,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A same-size replacement within the same second must not keep a stale `content_hash`
    /// on import (the move repair matches on it). `set_modified` pins the mtime.
    #[test]
    fn the_unchanged_stat_shortcut_is_the_walks_alone() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let conn = crate::db::open(&root.join("b2.sqlite")).unwrap();
        let rel = "clip.txt";
        let abs = root.join(rel);
        let hash = |conn: &Connection| -> String {
            conn.query_row(
                "SELECT content_hash FROM resources WHERE path = ?1",
                [rel],
                |r| r.get(0),
            )
            .unwrap()
        };

        fs::write(&abs, b"AAAA").unwrap();
        project_resource_file(&conn, root, rel, ResourceClass::Text, false).unwrap();
        let first = hash(&conn);
        let stamped = fs::metadata(&abs).unwrap().modified().unwrap();

        // Different bytes, same length, same mtime.
        fs::write(&abs, b"BBBB").unwrap();
        fs::File::options()
            .write(true)
            .open(&abs)
            .unwrap()
            .set_modified(stamped)
            .unwrap();

        project_resource_file(&conn, root, rel, ResourceClass::Text, false).unwrap();
        assert_eq!(hash(&conn), first, "the walk trusts an unchanged stat");

        project_resource_file(&conn, root, rel, ResourceClass::Text, true).unwrap();
        assert_ne!(
            hash(&conn),
            first,
            "an import hashes what it actually placed"
        );
    }
}
