//! Ingest (Flow ① of planning/specs/index-engine-build.md): parse → stamp a
//! missing `b2id` (write file + log) → project into `notes`/`note_aliases`,
//! `chunks` (+FTS), and the typed `edges` graph.
//!
//! `ingest_vault` runs in two phases so link resolution never depends on file
//! order: phase 1 projects every note + its chunks (filling the resolver), phase
//! 2 derives edges against the now-complete resolver. `ingest_file` re-projects a
//! single note (note + chunks + edges) against an already-built index — the
//! incremental path, which equals a full rebuild for that note's rows.

use crate::chunk::chunk_body;
use crate::db::{self, EdgeRow, NoteRow};
use crate::embed::Embedder;
use crate::error::Result;
use crate::id::IdGen;
use crate::note;
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
/// `(chunk_id, text)` pairs still needing a vector — embedding is deferred to
/// [`embed_pending`] so it can be **batched** (and, in a full reindex, report
/// progress). No embedder here.
///
/// **Incremental:** unless `force`, a note whose body hash is unchanged *and* whose
/// chunks already all have vectors is left untouched — its chunks/vectors are
/// re-used verbatim and the returned `pending` is empty, so nothing is re-embedded.
/// Frontmatter-only edits still re-project the note row + edges (phase 2), just not
/// the body vectors. This is what makes a routine reindex cheap; the invariant
/// (`incremental ≡ full rebuild`) holds because the re-used vectors are byte-for-byte
/// what a fresh embed would produce.
fn project_note_and_chunks(
    conn: &Connection,
    vault_root: &Path,
    rel_path: &str,
    idgen: &dyn IdGen,
    force: bool,
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

    // Decide the incremental embed BEFORE the upsert overwrites `body_hash`, reading
    // the note's stored hash + vector state. The embedding space is ensured by the
    // caller here, so it always exists (hence `space_exists = true`).
    let reembed = would_reembed(conn, &b2id, &body_hash, force, true)?;

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

    // Incremental fast path: an unchanged body means identical chunks, and if those
    // chunks already all have vectors there is nothing to redo — reuse them and
    // return no pending work (`reembed = false`). `force` bypasses this; so does a
    // model swap, which emptied `chunks_vec` (note_fully_embedded then returns false).
    let pending = if reembed {
        // Chunk → project rows; hand the (id, text) pairs back for a batched embed
        // (Flow ①). replace_chunks also clears any stale vectors for this note.
        let chunks = chunk_body(&body);
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
/// fully embedded (a fresh note, or a model swap emptied `chunks_vec`). Shared by
/// the real ingest and the [`plan_reindex`] dry-run so the preview cannot drift from
/// the run. `space_exists` lets a pristine vault short-circuit without querying a
/// `chunks_vec` that does not exist yet (which would error).
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
struct EmbedOutcome {
    /// `on_batch` returned [`ControlFlow::Break`] at a batch boundary — the caller
    /// should stop starting new notes (a cooperative cancel, async-indexing.md §3).
    cancelled: bool,
    /// Every pending chunk was embedded, so the note is now fully embedded. True even
    /// when the cancel landed on the *final* batch: each batch is written before its
    /// cancel check, so there is nothing left to do for this note.
    completed: bool,
}

/// Embed a note's pending `(chunk_id, text)` pairs into `chunks_vec`, in batches of
/// [`EMBED_BATCH`] via [`Embedder::embed_batch`], calling `on_batch` with each
/// batch's size (so a full reindex can report cumulative progress **and** cooperatively
/// cancel). Chunk vectors are independent, so batch boundaries never change the result.
///
/// The cancel check runs **after** a batch is fully written, so a cancel never tears a
/// batch (async-indexing.md §5.6) — it only stops *further* batches. Returns whether a
/// cancel was seen and whether the note finished embedding (see [`EmbedOutcome`]).
fn embed_pending(
    conn: &Connection,
    embedder: &dyn Embedder,
    pending: &[(i64, String)],
    mut on_batch: impl FnMut(usize) -> ControlFlow<()>,
) -> Result<EmbedOutcome> {
    let total = pending.len();
    let mut done = 0usize;
    for batch in pending.chunks(EMBED_BATCH) {
        let texts: Vec<&str> = batch.iter().map(|(_, t)| t.as_str()).collect();
        let vectors = embedder.embed_batch(&texts)?;
        for ((id, _), v) in batch.iter().zip(&vectors) {
            db::set_chunk_vector(conn, *id, v)?;
        }
        done += batch.len();
        if on_batch(batch.len()).is_break() {
            return Ok(EmbedOutcome {
                cancelled: true,
                completed: done == total,
            });
        }
    }
    Ok(EmbedOutcome {
        cancelled: false,
        completed: true,
    })
}

/// Derive a note's authored edges and project them — the union of **body** links
/// (`origin=inline`) and frontmatter **`relations:`** (`origin=frontmatter`),
/// resolving each target against the current resolver. On overlap (the same
/// `(target, type)` authored in both homes) the **body wins** and the redundant
/// frontmatter entry is dropped (data-model §0/§3). Occurrence is assigned per
/// `(target, type)` over the kept set.
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

    // Resolve targets; record which (target, type) the body already authors.
    let mut body_keys: HashSet<(String, String)> = HashSet::new();
    let mut resolved = Vec::with_capacity(staged.len());
    for (link, origin) in staged {
        let dst_id = db::resolve_link_target(conn, &link.target_path)?;
        let target_key = dst_id.clone().unwrap_or_else(|| link.target_path.clone());
        if origin == "inline" {
            body_keys.insert((target_key.clone(), link.edge_type.clone()));
        }
        resolved.push((link, origin, dst_id, target_key));
    }

    let mut occ: HashMap<(String, String), i64> = HashMap::new();
    let mut rows = Vec::with_capacity(resolved.len());
    for (link, origin, dst_id, target_key) in resolved {
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
            dst_path_raw: link.target_path.clone(),
            r#type: link.edge_type.clone(),
            origin: origin.to_string(),
            explanation: link.explanation.clone(),
            occurrence_index,
        });
    }

    db::replace_authored_edges(conn, src_id, &rows)
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
    // needlessly re-embedding it.
    let (b2id, stamped, body, relations, pending) =
        project_note_and_chunks(conn, vault_root, rel_path, idgen, false)?;
    let embedded = !pending.is_empty();
    // A single-note re-projection is never cancelled — always run to completion.
    embed_pending(conn, embedder, &pending, |_| ControlFlow::Continue(()))?;
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

/// Like [`ingest_vault`], but takes `force` (re-embed every note, even unchanged
/// ones) and calls `on_progress` after every embed batch so a slow full reindex
/// (real model on CPU) never looks frozen — **and can be cooperatively cancelled**:
/// when `on_progress` returns [`ControlFlow::Break`], the embed phase stops at that
/// batch boundary. Phase 2 (edge projection) then still runs for *every* projected
/// note, so a cancelled index is consistent — keyword search + graph are complete,
/// only a prefix of notes has vectors (async-indexing.md §3/§5). The embed phase is
/// batched ([`embed_pending`]) and incremental unless `force`; the two-phase order,
/// idempotency, and determinism are unchanged, and a run that is never cancelled is
/// **byte-identical** to before.
pub fn ingest_vault_with_progress(
    conn: &Connection,
    vault_root: &Path,
    idgen: &dyn IdGen,
    embedder: &dyn Embedder,
    force: bool,
    on_progress: &mut dyn FnMut(ReindexProgress) -> ControlFlow<()>,
) -> Result<IngestOutcome> {
    db::ensure_embedding_space(conn, embedder.model_id(), embedder.dim())?;

    let mut rel_paths = Vec::new();
    collect_md_files(vault_root, vault_root, &mut rel_paths)?;
    rel_paths.sort();
    let notes_total = rel_paths.len();

    // Phase 1: project every note + its chunks (this fills the link resolver for
    // every note). Stage each note's pending vectors instead of embedding inline:
    // the progress line below counts the notes that *actually* embed, and that
    // denominator isn't known until every note's incremental decision is made — so
    // all projecting precedes any embedding.
    let mut staged = Vec::with_capacity(notes_total);
    for rel in &rel_paths {
        let (b2id, stamped, body, relations, pending) =
            project_note_and_chunks(conn, vault_root, rel, idgen, force)?;
        staged.push((rel.clone(), b2id, stamped, body, relations, pending));
    }

    // Phase 1b: embed the staged pending vectors (batched, incremental). Progress is
    // reported per batch in terms of the notes that do work — an incremental run
    // reuses most notes' vectors, so those contribute no chunks and no progress line.
    let notes_to_embed = staged
        .iter()
        .filter(|(_, _, _, _, _, pending)| !pending.is_empty())
        .count();
    let mut embedded_so_far = 0usize;
    let mut chunks_done = 0usize;
    let mut cancelled = false;
    // Did each staged note fully (re)embed *this run*? Notes with no pending work stay
    // `false` (they reused vectors — didn't embed now), which matches the old
    // `!pending.is_empty()` on the non-cancel path and stays honest under a cancel.
    let mut embedded_flags = vec![false; staged.len()];
    for (i, (rel, _, _, _, _, pending)) in staged.iter().enumerate() {
        if pending.is_empty() {
            continue;
        }
        embedded_so_far += 1;
        let notes_embedded = embedded_so_far; // immutable per-note snapshot for the closure
        let note_chunks = pending.len();
        let outcome = embed_pending(conn, embedder, pending, |n| {
            chunks_done += n;
            on_progress(ReindexProgress {
                note_path: rel.clone(),
                note_chunks,
                notes_embedded,
                notes_to_embed,
                chunks_done,
            })
        })?;
        embedded_flags[i] = outcome.completed;
        if outcome.cancelled {
            cancelled = true;
            break; // cooperative cancel: stop starting new notes
        }
    }

    // Phase 2: edges (resolve links against the now-complete resolver). Runs for every
    // projected note **even after a cancel**, so the keyword index + typed graph are
    // always complete at any cancel point (async-indexing.md §5.1); only vectors are
    // partial.
    let mut out = Vec::with_capacity(staged.len());
    for (i, (_, b2id, stamped, body, relations, _)) in staged.iter().enumerate() {
        project_edges(conn, b2id, body, relations)?;
        out.push(Ingested {
            b2id: b2id.clone(),
            stamped: *stamped,
            embedded: embedded_flags[i],
        });
    }
    Ok(IngestOutcome {
        notes: out,
        cancelled,
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
    collect_md_files(vault_root, vault_root, &mut rel_paths)?;
    rel_paths.sort();

    let mut out = Vec::with_capacity(rel_paths.len());
    for rel in rel_paths {
        let raw = fs::read_to_string(vault_root.join(&rel))?;
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

fn collect_md_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let is_dotdir = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'));
            if !is_dotdir {
                collect_md_files(root, &path, out)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            // `path` was produced by walking `root`, so `strip_prefix` cannot fail;
            // handle it gracefully anyway rather than panic on the invariant.
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}
