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
use crate::error::Result;
use crate::event::{Event, EventSink};
use crate::id::IdGen;
use crate::note;
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Outcome of ingesting one file.
pub struct Ingested {
    pub b2id: String,
    /// Whether B2 had to stamp a missing `b2id` (and thus wrote the file).
    pub stamped: bool,
}

/// Project one note's frontmatter + chunks (everything derivable without
/// resolving links). Returns the note's `b2id`, whether it was stamped, and its
/// body (kept so phase 2 can derive edges without re-reading).
fn project_note_and_chunks(
    conn: &Connection,
    vault_root: &Path,
    rel_path: &str,
    idgen: &dyn IdGen,
    sink: &dyn EventSink,
) -> Result<(String, bool, String)> {
    let abs = vault_root.join(rel_path);
    let raw = fs::read_to_string(&abs)?;
    let mut parsed = note::parse(&raw);

    let mut stamped = false;
    let b2id = match parsed.fields().b2id.clone() {
        Some(id) => id,
        None => {
            // The one always-allowed write: stamp, persist, then log it.
            let id = idgen.new_id();
            parsed.stamp_b2id(&id);
            fs::write(&abs, parsed.as_str())?;
            sink.append(Event::B2idStamped {
                b2id: id.clone(),
                path: rel_path.to_string(),
            });
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

    db::replace_chunks(conn, &b2id, &chunk_body(&body))?;

    Ok((b2id, stamped, body))
}

/// Derive a note's authored edges from its body and project them, resolving each
/// link target against the current resolver.
fn project_edges(conn: &Connection, src_id: &str, body: &str) -> Result<()> {
    let links = crate::link::parse_links(body);
    let mut occ: HashMap<(String, String), i64> = HashMap::new();
    let mut rows = Vec::with_capacity(links.len());

    for link in &links {
        let dst_id = db::resolve_link_target(conn, &link.target_path)?;
        // Occurrence is keyed by the resolved target (or the raw path while
        // dangling) + type, so the same target+verb twice gets index 0,1,…
        let target_key = dst_id.clone().unwrap_or_else(|| link.target_path.clone());
        let key = (target_key.clone(), link.edge_type.clone());
        let occurrence_index = *occ.get(&key).unwrap_or(&0);
        occ.insert(key, occurrence_index + 1);

        rows.push(EdgeRow {
            id: derive_edge_id(src_id, &target_key, &link.edge_type, occurrence_index),
            src_id: src_id.to_string(),
            dst_id,
            dst_path_raw: link.target_path.clone(),
            r#type: link.edge_type.clone(),
            origin: "inline".to_string(),
            status: "active".to_string(),
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
    sink: &dyn EventSink,
) -> Result<Ingested> {
    let (b2id, stamped, body) = project_note_and_chunks(conn, vault_root, rel_path, idgen, sink)?;
    project_edges(conn, &b2id, &body)?;
    Ok(Ingested { b2id, stamped })
}

/// Ingest every `.md` file under `vault_root` (two-phase, deterministic order).
/// Dotfolders (e.g. `.b2/`) are skipped.
pub fn ingest_vault(
    conn: &Connection,
    vault_root: &Path,
    idgen: &dyn IdGen,
    sink: &dyn EventSink,
) -> Result<Vec<Ingested>> {
    let mut rel_paths = Vec::new();
    collect_md_files(vault_root, vault_root, &mut rel_paths)?;
    rel_paths.sort();

    // Phase 1: notes + chunks (fills the resolver for every note).
    let mut staged = Vec::with_capacity(rel_paths.len());
    for rel in &rel_paths {
        let (b2id, stamped, body) = project_note_and_chunks(conn, vault_root, rel, idgen, sink)?;
        staged.push((b2id, stamped, body));
    }

    // Phase 2: edges (resolve links against the now-complete resolver).
    let mut out = Vec::with_capacity(staged.len());
    for (b2id, stamped, body) in &staged {
        project_edges(conn, b2id, body)?;
        out.push(Ingested {
            b2id: b2id.clone(),
            stamped: *stamped,
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
            let rel = path
                .strip_prefix(root)
                .expect("walked path is under root")
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}
