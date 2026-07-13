//! Ingest (Flow ① of planning/specs/completed/index-engine-build.md): parse → stamp a
//! missing `b2id` (write file + log) → project into `notes`/`note_aliases`,
//! `chunks` (+FTS), and the typed `edges` graph.
//!
//! A full ingest is **two separately-invokable passes**
//! (planning/specs/completed/projection-embedding-split.md §4): [`project_vault`] — the
//! model-free pass, which runs in two phases so link resolution never depends on
//! file order (phase 1 projects every note + its chunks, filling the resolver;
//! phase 2 derives edges against the now-complete resolver) — and [`embed_vault`] —
//! the model-bound pass, which fills whatever chunks still lack a vector (a pending
//! set **derived from the DB**, never handed over in memory). `ingest_vault` /
//! [`ingest_vault_with_progress`] remain their composition, so a full reindex is
//! unchanged. `ingest_file` re-projects a single note (note + chunks + embeddings +
//! edges, inline) against an already-built index — the incremental path, which
//! equals a full rebuild for that note's rows.

use crate::chunk::{chunk_body, ChunkConfig};
use crate::db::{self, EdgeRow, NoteRow};
use crate::embed::Embedder;
use crate::error::{Error, Result};
use crate::id::IdGen;
use crate::note;
use crate::resource::ResourceClass;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

/// How many chunks to embed per forward pass. Batching lets the real model amortize
/// one matmul over many texts (a CPU win on the reindex hot path); the fake's default
/// `embed_batch` maps 1:1 regardless. Sized to trade that amortization against the
/// **padding waste** of batching short chunks with long ones — the tokenizer pads every
/// chunk in a batch to the batch's *longest*, so an over-large batch runs the whole
/// forward pass at the longest length. Measured on a real (variable-length) vault, 16
/// beat 32 (~40% faster: less padding waste) and 8 (better amortization). It also sets
/// the reindex **cancel granularity**: the cancel flag is checked once per batch
/// (async-indexing.md §3), so a smaller batch means the desktop **Cancel** responds
/// sooner — another reason not to over-size it.
const EMBED_BATCH: usize = 16;

/// Outcome of ingesting one file.
pub struct Ingested {
    pub b2id: String,
    /// Whether B2 had to stamp a missing `b2id` (and thus wrote the file).
    pub stamped: bool,
    /// Whether this note was (re)embedded this run — `false` when an unchanged body
    /// let the incremental path reuse its existing vectors.
    pub embedded: bool,
}

/// What [`project_note_and_chunks`] returns: the note's `b2id`, whether it was
/// stamped, its body, its frontmatter relations, and the `(chunk_id, text)` pairs
/// still needing a vector. A named alias so the 5-tuple stays readable.
type ProjectedNote = (String, bool, String, Vec<String>, Vec<(i64, String)>);

/// One note's entry in a [`plan_reindex`] preview (the `reindex --dry-run`): what a
/// real reindex *would* do to this file, decided read-only (no writes).
#[derive(Debug, Clone)]
pub struct PlannedNote {
    /// Vault-relative path of the note.
    pub path: String,
    /// A real reindex would stamp a `b2id` (the file currently lacks one).
    pub would_stamp: bool,
    /// A real reindex would (re)embed this note's body (changed, fresh, or forced).
    pub would_embed: bool,
}

/// Progress during the embed phase of a full reindex, reported **per batch** so a
/// large vault never looks frozen while it embeds (the one genuinely slow step
/// under a real model). Purely observational — it changes nothing about the result.
///
/// The counts describe the notes that actually (re)embed this run, *not* every note:
/// an incremental reindex reuses most notes' vectors untouched, so `notes_to_embed`
/// is the real unit of work (it equals the report's `embedded` count). Reporting
/// position in the full note list instead would jump to e.g. "note 14/18" while only
/// a handful of notes are doing any work.
///
/// `Serialize` so the desktop host can stream it to the webview over a
/// `tauri::ipc::Channel` (async-indexing.md §4); the field names are the JSON keys the
/// frontend reads.
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

/// Project one note's frontmatter + chunks (everything derivable without resolving
/// links). Returns the note's `b2id`, whether it was stamped, its body (kept so
/// phase 2 can derive edges without re-reading), its frontmatter relations, and the
/// `(chunk_id, text)` pairs still needing a vector — embedding is deferred (to
/// [`embed_pending`] on the inline path, to [`embed_vault`] on the full-vault path).
/// No embedder here.
///
/// **Incremental:** unless `force`, a note whose body hash is unchanged is left
/// untouched — its chunks (and any vectors they carry) are re-used verbatim and the
/// returned `pending` is empty. Frontmatter-only edits still re-project the note
/// row and edges (phase 2), just not the body chunks. This is what makes a routine
/// reindex cheap; the invariant (`incremental ≡ full rebuild`) holds because the
/// re-used rows are byte-for-byte what a fresh projection would produce.
///
/// `consult_vectors` selects the re-chunk predicate. The full-vault projection pass
/// passes `false`: it reads only `notes` (`force || body changed || note is new`),
/// because "unchanged body but missing vectors" is [`embed_vault`]'s job, not a
/// reason to re-chunk — and this is what keeps [`project_vault`] free of the
/// vector tables (projection-embedding-split.md §4). [`ingest_file`] passes `true`
/// (it embeds inline and has ensured the space exists), so a note left mid-embed is
/// also healed by [`would_reembed`]'s vector-state check, exactly as before.
fn project_note_and_chunks(
    conn: &Connection,
    vault_root: &Path,
    rel_path: &str,
    idgen: &dyn IdGen,
    force: bool,
    consult_vectors: bool,
) -> Result<ProjectedNote> {
    let abs = vault_root.join(rel_path);
    let raw = fs::read_to_string(&abs)?;
    let mut parsed = note::parse(&raw);

    let mut stamped = false;
    let b2id = match parsed.fields().b2id.clone() {
        Some(id) => id,
        None => {
            // The one always-allowed write: stamp it into the file. The id lives in the
            // frontmatter, so identity travels with the note — nothing else to record.
            let id = idgen.new_id();
            parsed.stamp_b2id(&id);
            fs::write(&abs, parsed.as_str())?;
            stamped = true;
            id
        }
    };

    let body = parsed.body().to_string();
    let body_hash = blake3::hash(body.as_bytes()).to_hex().to_string();
    let mtime = fs::metadata(&abs)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    // Decide the re-chunk BEFORE the upsert overwrites `body_hash`. The inline path
    // also reads vector state (its caller ensured the space, hence
    // `space_exists = true`); the projection pass reads only `notes`.
    let rechunk = if consult_vectors {
        would_reembed(conn, &b2id, &body_hash, force, true)?
    } else {
        force || db::note_body_hash(conn, &b2id)?.as_deref() != Some(body_hash.as_str())
    };

    let fields = parsed.fields();
    db::upsert_note(
        conn,
        &NoteRow {
            b2id: &b2id,
            path: rel_path,
            // `type` is required by the model; default the projection to "note"
            // for the rare untyped file (the file itself is never modified).
            r#type: fields.r#type.as_deref().unwrap_or("note"),
            title: fields.title.as_deref(),
            description: fields.description.as_deref(),
            created: fields.created.as_deref(),
            updated: fields.updated.as_deref(),
            body_hash: &body_hash,
            mtime,
            aliases: &fields.aliases,
        },
    )?;

    let relations = parsed.fields().relations.clone();

    // Incremental fast path: an unchanged body means identical chunks — reuse them
    // and return no pending work (`rechunk = false`). `force` bypasses this; on the
    // inline path so does a model swap, which emptied the vector tables
    // (note_fully_embedded then returns false).
    let pending = if rechunk {
        // Chunk → project rows; hand the (id, text) pairs back for a batched embed
        // (Flow ①). replace_chunks also clears any stale vectors for this note.
        let chunks = chunk_body(&body, &ChunkConfig::default());
        let chunk_ids = db::replace_chunks(conn, &b2id, &chunks)?;
        chunk_ids
            .into_iter()
            .zip(chunks.into_iter().map(|c| c.text))
            .collect()
    } else {
        Vec::new()
    };

    Ok((b2id, stamped, body, relations, pending))
}

/// Whether a note's body would be (re)embedded this run — the negation of the
/// incremental "unchanged" fast path: true when `force`, when the vault has no
/// embedding space yet (`space_exists = false` → a pristine/never-embedded index),
/// when the stored body hash differs (content changed), or when the note is not
/// fully embedded (a fresh note, or a model swap emptied the vector tables). Shared
/// by the inline single-note ingest ([`ingest_file`]) and the [`plan_reindex`] dry-run.
/// [`project_vault`] deliberately does **not** use it (projection never reads vector
/// state); the dry-run's `would_embed` still predicts the composed project+embed run
/// correctly, since a body-changed *or* vector-missing note both end up embedded.
/// `space_exists` lets a pristine vault short-circuit without querying an
/// `embeddings` table that does not exist yet (which would error).
fn would_reembed(
    conn: &Connection,
    b2id: &str,
    body_hash: &str,
    force: bool,
    space_exists: bool,
) -> Result<bool> {
    if force || !space_exists {
        return Ok(true);
    }
    let unchanged = db::note_body_hash(conn, b2id)?.as_deref() == Some(body_hash)
        && db::note_fully_embedded(conn, b2id)?;
    Ok(!unchanged)
}

/// The result of embedding one note's pending chunks: whether a cancel was signalled
/// at a batch boundary, and whether **every** pending chunk got a vector.
struct NoteEmbedOutcome {
    /// `on_batch` returned [`ControlFlow::Break`] at a batch boundary — the caller
    /// should stop starting new notes (a cooperative cancel, async-indexing.md §3).
    cancelled: bool,
    /// Every pending chunk was embedded, so the note is now fully embedded. True even
    /// when the cancel landed on the *final* batch: each batch is written before its
    /// cancel check, so there is nothing left to do for this note.
    completed: bool,
}

/// Embed a note's pending `(chunk_id, text)` pairs into `embeddings`, in batches of
/// [`EMBED_BATCH`] via [`Embedder::embed_batch`], calling `on_batch` with each
/// batch's size (so a full reindex can report cumulative progress **and** cooperatively
/// cancel). Chunk vectors are independent, so batch boundaries never change the result.
///
/// The cancel check runs **after** a batch is fully written, so a cancel never tears a
/// batch (async-indexing.md §5.6) — it only stops *further* batches. Returns whether a
/// cancel was seen and whether the note finished embedding (see [`NoteEmbedOutcome`]).
///
/// A note that finishes has its **centroid** refreshed from its now-complete stored
/// vectors (`note_centroids` — discovery's coarse stage, #38): the centroid is
/// derived data with the same lifecycle as the vectors themselves, so maintaining it
/// here — the one place vectors are written — means no other pass ever reconciles
/// it. A note cut off mid-embed skips the refresh; its resume completes the vectors
/// and refreshes then. Running this even when `pending` is empty is deliberate: it
/// costs one indexed read and re-derives (or heals a missing) centroid for an
/// already-embedded note.
fn embed_pending(
    conn: &Connection,
    embedder: &dyn Embedder,
    note_b2id: &str,
    pending: &[(i64, String)],
    mut on_batch: impl FnMut(usize) -> ControlFlow<()>,
) -> Result<NoteEmbedOutcome> {
    let total = pending.len();
    let mut done = 0usize;
    let mut cancelled = false;
    for batch in pending.chunks(EMBED_BATCH) {
        let texts: Vec<&str> = batch.iter().map(|(_, t)| t.as_str()).collect();
        let vectors = embedder.embed_batch(&texts)?;
        for ((id, _), v) in batch.iter().zip(&vectors) {
            db::set_chunk_vector(conn, *id, v)?;
        }
        done += batch.len();
        if on_batch(batch.len()).is_break() {
            cancelled = true;
            break;
        }
    }
    let completed = done == total;
    if completed {
        db::refresh_note_centroid(conn, note_b2id)?;
    }
    Ok(NoteEmbedOutcome {
        cancelled,
        completed,
    })
}

/// Derive a note's authored edges and project them — the union of **body** links
/// (`origin=inline`) and frontmatter **`relations:`** (`origin=frontmatter`),
/// resolving each target against the current resolver. On overlap (the same
/// `(target, type)` authored in both homes) the **body wins** and the redundant
/// frontmatter entry is dropped (data-model §0/§3). Occurrence is assigned per
/// `(target, type)` over the kept set.
///
/// Resolution dispatches by the target's **extension** (slice-1 spec §3,
/// research §9b #8): a `.md` or extensionless target resolves against `notes`
/// (the wikilink `+ ".md"` ladder), any other extension against `resources`. A
/// `#fragment` suffix is stripped for the lookup only (`dst_path_raw` keeps the
/// authored text). Markdown-form targets (`[…](path)`) additionally try
/// **note-relative first** — standard Markdown semantics — before vault-root.
fn project_edges(conn: &Connection, src_id: &str, body: &str, relations: &[String]) -> Result<()> {
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

    // The source note's directory — the base for a Markdown-form relative target.
    let src_dir = db::resolve_b2id_to_path(conn, src_id)?
        .as_deref()
        .and_then(|p| p.rsplit_once('/').map(|(dir, _)| dir.to_string()))
        .unwrap_or_default();

    // Resolve targets; record which (target, type) the body already authors.
    let mut body_keys: HashSet<(String, String)> = HashSet::new();
    let mut resolved = Vec::with_capacity(staged.len());
    for (link, origin) in staged {
        let (dst_id, dst_resource_path) = resolve_target(conn, &src_dir, &link)?;
        let target_key = dst_id
            .clone()
            .or_else(|| dst_resource_path.clone())
            .unwrap_or_else(|| link.target_path.clone());
        if origin == "inline" {
            body_keys.insert((target_key.clone(), link.edge_type.clone()));
        }
        resolved.push((link, origin, dst_id, dst_resource_path, target_key));
    }

    let mut occ: HashMap<(String, String), i64> = HashMap::new();
    let mut rows = Vec::with_capacity(resolved.len());
    for (link, origin, dst_id, dst_resource_path, target_key) in resolved {
        let key = (target_key.clone(), link.edge_type.clone());
        if origin == "frontmatter" && body_keys.contains(&key) {
            continue; // inline wins — drop the redundant frontmatter dup
        }
        let occurrence_index = *occ.get(&key).unwrap_or(&0);
        occ.insert(key, occurrence_index + 1);

        rows.push(EdgeRow {
            id: derive_edge_id(src_id, &target_key, &link.edge_type, occurrence_index),
            src_id: src_id.to_string(),
            dst_id,
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

    db::replace_authored_edges(conn, src_id, &rows)
}

/// Resolve one parsed link to `(dst_id, dst_resource_path)` — at most one is
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

    // Extension-only kind dispatch — the one rule, shared with the adapters'
    // argument dispatch (research §9b #8): an extension other than `md` means
    // resource; `.md` or none means note (the wikilink habit writes
    // `[[concepts/memory]]` — extensionless — and the note ladder appends `.md`).
    let is_resource = crate::resource::doc_kind(lookup) == crate::resource::DocKind::Resource;
    for candidate in &candidates {
        if is_resource {
            if let Some(path) = db::resolve_resource_target(conn, candidate)? {
                return Ok((None, Some(path)));
            }
        } else if let Some(id) = db::resolve_link_target(conn, candidate)? {
            return Ok((Some(id), None));
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

/// Deterministic id for an authored edge from its identity tuple (data-model.md
/// §2/§3): `(src, dst|dst_path_raw, type, occurrence)`. Stable across re-index,
/// so the same body always yields the same edge id.
fn derive_edge_id(src_id: &str, target_key: &str, edge_type: &str, occurrence: i64) -> String {
    let mut h = blake3::Hasher::new();
    for part in [src_id, target_key, edge_type] {
        h.update(part.as_bytes());
        h.update(b"\x1f"); // unit separator — avoids field-boundary collisions
    }
    h.update(occurrence.to_string().as_bytes());
    h.finalize().to_hex()[..32].to_string()
}

/// Ingest a single note at `vault_root/rel_path` against an already-built index
/// (the incremental path). Projects note + chunks + edges.
pub fn ingest_file(
    conn: &Connection,
    vault_root: &Path,
    rel_path: &str,
    idgen: &dyn IdGen,
    embedder: &dyn Embedder,
) -> Result<Ingested> {
    db::ensure_embedding_space(conn, embedder.model_id(), embedder.dim())?;
    // Incremental (force=false): a frontmatter-only edit (e.g. a committed relation)
    // leaves the body unchanged, so this re-projects the note + edges without
    // needlessly re-embedding it. Vector state IS consulted (`consult_vectors`):
    // this path embeds inline, so a note left mid-embed re-chunks + re-embeds here.
    let (b2id, stamped, body, relations, pending) =
        project_note_and_chunks(conn, vault_root, rel_path, idgen, false, true)?;
    let embedded = !pending.is_empty();
    // A single-note re-projection is never cancelled — always run to completion.
    embed_pending(conn, embedder, &b2id, &pending, |_| {
        ControlFlow::Continue(())
    })?;
    project_edges(conn, &b2id, &body, &relations)?;
    Ok(Ingested {
        b2id,
        stamped,
        embedded,
    })
}

/// The result of a (possibly cancelled) full ingest: every projected note, plus
/// whether the embed phase was cut short by a cooperative cancel (async-indexing.md
/// §3). A cancelled run is still **consistent** — every note has chunks + FTS + edges
/// (Phase 1/2), only a *prefix* has vectors — so `notes` describes the partial work
/// truthfully (its `embedded` flags count only notes that fully embedded this run) and
/// an incremental re-run embeds the notes the cancel left unfinished. Vectors are
/// tracked **per note**, not per chunk: a note interrupted *mid-embed* (a cancel on a
/// non-final batch) is not fully embedded, so its resume re-embeds it in full — at most
/// one note's worth of redo, never a correctness issue.
pub struct IngestOutcome {
    pub notes: Vec<Ingested>,
    /// The embed phase stopped early because `on_progress` returned
    /// [`ControlFlow::Break`]. Always `false` for a run that was never cancelled.
    pub cancelled: bool,
    /// Files the projection pass skipped as unreadable (see [`SkippedNote`]); empty on
    /// a clean vault. A whole-vault reindex reports these rather than failing on them.
    pub skipped: Vec<SkippedNote>,
    /// The resource inventory's counts (see [`ProjectOutcome`]).
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// Ingest every `.md` file under `vault_root` (two-phase, deterministic order),
/// incrementally (unchanged notes reuse their vectors) and with no progress
/// reporting. Dotfolders (e.g. `.b2/`) are skipped. Never cancelled, so it returns
/// the note list directly.
pub fn ingest_vault(
    conn: &Connection,
    vault_root: &Path,
    idgen: &dyn IdGen,
    embedder: &dyn Embedder,
) -> Result<Vec<Ingested>> {
    Ok(
        ingest_vault_with_progress(conn, vault_root, idgen, embedder, false, &mut |_| {
            ControlFlow::Continue(())
        })?
        .notes,
    )
}

/// One note's projection outcome: its `b2id` and whether a missing `b2id` was
/// stamped (B2's one always-allowed write to the vault, data-model.md §1).
#[derive(Debug, Clone)]
pub struct Projected {
    pub b2id: String,
    pub stamped: bool,
}

/// Re-project a single note at `vault_root/rel_path` **model-free** — the
/// single-note sibling of [`project_vault`], and the pass `Vault::write` runs after
/// its body splice (desktop-editing.md §4): note + chunks (+FTS) + edges, stamping
/// a missing `b2id`, never touching the embedding space. A changed body re-chunks
/// (clearing its stale vectors), and the chunks join the DB-derived pending set for
/// **any** later embed pass to fill — so the save path needs no embedder and no
/// coordination with one. Contrast [`ingest_file`], which embeds inline (the
/// `add`/`link`/`mv` path — those ops already require the model).
pub fn project_file(
    conn: &Connection,
    vault_root: &Path,
    rel_path: &str,
    idgen: &dyn IdGen,
) -> Result<Projected> {
    let (b2id, stamped, body, relations, _pending) =
        project_note_and_chunks(conn, vault_root, rel_path, idgen, false, false)?;
    project_edges(conn, &b2id, &body, &relations)?;
    Ok(Projected { b2id, stamped })
}

/// A vault file (a note **or** a resource) the projection pass could **not** read,
/// and therefore skipped, so one
/// unreadable file never aborts a whole-vault reindex (a real vault holds the odd
/// non-UTF-8 or unreadable file). Carries the vault-relative `path` and a short,
/// user-appropriate `reason` — about the *file itself* ("not valid UTF-8 text",
/// "permission denied"), never a B2 internal — so it is safe both to show and to log.
///
/// Only a *filesystem* failure reading one note is recoverable this way; a systemic
/// error (SQLite, …) still aborts the pass, since it is not about a single file.
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
    /// Resources inventoried this pass (unchanged ones included), and stale
    /// inventory rows pruned — the slice-1 resource pass (spec §2).
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// The result of a (possibly cancelled) **embed pass** ([`embed_vault`]): which
/// notes fully embedded this run, and whether a cooperative cancel cut it short.
#[derive(Debug, Clone)]
pub struct EmbedOutcome {
    /// `b2id`s of the notes that fully embedded this run, in the order they were
    /// worked (path order). Notes whose vectors were already complete do no work and
    /// are not listed.
    pub embedded: Vec<String>,
    /// The pass stopped early because `on_progress` returned [`ControlFlow::Break`].
    pub cancelled: bool,
}

/// The **projection pass** (projection-embedding-split.md §4): project every `.md`
/// file under `vault_root` — Phase 1 (note + chunks + FTS, stamping missing
/// `b2id`s) then Phase 2 (the typed edges) — with **no embedder and no embedding
/// space**: it never creates the vector tables, so it needs neither the model nor
/// its `dim`, and a projected-but-unembedded index is already complete for keyword
/// search and the graph. Incremental: unless `force`, a note is re-chunked only
/// when its body changed or it is new — read purely from `notes`, never from vector
/// state (missing vectors are [`embed_vault`]'s job). Re-chunking a previously
/// embedded note clears its stale vectors (via [`db::replace_chunks`]), so
/// `project(force)` → `embed()` is the full rebuild.
///
/// *Naming note:* the index invariant's "index = projection of Markdown" means the
/// **full** index — this pass plus [`embed_vault`] together. The pass is named for
/// the row-projection it performs ([`project_note_and_chunks`] / [`project_edges`]).
pub fn project_vault(
    conn: &Connection,
    vault_root: &Path,
    idgen: &dyn IdGen,
    force: bool,
) -> Result<ProjectOutcome> {
    let mut rel_paths = Vec::new();
    let mut resource_files = Vec::new();
    collect_vault_files(vault_root, vault_root, &mut rel_paths, &mut resource_files)?;
    rel_paths.sort();
    resource_files.sort_by(|a, b| a.0.cmp(&b.0)); // paths are unique — a total order

    // Phase 1: project every note + its chunks (this fills the link resolver for
    // every note, so phase 2 never depends on file order). The returned pending
    // pairs are deliberately dropped: the embed pass derives its work from the DB
    // (`chunks_missing_vectors`), so nothing is handed over in memory (§2).
    let mut staged = Vec::with_capacity(rel_paths.len());
    let mut skipped = Vec::new();
    for rel in &rel_paths {
        match project_note_and_chunks(conn, vault_root, rel, idgen, force, false) {
            Ok((b2id, stamped, body, relations, _pending)) => {
                staged.push((b2id, stamped, body, relations));
            }
            // A note we cannot read or stamp (non-UTF-8, permission-denied, vanished
            // mid-walk) is *skipped*, not fatal: one bad file must never abort a
            // whole-vault reindex. This catches only filesystem failures reading THIS
            // note — the DB layer surfaces `Error::Sqlite`, a systemic failure that
            // still aborts. No partial row is written for a skipped note, since the
            // read/stamp fails before any `upsert` (§ — the invariant holds).
            Err(Error::Io(e)) => skipped.push(SkippedNote {
                path: rel.clone(),
                reason: skip_reason(&e),
            }),
            Err(other) => return Err(other),
        }
    }

    // Resource inventory — between the phases so the rows exist before phase 2
    // resolves links (a `![[img.png]]` edge resolves against `resources`, spec §3).
    let (resources_indexed, resources_pruned, mut resource_skips) =
        project_resources(conn, vault_root, &resource_files)?;
    skipped.append(&mut resource_skips);

    // Phase 2: edges (resolve links against the now-complete resolver). Only the notes
    // that projected are here, so a skipped note simply has no rows and no edges; a
    // link pointing at it stays unresolved, exactly as for any absent target.
    let mut notes = Vec::with_capacity(staged.len());
    for (b2id, stamped, body, relations) in staged {
        project_edges(conn, &b2id, &body, &relations)?;
        notes.push(Projected { b2id, stamped });
    }
    tracing::debug!(
        target: "b2::ingest",
        notes = notes.len(),
        stamped = notes.iter().filter(|n| n.stamped).count(),
        skipped = skipped.len(),
        resources = resources_indexed,
        resources_pruned,
        force,
        "projection pass complete"
    );
    Ok(ProjectOutcome {
        notes,
        skipped,
        resources_indexed,
        resources_pruned,
    })
}

/// The **embed pass** (projection-embedding-split.md §4): fill a vector for every
/// chunk that lacks one. Ensures the embedding space first (creates the
/// `embeddings` + `note_centroids` tables; a model swap drops + resets them, so
/// *all* chunks then count as missing), then works the DB-derived pending set
/// ([`db::chunks_missing_vectors`])
/// note by note through the batched [`embed_pending`] loop — firing `on_progress`
/// per batch and honoring its [`ControlFlow::Break`] as the cooperative cancel
/// checkpoint (async-indexing.md §3). Takes **no `force`**: re-chunking (which
/// clears vectors) is a projection concern, so this pass is purely "fill what's
/// missing" — which is also why any interruption heals on the next call (§7.2).
///
/// The pending notes are counted before any work starts, so progress is determinate
/// from the first batch.
pub fn embed_vault(
    conn: &Connection,
    embedder: &dyn Embedder,
    on_progress: &mut dyn FnMut(ReindexProgress) -> ControlFlow<()>,
) -> Result<EmbedOutcome> {
    db::ensure_embedding_space(conn, embedder.model_id(), embedder.dim())?;

    // Group the (path, seq)-ordered pending chunks by note; consecutive rows share a
    // note, so per-note batching + progress reproduce the fused reindex's shape.
    // One entry per pending note: `(b2id, path, that note's (chunk_id, text) pairs)`.
    type PendingNote = (String, String, Vec<(i64, String)>);
    let mut by_note: Vec<PendingNote> = Vec::new();
    for (note_b2id, path, chunk_id, text) in db::chunks_missing_vectors(conn)? {
        match by_note.last_mut() {
            Some((last, _, pending)) if *last == note_b2id => pending.push((chunk_id, text)),
            _ => by_note.push((note_b2id, path, vec![(chunk_id, text)])),
        }
    }

    let notes_to_embed = by_note.len();
    tracing::debug!(
        target: "b2::ingest",
        notes_to_embed,
        pending_chunks = by_note.iter().map(|(_, _, p)| p.len()).sum::<usize>(),
        "embed pass starting (DB-derived pending set)"
    );
    let mut embedded = Vec::new();
    let mut chunks_done = 0usize;
    let mut cancelled = false;
    for (i, (b2id, path, pending)) in by_note.iter().enumerate() {
        // Per-note span: under a span-close subscriber each note reports how long
        // its embed took — the kernel's slowest step, hence the one worth plotting.
        let _note_span = tracing::debug_span!(
            target: "b2::ingest", "embed_note",
            path = path.as_str(), chunks = pending.len()
        )
        .entered();
        let notes_embedded = i + 1; // 1-based position for the progress line
        let note_chunks = pending.len();
        let outcome = embed_pending(conn, embedder, b2id, pending, |n| {
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
            embedded.push(b2id.clone());
        }
        if outcome.cancelled {
            cancelled = true;
            break; // cooperative cancel: stop starting new notes
        }
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

/// Like [`ingest_vault`], but takes `force` (re-embed every note, even unchanged
/// ones) and calls `on_progress` after every embed batch so a slow full reindex
/// (real model on CPU) never looks frozen — **and can be cooperatively cancelled**:
/// when `on_progress` returns [`ControlFlow::Break`], the embed pass stops at that
/// batch boundary. Projection (notes + chunks + FTS **and** edges) has completed
/// before embedding starts, so a cancelled index is consistent — keyword search +
/// graph are complete, only a prefix of notes has vectors (async-indexing.md §3/§5).
///
/// A thin composition of [`project_vault`] then [`embed_vault`]
/// (projection-embedding-split.md §4): from a clean index the composed run is
/// byte-identical to the old fused one; the sole intentional divergence is a
/// resume-after-partial run, where projection leaves an unchanged-body note's
/// chunks in place rather than regenerating their rowids — observably identical
/// (notes, chunk text, FTS, text→vector, edges), only internal rowids differ (§7.1).
pub fn ingest_vault_with_progress(
    conn: &Connection,
    vault_root: &Path,
    idgen: &dyn IdGen,
    embedder: &dyn Embedder,
    force: bool,
    on_progress: &mut dyn FnMut(ReindexProgress) -> ControlFlow<()>,
) -> Result<IngestOutcome> {
    let ProjectOutcome {
        notes: projected_notes,
        skipped,
        resources_indexed,
        resources_pruned,
    } = project_vault(conn, vault_root, idgen, force)?;
    let embed = embed_vault(conn, embedder, on_progress)?;

    // Merge the two outcomes into the per-note report shape `reindex` has always
    // returned: a note "embedded this run" iff the embed pass fully filled it.
    let embedded: HashSet<&str> = embed.embedded.iter().map(String::as_str).collect();
    let notes = projected_notes
        .into_iter()
        .map(|p| {
            let was_embedded = embedded.contains(p.b2id.as_str());
            Ingested {
                b2id: p.b2id,
                stamped: p.stamped,
                embedded: was_embedded,
            }
        })
        .collect();
    Ok(IngestOutcome {
        notes,
        cancelled: embed.cancelled,
        skipped,
        resources_indexed,
        resources_pruned,
    })
}

/// A **read-only** preview of a reindex — the `reindex --dry-run`. Walks every `.md`
/// file (same sorted order + dotfolder skip as [`ingest_vault`]) and decides, per
/// note, whether a real run would stamp a `b2id` (the file lacks one) and would
/// (re)embed its body — with **no writes at all**: no stamp to the Markdown (B2's
/// one vault write, data-model.md §1), no index or log mutation, no embedding. So a
/// user can preview an (incremental) reindex against a pristine vault.
///
/// The embed decision reads the *currently stored* vectors, so it previews an
/// incremental run under the embedder the index was built with; it does **not**
/// detect a pending model swap (that needs the real model loaded, which a dry-run
/// deliberately avoids). Needs no embedder — a pure read, like the graph queries.
pub fn plan_reindex(conn: &Connection, vault_root: &Path, force: bool) -> Result<Vec<PlannedNote>> {
    let space_exists = db::embedding_space_exists(conn)?;
    let mut rel_paths = Vec::new();
    // The dry-run previews *notes* (stamp/embed decisions); the resource inventory
    // has no per-file decisions to preview, so its walk output is unused here.
    let mut resource_files = Vec::new();
    collect_vault_files(vault_root, vault_root, &mut rel_paths, &mut resource_files)?;
    rel_paths.sort();

    let mut out = Vec::with_capacity(rel_paths.len());
    for rel in rel_paths {
        // Skip an unreadable file rather than abort the preview — a real reindex would
        // skip it too (see [`project_vault`]), so the dry-run must not be the one place
        // a non-UTF-8 or unreadable note still crashes the whole run.
        let raw = match fs::read_to_string(vault_root.join(&rel)) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let parsed = note::parse(&raw);
        let would_stamp = parsed.fields().b2id.is_none();
        let body_hash = blake3::hash(parsed.body().as_bytes()).to_hex().to_string();
        // A note with no b2id is new to the index → always (re)embedded; one with a
        // b2id is compared against its stored state, exactly as the real run decides.
        let would_embed = match &parsed.fields().b2id {
            Some(id) => would_reembed(conn, id, &body_hash, force, space_exists)?,
            None => true,
        };
        out.push(PlannedNote {
            path: rel,
            would_stamp,
            would_embed,
        });
    }
    Ok(out)
}

/// Walk the vault once, routing every file: `.md` (case-insensitive) → `notes`,
/// everything else → `resources` with its class, per
/// [`ResourceClass::of_path`] — the `index = projection of (the vault directory)`
/// walk (planning/specs/resources-inventory-graph.md §2). Dot-prefixed
/// **directories** are skipped as always (`.b2/`, `.git/`); dot-prefixed **files**
/// are skipped from the resource inventory (`.DS_Store`, `.gitignore` are not
/// vault material) while the note route keeps its historical behavior.
fn collect_vault_files(
    root: &Path,
    dir: &Path,
    notes: &mut Vec<String>,
    resources: &mut Vec<(String, ResourceClass)>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let is_dotdir = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'));
            if !is_dotdir {
                collect_vault_files(root, &path, notes, resources)?;
            }
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
            Some(class) => {
                let is_dotfile = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'));
                if !is_dotfile {
                    resources.push((rel, class));
                }
            }
        }
    }
    Ok(())
}

/// The **resource inventory pass** (slice-1 spec §2): stat every walked resource,
/// short-circuit on an unchanged `(size, mtime)`, otherwise read the bytes once to
/// blake3 them, and upsert the row; then prune the rows the walk no longer saw
/// (inbound edges re-dangle via the schema's `ON DELETE SET NULL`). Model-free and
/// chunk-free — hashing is the only byte-read. An unreadable file is *skipped*
/// (reported, never fatal), and any prior row it had survives: the file was seen
/// on disk, so pruning it would lie.
///
/// Returns `(indexed, pruned, skipped)` where `indexed` counts the resources
/// inventoried this pass (unchanged ones included — the mirror of the note
/// `indexed` count).
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
        let abs = vault_root.join(rel);
        let meta = match fs::metadata(&abs) {
            Ok(m) => m,
            Err(e) => {
                skipped.push(SkippedNote {
                    path: rel.clone(),
                    reason: skip_reason(&e),
                });
                continue;
            }
        };
        let size = meta.len() as i64;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        if db::resource_stat(conn, rel)? == Some((size, mtime)) {
            indexed += 1; // unchanged — inventoried without touching the bytes
            continue;
        }
        let bytes = match fs::read(&abs) {
            Ok(b) => b,
            Err(e) => {
                skipped.push(SkippedNote {
                    path: rel.clone(),
                    reason: skip_reason(&e),
                });
                continue;
            }
        };
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
        indexed += 1;
    }
    let pruned = db::prune_resources_except(conn, &seen)?;
    Ok((indexed, pruned, skipped))
}
