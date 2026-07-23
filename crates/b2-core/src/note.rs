//! Lossless note parsing + the surgical `b2id` stamp.
//!
//! A note is YAML frontmatter (optional) followed by a Markdown body. To make
//! `parse → serialize → parse` byte-identical (data-model.md §6), a [`ParsedNote`]
//! keeps the **raw text verbatim** and records only the byte spans of the
//! frontmatter. Serialization returns the raw bytes; the *only* mutation is the
//! surgical insertion of a `b2id:` line. The queryable fields are extracted by a
//! read-only YAML parse and never used to re-serialize.

use crate::error::{Error, Result};
use yaml_rust2::{Yaml, YamlLoader};

/// The frontmatter fields B2 projects into the `notes` table. Extraction is
/// best-effort: unparseable frontmatter still round-trips (raw is preserved); the
/// fields just come back empty — except `b2id`, which falls back to a raw line
/// scan so the stamp write can never re-fire on YAML it can't read (#75, see
/// [`extract_fields`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NoteFields {
    pub b2id: Option<String>,
    pub r#type: Option<String>,
    /// The frontmatter `title:` value, parsed only so it round-trips and can be
    /// inspected. It has **no special meaning**: a note's display title is its
    /// filename ([`display_title`]), and B2 never privileges this field
    /// (data-model.md §1). Kept recognized so the key is understood, not stripped.
    pub title: Option<String>,
    pub description: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    /// Raw `b2_relations:` entries (typed-link strings, §2) — B2's namespaced
    /// frontmatter home for typed edges, the only place a verb/explanation lives.
    /// Parsed into `origin=frontmatter` edges at ingest. (A generic `relations:`
    /// key is NOT read — it is another tool's, preserved verbatim like any
    /// unknown key; data-model §1.)
    pub relations: Vec<String>,
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

    /// The raw frontmatter YAML **verbatim** — the text between the `---` fences
    /// (fences excluded), exactly as on disk — or `None` when the note has no
    /// frontmatter block. Byte-honest like [`body`](Self::body): the actual bytes,
    /// not a re-serialization of the parsed [`fields`](Self::fields), so keys B2
    /// doesn't model (`b2_relations:`, `aliases:`, custom keys) show as written. Powers
    /// the Desktop UI's frontmatter drawer (crates/b2-desktop/CLAUDE.md).
    pub fn frontmatter(&self) -> Option<&str> {
        self.fm.map(|f| &self.raw[f.content_start..f.content_end])
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

    /// Replace the note's **body** with `new_body`, verbatim — the byte-honest
    /// splice behind `Vault::write`. Everything up to
    /// `body_start` (the frontmatter block and its fences — every byte, including
    /// keys B2 doesn't model) is preserved *by construction*; a note with no
    /// frontmatter is replaced wholesale (its body **is** the file). No newline
    /// normalization or trimming: the editor buffer is the user's text.
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
        // Re-derive spans + fields from the mutated text so state stays exact
        // (the same discipline as stamp_b2id/add_relation; a body could even
        // introduce a frontmatter block if it starts with `---`).
        let reparsed = parse(&self.raw);
        self.fm = reparsed.fm;
        self.fields = reparsed.fields;
    }

    /// Append a typed-link `spec` (e.g. `contradicts [[path|title]] — why`) to the
    /// frontmatter `b2_relations:` list, creating the block (or the whole frontmatter)
    /// if absent. **Frontmatter only — never the body** (data-model §0). The single
    /// surgical insertion preserves every other byte; the new item is YAML-quoted so
    /// `[[`, `|`, `:` are safe. Errors on a pre-existing flow-style `b2_relations:`
    /// rather than risk corrupting it.
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
        let reparsed = parse(&self.raw);
        self.fm = reparsed.fm;
        self.fields = reparsed.fields;
        Ok(())
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
    if let Ok(docs) = YamlLoader::load_from_str(yaml) {
        if let Some(doc) = docs.first() {
            f.b2id = doc["b2id"].as_str().map(str::to_string);
            f.r#type = doc["type"].as_str().map(str::to_string);
            f.title = doc["title"].as_str().map(str::to_string);
            f.description = doc["description"].as_str().map(str::to_string);
            f.created = scalar_to_string(&doc["created"]);
            f.updated = scalar_to_string(&doc["updated"]);
            f.aliases = string_list(&doc["aliases"]);
            f.tags = string_list(&doc["tags"]);
            f.relations = string_list(&doc["b2_relations"]);
        }
    }
    // Stamping — B2's one autonomous write — is gated on `b2id` being *definitively
    // absent*, never on "the YAML wouldn't parse" (#75): without this fallback, a
    // note with unparseable frontmatter read as id-less on every pass, so ingest
    // stamped a fresh line each reindex — the file grew forever and the note's
    // identity churned. When the parsed route yields no id (parse failure, or a
    // value that isn't a plain string), a conservative raw scan for a `b2id:` line
    // supplies it: unreadable-but-present still counts, and the projection keeps a
    // stable identity while the malformed YAML stays the human's to fix.
    if f.b2id.is_none() {
        f.b2id = scan_b2id(yaml);
    }
    f
}

/// Raw-scan fallback for [`extract_fields`]: the first top-level (column-0)
/// `b2id:` line's value, whitespace-trimmed with one pair of surrounding quotes
/// stripped. First match wins — the stamp inserts at the top of the block. This is
/// a gate for the stamp write plus a stable identity, not a YAML parser: an odd
/// value is kept verbatim rather than risking a second stamp.
fn scan_b2id(yaml: &str) -> Option<String> {
    for line in yaml.lines() {
        let Some(rest) = line.strip_prefix("b2id:") else {
            continue;
        };
        let v = rest.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
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

/// Locate where to insert a new `b2_relations:` block item, scanning the frontmatter
/// region. Returns `Some((byte_offset, indent))` to insert `"{indent}- …\n"` (the
/// indent matches existing items, or 2 spaces for a fresh/empty block), or `None`
/// if there is no `b2_relations:` key (the caller then creates one). Errors on a
/// flow-style `b2_relations:` (e.g. `b2_relations: [a, b]`) — not safely appendable.
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
                insert_at = Some(pos + len); // after the key line (updated as items appear)
            } else if !line.starts_with([' ', '\t']) && stripped.starts_with("b2_relations:") {
                return Err(Error::Frontmatter(
                    "a flow-style `b2_relations:` value cannot be appended in place".into(),
                ));
            }
        } else if stripped.starts_with('-') {
            indent = body[..body.len() - stripped.len()].to_string();
            insert_at = Some(pos + len); // after the latest item
        } else if !stripped.is_empty() {
            in_block = false; // a new key ends the block
        }
        pos += len;
    }
    Ok(insert_at.map(|at| (at, indent)))
}

/// A note's **display title — its filename** (data-model.md §1). The filename *is*
/// the title: a frontmatter `title:` key is recognized but carries no special
/// meaning, so the title is a pure function of the note's vault-relative,
/// `/`-separated `path` — its base name with a trailing `.md` (any case) removed
/// (`notes/spaced-repetition.md` → `spaced-repetition`). A name that is only an
/// extension (`.md`) or carries a non-`.md` extension is returned whole.
pub fn display_title(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    let stem = base.rsplit_once('.').map_or(base, |(stem, ext)| {
        if ext.eq_ignore_ascii_case("md") && !stem.is_empty() {
            stem
        } else {
            base
        }
    });
    stem.to_string()
}

/// YAML double-quote a string (escaping `\` and `"`), so a value with `[[`, `|`,
/// `:` (a typed-link spec, or a new note's `title`) is always a safe scalar.
/// `pub(crate)` so [`crate::add`] renders a new note's frontmatter with the same
/// quoting the relation-append path uses.
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
        // The exact bytes on disk, not a re-serialization — `b2_relations:` (a key
        // the projected fields flatten) survives verbatim.
        assert_eq!(
            note.frontmatter(),
            Some("b2id: 01ABC\ntitle: Foo\nb2_relations:\n  - references [[x]]\n")
        );
        assert_eq!(note.body(), "body\n");
    }

    #[test]
    fn frontmatter_is_none_when_there_is_no_block() {
        let note = parse("just a body, no fences\n");
        assert_eq!(note.frontmatter(), None);
    }

    #[test]
    fn frontmatter_is_some_empty_for_an_empty_block() {
        // Fences with nothing between them: the block exists (Some) but is empty —
        // distinct from no frontmatter at all (None).
        let note = parse("---\n---\nbody\n");
        assert_eq!(note.frontmatter(), Some(""));
    }

    #[test]
    fn display_title_is_the_filename_without_the_md_extension() {
        assert_eq!(
            display_title("notes/spaced-repetition.md"),
            "spaced-repetition"
        );
        assert_eq!(display_title("memory.md"), "memory");
        // Case-insensitive extension; nested folders drop away.
        assert_eq!(display_title("a/b/Read Me.MD"), "Read Me");
        // A dotted stem keeps every dot but the extension.
        assert_eq!(display_title("notes/2026.07.14-log.md"), "2026.07.14-log");
        // Non-`.md` or extension-only names are returned whole (never used for a
        // real note, but the helper must not mangle them).
        assert_eq!(display_title("data.csv"), "data.csv");
        assert_eq!(display_title(".md"), ".md");
    }
}
