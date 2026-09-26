//! Vault-relative path algebra — the one place B2 spells out what a vault path is and how
//! paths relate. A vault path is `/`-separated and relative to the vault root; it is the
//! identity of the note or resource at it (L1, L3), so every op that validates, compares
//! or rewrites one goes through here rather than re-deriving the rules.
//!
//! Three concerns: validating a user-supplied destination ([`normalize_rel`] and its
//! folder/note variants — error-type-free, returning `Err(reason)` so each op maps it onto
//! its own [`crate::Error`] variant); the hidden-path predicate ([`is_hidden`]); and the
//! pure string algebra over paths that are already vault-relative ([`file_name`],
//! [`parent_dir`], [`extension`], [`is_under`], [`rebase`], [`relativize`]).

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
    Ok(if is_md(&s) { s } else { format!("{s}.md") })
}

/// A vault path's last segment — the file (or folder) name.
pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A vault path's folder: everything before the last `/`, and `""` at the vault root —
/// the base a note-relative Markdown target is resolved and re-relativized against.
pub(crate) fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The extension of a vault path's file name, as written (case kept): the text after
/// the name's last `.`. `None` when there is none — no `.`, a trailing `.`, or only a
/// leading one (a dotfile's name is all stem).
pub(crate) fn extension(path: &str) -> Option<&str> {
    match file_name(path).rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => Some(ext),
        _ => None,
    }
}

/// Whether a vault path names a note by its shape: its file name's extension is `md`,
/// in any case (`Foo.MD` is a note, as the walk classifies it).
pub(crate) fn is_md(path: &str) -> bool {
    extension(path).is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Whether `path` lies strictly inside the folder `dir` (at any depth). A folder is not
/// under itself, and a prefix-sharing sibling (`docs2` for `docs`) is not under it.
pub(crate) fn is_under(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// `path` carried by moving the folder `from` to `to`: the same place under `to`, or
/// `None` when `path` is not under `from` ([`is_under`]).
pub(crate) fn rebase(path: &str, from: &str, to: &str) -> Option<String> {
    let rest = path.strip_prefix(from)?.strip_prefix('/')?;
    Some(format!("{to}/{rest}"))
}

/// The relative path from the folder `base_dir` (`""` = the vault root) to the vault
/// path `to_path`: the shared leading folders dropped, one `..` per remaining `base_dir`
/// segment — the inverse of resolving a note-relative Markdown target.
pub(crate) fn relativize(base_dir: &str, to_path: &str) -> String {
    let base: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    let to: Vec<&str> = to_path.split('/').collect();
    let shared = base.iter().zip(&to).take_while(|(a, b)| a == b).count();
    std::iter::repeat_n("..", base.len() - shared)
        .chain(to[shared..].iter().copied())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_md_when_missing_and_keeps_it_when_present() {
        assert_eq!(normalize_rel_md("notes/foo").unwrap(), "notes/foo.md");
        assert_eq!(normalize_rel_md("notes/foo.md").unwrap(), "notes/foo.md");
    }

    /// An authored `.MD` is already a note path: the check is on the file name's
    /// extension in any case, so no second `.md` is stacked on.
    #[test]
    fn an_uppercase_md_extension_is_kept_not_doubled() {
        assert_eq!(normalize_rel_md("Foo.MD").unwrap(), "Foo.MD");
        assert_eq!(normalize_rel_md("a/b.Md").unwrap(), "a/b.Md");
        // A folder named like a note does not make its contents notes.
        assert_eq!(normalize_rel_md("x.md/y").unwrap(), "x.md/y.md");
    }

    #[test]
    fn names_folders_and_extensions_split_on_the_last_separator() {
        assert_eq!(file_name("a/b/c.md"), "c.md");
        assert_eq!(file_name("c.md"), "c.md");
        assert_eq!(parent_dir("a/b/c.md"), "a/b");
        assert_eq!(parent_dir("c.md"), "");
        assert_eq!(extension("a/b.tar.gz"), Some("gz"));
        assert_eq!(extension("a/Photo.PNG"), Some("PNG"));
        assert_eq!(
            extension("dir.v2/Makefile"),
            None,
            "a dotted folder lends no extension"
        );
        assert_eq!(
            extension("a/.gitignore"),
            None,
            "a dotfile's name is all stem"
        );
        assert_eq!(extension("a/trailing."), None);
        assert!(is_md("a/B.MD") && is_md("b.md"));
        assert!(!is_md("a.md/b") && !is_md("a/.md") && !is_md("a.mdx"));
    }

    #[test]
    fn under_means_strictly_inside_and_rebase_follows_it() {
        assert!(is_under("docs/a.md", "docs"));
        assert!(is_under("docs/x/y/a.md", "docs"));
        assert!(!is_under("docs", "docs"), "a folder is not under itself");
        assert!(
            !is_under("docs2/a.md", "docs"),
            "a prefix-sharing sibling is not under it"
        );
        assert!(!is_under("a.md", "docs"));

        assert_eq!(
            rebase("docs/x/a.md", "docs", "media").as_deref(),
            Some("media/x/a.md")
        );
        assert_eq!(rebase("docs2/a.md", "docs", "media"), None);
        assert_eq!(rebase("docs", "docs", "media"), None);
    }

    #[test]
    fn relativize_walks_up_to_the_shared_folder_then_down() {
        assert_eq!(relativize("", "a/b.png"), "a/b.png");
        assert_eq!(relativize("a", "a/b.png"), "b.png");
        assert_eq!(relativize("notes", "docs/plan.pdf"), "../docs/plan.pdf");
        assert_eq!(relativize("a/b/c", "a/x.png"), "../../x.png");
        assert_eq!(relativize("a/b", "c.png"), "../../c.png");
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
