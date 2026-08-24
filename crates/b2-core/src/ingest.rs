//! Ingest (flow ①): parse -> project into `notes`/`note_aliases`, `chunks` (+FTS) and
//! the typed `edges` graph, all keyed by the note's vault-relative path (ADR-0003).
//! **Ingest writes nothing to the vault** (ADR-0004) — it is a pure read of it.
//!
//! A full ingest is **two separately-invokable passes**: [`project_vault`], the
//! model-free one, which runs in two phases so link resolution never depends on file
//! order (phase 1 projects every note + its chunks, phase 2 derives edges against the
//! now-complete resolver); and [`embed_vault`], the model-bound one, which fills
//! whatever chunks still lack a vector — a pending set **derived from the DB**, never
//! handed over in memory. [`ingest_vault_with_progress`] is their composition;
//! [`ingest_file`] re-projects a single note inline against an already-built index.

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

/// How many chunks to embed per forward pass. Batching amortizes one matmul over many
/// texts, traded against the **padding waste** of batching short chunks with long ones
/// — the tokenizer pads every chunk to the batch's longest. Measured on a real vault,
/// 16 beat 32 (~40% faster) and 8. It is also the reindex **cancel granularity**: the
/// flag is checked once per batch.
const EMBED_BATCH: usize = 16;

/// Everything a **projection** needs: the index connection, the vault root, and the
/// vault's chunking policy. The three travel together through every write-side op, so
/// they are one parameter rather than three in two orders (GH #134).
///
/// It carries a **posture**, and that is the point: an op that takes a `ProjectionCtx`
/// holds no embedder and therefore *cannot* embed. The model-free rule for
/// `rm`/`create_note`/`write` is the type system's to keep.
///
/// A short-lived, `Copy`, borrow-only view struct never stored — the sanctioned
/// exception to "prefer owned fields" (CLAUDE.md), as `NoteRow` is.
#[derive(Clone, Copy)]
pub struct ProjectionCtx<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) root: &'a Path,
    pub(crate) cfg: &'a ChunkConfig,
}

impl<'a> ProjectionCtx<'a> {
    /// Bundle the three projection inputs. `cfg` is the vault's *one* chunking policy
    /// (never re-defaulted per call), so every path that chunks a given vault cuts
    /// identically and `incremental ≡ full rebuild` holds by construction.
    pub fn new(conn: &'a Connection, root: &'a Path, cfg: &'a ChunkConfig) -> Self {
        Self { conn, root, cfg }
    }
}

/// A [`ProjectionCtx`] plus the embedder — the **embedding** posture, and the other
/// half of GH #134's split. The ops that re-embed what they touch take this; the
/// model-free ones take the projection context alone, so the two are unmixable by the
/// compiler. Same view-struct rules as [`ProjectionCtx`].
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

/// A file's mtime as Unix seconds — the projection's shared stat reading
/// (notes, resources, and a moved resource's repoint all record the same
/// shape). `None` when the platform clock can't supply one.
pub(crate) fn unix_mtime(meta: &fs::Metadata) -> Option<i64> {
    let modified = meta.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since_epoch.as_secs() as i64)
}

/// Outcome of ingesting one file.
#[derive(Debug, Clone)]
pub struct Ingested {
    /// The note's vault-relative path — its identity (L1).
    pub path: String,
    /// Whether this note was (re)embedded this run — `false` when an unchanged body
    /// let the incremental path reuse its existing vectors.
    pub embedded: bool,
}

/// What [`project_note_and_chunks`] returns for one note: the material later phases
/// need — the body for edge derivation, and the `(text_hash, text)` pairs still
/// needing a vector for the embed step.
struct ProjectedNote {
    body: String,
    relations: Vec<String>,
    pending: Vec<(String, String)>,
}

/// One note's entry in a [`plan_reindex`] preview (`reindex --dry-run`): what a real
/// reindex *would* do to this file, decided read-only. Only one question is left to
/// preview — a run that writes nothing (ADR-0004) has nothing to warn about beyond the
/// work it will do.
#[derive(Debug, Clone)]
pub struct PlannedNote {
    /// Vault-relative path of the note.
    pub path: String,
    /// A real reindex would (re)embed this note's body (changed, fresh, or forced).
    pub would_embed: bool,
}

/// Progress during the embed phase, reported **per batch** so a large vault never
/// looks frozen while it embeds. Purely observational.
///
/// The counts describe the notes that actually (re)embed this run, not every note: an
/// incremental reindex reuses most notes' vectors, so `notes_to_embed` is the real unit
/// of work. Reporting position in the full note list would jump to "note 14/18" while
/// only a handful are doing anything. `Serialize` so the desktop can stream it to the
/// webview; the field names are the JSON keys the frontend reads.
#[derive(Debug, Clone, Serialize)]
pub struct ReindexProgress {
    /// Vault-relative path of the note currently embedding.
    pub note_path: String,
    /// Number of chunks in the current note (this file's own chunk count).
    pub note_chunks: usize,
    /// How many notes have begun embedding so far (1-based)…
    pub notes_embedded: usize,
    /// …out of this many notes that need (re)embedding this run — the changed/fresh
    /// notes (or every note under `force`), not the whole vault.
    pub notes_to_embed: usize,
    /// Chunks embedded so far, cumulative across every note this run.
    pub chunks_done: usize,
}

/// Project one note's frontmatter + chunks — everything derivable without resolving
/// links. Returns its body (kept so phase 2 derives edges without re-reading), its
/// frontmatter relations, and the `(text_hash, text)` pairs still needing a vector;
/// embedding is deferred. No embedder here, and **no write to the vault**.
///
/// **Incremental:** unless `force`, a note whose body hash is unchanged is left
/// untouched and `pending` comes back empty; frontmatter-only edits still re-project
/// the note row and its edges. The invariant `incremental ≡ full rebuild` holds because
/// the re-used rows are byte-for-byte what a fresh projection would produce.
///
/// `consult_vectors` selects the re-chunk predicate. The full-vault pass passes `false`
/// — it reads only `notes`, because "unchanged body but missing vectors" is
/// [`embed_vault`]'s job, and that is what keeps [`project_vault`] free of the vector
/// tables. [`ingest_file`] passes `true` (it embeds inline), so a note left mid-embed
/// is healed by [`would_reembed`]'s vector-state check.
fn project_note_and_chunks(
    ctx: ProjectionCtx,
    rel_path: &str,
    force: bool,
    consult_vectors: bool,
) -> Result<ProjectedNote> {
    let ProjectionCtx { conn, root, cfg } = ctx;
    let abs = root.join(rel_path);
    let raw = fs::read_to_string(&abs)?;
    let parsed = note::parse(&raw);

    let body = parsed.body().to_string();
    let body_hash = blake3::hash(body.as_bytes()).to_hex().to_string();
    let mtime = fs::metadata(&abs).ok().as_ref().and_then(unix_mtime);

    // Decide the re-chunk BEFORE the upsert overwrites `body_hash`. The inline path
    // also reads vector state (its caller ensured the space, hence
    // `space_exists = true`); the projection pass reads only `notes`.
    let rechunk = if consult_vectors {
        would_reembed(conn, rel_path, &body_hash, force, true)?
    } else {
        force || db::note_body_hash(conn, rel_path)?.as_deref() != Some(body_hash.as_str())
    };

    let fields = parsed.fields();
    // A note's display title is its **filename**; a frontmatter `title:` is inert.
    // Projected here so every read path shows it with no per-call derivation.
    let title = note::display_title(rel_path);
    db::upsert_note(
        conn,
        &NoteRow {
            path: rel_path,
            // `type` is required by the model; default the projection to "note"
            // for the rare untyped file (the file itself is never modified).
            r#type: fields.r#type.as_deref().unwrap_or("note"),
            title: Some(title.as_str()),
            description: fields.description.as_deref(),
            created: fields.created.as_deref(),
            updated: fields.updated.as_deref(),
            body_hash: &body_hash,
            mtime,
            aliases: &fields.aliases,
        },
    )?;

    let relations = parsed.fields().relations.clone();

    // Incremental fast path: an unchanged body means identical chunks — reuse them and
    // return no pending work. A re-chunk hands back pairs only for chunks with **no
    // stored vector**: the store is content-addressed (ADR-0006), so re-chunking a note
    // whose text is unchanged — what a move produces — yields nothing pending at all.
    let pending = if rechunk {
        let chunks = chunk_body(&body, cfg);
        db::replace_chunks(conn, rel_path, &chunks)?;
        if consult_vectors {
            pending_for_note(conn, rel_path)?
        } else {
            // The projection pass never reads vector state; the embed pass derives
            // its own pending set from the DB.
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

/// The `(text_hash, text)` pairs of one note's chunks that still lack a vector — the
/// single-note form of [`db::chunks_missing_vectors`], for the inline path. Its own
/// query rather than a filter over the whole-vault set, because `move_dir` calls this
/// once per moved note. Requires the embedding space to exist.
fn pending_for_note(conn: &Connection, note_path: &str) -> Result<Vec<(String, String)>> {
    Ok(db::note_chunks_missing_vectors(conn, note_path)?
        .into_iter()
        .map(|c| (c.text_hash, c.text))
        .collect())
}

/// Whether a note's body would be (re)embedded this run — the negation of the
/// incremental fast path: true under `force`, on a vault with no embedding space yet,
/// when the stored body hash differs, or when the note is not fully embedded (fresh, or
/// a model swap emptied the tables). Shared by [`ingest_file`] and [`plan_reindex`];
/// [`project_vault`] deliberately does **not** use it, since projection never reads
/// vector state. `space_exists` lets a pristine vault short-circuit without querying an
/// `embeddings` table that does not exist yet.
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

/// The result of embedding one note's pending chunks: whether a cancel was signalled
/// at a batch boundary, and whether **every** pending chunk got a vector.
struct NoteEmbedOutcome {
    /// `on_batch` returned [`ControlFlow::Break`] at a batch boundary — the caller
    /// should stop starting new notes (a cooperative cancel).
    cancelled: bool,
    /// Every pending chunk was embedded, so the note is now fully embedded. True even
    /// when the cancel landed on the *final* batch: each batch is written before its
    /// cancel check, so there is nothing left to do for this note.
    completed: bool,
}

/// Embed a note's pending `(text_hash, text)` pairs in batches of [`EMBED_BATCH`],
/// calling `on_batch` with each batch's size, so a reindex can report progress **and**
/// cooperatively cancel. Chunk vectors are independent, so batch boundaries never
/// change the result, and the cancel check runs **after** a batch is fully written — a
/// cancel never tears a batch, it only stops further ones.
///
/// A note that finishes has its **centroid** refreshed from its now-complete vectors:
/// the centroid is derived data on the vectors' lifecycle (ADR-0006), so maintaining it
/// here — the one place vectors are written — means no other pass reconciles it. Run
/// even when `pending` is empty, which costs one indexed read and heals a missing
/// centroid.
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

/// Derive a note's authored edges and project them — the union of **body** links
/// (`origin=inline`, always untyped `references`) and frontmatter **`b2_relations:`**
/// (`origin=frontmatter`, the sole typed home), each target resolved against the
/// current resolver. On overlap the **frontmatter entry wins** and the redundant body
/// reference is dropped, because only the frontmatter row can carry an explanation
/// (ADR-0010). Occurrence is assigned per `(target, type)` over the kept set.
///
/// Resolution dispatches by the target's **extension**: `.md` or extensionless
/// resolves against `notes` (the wikilink `+ ".md"` ladder), any other extension
/// against `resources`. A `#fragment` is stripped for the lookup only. Markdown-form
/// targets (`[…](path)`) additionally try note-relative first, per standard Markdown.
fn project_edges(
    conn: &Connection,
    src_path: &str,
    body: &str,
    relations: &[String],
) -> Result<()> {
    // Gather authored links: body first (inline), then frontmatter (frontmatter).
    let mut staged: Vec<(crate::link::ParsedLink, &'static str)> = Vec::new();
    for link in crate::link::parse_links(body) {
        staged.push((link, "inline"));
    }
    for spec in relations {
        if let Some(link) = crate::link::parse_relation(spec) {
            staged.push((link, "frontmatter"));
        }
    }

    // The source note's directory — the base for a Markdown-form relative target, read
    // straight off the path now that the path *is* the identity (ADR-0003).
    let src_dir = src_path
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or_default();

    // Resolve targets; record which (target, type) the frontmatter authors.
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

/// Resolve one parsed link to `(dst_path, dst_resource_path)` — at most one is
/// `Some`; both `None` means dangling. The lookup path is the authored target
/// minus any `#fragment`; kind dispatch is extension-only (see [`project_edges`]).
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

    // Candidate paths, most specific first: a Markdown-form target is
    // note-relative per standard Markdown, falling back to vault-root (the
    // wikilink habit); wikilinks are vault-root only, as today.
    let mut candidates: Vec<String> = Vec::with_capacity(2);
    if link.md_form {
        if let Some(joined) = join_vault_relative(src_dir, lookup) {
            candidates.push(joined);
        }
    }
    if !candidates.iter().any(|c| c == lookup) {
        candidates.push(lookup.to_string());
    }

    // Extension-only kind dispatch, the one rule, shared with the adapters' argument
    // dispatch: an extension other than `md` means resource; `.md` or none means note.
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

/// Join a relative `target` onto `base_dir` (both vault-relative, `/`-separated),
/// normalizing `.` and `..` segments. `None` when the target escapes the vault
/// root — such a path can never resolve, and the vault-root fallback still runs.
fn join_vault_relative(base_dir: &str, target: &str) -> Option<String> {
    let mut segments: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            s => segments.push(s),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

/// Deterministic id for an authored edge from its identity tuple (ADR-0010): stable
/// across re-index, so the same body at the same path always yields the same id — and
/// deliberately *not* stable across a move, since both ends are paths. Edge ids live
/// only in the disposable index and are re-derived by a move's re-projection.
fn derive_edge_id(src_path: &str, target_key: &str, edge_type: &str, occurrence: i64) -> String {
    let mut h = blake3::Hasher::new();
    for part in [src_path, target_key, edge_type] {
        h.update(part.as_bytes());
        h.update(b"\x1f"); // unit separator — avoids field-boundary collisions
    }
    h.update(occurrence.to_string().as_bytes());
    h.finalize().to_hex()[..32].to_string()
}

/// Ingest a single note at `ctx.root/rel_path` against an already-built index
/// (the incremental path). Projects note + chunks + edges. The context's chunking
/// policy is the same one every other path uses, so a single-note re-projection cuts
/// identically to a full rebuild.
pub fn ingest_file(ctx: EmbedCtx, rel_path: &str) -> Result<Ingested> {
    let EmbedCtx { proj, embedder } = ctx;
    let conn = proj.conn;
    db::ensure_embedding_space(conn, embedder.model_id(), embedder.dim())?;
    // Incremental (force=false): a frontmatter-only edit leaves the body unchanged, so
    // this re-projects note + edges without re-embedding. Vector state IS consulted —
    // this path embeds inline, so a note left mid-embed re-chunks and re-embeds here.
    let p = project_note_and_chunks(proj, rel_path, false, true)?;
    let embedded = !p.pending.is_empty();
    // A single-note re-projection is never cancelled — always run to completion.
    embed_pending(conn, embedder, rel_path, &p.pending, |_| {
        ControlFlow::Continue(())
    })?;
    project_edges(conn, rel_path, &p.body, &p.relations)?;
    Ok(Ingested {
        path: rel_path.to_string(),
        embedded,
    })
}

/// The result of a (possibly cancelled) full ingest: every projected note, plus
/// whether the embed phase was cut short. A cancelled run is still **consistent** —
/// every note has chunks + FTS + edges, only a *prefix* has vectors — so `notes`
/// describes the partial work truthfully and a re-run embeds the remainder. Vectors are
/// tracked **per note**: a note interrupted mid-embed is not fully embedded, so its
/// resume re-embeds it in full — at most one note's worth of redo.
pub struct IngestOutcome {
    pub notes: Vec<Ingested>,
    /// The embed phase stopped early because `on_progress` returned
    /// [`ControlFlow::Break`]. Always `false` for a run that was never cancelled.
    pub cancelled: bool,
    /// Files the projection pass skipped as unreadable (see [`SkippedNote`]); empty on
    /// a clean vault. A whole-vault reindex reports these rather than failing on them.
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned by the projection pass (see [`ProjectOutcome`], #31).
    pub notes_pruned: usize,
    /// The resource inventory's counts (see [`ProjectOutcome`]).
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// Ingest every `.md` file under `vault_root` (two-phase, deterministic order),
/// incrementally and with no progress reporting; dot-folders are skipped. A convenience
/// wrapper the test suite drives: it builds the [`EmbedCtx`] itself around the default
/// [`ChunkConfig`], which is why it takes loose arguments.
pub fn ingest_vault(
    conn: &Connection,
    vault_root: &Path,
    embedder: &dyn Embedder,
) -> Result<Vec<Ingested>> {
    let cfg = ChunkConfig::default();
    let ctx = EmbedCtx::new(ProjectionCtx::new(conn, vault_root, &cfg), embedder);
    Ok(ingest_vault_with_progress(ctx, false, &mut |_| ControlFlow::Continue(()))?.notes)
}

/// One note's projection outcome: its vault-relative path, which is its identity
/// (L1) and everything a caller can learn from a pass that writes nothing.
#[derive(Debug, Clone)]
pub struct Projected {
    pub path: String,
}

/// Re-project a single note **model-free** — the single-note sibling of
/// [`project_vault`], and the pass `Vault::write` runs after its body splice. A changed
/// body re-chunks, and the chunks join the DB-derived pending set for any later embed
/// pass, so the save path needs no embedder. Contrast [`ingest_file`], which embeds
/// inline for `add`/`link`/`mv` — ops that already require the model.
pub fn project_file(ctx: ProjectionCtx, rel_path: &str) -> Result<Projected> {
    let p = project_note_and_chunks(ctx, rel_path, false, false)?;
    project_edges(ctx.conn, rel_path, &p.body, &p.relations)?;
    Ok(Projected {
        path: rel_path.to_string(),
    })
}

/// A vault file (note **or** resource) the projection pass could not read, and
/// therefore skipped, so one unreadable file never aborts a whole-vault reindex.
/// `reason` is about the *file itself* ("not valid UTF-8 text", "permission denied"),
/// never a B2 internal, so it is safe to show and to log. Only a *filesystem* failure
/// on one file is recoverable this way; a systemic error still aborts the pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedNote {
    pub path: String,
    pub reason: String,
}

/// Classify an I/O error hit while reading/stamping one note into a short, clean,
/// user-appropriate reason — the file's problem stated plainly, with no raw OS jargon
/// (e.g. "stream did not contain valid UTF-8") and no B2 internal. Anything unusual
/// falls back to a generic "could not be read".
fn skip_reason(err: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::InvalidData => "not valid UTF-8 text".to_string(),
        ErrorKind::PermissionDenied => "permission denied".to_string(),
        ErrorKind::NotFound => "file no longer exists".to_string(),
        _ => "could not be read".to_string(),
    }
}

/// The result of the model-free **projection pass** over the whole vault
/// ([`project_vault`]): every projected note, in the deterministic (sorted-path)
/// walk order, plus any files skipped as unreadable (empty on a clean vault).
#[derive(Debug, Clone)]
pub struct ProjectOutcome {
    pub notes: Vec<Projected>,
    pub skipped: Vec<SkippedNote>,
    /// Ghost rows pruned this pass (#31) — notes whose files were deleted outside
    /// b2 with no replacement. Zero on a vault with no out-of-band deletions.
    pub notes_pruned: usize,
    /// Resources inventoried this pass (unchanged ones included), and stale
    /// inventory rows pruned — the slice-1 resource pass (spec §2).
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// The result of a (possibly cancelled) **embed pass** ([`embed_vault`]): which
/// notes fully embedded this run, and whether a cooperative cancel cut it short.
#[derive(Debug, Clone)]
pub struct EmbedOutcome {
    /// Paths of the notes that fully embedded this run, in the order they were
    /// worked (path order). Notes whose vectors were already complete do no work and
    /// are not listed.
    pub embedded: Vec<String>,
    /// The pass stopped early because `on_progress` returned [`ControlFlow::Break`].
    pub cancelled: bool,
}

/// The **projection pass**: project every `.md` file under `ctx.root` — phase 1
/// (note, chunks, FTS) then phase 2 (the typed edges) — with **no embedder and no embedding
/// space**, so a projected-but-unembedded index is already complete for keyword search
/// and the graph. Incremental: unless `force`, a note is re-chunked only when its body
/// changed or it is new, read purely from `notes` and never from vector state (missing
/// vectors are [`embed_vault`]'s job), so `project(force)` then `embed()` is the full
/// rebuild. It writes nothing to the vault (ADR-0004).
pub fn project_vault(ctx: ProjectionCtx, force: bool) -> Result<ProjectOutcome> {
    let ProjectionCtx { conn, root, .. } = ctx;
    let mut rel_paths = Vec::new();
    let mut resource_files = Vec::new();
    collect_vault_files(root, root, &mut rel_paths, &mut resource_files)?;
    rel_paths.sort();
    resource_files.sort_by(|a, b| a.0.cmp(&b.0)); // paths are unique — a total order

    // Phase 1: project every note + its chunks, which fills the resolver so phase 2
    // never depends on file order. No pending pairs come back — the embed pass derives
    // its work from the DB, so nothing is handed over in memory.
    let mut staged = Vec::with_capacity(rel_paths.len());
    let mut skipped = Vec::new();
    for rel in &rel_paths {
        match project_note_and_chunks(ctx, rel, force, false) {
            Ok(p) => staged.push((rel.clone(), p.body, p.relations)),
            // A note we cannot read is *skipped*, not fatal: one bad file must never
            // abort a whole-vault reindex. Only filesystem failures reading THIS note
            // land here — the DB layer's `Error::Sqlite` is systemic and still aborts.
            // The read fails before any upsert, so no partial row is written.
            Err(Error::Io(e)) => skipped.push(SkippedNote {
                path: rel.clone(),
                reason: skip_reason(&e),
            }),
            Err(other) => return Err(other),
        }
    }

    // Deletion reconciliation (#31): prune the rows of notes whose files are gone, so
    // an incremental reindex converges on what a from-scratch rebuild would hold instead
    // of serving ghosts. "Gone" means "the walk did not meet this path" — a file it saw
    // but could not read is kept, since evicting it would lie. Runs before phase 2 so
    // links at a deleted note re-dangle, exactly as a full rebuild resolves them. The
    // single-note paths touch one note and never prune.
    let seen: HashSet<&str> = staged
        .iter()
        .map(|(path, ..)| path.as_str())
        .chain(skipped.iter().map(|s| s.path.as_str()))
        .collect();
    let notes_pruned = db::prune_notes_except(conn, &seen)?;

    // Vectors are content-addressed (ADR-0006), so they do NOT die with the chunk rows
    // just removed — that survival is what lets a moved note re-use them. Collecting
    // what is now unreferenced belongs here, where the run's chunk set is final, and is
    // guarded so the model-free pass never touches a never-embedded vault.
    if db::embedding_space_exists(conn)? {
        db::prune_orphan_vectors(conn)?;
    }

    // Resource inventory — between the phases so the rows exist before phase 2
    // resolves links (a `![[img.png]]` edge resolves against `resources`, spec §3).
    let (resources_indexed, resources_pruned, mut resource_skips) =
        project_resources(conn, root, &resource_files)?;
    skipped.append(&mut resource_skips);

    // Phase 2: edges, resolved against the now-complete resolver. A skipped note has no
    // rows and no edges; a link pointing at it stays unresolved, as for any absent target.
    let mut notes = Vec::with_capacity(staged.len());
    for (path, body, relations) in staged {
        project_edges(conn, &path, &body, &relations)?;
        notes.push(Projected { path });
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

/// The **embed pass**: fill a vector for every chunk that lacks one. Ensures the
/// embedding space first (a model swap drops and resets it, so *all* chunks then count
/// as missing), then works the DB-derived pending set note by note through the batched
/// [`embed_pending`] loop, firing `on_progress` per batch and honoring its
/// [`ControlFlow::Break`] as the cancel checkpoint. Takes **no `force`**: re-chunking is
/// a projection concern, so this pass is purely "fill what's missing" — which is also
/// why any interruption heals on the next call. Pending notes are counted before any
/// work starts, so progress is determinate from the first batch.
pub fn embed_vault(
    conn: &Connection,
    embedder: &dyn Embedder,
    on_progress: &mut dyn FnMut(ReindexProgress) -> ControlFlow<()>,
) -> Result<EmbedOutcome> {
    db::ensure_embedding_space(conn, embedder.model_id(), embedder.dim())?;

    // Group the (path, seq)-ordered pending chunks by note; consecutive rows share a
    // note, so per-note batching and progress reproduce the fused reindex's shape.
    //
    // A hash is embedded **once per run** however many notes hold that text: the store
    // is content-addressed (ADR-0006). Notes are worked in order and each batch is
    // written before the next note starts, so a de-duplicated note finds its vectors
    // already in the table, completes immediately, and still refreshes its centroid.
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
        // Per-note span: under a span-close subscriber each note reports how long
        // its embed took — the kernel's slowest step, hence the one worth plotting.
        let _note_span = tracing::debug_span!(
            target: "b2::ingest", "embed_note",
            path = path.as_str(), chunks = pending.len()
        )
        .entered();
        let notes_embedded = i + 1; // 1-based position for the progress line
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
            break; // cooperative cancel: stop starting new notes
        }
    }
    // The centroid half of the pass. A note re-cut but textually unchanged has every
    // vector already stored, so it never enters the loop above — yet `replace_chunks`
    // dropped the centroid summarizing its old chunks. Left unrefreshed it would vanish
    // from discovery's coarse scan while looking fully indexed (S3). Runs after a cancel
    // too: the query offers only *fully* embedded notes.
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

/// [`ingest_vault`] with `force` and an `on_progress` callback, so a slow full reindex
/// never looks frozen — **and can be cooperatively cancelled**: returning
/// [`ControlFlow::Break`] stops the embed pass at that batch boundary. Projection
/// completes before embedding starts, so a cancelled index is consistent: keyword search
/// and the graph are complete, only a prefix of notes has vectors.
///
/// A thin composition of [`project_vault`] then [`embed_vault`]. From a clean index the
/// composed run is byte-identical to the old fused one; the sole intentional divergence
/// is a resume-after-partial run, where projection leaves an unchanged note's chunks in
/// place rather than regenerating their rowids — observably identical.
pub fn ingest_vault_with_progress(
    ctx: EmbedCtx,
    force: bool,
    on_progress: &mut dyn FnMut(ReindexProgress) -> ControlFlow<()>,
) -> Result<IngestOutcome> {
    let ProjectOutcome {
        notes: projected_notes,
        skipped,
        notes_pruned,
        resources_indexed,
        resources_pruned,
    } = project_vault(ctx.proj, force)?;
    let embed = embed_vault(ctx.proj.conn, ctx.embedder, on_progress)?;

    // Merge the two outcomes into the per-note report shape `reindex` has always
    // returned: a note "embedded this run" iff the embed pass fully filled it.
    let embedded: HashSet<&str> = embed.embedded.iter().map(String::as_str).collect();
    let notes = projected_notes
        .into_iter()
        .map(|p| Ingested {
            embedded: embedded.contains(p.path.as_str()),
            path: p.path,
        })
        .collect();
    Ok(IngestOutcome {
        notes,
        cancelled: embed.cancelled,
        skipped,
        notes_pruned,
        resources_indexed,
        resources_pruned,
    })
}

/// A **read-only** preview of a reindex (`reindex --dry-run`). Walks every `.md` file
/// in the same order as [`ingest_vault`] and decides, per note, whether a real run would
/// (re)embed its body. That is the *whole* preview, and the shrinkage is the feature:
/// the dry-run existed largely because a real run wrote to the vault, and one that
/// writes nothing (ADR-0004) has only work to forecast.
///
/// The embed decision reads the *currently stored* vectors, so it previews an
/// incremental run under the embedder the index was built with; it does **not** detect a
/// pending model swap, which would need the real model a dry-run avoids loading.
pub fn plan_reindex(conn: &Connection, vault_root: &Path, force: bool) -> Result<Vec<PlannedNote>> {
    let space_exists = db::embedding_space_exists(conn)?;
    let mut rel_paths = Vec::new();
    // The dry-run previews *notes* (the embed decision); the resource inventory has
    // no per-file decisions to preview, so its walk output is unused here.
    let mut resource_files = Vec::new();
    collect_vault_files(vault_root, vault_root, &mut rel_paths, &mut resource_files)?;
    rel_paths.sort();

    let mut out = Vec::with_capacity(rel_paths.len());
    for rel in rel_paths {
        // Skip an unreadable file rather than abort — a real reindex would skip it too,
        // so the dry-run must not be the one place it still crashes the run.
        let raw = match fs::read_to_string(vault_root.join(&rel)) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let body_hash = blake3::hash(note::parse(&raw).body().as_bytes())
            .to_hex()
            .to_string();
        // An unindexed path has no stored body hash, so this reads `true` for a note
        // new to the index — exactly as the real run decides it.
        let would_embed = would_reembed(conn, &rel, &body_hash, force, space_exists)?;
        out.push(PlannedNote {
            path: rel,
            would_embed,
        });
    }
    Ok(out)
}

/// Walk the vault once, routing every file: `.md` (case-insensitive) to `notes`,
/// everything else to `resources` with its class — the `index = projection of the vault
/// directory` walk (ADR-0002).
///
/// **Hidden means hidden** (GH #136): a dot-prefixed entry is not vault material, so
/// [`is_hidden`](crate::pathspec::is_hidden) is applied *above* the note/resource
/// dispatch and the recursion alike. A `.DS_Store` and a `.scratch.md` are equally
/// invisible; the files stay on disk untouched, simply outside the projection.
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
        // `path` was produced by walking `root`, so `strip_prefix` cannot fail;
        // handle it gracefully anyway rather than panic on the invariant.
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

/// The **resource inventory pass**: stat every walked resource, short-circuit on an
/// unchanged `(size, mtime)`, otherwise read the bytes once to blake3 them and upsert;
/// then prune the rows the walk no longer saw (inbound edges re-dangle via the schema's
/// `ON DELETE SET NULL`). Model-free and chunk-free — hashing is the only byte-read. An
/// unreadable file is skipped and any prior row survives: the file was seen on disk, so
/// pruning it would lie. Returns `(indexed, pruned, skipped)`, `indexed` counting
/// unchanged resources too.
fn project_resources(
    conn: &Connection,
    vault_root: &Path,
    resources: &[(String, ResourceClass)],
) -> Result<(usize, usize, Vec<SkippedNote>)> {
    let mut skipped = Vec::new();
    let mut seen: HashSet<String> = HashSet::with_capacity(resources.len());
    let mut indexed = 0;
    for (rel, class) in resources {
        // The walk saw the file, so it exists: it is never pruned this pass, even
        // if reading it fails below.
        seen.insert(rel.clone());
        match project_resource_file(conn, vault_root, rel, *class, false) {
            Ok(()) => indexed += 1,
            // Only a *filesystem* failure on this one file is recoverable — anything
            // else (SQLite, …) is systemic and aborts the pass, as everywhere else.
            Err(Error::Io(e)) => skipped.push(SkippedNote {
                path: rel.clone(),
                reason: skip_reason(&e),
            }),
            Err(e) => return Err(e),
        }
    }
    let pruned = db::prune_resources_except(conn, &seen)?;
    Ok((indexed, pruned, skipped))
}

/// Inventory **one** resource: short-circuit on an unchanged `(size, mtime)`, otherwise
/// read the bytes once to blake3 them and upsert. The per-file kernel
/// [`project_resources`] loops over, and the resource arm of an import.
///
/// `force` skips that short-circuit, and the two callers differ on it because they know
/// different things. The **walk** meets files it has seen before, so an unchanged stat
/// means "the row already describes this" — the optimization that keeps a reindex from
/// re-reading every PDF. An **import** just created the file, so any row at that path is
/// about a *different* file, and trusting a matching stat would keep a `content_hash`
/// for bytes that are gone.
///
/// I/O failures travel as [`Error::Io`] and each caller decides what they mean: the walk
/// classifies one into a skip, the import treats it as the failure it is.
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
        return Ok(()); // unchanged — inventoried without touching the bytes
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

    /// The `(size, mtime)` shortcut is the **walk's**, and an import must not inherit
    /// it: a row can outlive the file it describes, so a same-size replacement landing
    /// inside the same second would keep a `content_hash` for bytes that are gone — and
    /// that hash is what the out-of-band move repair matches a dangling link against.
    ///
    /// The stat equality is **constructed**, not raced for: `set_modified` pins the
    /// replacement's mtime to the original's, so the case under test runs every time.
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

        // Different bytes, same length, same mtime — the one state a stat cannot tell
        // apart from "nothing happened".
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
