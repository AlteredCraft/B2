//! Resource classification and document-kind dispatch.
//!
//! Class is decided by extension only, no content sniffing. [`ResourceClass::Binary`] is
//! the fallback, so every file classifies.

/// The closed class table (data-model.md §10) for everything that is not a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceClass {
    Text,
    Html,
    Pdf,
    Image,
    Media,
    Binary,
}

impl ResourceClass {
    /// The `resources.class` column value (and the CHECK vocabulary in `db.rs`).
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceClass::Text => "text",
            ResourceClass::Html => "html",
            ResourceClass::Pdf => "pdf",
            ResourceClass::Image => "image",
            ResourceClass::Media => "media",
            ResourceClass::Binary => "binary",
        }
    }

    /// Classify a vault-relative path; `None` means a note (`.md`). Case-insensitive,
    /// read off the file name; no extension is `Binary`.
    pub fn of_path(path: &str) -> Option<ResourceClass> {
        let ext = crate::pathspec::extension(path)
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        Some(match ext.as_str() {
            "md" => return None,
            "txt" | "csv" | "tsv" | "json" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf"
            | "log" | "xml" | "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "sh" | "c" | "h"
            | "cpp" | "hpp" | "go" | "java" | "rb" | "swift" | "kt" | "css" | "scss" | "sql" => {
                ResourceClass::Text
            }
            "html" | "htm" => ResourceClass::Html,
            "pdf" => ResourceClass::Pdf,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" => ResourceClass::Image,
            "mp3" | "wav" | "mp4" | "mov" | "webm" => ResourceClass::Media,
            _ => ResourceClass::Binary,
        })
    }
}

/// Which arm of the vault an argument names (data-model.md §10). In core so the adapters
/// can't drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    /// A note ref: a path, with or without the `.md`.
    Note,
    /// A resource ref: any other path.
    Resource,
}

/// Dispatch a reference by its shape, never DB state: an extension other than `md` is a
/// resource; `.md` or none is a note (wikilinks omit it, ADR-0003). Accepted limit: an
/// extensionless file (`Makefile`) dispatches as a note ref.
pub fn doc_kind(arg: &str) -> DocKind {
    match crate::pathspec::extension(arg) {
        Some(ext) if !ext.eq_ignore_ascii_case("md") => DocKind::Resource,
        _ => DocKind::Note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_is_total_and_extension_only() {
        // None = note.
        let cases: &[(&str, Option<ResourceClass>)] = &[
            ("notes/a.md", None),
            ("NOTES/A.MD", None),
            ("data/report.txt", Some(ResourceClass::Text)),
            ("src/main.rs", Some(ResourceClass::Text)),
            ("logs/app.LOG", Some(ResourceClass::Text)),
            ("clip/page.html", Some(ResourceClass::Html)),
            ("clip/page.htm", Some(ResourceClass::Html)),
            ("papers/attention.pdf", Some(ResourceClass::Pdf)),
            ("img/photo.PNG", Some(ResourceClass::Image)),
            ("img/anim.gif", Some(ResourceClass::Image)),
            ("img/vec.svg", Some(ResourceClass::Image)),
            ("media/song.mp3", Some(ResourceClass::Media)),
            ("media/clip.webm", Some(ResourceClass::Media)),
            ("blob.xyz", Some(ResourceClass::Binary)),
            ("Makefile", Some(ResourceClass::Binary)), // no extension
            ("archive.tar.gz", Some(ResourceClass::Binary)), // last extension wins
        ];
        for (path, expected) in cases {
            assert_eq!(ResourceClass::of_path(path), *expected, "path: {path}");
        }
    }

    #[test]
    fn doc_kind_dispatches_by_shape_alone() {
        let cases: &[(&str, DocKind)] = &[
            ("notes/a.md", DocKind::Note),
            ("A.MD", DocKind::Note),
            ("concepts/memory", DocKind::Note), // the wikilink habit: extensionless
            ("papers/attention.pdf", DocKind::Resource),
            ("img/photo.png", DocKind::Resource),
            ("archive.tar.gz", DocKind::Resource),
            ("notes/a.md.bak", DocKind::Resource),
            ("LICENSE", DocKind::Note), // extensionless file: the documented limit
        ];
        for (arg, expected) in cases {
            assert_eq!(doc_kind(arg), *expected, "arg: {arg}");
        }
    }
}
