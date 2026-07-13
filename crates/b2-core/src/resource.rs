//! Resource classification and document-kind dispatch — file-type support
//! slice 1 (planning/specs/resources-inventory-graph.md §1/§4; the taxonomy:
//! research/file-type-support.md §3).
//!
//! Class is decided by **extension only** — deterministic, no content sniffing;
//! a mislabeled file degrades gracefully rather than mis-executing. The table is
//! closed with [`ResourceClass::Binary`] as the total fallback, so *every* file
//! classifies ("any file GitHub could store").

/// The closed class table (research §3). Everything that is not a note maps to
/// exactly one of these; `Binary` catches all the rest.
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

    /// Classify a vault-relative path. `None` means the file is a **note**
    /// (`.md`) and belongs to the note pipeline, not the resource inventory.
    /// Extensions are case-insensitive; no extension → `Binary`.
    pub fn of_path(path: &str) -> Option<ResourceClass> {
        let ext = path
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
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

/// Which arm of the vault an argument names — the pure dispatch rule locked in
/// research/file-type-support.md §9b #8, kept in core so the CLI and the desktop
/// can never drift on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    /// A note ref: a `.md` path, or a syntactically valid ULID (`b2id`).
    Note,
    /// A resource ref: any other path.
    Resource,
}

/// Dispatch an adapter argument (`b2 explain <arg>`, `b2 mv <arg> …`, a `similar`
/// anchor) to the note or resource arm — by the argument's own shape, never by
/// DB state. Known ambiguity, resolved by rule: an extensionless filename that
/// happens to be a valid 26-char ULID dispatches as a `b2id`.
pub fn doc_kind(arg: &str) -> DocKind {
    if arg.to_ascii_lowercase().ends_with(".md") {
        return DocKind::Note;
    }
    if ulid::Ulid::from_string(arg).is_ok() {
        return DocKind::Note;
    }
    DocKind::Resource
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_is_total_and_extension_only() {
        // (path, expected) — parameterized over the class table; None = note.
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
            ("01JMEM0000000000000000000A", DocKind::Note), // valid ULID → b2id ref
            ("papers/attention.pdf", DocKind::Resource),
            ("img/photo.png", DocKind::Resource),
            ("LICENSE", DocKind::Resource),           // extensionless, not a ULID
            ("01JMEM000000000000000000", DocKind::Resource), // 24 chars: not a ULID
            ("notes/a.md.bak", DocKind::Resource),
        ];
        for (arg, expected) in cases {
            assert_eq!(doc_kind(arg), *expected, "arg: {arg}");
        }
    }
}
