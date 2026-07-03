//! Shared validation for a user-supplied vault-relative Markdown destination —
//! the one concern `b2 mv` (a move destination) and `b2 add` (a new-note path)
//! have in common. Kept error-type-free (returns `Err(reason)` as a plain string)
//! so each authoring op maps the reason onto its own [`crate::Error`] variant and
//! its own user-facing phrasing, without the two coupling through a shared error.

/// Normalize + validate `input` into a vault-relative `.md` path. Trims, switches
/// backslashes to `/` (so `notes.path` stays in the index's one separator
/// convention), and appends `.md` if the user omitted it. Rejects an empty,
/// absolute, or vault-escaping (`..`) path, returning the reason as `Err(String)`.
pub(crate) fn normalize_rel_md(input: &str) -> Result<String, String> {
    let s = input.trim().replace('\\', "/");
    if s.is_empty() {
        return Err("destination is empty".into());
    }
    if s.starts_with('/') {
        return Err(format!("{s} is absolute; give a vault-relative path"));
    }
    if s.split('/').any(|c| c == "..") {
        return Err(format!("{s} escapes the vault"));
    }
    Ok(if s.ends_with(".md") {
        s
    } else {
        format!("{s}.md")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_md_when_missing_and_keeps_it_when_present() {
        assert_eq!(normalize_rel_md("notes/foo").unwrap(), "notes/foo.md");
        assert_eq!(normalize_rel_md("notes/foo.md").unwrap(), "notes/foo.md");
    }

    #[test]
    fn trims_and_normalizes_separators() {
        assert_eq!(normalize_rel_md("  a\\b  ").unwrap(), "a/b.md");
    }

    #[test]
    fn rejects_empty_absolute_and_escaping() {
        assert!(normalize_rel_md("   ").is_err());
        assert!(normalize_rel_md("/abs/path.md").is_err());
        assert!(normalize_rel_md("../escape.md").is_err());
        assert!(normalize_rel_md("a/../../b.md").is_err());
    }
}
