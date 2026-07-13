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

/// Dispatch a document reference — an adapter argument (`b2 explain <arg>`,
/// `b2 mv <arg> …`, a `similar` anchor) or a link target — to the note or
/// resource arm, by the reference's own shape, never by DB state: **an
/// extension other than `md` means resource; `.md` or no extension means
/// note.** Extensionless covers both the wikilink habit (`concepts/memory`)
/// and a `b2id` (ULIDs carry no dot), so no separate ULID rule is needed.
/// Known limit, accepted: an extensionless *file* (`Makefile`) dispatches as a
/// note ref here — it is still walked, inventoried, and reachable through
/// surfaces that know its kind (the tree); revisit if a real vault hurts.
pub fn doc_kind(arg: &str) -> DocKind {
    let name = arg.rsplit('/').next().unwrap_or(arg);
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && !ext.is_empty() && !ext.eq_ignore_ascii_case("md") =>
        {
            DocKind::Resource
        }
        _ => DocKind::Note,
    }
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
            ("concepts/memory", DocKind::Note), // the wikilink habit: extensionless
            ("01JMEM0000000000000000000A", DocKind::Note), // a b2id: extensionless
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
