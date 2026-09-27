//! Create a new note (`b2 add`). B2 authors a new file on request; it never injects
//! structure into an existing note (ADR-0004). Markdown first, then project; the path is
//! the identity (ADR-0003), so nothing else is recorded. `created` is passed in, keeping
//! the core clock-free.

use crate::error::{Error, Result};
use crate::ingest::{self, EmbedCtx, ProjectionCtx};
use crate::note::yaml_quote;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

/// The created note's `.md`-normalized path, its identity (L1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddReport {
    pub path: String,
}

/// Create a note at `path_input` (`.md` optional) with minimal frontmatter and `content`,
/// then project and embed it. Refuses to clobber; creates missing parent folders.
pub fn add_note(
    ctx: EmbedCtx,
    path_input: &str,
    title: Option<&str>,
    content: Option<&str>,
    created: &str,
) -> Result<AddReport> {
    let rel = write_new_note(ctx.proj.root, path_input, title, content, created)?;

    ingest::ingest_file(ctx, &rel)?;
    Ok(AddReport { path: rel })
}

/// The model-free [`add_note`] (the desktop's New note): projected without embedding,
/// as `Vault::write` does. Same refusals.
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

/// Validate, render and write the new file, refusing to clobber. Returns its path.
fn write_new_note(
    vault_root: &Path,
    path_input: &str,
    title: Option<&str>,
    content: Option<&str>,
    created: &str,
) -> Result<String> {
    let rel = crate::pathspec::normalize_rel_md(path_input).map_err(Error::AddDestination)?;
    let doc = render_note(title, content, created);
    // The create itself is the refusal (see `place_new`).
    crate::import::place_new(
        &vault_root.join(&rel),
        || Error::AddTargetExists(rel.clone()),
        |file| file.write_all(doc.as_bytes()),
    )?;
    Ok(rel)
}

/// Render a new note: minimal frontmatter, then the body after a blank line. Seeds only
/// what can't be reconstructed later (`created`, optional `title`); not `type:` (GH #80).
/// The human owns these keys once written.
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
        let blank = render_note(None, Some("\n\n"), "2026-07-03");
        assert_eq!(blank, "---\ncreated: 2026-07-03\n---\n");
    }

    #[test]
    fn does_not_seed_type_ingest_defaults_it() {
        // GH #80.
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
        let out = render_note(Some("Spaced repetition"), Some("Body."), "2026-07-03");
        let parsed = crate::note::parse(&out);
        assert_eq!(parsed.as_str(), out, "renders round-trip losslessly");
        let f = parsed.fields();
        assert!(f.r#type.is_none(), "type is not seeded (GH #80)");
        assert_eq!(f.created.as_deref(), Some("2026-07-03"));
        // `title:` is inert, so check the bytes.
        assert_eq!(
            parsed.frontmatter(),
            Some("title: \"Spaced repetition\"\ncreated: 2026-07-03\n")
        );
    }
}
