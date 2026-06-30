//! Lossless note parsing + the surgical `b2id` stamp.
//!
//! A note is YAML frontmatter (optional) followed by a Markdown body. To make
//! `parse → serialize → parse` byte-identical (data-model.md §6), a [`ParsedNote`]
//! keeps the **raw text verbatim** and records only the byte spans of the
//! frontmatter. Serialization returns the raw bytes; the *only* mutation is the
//! surgical insertion of a `b2id:` line. The queryable fields are extracted by a
//! read-only YAML parse and never used to re-serialize.

use yaml_rust2::{Yaml, YamlLoader};

/// The frontmatter fields B2 projects into the `notes` table. Extraction is
/// best-effort: unparseable frontmatter still round-trips (raw is preserved); the
/// fields just come back empty.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NoteFields {
    pub b2id: Option<String>,
    pub r#type: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
}

/// Byte spans of a frontmatter block within the raw text (fences excluded).
#[derive(Debug, Clone, Copy)]
struct Frontmatter {
    /// First byte of the YAML content (right after the opening `---` line).
    content_start: usize,
    /// First byte of the closing `---` line (YAML content ends here).
    content_end: usize,
    /// First byte of the body (right after the closing `---` line's newline).
    body_start: usize,
}

/// A parsed note that can be serialized back byte-identically.
#[derive(Debug, Clone)]
pub struct ParsedNote {
    raw: String,
    fm: Option<Frontmatter>,
    fields: NoteFields,
}

/// Parse `raw` into a [`ParsedNote`]. Never fails: text with no/!invalid
/// frontmatter is still represented (and still round-trips).
pub fn parse(raw: &str) -> ParsedNote {
    let fm = detect_frontmatter(raw);
    let fields = match &fm {
        Some(f) => extract_fields(&raw[f.content_start..f.content_end]),
        None => NoteFields::default(),
    };
    ParsedNote {
        raw: raw.to_string(),
        fm,
        fields,
    }
}

impl ParsedNote {
    /// The note serialized — byte-identical to what was parsed (plus any stamp).
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The extracted, queryable frontmatter fields.
    pub fn fields(&self) -> &NoteFields {
        &self.fields
    }

    /// The body (everything after the frontmatter; the whole text if there is
    /// none). This is what gets hashed and, later, chunked.
    pub fn body(&self) -> &str {
        match &self.fm {
            Some(f) => &self.raw[f.body_start..],
            None => &self.raw,
        }
    }

    /// Stamp a missing `b2id`. A no-op if one is already present (never
    /// re-stamp). Inserts exactly one line at the top of the frontmatter, or
    /// creates a minimal frontmatter block if the note has none. This is B2's one
    /// always-allowed write to the vault (data-model.md §1).
    pub fn stamp_b2id(&mut self, id: &str) {
        if self.fields.b2id.is_some() {
            return;
        }
        match self.fm {
            Some(f) => self
                .raw
                .insert_str(f.content_start, &format!("b2id: {id}\n")),
            None => self.raw.insert_str(0, &format!("---\nb2id: {id}\n---\n")),
        }
        // Re-derive spans + fields from the mutated text so state stays exact.
        let reparsed = parse(&self.raw);
        self.fm = reparsed.fm;
        self.fields = reparsed.fields;
    }
}

/// Locate a frontmatter block: an opening `---` line at the very top and the next
/// `---` line. Returns `None` if the first line isn't `---` or no closing fence is
/// found (in which case the whole text is body).
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

fn extract_fields(yaml: &str) -> NoteFields {
    let mut f = NoteFields::default();
    let Ok(docs) = YamlLoader::load_from_str(yaml) else {
        return f;
    };
    let Some(doc) = docs.first() else {
        return f;
    };
    f.b2id = doc["b2id"].as_str().map(str::to_string);
    f.r#type = doc["type"].as_str().map(str::to_string);
    f.title = doc["title"].as_str().map(str::to_string);
    f.description = doc["description"].as_str().map(str::to_string);
    f.created = scalar_to_string(&doc["created"]);
    f.updated = scalar_to_string(&doc["updated"]);
    f.aliases = string_list(&doc["aliases"]);
    f.tags = string_list(&doc["tags"]);
    f
}

/// Read a scalar as text, tolerating YAML that typed it as a number/bool (e.g. a
/// date-looking `created:` or a numeric value).
fn scalar_to_string(y: &Yaml) -> Option<String> {
    match y {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Read a YAML sequence of strings; empty for a missing key or a non-sequence.
fn string_list(y: &Yaml) -> Vec<String> {
    match y.as_vec() {
        Some(items) => items
            .iter()
            .filter_map(|e| e.as_str().map(str::to_string))
            .collect(),
        None => Vec::new(),
    }
}
