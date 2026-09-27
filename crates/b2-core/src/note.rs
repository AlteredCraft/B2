//! Lossless note parsing and the surgical frontmatter/body splices.
//!
//! A [`ParsedNote`] keeps the raw text verbatim plus the frontmatter's byte spans, so
//! serializing is byte-identical. Every mutation is a splice on an explicit command
//! (ADR-0004). Parsed fields are read-only and never re-serialized.

use crate::error::{Error, Result};
use yaml_rust2::{Yaml, YamlLoader};

/// The frontmatter fields B2 reads, best-effort: unparseable YAML just yields empty
/// fields. Every other key round-trips verbatim, read by nothing (data-model.md §1).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NoteFields {
    pub r#type: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
    /// Raw `b2_relations:` entries (§2), the only home of typed edges. A generic
    /// `relations:` key is another tool's and is not read (data-model §1).
    pub relations: Vec<String>,
}

/// Byte spans of a frontmatter block within the raw text (fences excluded).
#[derive(Debug, Clone, Copy)]
struct Frontmatter {
    content_start: usize,
    /// First byte of the closing `---` line.
    content_end: usize,
    body_start: usize,
}

/// A parsed note that can be serialized back byte-identically.
#[derive(Debug, Clone)]
pub struct ParsedNote {
    raw: String,
    fm: Option<Frontmatter>,
    fields: NoteFields,
    fm_readable: bool,
}

/// Parse `raw` into a [`ParsedNote`]. Never fails: invalid frontmatter still round-trips.
pub fn parse(raw: &str) -> ParsedNote {
    let fm = detect_frontmatter(raw);
    let (fields, fm_readable) = match &fm {
        Some(f) => extract_fields(&raw[f.content_start..f.content_end]),
        None => (NoteFields::default(), true),
    };
    ParsedNote {
        raw: raw.to_string(),
        fm,
        fields,
        fm_readable,
    }
}

impl ParsedNote {
    /// The note serialized: the parsed bytes plus any splices.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn fields(&self) -> &NoteFields {
        &self.fields
    }

    /// Everything after the frontmatter: what gets hashed and chunked.
    pub fn body(&self) -> &str {
        match &self.fm {
            Some(f) => &self.raw[f.body_start..],
            None => &self.raw,
        }
    }

    /// The frontmatter YAML verbatim, fences excluded, or `None` with no block. Feeds the
    /// desktop's frontmatter drawer.
    pub fn frontmatter(&self) -> Option<&str> {
        self.fm.map(|f| &self.raw[f.content_start..f.content_end])
    }

    /// Whether the frontmatter reads as a YAML mapping (or is absent/empty). `false` means
    /// [`fields`](Self::fields) came back empty, so an adapter can warn (GH #79).
    pub fn frontmatter_readable(&self) -> bool {
        self.fm_readable
    }

    /// Replace the body with `new_body` verbatim (`Vault::write`). The frontmatter block is
    /// preserved byte for byte. No newline normalization: the buffer is the user's text.
    pub fn replace_body(&mut self, new_body: &str) {
        match self.fm {
            Some(f) => {
                self.raw.truncate(f.body_start);
                self.raw.push_str(new_body);
            }
            None => {
                self.raw.clear();
                self.raw.push_str(new_body);
            }
        }
        // A body starting with `---` could even introduce a frontmatter block.
        self.reparse();
    }

    /// Replace the frontmatter YAML with `new_yaml` verbatim (`Vault::write_frontmatter`,
    /// GH #79). No block gains one; empty `new_yaml` removes it. A missing final newline is
    /// added. The façade refuses a top-level `---` line, which would end the block early.
    pub fn replace_frontmatter(&mut self, new_yaml: &str) {
        let mut block = new_yaml.to_string();
        if !block.is_empty() && !block.ends_with('\n') {
            block.push('\n');
        }
        match self.fm {
            Some(f) if block.is_empty() => self.raw.replace_range(..f.body_start, ""),
            Some(f) => self
                .raw
                .replace_range(f.content_start..f.content_end, &block),
            None if block.is_empty() => {}
            None => self.raw.insert_str(0, &format!("---\n{block}---\n")),
        }
        self.reparse();
    }

    /// Append a typed-link `spec` to `b2_relations:`, creating the list or block if absent.
    /// Frontmatter only, never the body (data-model §0). Errors on a flow-style list rather
    /// than risk corrupting it.
    pub fn add_relation(&mut self, spec: &str) -> Result<()> {
        let quoted = yaml_quote(spec);
        match self.fm {
            None => {
                self.raw
                    .insert_str(0, &format!("---\nb2_relations:\n  - {quoted}\n---\n"));
            }
            Some(fm) => match relations_insertion(&self.raw, &fm)? {
                Some((at, indent)) => self.raw.insert_str(at, &format!("{indent}- {quoted}\n")),
                None => self
                    .raw
                    .insert_str(fm.content_end, &format!("b2_relations:\n  - {quoted}\n")),
            },
        }
        self.reparse();
        Ok(())
    }

    /// Re-derive state from the mutated `raw`; every mutator's last step.
    fn reparse(&mut self) {
        let reparsed = parse(&self.raw);
        self.fm = reparsed.fm;
        self.fields = reparsed.fields;
        self.fm_readable = reparsed.fm_readable;
    }
}

/// Locate a frontmatter block: `---` as the first line, up to the next `---` line.
fn detect_frontmatter(raw: &str) -> Option<Frontmatter> {
    let first_nl = raw.find('\n')?;
    if raw[..first_nl].trim_end_matches('\r') != "---" {
        return None;
    }
    let content_start = first_nl + 1;

    let mut idx = content_start;
    loop {
        match raw[idx..].find('\n') {
            Some(rel) => {
                let line_end = idx + rel;
                if raw[idx..line_end].trim_end_matches('\r') == "---" {
                    return Some(Frontmatter {
                        content_start,
                        content_end: idx,
                        body_start: line_end + 1,
                    });
                }
                idx = line_end + 1;
            }
            None => {
                // Last line (no trailing newline) could still be the fence.
                if raw[idx..].trim_end_matches('\r') == "---" {
                    return Some(Frontmatter {
                        content_start,
                        content_end: idx,
                        body_start: raw.len(),
                    });
                }
                return None;
            }
        }
    }
}

/// Extract the fields from a frontmatter region, plus
/// [`ParsedNote::frontmatter_readable`].
fn extract_fields(yaml: &str) -> (NoteFields, bool) {
    let mut f = NoteFields::default();
    let mut readable = false;
    if let Ok(docs) = YamlLoader::load_from_str(yaml) {
        match docs.first() {
            None => readable = true,
            Some(doc) => {
                readable = doc.as_hash().is_some() || matches!(doc, Yaml::Null);
                f.r#type = doc["type"].as_str().map(str::to_string);
                f.created = scalar_to_string(&doc["created"]);
                f.updated = scalar_to_string(&doc["updated"]);
                f.tags = string_list(&doc["tags"]);
                f.relations = string_list(&doc["b2_relations"]);
            }
        }
    }
    (f, readable)
}

/// A scalar as text, even when YAML typed it as a number or bool.
fn scalar_to_string(y: &Yaml) -> Option<String> {
    match y {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn string_list(y: &Yaml) -> Vec<String> {
    match y.as_vec() {
        Some(items) => items
            .iter()
            .filter_map(|e| e.as_str().map(str::to_string))
            .collect(),
        None => Vec::new(),
    }
}

/// Where to insert a new `b2_relations:` item: `Some((offset, indent))`, or `None` with no
/// key. Errors on a flow-style value, which isn't safely appendable.
fn relations_insertion(raw: &str, fm: &Frontmatter) -> Result<Option<(usize, String)>> {
    let region = &raw[fm.content_start..fm.content_end];
    let mut pos = fm.content_start;
    let mut in_block = false;
    let mut insert_at: Option<usize> = None;
    let mut indent = String::from("  ");

    for line in region.split_inclusive('\n') {
        let len = line.len();
        let body = line.trim_end_matches('\n').trim_end_matches('\r');
        let stripped = body.trim_start();

        if !in_block {
            if body == "b2_relations:" {
                in_block = true;
                insert_at = Some(pos + len);
            } else if !line.starts_with([' ', '\t']) && stripped.starts_with("b2_relations:") {
                return Err(Error::Frontmatter(
                    "a flow-style `b2_relations:` value cannot be appended in place".into(),
                ));
            }
        } else if stripped.starts_with('-') {
            indent = body[..body.len() - stripped.len()].to_string();
            insert_at = Some(pos + len);
        } else if !stripped.is_empty() {
            in_block = false; // a new key ends the block
        }
        pos += len;
    }
    Ok(insert_at.map(|at| (at, indent)))
}

/// A note's display title: its filename without `.md` (data-model.md §1). A frontmatter
/// `title:` means nothing.
pub fn display_title(path: &str) -> String {
    let name = crate::pathspec::file_name(path);
    match name.get(..name.len().saturating_sub(".md".len())) {
        Some(stem) if crate::pathspec::is_md(name) => stem.to_string(),
        _ => name.to_string(),
    }
}

/// YAML double-quote a string, so `[[`, `|` and `:` are safe. Shared with [`crate::add`].
pub(crate) fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_returns_raw_yaml_verbatim_between_the_fences() {
        let raw = "---\nb2id: 01ABC\ntitle: Foo\nb2_relations:\n  - references [[x]]\n---\nbody\n";
        let note = parse(raw);
        assert_eq!(
            note.frontmatter(),
            Some("b2id: 01ABC\ntitle: Foo\nb2_relations:\n  - references [[x]]\n")
        );
        assert_eq!(note.body(), "body\n");
    }

    /// Legacy `b2id:` lines are never read or rewritten (W5, GH #170).
    #[test]
    fn a_legacy_b2id_line_is_an_ordinary_unknown_key() {
        let raw = "---\nb2id: 01JMEM0000000000000000000A\ntype: concept\n---\nThe body.\n";
        let mut note = parse(raw);
        assert_eq!(note.as_str(), raw, "round-trips byte-identically");
        assert_eq!(note.fields().r#type.as_deref(), Some("concept"));
        note.replace_body("Rewritten.\n");
        assert_eq!(
            note.as_str(),
            "---\nb2id: 01JMEM0000000000000000000A\ntype: concept\n---\nRewritten.\n"
        );
    }

    #[test]
    fn frontmatter_is_none_when_there_is_no_block() {
        let note = parse("just a body, no fences\n");
        assert_eq!(note.frontmatter(), None);
    }

    #[test]
    fn frontmatter_is_some_empty_for_an_empty_block() {
        let note = parse("---\n---\nbody\n");
        assert_eq!(note.frontmatter(), Some(""));
    }

    #[test]
    fn frontmatter_readable_reflects_whether_the_yaml_reads_as_metadata() {
        assert!(parse("just a body\n").frontmatter_readable());
        assert!(parse("---\n---\nbody\n").frontmatter_readable());
        assert!(parse("---\ntitle: X\ntags: [x]\n---\nbody\n").frontmatter_readable());
        let broken = parse("---\ntitle: \"unclosed\ntags: [a\n---\nbody\n");
        assert!(!broken.frontmatter_readable());
        assert_eq!(broken.frontmatter(), Some("title: \"unclosed\ntags: [a\n"));
        assert!(!parse("---\njust a sentence\n---\nbody\n").frontmatter_readable());
    }

    #[test]
    fn replace_frontmatter_splices_only_between_the_fences() {
        let mut note = parse("---\nb2id: 01A\ntitle: Old\n---\nThe body stays.\n");
        note.replace_frontmatter("b2id: 01A\ntags: [new]\n");
        assert_eq!(
            note.as_str(),
            "---\nb2id: 01A\ntags: [new]\n---\nThe body stays.\n"
        );
        assert_eq!(note.body(), "The body stays.\n");
        assert_eq!(note.fields().tags, vec!["new"]);
    }

    #[test]
    fn replace_frontmatter_adds_the_missing_final_newline() {
        // Otherwise the closing fence would join the last YAML line.
        let mut note = parse("---\nb2id: 01A\n---\nbody\n");
        note.replace_frontmatter("b2id: 01A\ntitle: X");
        assert_eq!(note.as_str(), "---\nb2id: 01A\ntitle: X\n---\nbody\n");
    }

    #[test]
    fn replace_frontmatter_creates_and_removes_the_block() {
        let mut note = parse("only a body\n");
        note.replace_frontmatter("b2id: 01A\n");
        assert_eq!(note.as_str(), "---\nb2id: 01A\n---\nonly a body\n");
        note.replace_frontmatter("");
        assert_eq!(note.as_str(), "only a body\n");
        assert_eq!(note.frontmatter(), None);
    }

    #[test]
    fn display_title_is_the_filename_without_the_md_extension() {
        assert_eq!(
            display_title("notes/spaced-repetition.md"),
            "spaced-repetition"
        );
        assert_eq!(display_title("memory.md"), "memory");
        assert_eq!(display_title("a/b/Read Me.MD"), "Read Me");
        assert_eq!(display_title("notes/2026.07.14-log.md"), "2026.07.14-log");
        // Returned whole.
        assert_eq!(display_title("data.csv"), "data.csv");
        assert_eq!(display_title(".md"), ".md");
    }
}
