//! Create a new note (the `b2 add` kernel op — note CRUD's *create*).
//!
//! B2 authors a *new* file here — which is not the same as authoring a human's
//! body: the whole document is B2-minted on the user's request, and its frontmatter
//! is B2's managed zone (data-model.md §0/§1). What B2 still never does is inject
//! structure into an *existing* human note; `add` only ever writes a file that did
//! not exist.
//!
//! **Markdown-first**, like [`crate::mv`] and [`crate::vault::Vault::link`]: write the
//! `.md` file, then project it into the index from that source of truth. The new note
//! is stamped its `b2id` by the ordinary ingest path ([`ingest::ingest_file`]) —
//! "stamp on first sight" (§1), one code path for every note's identity. The note is
//! fully reconstructible from Markdown (file on disk, `b2id` inside), so `add` records
//! nothing durable of its own.
//!
//! The `created` date is passed in (the façade's determinism boundary, like the
//! move/link timestamps), keeping `b2-core` wall-clock-free.

use crate::chunk::ChunkConfig;
use crate::embed::Embedder;
use crate::error::{Error, Result};
use crate::id::IdGen;
use crate::ingest;
use crate::note::yaml_quote;
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// What [`add_note`] did: the created note's `b2id` (stamped by ingest) and its
/// vault-relative path (`.md`-normalized).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddReport {
    pub b2id: String,
    pub path: String,
}

/// Create a new note at `path_input` (a vault-relative path; a `.md` suffix is
/// optional and added if missing) with a minimal, valid frontmatter (an optional
/// `title`, and `created`) and `content` as its body, then project it into the index.
///
/// Refuses to clobber: [`Error::AddTargetExists`] if a file already sits at the
/// destination, [`Error::AddDestination`] for an empty/absolute/vault-escaping path
/// (the vault never overwrites, data-model.md §1). Missing parent directories are
/// created, mirroring `mv`.
///
/// Projection **embeds** the new note's chunks, so the caller must open the vault
/// with the same embedder the index was built with (the CLI loads the real model
/// for `add`, as for `reindex`/`link`/`mv`).
#[allow(clippy::too_many_arguments)]
pub fn add_note(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    embedder: &dyn Embedder,
    vault_root: &Path,
    path_input: &str,
    title: Option<&str>,
    content: Option<&str>,
    created: &str,
) -> Result<AddReport> {
    let rel = write_new_note(vault_root, path_input, title, content, created)?;

    // 2. Project from that Markdown: stamp the `b2id`, chunk + embed the body, and
    //    derive any edges its content authors.
    let ingested = ingest::ingest_file(conn, vault_root, &rel, idgen, cfg, embedder)?;
    Ok(AddReport {
        b2id: ingested.b2id,
        path: rel,
    })
}

/// The **model-free** sibling of [`add_note`] — the desktop's New-note action
/// (`Vault::create_note`): same file write, but the projection is
/// [`ingest::project_file`] (chunks + FTS + edges, **no embedder**), the same pass
/// `Vault::write` runs after a save. The new note's chunks join the DB-derived
/// missing-vector set for any later embed/reindex to fill
/// (index-engine.md) — and a body-less note has nothing to
/// embed anyway. Same validation and refusals as [`add_note`].
#[allow(clippy::too_many_arguments)]
pub fn create_note(
    conn: &Connection,
    idgen: &dyn IdGen,
    cfg: &ChunkConfig,
    vault_root: &Path,
    path_input: &str,
    title: Option<&str>,
    content: Option<&str>,
    created: &str,
) -> Result<AddReport> {
    let rel = write_new_note(vault_root, path_input, title, content, created)?;
    let projected = ingest::project_file(conn, vault_root, &rel, idgen, cfg)?;
    Ok(AddReport {
        b2id: projected.b2id,
        path: rel,
    })
}

/// The shared create step: validate `path_input`, refuse to clobber, render the
/// minimal frontmatter + body, and write the new file (creating missing parent
/// dirs). Markdown first (step 1 of both entry points) — the `b2id` is deliberately
/// left off; ingest/projection stamps it on first sight (§1). Returns the
/// vault-relative `.md` path.
fn write_new_note(
    vault_root: &Path,
    path_input: &str,
    title: Option<&str>,
    content: Option<&str>,
    created: &str,
) -> Result<String> {
    let rel = crate::pathspec::normalize_rel_md(path_input).map_err(Error::AddDestination)?;
    let abs = vault_root.join(&rel);
    if abs.exists() {
        return Err(Error::AddTargetExists(rel));
    }
    let doc = render_note(title, content, created);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&abs, doc)?;
    Ok(rel)
}

/// Render a new note's text: a minimal valid frontmatter block followed by the body.
/// `title` is YAML-quoted (so any character is a safe scalar) and omitted entirely
/// when `None`; `content` is trimmed of trailing newlines and, when non-empty,
/// placed after one blank line. The `b2id` is intentionally absent — ingest stamps
/// it (§1), keeping one identity-minting code path for every note.
///
/// The template seeds only what can't be reconstructed later: `created` (deterministic,
/// lost forever if not stamped now) and an optional `title`. `type:` is deliberately
/// *not* seeded — ingest defaults an absent `type` to `"note"` (data-model.md §1), so
/// stamping it here would be pure redundancy (GH #80). And no key is `b2`-namespaced:
/// these are seeded courtesies owned by the human the moment they're written, not keys
/// B2 owns and machines on (`b2id`, `b2_relations:` are the only such keys).
fn render_note(title: Option<&str>, content: Option<&str>, created: &str) -> String {
    let mut s = String::from("---\n");
    if let Some(t) = title {
        s.push_str(&format!("title: {}\n", yaml_quote(t)));
    }
    s.push_str(&format!("created: {created}\n---\n"));
    if let Some(body) = content {
        let body = body.trim_end_matches('\n');
        if !body.is_empty() {
            s.push('\n');
            s.push_str(body);
            s.push('\n');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_full_frontmatter_and_body() {
        let out = render_note(Some("My Title"), Some("Hello world."), "2026-07-03");
        assert_eq!(
            out,
            "---\ntitle: \"My Title\"\ncreated: 2026-07-03\n---\n\nHello world.\n"
        );
    }

    #[test]
    fn omits_title_when_absent_and_body_when_empty() {
        let out = render_note(None, None, "2026-07-03");
        assert_eq!(out, "---\ncreated: 2026-07-03\n---\n");
        // An explicitly-empty content string is treated like no body.
        let blank = render_note(None, Some("\n\n"), "2026-07-03");
        assert_eq!(blank, "---\ncreated: 2026-07-03\n---\n");
    }

    #[test]
    fn does_not_seed_type_ingest_defaults_it() {
        // `type:` is not seeded (GH #80) — the template stamps only what can't be
        // reconstructed later; ingest defaults an absent type to "note".
        let out = render_note(None, None, "2026-07-03");
        assert!(!out.contains("type:"), "{out}");
    }

    #[test]
    fn a_title_with_special_chars_is_quoted_safely() {
        let out = render_note(Some(r#"A: "quoted" \ path"#), None, "2026-07-03");
        assert!(out.contains(r#"title: "A: \"quoted\" \\ path""#), "{out}");
    }

    #[test]
    fn the_rendered_note_round_trips_and_parses_its_fields() {
        // A note `add` writes must parse back with exactly the fields it set (and no
        // b2id yet — ingest stamps that).
        let out = render_note(Some("Spaced repetition"), Some("Body."), "2026-07-03");
        let parsed = crate::note::parse(&out);
        assert_eq!(parsed.as_str(), out, "renders round-trip losslessly");
        let f = parsed.fields();
        assert!(f.r#type.is_none(), "type is not seeded (GH #80)");
        assert_eq!(f.title.as_deref(), Some("Spaced repetition"));
        assert_eq!(f.created.as_deref(), Some("2026-07-03"));
        assert!(f.b2id.is_none(), "b2id is stamped by ingest, not by render");
    }
}
