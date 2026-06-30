//! Ingest one note (Flow ① of planning/specs/index-engine-build.md, the step-1
//! slice): parse → stamp a missing `b2id` (write file + log) → project into
//! `notes`/`note_aliases`. Chunks and edges are layered on in step 2.

use crate::db::{self, NoteRow};
use crate::error::Result;
use crate::event::{Event, EventSink};
use crate::id::IdGen;
use crate::note;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

/// Outcome of ingesting one file.
pub struct Ingested {
    pub b2id: String,
    /// Whether B2 had to stamp a missing `b2id` (and thus wrote the file).
    pub stamped: bool,
}

/// Ingest a single note at `vault_root/rel_path` (`rel_path` is the
/// vault-relative, POSIX-style path stored in `notes.path`).
pub fn ingest_file(
    conn: &Connection,
    vault_root: &Path,
    rel_path: &str,
    idgen: &dyn IdGen,
    sink: &dyn EventSink,
) -> Result<Ingested> {
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

    let body_hash = blake3::hash(parsed.body().as_bytes()).to_hex().to_string();
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

    Ok(Ingested { b2id, stamped })
}

/// Ingest every `.md` file under `vault_root`, in a deterministic order. Dotfolders
/// (e.g. `.b2/`) are skipped.
pub fn ingest_vault(
    conn: &Connection,
    vault_root: &Path,
    idgen: &dyn IdGen,
    sink: &dyn EventSink,
) -> Result<Vec<Ingested>> {
    let mut rel_paths = Vec::new();
    collect_md_files(vault_root, vault_root, &mut rel_paths)?;
    rel_paths.sort();

    let mut ingested = Vec::with_capacity(rel_paths.len());
    for rel in rel_paths {
        ingested.push(ingest_file(conn, vault_root, &rel, idgen, sink)?);
    }
    Ok(ingested)
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
