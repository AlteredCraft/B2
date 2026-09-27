//! Vault-relative path algebra. A vault path is `/`-separated, relative to the root, and
//! the identity of what it names (L1, L3), so every op validates and compares paths here.
//! Validators return `Err(reason)` for each op to map onto its own [`crate::Error`].

/// Whether the entry's name is dot-prefixed, whatever its extension (GH #136). Tested on
/// bytes, not `to_str`, so a non-UTF-8 name like `.draft-\xFF.md` is still hidden.
pub(crate) fn is_hidden(path: &std::path::Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.as_encoded_bytes().starts_with(b"."))
}

/// Normalize and validate a vault-relative path: trim, `\` → `/`, and reject empty,
/// absolute, escaping or hidden paths. Hidden is refused for every kind, since the walk
/// would never see what b2 created (GH #136).
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

/// [`normalize_rel`] for a folder: a trailing `/` is trimmed.
pub(crate) fn normalize_rel_dir(input: &str) -> Result<String, String> {
    normalize_rel(input.trim().trim_end_matches('/'))
}

/// [`normalize_rel`] for a note: `.md` appended if omitted.
pub(crate) fn normalize_rel_md(input: &str) -> Result<String, String> {
    let s = normalize_rel(input)?;
    Ok(if is_md(&s) { s } else { format!("{s}.md") })
}

pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A vault path's folder; `""` at the root.
pub(crate) fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The file name's extension, case kept. A dotfile's name is all stem.
pub(crate) fn extension(path: &str) -> Option<&str> {
    match file_name(path).rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => Some(ext),
        _ => None,
    }
}

/// Whether the extension is `md`, in any case.
pub(crate) fn is_md(path: &str) -> bool {
    extension(path).is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Whether `path` lies strictly inside the folder `dir`, at any depth.
pub(crate) fn is_under(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// `path` after moving the folder `from` to `to`; `None` when not under `from`.
pub(crate) fn rebase(path: &str, from: &str, to: &str) -> Option<String> {
    let rest = path.strip_prefix(from)?.strip_prefix('/')?;
    Some(format!("{to}/{rest}"))
}

/// Join a relative `target` onto `base_dir`, normalizing `.` and `..`. `None` when it
/// escapes the vault. The inverse of [`relativize`].
pub(crate) fn join_relative(base_dir: &str, target: &str) -> Option<String> {
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

/// The relative path from folder `base_dir` (`""` = root) to `to_path`.
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
    fn join_relative_normalizes_and_refuses_to_escape_the_vault() {
        assert_eq!(
            join_relative("notes", "../docs/./plan.pdf").as_deref(),
            Some("docs/plan.pdf")
        );
        assert_eq!(join_relative("", "a/b.png").as_deref(), Some("a/b.png"));
        assert_eq!(join_relative("", "../x.png"), None);
    }

    #[test]
    fn join_relative_inverts_relativize() {
        for (base, to) in [
            ("", "a/b.png"),
            ("notes", "docs/plan.pdf"),
            ("a/b/c", "a/x.png"),
        ] {
            assert_eq!(
                join_relative(base, &relativize(base, to)).as_deref(),
                Some(to)
            );
        }
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

    /// Tested on the predicate, not a real file: APFS refuses to create such a name.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_dot_prefixed_name_is_hidden() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;

        let undecodable = |bytes: &[u8]| is_hidden(Path::new(OsStr::from_bytes(bytes)));
        assert!(undecodable(b"/vault/.draft-\xFF.md"));
        assert!(undecodable(b"/vault/.\xFF"));
        // Without the leading dot, not hidden.
        assert!(!undecodable(b"/vault/draft-\xFF.md"));
    }

    /// GH #136. Only a leading dot hides; an interior one (`a.b.md`) does not.
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
