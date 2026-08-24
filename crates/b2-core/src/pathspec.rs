//! Shared validation for a user-supplied vault-relative destination — the one concern
//! `b2 mv` and `b2 add` have in common. Error-type-free (it returns `Err(reason)` as a
//! plain string) so each authoring op maps the reason onto its own [`crate::Error`]
//! variant without the two coupling. Also home to the shared hidden-path predicate.

/// Whether this entry's *name* is dot-prefixed — the hidden-path predicate shared by the
/// folder walk, the ingest walk, and [`normalize_rel`]. **Hidden means hidden** (GH #136):
/// a dot-prefixed name is not vault material whatever its extension, so a `.scratch.md`
/// is skipped exactly as `.DS_Store` is.
///
/// Asked of the name's **bytes**, not a decoded `&str`: `to_str` answers `None` for a name
/// UTF-8 rejects, which would make `.draft-\xFF.md` *not hidden* and route it to the note
/// collector, where the lossy path fails to reopen and surfaces as a bogus "file no longer
/// exists" skip. A leading `.` is ASCII and `OsStr`'s encoding is ASCII-compatible, so the
/// byte test is exact on every platform.
pub(crate) fn is_hidden(path: &std::path::Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.as_encoded_bytes().starts_with(b"."))
}

/// Normalize + validate `input` into a vault-relative path of any file kind: trim, switch
/// backslashes to `/` (the index keeps one separator convention), and reject an empty,
/// absolute, vault-escaping or **hidden** path, returning the reason as `Err(String)`. The
/// extension is left exactly as given.
///
/// The hidden check is [`is_hidden`]'s rule over every segment, and it lives on the base
/// validator rather than the directory variant alone (GH #136): the walk indexes no
/// dot-prefixed member of any kind, so a destination b2 would create and then never see is
/// a silent fs/index desync.
pub(crate) fn normalize_rel(input: &str) -> Result<String, String> {
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
    if s.split('/').any(|seg| seg.starts_with('.')) {
        return Err(format!(
            "{s} is a hidden path; b2 does not manage dot-prefixed files or folders"
        ));
    }
    Ok(s)
}

/// Normalize + validate `input` into a vault-relative *directory* path — the
/// folder variant of [`normalize_rel`]: same checks, plus a trailing `/` trimmed
/// so `notes/` and `notes` name the same folder.
pub(crate) fn normalize_rel_dir(input: &str) -> Result<String, String> {
    normalize_rel(input.trim().trim_end_matches('/'))
}

/// Normalize + validate `input` into a vault-relative `.md` path — the note
/// variant of [`normalize_rel`]: same checks, plus `.md` appended if omitted.
pub(crate) fn normalize_rel_md(input: &str) -> Result<String, String> {
    let s = normalize_rel(input)?;
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
    fn dir_form_trims_trailing_slash() {
        assert_eq!(normalize_rel_dir("notes/").unwrap(), "notes");
        assert_eq!(normalize_rel_dir("a/b").unwrap(), "a/b");
        assert!(normalize_rel_dir("/").is_err());
        assert!(normalize_rel_dir("../up").is_err());
    }

    /// A filename is bytes, not text. `to_str` answers `None` for a name UTF-8 rejects,
    /// which made `.draft-\xFF.md` read as *not* hidden and routed it into the note
    /// collector — where the lossy path names no file, so every pass reported a bogus
    /// "file no longer exists" skip.
    ///
    /// Asserted on the predicate rather than through a real file on purpose: `read_dir`
    /// can hand us any bytes the filesystem holds, but APFS refuses to *create* such a
    /// name, so a fixture that writes one cannot run on the platform B2 ships on.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_dot_prefixed_name_is_hidden() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;

        let undecodable = |bytes: &[u8]| is_hidden(Path::new(OsStr::from_bytes(bytes)));
        assert!(undecodable(b"/vault/.draft-\xFF.md"));
        assert!(undecodable(b"/vault/.\xFF"));
        // …and the same bytes without the leading dot stay ordinary vault material,
        // so the fix widens what counts as hidden by exactly the leading dot.
        assert!(!undecodable(b"/vault/draft-\xFF.md"));
    }

    /// GH #136: b2 indexes no dot-prefixed member, so no authoring destination may
    /// name one — folder, resource, or note alike. An interior dot (`a.b.md`) is
    /// not hidden; only a *leading* one is.
    #[test]
    fn every_destination_form_refuses_a_hidden_segment() {
        assert!(normalize_rel_dir(".b2").is_err());
        assert!(normalize_rel_dir("a/.git/b").is_err());
        assert!(normalize_rel(".notes.csv").is_err());
        assert!(normalize_rel("a/.hidden.png").is_err());
        assert!(normalize_rel_md(".scratch.md").is_err());
        assert!(normalize_rel_md("notes/.draft").is_err());

        assert_eq!(normalize_rel_md("notes/a.b").unwrap(), "notes/a.b.md");
        assert_eq!(normalize_rel("a/b.tar.gz").unwrap(), "a/b.tar.gz");
    }

    #[test]
    fn rejects_empty_absolute_and_escaping() {
        assert!(normalize_rel_md("   ").is_err());
        assert!(normalize_rel_md("/abs/path.md").is_err());
        assert!(normalize_rel_md("../escape.md").is_err());
        assert!(normalize_rel_md("a/../../b.md").is_err());
    }
}
