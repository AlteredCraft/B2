//! Ingest (Flow ① of index-engine.md): parse → project into `notes`/`note_aliases`,
//! `chunks` (+FTS), and the typed `edges` graph, all keyed by the note's
//! vault-relative **path** (invariants L1, GH #170).
//!
//! **Ingest writes nothing to the vault** (W1). It used to make exactly one write —
//! stamping a missing `b2id` into a note's frontmatter on first sight — and that
//! stamp is what a whole subsystem hung off: a claim pre-scan and incumbent-wins
//! collision resolution, identity-restamp surfacing, and a single-note steal guard.
//! With identity path-keyed, none of those states are representable (the filesystem
//! already gives one file per path), so this module is a pure read of the vault.
//!
//! A full ingest is **two separately-invokable passes**
//! (index-engine.md): [`project_vault`] — the
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
/// the reindex **cancel granularity**: the cancel flag is checked once per batch,
/// so a smaller batch means the desktop **Cancel** responds
/// sooner — another reason not to over-size it.
const EMBED_BATCH: usize = 16;

/// Everything a **projection** needs: the index connection, the vault root, and the
/// vault's chunking policy. The three travel together through every write-side op —
/// `mv`, `add`, `rm`, `link`, the save path — so they are one parameter rather than
/// three in two orders ([#134](https://github.com/AlteredCraft/B2/issues/134)).
/// (It was four before GH #170: the id generator went with the `b2id` stamp, since
/// nothing in the core mints anything any more — a note's identity is where it sits.)
///
/// It carries a **posture**, and that is the point: an op that takes a `ProjectionCtx`
/// holds no embedder and therefore *cannot* embed. The model-free rule the module docs
/// state for `rm`/`create_note`/`write` is now the type system's to keep.
///
/// A short-lived, `Copy`, borrow-only view struct passed into a call and never stored
/// — the sanctioned exception to "prefer owned fields" (CLAUDE.md, Rust data modeling;
/// the `NoteRow` precedent). Built by `Vault::ctx`; the integration tests build their
/// own with [`ProjectionCtx::new`].
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
/// half of #134's split. The ops that re-embed what they touch (`reindex`, `add`,
/// `link`, `mv`) take this; the model-free ones take the projection context alone, so
/// the two postures are distinguishable at a glance and unmixable by the compiler.
///
/// Same view-struct rules as [`ProjectionCtx`] (`Copy`, borrow-only, never stored).
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

/// One note's entry in a [`plan_reindex`] preview (the `reindex --dry-run`): what a
/// real reindex *would* do to this file, decided read-only.
///
/// Only one question is left to preview. Before GH #170 the dry-run also answered
/// "which notes would be stamped", "which stamps would churn an identity", and
/// "which files collide" — a preview worth having because a real run *wrote* to the
/// vault. A run that writes nothing (W1) has nothing to warn about beyond the work
/// it will do.
#[derive(Debug, Clone)]
pub struct PlannedNote {
    /// Vault-relative path of the note.
    pub path: String,
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
/// `tauri::ipc::Channel`; the field names are the JSON keys the
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
/// links). Returns its body (kept so phase 2 can derive edges without re-reading),
/// its frontmatter relations, and the `(text_hash, text)` pairs still needing a
/// vector — embedding is deferred (to [`embed_pending`] on the inline path, to
/// [`embed_vault`] on the full-vault path). No embedder here, and **no write to the
/// vault**: the note is identified by `rel_path`, so there is nothing to stamp.
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
/// vector tables (index-engine.md). [`ingest_file`] passes `true`
/// (it embeds inline and has ensured the space exists), so a note left mid-embed is
/// also healed by [`would_reembed`]'s vector-state check, exactly as before.
///
/// `ctx.cfg` is the chunking policy (spec §3 D5) — threaded from the caller
/// (ultimately the `Vault`, which defaults it) so *every* path that chunks a given
/// vault cuts identically and `incremental ≡ full rebuild` holds by construction. The
/// retrieval eval injects non-default configs here to A/B chunker levers in one
/// process (the eval harness, crates/b2-embed/evals/).
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
    // A note's display title is its **filename** (data-model.md §1) — a frontmatter
    // `title:` is inert. Projected into `notes.title` so every read path (both
    // adapters, search, neighbors, discovery) shows the filename with no per-call
    // derivation; the column stays populated (never NULL) for an indexed note.
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

    // Incremental fast path: an unchanged body means identical chunks — reuse them
    // and return no pending work (`rechunk = false`). `force` bypasses this; on the
    // inline path so does a model swap, which emptied the vector tables
    // (note_fully_embedded then returns false).
    //
    // A re-chunk hands back `(text_hash, text)` pairs for a batched embed (Flow ①),
    // and only for chunks with **no stored vector**: the store is content-addressed
    // (M4), so re-chunking a note whose text is unchanged — the case a move produces
    // — finds every vector already there and yields nothing pending at all.
    let pending = if rechunk {
        let chunks = chunk_body(&body, cfg);
        db::replace_chunks(conn, rel_path, &chunks)?;
        if consult_vectors {
            pending_for_note(conn, rel_path)?
        } else {
            // The projection pass never reads vector state (index-engine.md); the
            // embed pass derives its own pending set from the DB.
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

/// The `(text_hash, text)` pairs of one note's chunks that still lack a stored
/// vector — the single-note form of [`db::chunks_missing_vectors`], for the inline
/// path that embeds as it projects. Its own query rather than a filter over the
/// whole-vault set, because `move_dir` calls this once per moved note and the
/// filtered version would make that O(vault × moved). Requires the embedding space
/// to exist.
fn pending_for_note(conn: &Connection, note_path: &str) -> Result<Vec<(String, String)>> {
    Ok(db::note_chunks_missing_vectors(conn, note_path)?
        .into_iter()
        .map(|c| (c.text_hash, c.text))
        .collect())
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

/// Embed a note's pending `(text_hash, text)` pairs into `embeddings`, in batches of
/// [`EMBED_BATCH`] via [`Embedder::embed_batch`], calling `on_batch` with each
/// batch's size (so a full reindex can report cumulative progress **and** cooperatively
/// cancel). Chunk vectors are independent, so batch boundaries never change the result.
///
/// The cancel check runs **after** a batch is fully written, so a cancel never tears a
/// batch — it only stops *further* batches. Returns whether a
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
/// (`origin=inline`, all untyped `references`) and frontmatter **`b2_relations:`**
/// (`origin=frontmatter`, the sole typed home), resolving each target against the
/// current resolver. A frontmatter entry whose verb differs from `references`
/// simply coexists with a body link to the same target (the augment case). On
/// overlap (the same `(target, type)` authored in both homes — necessarily
/// `references`) the **frontmatter entry wins** and the redundant body reference
/// is dropped: only the frontmatter row can carry an explanation (data-model
/// §0/§3). Occurrence is assigned per `(target, type)` over the kept set.
///
/// Resolution dispatches by the target's **extension** (slice-1 spec §3,
/// research §9b #8): a `.md` or extensionless target resolves against `notes`
/// (the wikilink `+ ".md"` ladder), any other extension against `resources`. A
/// `#fragment` suffix is stripped for the lookup only (`dst_path_raw` keeps the
/// authored text). Markdown-form targets (`[…](path)`) additionally try
/// **note-relative first** — standard Markdown semantics — before vault-root.
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

    // The source note's directory — the base for a Markdown-form relative target.
    // Read straight off the path now that the path *is* the identity (GH #170); this
    // was a resolver round-trip through the index before.
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

/// Deterministic id for an authored edge from its identity tuple (data-model.md
/// §2/§3): `(src path, dst path|dst_path_raw, type, occurrence)`. Stable across
/// re-index, so the same body at the same path always yields the same edge id — and
/// deliberately *not* stable across a move, since both ends are now paths (L1). Edge
/// ids live only in the disposable index and are re-derived by the re-projection a
/// move performs, so that is bookkeeping rather than a lost handle.
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
    // Incremental (force=false): a frontmatter-only edit (e.g. a committed relation)
    // leaves the body unchanged, so this re-projects the note + edges without
    // needlessly re-embedding it. Vector state IS consulted (`consult_vectors`):
    // this path embeds inline, so a note left mid-embed re-chunks + re-embeds here.
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
/// whether the embed phase was cut short by a cooperative cancel. A cancelled run is
/// still **consistent** — every note has chunks + FTS + edges
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
    /// Ghost rows pruned by the projection pass (see [`ProjectOutcome`], #31).
    pub notes_pruned: usize,
    /// The resource inventory's counts (see [`ProjectOutcome`]).
    pub resources_indexed: usize,
    pub resources_pruned: usize,
}

/// Ingest every `.md` file under `vault_root` (two-phase, deterministic order),
/// incrementally (unchanged notes reuse their vectors) and with no progress
/// reporting. Dotfolders (e.g. `.b2/`) are skipped. Never cancelled, so it returns
/// the note list directly. A convenience wrapper (what the test suite drives): it
/// builds the [`EmbedCtx`] itself around the **default** [`ChunkConfig`], which is
/// why it still takes loose arguments; callers with a non-default policy build their
/// own context and use [`ingest_vault_with_progress`].
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

/// Re-project a single note at `ctx.root/rel_path` **model-free** — the
/// single-note sibling of [`project_vault`], and the pass `Vault::write` runs after
/// its body splice: note + chunks (+FTS) + edges, never touching the embedding
/// space. A changed body re-chunks, and the chunks join the DB-derived pending set
/// for **any** later embed pass to fill — so the save path needs no embedder and no
/// coordination with one. Contrast [`ingest_file`], which embeds inline (the
/// `add`/`link`/`mv` path — those ops already require the model).
pub fn project_file(ctx: ProjectionCtx, rel_path: &str) -> Result<Projected> {
    let p = project_note_and_chunks(ctx, rel_path, false, false)?;
    project_edges(ctx.conn, rel_path, &p.body, &p.relations)?;
    Ok(Projected {
        path: rel_path.to_string(),
    })
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

/// The **projection pass** (index-engine.md): project every `.md`
/// file under `ctx.root` — Phase 1 (note + chunks + FTS) then Phase 2 (the typed
/// edges) — with **no embedder and no embedding space**: it never creates the vector
/// tables, so it needs neither the model nor its `dim`, and a
/// projected-but-unembedded index is already complete for keyword search and the
/// graph. Incremental: unless `force`, a note is re-chunked only when its body
/// changed or it is new — read purely from `notes`, never from vector state (missing
/// vectors are [`embed_vault`]'s job), so `project(force)` → `embed()` is the full
/// rebuild.
///
/// It writes nothing to the vault at all (W1): the pass reads files and fills tables.
///
/// *Naming note:* the index invariant's "index = projection of Markdown" means the
/// **full** index — this pass plus [`embed_vault`] together. The pass is named for
/// the row-projection it performs ([`project_note_and_chunks`] / [`project_edges`]).
pub fn project_vault(ctx: ProjectionCtx, force: bool) -> Result<ProjectOutcome> {
    let ProjectionCtx { conn, root, .. } = ctx;
    let mut rel_paths = Vec::new();
    let mut resource_files = Vec::new();
    collect_vault_files(root, root, &mut rel_paths, &mut resource_files)?;
    rel_paths.sort();
    resource_files.sort_by(|a, b| a.0.cmp(&b.0)); // paths are unique — a total order

    // Phase 1: project every note + its chunks (this fills the resolver for every
    // note, so phase 2 never depends on file order). No pending pairs come back —
    // the embed pass derives its work from the DB (`chunks_missing_vectors`), so
    // nothing is handed over in memory (§2).
    let mut staged = Vec::with_capacity(rel_paths.len());
    let mut skipped = Vec::new();
    for rel in &rel_paths {
        match project_note_and_chunks(ctx, rel, force, false) {
            Ok(p) => staged.push((rel.clone(), p.body, p.relations)),
            // A note we cannot read (non-UTF-8, permission-denied, vanished mid-walk)
            // is *skipped*, not fatal: one bad file must never abort a whole-vault
            // reindex. This catches only filesystem failures reading THIS note — the
            // DB layer surfaces `Error::Sqlite`, a systemic failure that still aborts.
            // No partial row is written for a skipped note, since the read fails
            // before any `upsert` (§ — the invariant holds).
            Err(Error::Io(e)) => skipped.push(SkippedNote {
                path: rel.clone(),
                reason: skip_reason(&e),
            }),
            Err(other) => return Err(other),
        }
    }

    // Deletion reconciliation (#31): prune the rows of notes whose files are gone —
    // deleted outside b2 with no replacement — so an incremental reindex converges on
    // what a from-scratch rebuild would hold instead of serving ghosts to
    // `list_notes`/search/`similar`/the graph. "Gone" means "the walk did not meet
    // this path": every note it read was staged above, and a file it *saw* but could
    // not read is kept (evicting it would lie). Runs before phase 2 so edges
    // re-derive against the pruned resolver and links at a deleted note re-dangle,
    // exactly as a full rebuild resolves them. Whole-vault only: the single-note
    // paths (`ingest_file`/`project_file`) touch one note and never prune.
    let seen: HashSet<&str> = staged
        .iter()
        .map(|(path, ..)| path.as_str())
        .chain(skipped.iter().map(|s| s.path.as_str()))
        .collect();
    let notes_pruned = db::prune_notes_except(conn, &seen)?;

    // Vectors are content-addressed (M4), so they do NOT die with the chunk rows
    // that pruning and re-chunking just removed — that survival is what lets a moved
    // note re-use them. Collecting what is now unreferenced is therefore this pass's
    // job, and it belongs here, where the chunk set for the run is final. Guarded on
    // the space existing so the model-free pass still never touches a vault that has
    // never been embedded.
    if db::embedding_space_exists(conn)? {
        db::prune_orphan_vectors(conn)?;
    }

    // Resource inventory — between the phases so the rows exist before phase 2
    // resolves links (a `![[img.png]]` edge resolves against `resources`, spec §3).
    let (resources_indexed, resources_pruned, mut resource_skips) =
        project_resources(conn, root, &resource_files)?;
    skipped.append(&mut resource_skips);

    // Phase 2: edges (resolve links against the now-complete resolver). Only the notes
    // that projected are here, so a skipped note simply has no rows and no edges; a
    // link pointing at it stays unresolved, exactly as for any absent target.
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

/// The **embed pass** (index-engine.md): fill a vector for every
/// chunk that lacks one. Ensures the embedding space first (creates the
/// `embeddings` + `note_centroids` tables; a model swap drops + resets them, so
/// *all* chunks then count as missing), then works the DB-derived pending set
/// ([`db::chunks_missing_vectors`])
/// note by note through the batched [`embed_pending`] loop — firing `on_progress`
/// per batch and honoring its [`ControlFlow::Break`] as the cooperative cancel
/// checkpoint. Takes **no `force`**: re-chunking (which
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
    // One entry per pending note: `(path, that note's (text_hash, text) pairs)`.
    //
    // A hash is embedded **once per run**, however many notes hold that text: the
    // store is content-addressed (M4), so the second note's chunk is already served
    // by the first note's vector. Notes are worked in order and each batch is written
    // before the next note starts, so by the time a de-duplicated note is reached its
    // vectors are in the table — its (now empty) pending list completes immediately
    // and its centroid still refreshes off the shared rows.
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
/// graph are complete, only a prefix of notes has vectors.
///
/// A thin composition of [`project_vault`] then [`embed_vault`]
/// (index-engine.md): from a clean index the composed run is
/// byte-identical to the old fused one; the sole intentional divergence is a
/// resume-after-partial run, where projection leaves an unchanged-body note's
/// chunks in place rather than regenerating their rowids — observably identical
/// (notes, chunk text, FTS, text→vector, edges), only internal rowids differ (§7.1).
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

/// A **read-only** preview of a reindex — the `reindex --dry-run`. Walks every `.md`
/// file (same sorted order + dotfolder skip as [`ingest_vault`]) and decides, per
/// note, whether a real run would (re)embed its body.
///
/// Since GH #170 that is the *whole* preview, and the shrinkage is the feature: the
/// dry-run existed largely because a real run wrote to the vault (a `b2id` stamp,
/// possibly churning an identity, possibly shadowing a colliding copy), and "show me
/// before you touch my files" is a fair thing to ask. A run that writes nothing (W1)
/// has nothing to warn about — only work to forecast.
///
/// The embed decision reads the *currently stored* vectors, so it previews an
/// incremental run under the embedder the index was built with; it does **not**
/// detect a pending model swap (that needs the real model loaded, which a dry-run
/// deliberately avoids). Needs no embedder — a pure read, like the graph queries.
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
        // Skip an unreadable file rather than abort the preview — a real reindex would
        // skip it too (see [`project_vault`]), so the dry-run must not be the one place
        // a non-UTF-8 or unreadable note still crashes the whole run.
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

/// Walk the vault once, routing every file: `.md` (case-insensitive) → `notes`,
/// everything else → `resources` with its class, per
/// [`ResourceClass::of_path`] — the `index = projection of (the vault directory)`
/// walk (data-model.md §10).
///
/// **Hidden means hidden** (GH #136): a dot-prefixed entry is not vault material,
/// so [`is_hidden`](crate::pathspec::is_hidden) is applied *above* the
/// note/resource dispatch and the recursion alike — one rule, one place. A
/// `.DS_Store` and a `.scratch.md` are equally invisible: no `b2id` stamp, no
/// chunks, no embeddings, no graph presence. The files stay on disk untouched
/// (W4); they are simply outside the projection (data-model.md §1).
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

/// Inventory **one** resource: short-circuit on an unchanged `(size, mtime)`,
/// otherwise read the bytes once to blake3 them and upsert the row. The per-file
/// kernel [`project_resources`] loops over — so there is one definition of what a
/// resource's row *is* — and the resource arm of an import ([`crate::import`]),
/// where exactly one file arrived and walking the whole vault to notice it would be
/// wasteful. Model-free and chunk-free; hashing is the only byte-read.
///
/// `force` skips that short-circuit, and the two callers differ on it because they know
/// different things. The **walk** meets files it has seen before, so an unchanged
/// `(size, mtime)` means "the row already describes this" — the optimization that keeps
/// a reindex from re-reading every PDF. An **import** just created the file, so any row
/// at that path is about a *different* file (deleted out of band, not yet pruned), and
/// trusting a matching stat would keep a `content_hash` for bytes that are gone. Same
/// shape as [`project_note_and_chunks`]'s `force`, and for the same reason.
///
/// I/O failures travel as [`Error::Io`] and each caller decides what they mean: the
/// walk classifies one into a skip (a real vault holds the odd unreadable file), the
/// import treats it as the failure it is (it just wrote that file).
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
    /// it. A row can outlive the file it describes (deleted out of band, not yet
    /// pruned), so a same-size replacement landing inside the same second would keep a
    /// `content_hash` for bytes that are gone — and that hash is load-bearing: it is
    /// what the out-of-band move repair matches a dangling link against
    /// (data-model.md §10).
    ///
    /// The stat equality is **constructed**, not raced for: `set_modified` pins the
    /// replacement's mtime to the original's, so the case under test is the one that
    /// runs, on every machine, every time. Racing it would be a test that usually
    /// exercises nothing and occasionally fails.
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
