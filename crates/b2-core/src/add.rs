//! Create a new note (the `b2 add` kernel op — CRUD's *create*).
//!
//! B2 authors a *new* file here, which is not the same as authoring a human's body: the
//! whole document is B2-minted on request, and its frontmatter is B2's managed zone. What
//! B2 still never does is inject structure into an *existing* note (ADR-0004).
//!
//! **Markdown-first**: write the `.md`, then project it from that source of truth. The
//! note is fully reconstructible from Markdown — a file at a path, and that path is its
//! identity (ADR-0003) — so `add` records nothing durable of its own. The `created` date
//! is passed in, keeping `b2-core` wall-clock-free.

use crate::error::{Error, Result};
use crate::ingest::{self, EmbedCtx, ProjectionCtx};
use crate::note::yaml_quote;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// What [`add_note`] did: the created note's vault-relative path
/// (`.md`-normalized), which is its identity (L1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddReport {
    pub path: String,
}

/// Create a new note at `path_input` (a `.md` suffix is optional and added if missing)
/// with a minimal, valid frontmatter and `content` as its body, then project it.
///
/// Refuses to clobber: [`Error::AddTargetExists`] if a file already sits there,
/// [`Error::AddDestination`] for an empty/absolute/vault-escaping path. Missing parent
/// directories are created, mirroring `mv`. Projection **embeds** the new note's chunks,
/// so the caller must open with the embedder the index was built with.
pub fn add_note(
    ctx: EmbedCtx,
    path_input: &str,
    title: Option<&str>,
    content: Option<&str>,
    created: &str,
) -> Result<AddReport> {
    let rel = write_new_note(ctx.proj.root, path_input, title, content, created)?;

    // 2. Project from that Markdown: chunk + embed the body, and derive any edges
    //    its content authors.
    ingest::ingest_file(ctx, &rel)?;
    Ok(AddReport { path: rel })
}

/// The **model-free** sibling of [`add_note`] — the desktop's New-note action: same file
/// write, but projected through [`ingest::project_file`] with no embedder, the same pass
/// `Vault::write` runs after a save. The new chunks join the pending set for any later
/// embed, and a body-less note has nothing to embed anyway. Same refusals as [`add_note`].
/// Taking a [`ProjectionCtx`] is what makes that posture the type system's to keep.
pub fn create_note(
    ctx: ProjectionCtx,
    path_input: &str,
    title: Option<&str>,
    content: Option<&str>,
    created: &str,
) -> Result<AddReport> {
    let rel = write_new_note(ctx.root, path_input, title, content, created)?;
    ingest::project_file(ctx, &rel)?;
    Ok(AddReport { path: rel })
}

/// The shared create step: validate `path_input`, refuse to clobber, render the
/// minimal frontmatter + body, and write the new file (creating missing parent
/// dirs). Markdown first (step 1 of both entry points). Returns the vault-relative
/// `.md` path — which is also the note's identity, so there is nothing further to
/// mint or record.
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
/// `title` is YAML-quoted and omitted entirely when `None`; `content` is trimmed of
/// trailing newlines and placed after one blank line.
///
/// The template seeds only what can't be reconstructed later: `created` (lost forever if
/// not stamped now) and an optional `title`. `type:` is deliberately not seeded — ingest
/// defaults it to `"note"`, so stamping it would be redundancy (GH #80). No key is
/// `b2`-namespaced: these are seeded courtesies the human owns the moment they exist.
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
        // A note `add` writes must parse back with exactly the fields it set.
        let out = render_note(Some("Spaced repetition"), Some("Body."), "2026-07-03");
        let parsed = crate::note::parse(&out);
        assert_eq!(parsed.as_str(), out, "renders round-trip losslessly");
        let f = parsed.fields();
        assert!(f.r#type.is_none(), "type is not seeded (GH #80)");
        assert_eq!(f.title.as_deref(), Some("Spaced repetition"));
        assert_eq!(f.created.as_deref(), Some("2026-07-03"));
    }
}
