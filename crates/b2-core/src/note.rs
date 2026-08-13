//! Lossless note parsing and the surgical frontmatter/body splices.
//!
//! A note is YAML frontmatter (optional) followed by a Markdown body. To make
//! `parse → serialize → parse` byte-identical (data-model.md §6), a [`ParsedNote`]
//! keeps the **raw text verbatim** and records only the byte spans of the
//! frontmatter. Serialization returns the raw bytes; every mutation here is a
//! surgical splice performed on the human's explicit command (W3) — appending a
//! `b2_relations:` entry, replacing the body, replacing the frontmatter block. The
//! queryable fields are extracted by a read-only YAML parse and never used to
//! re-serialize.
//!
//! There is no `stamp_b2id` any more (GH #170). A note's identity is its path, so
//! nothing is written on first sight, and the `b2id:` key an older B2 left in a file
//! is now an ordinary unknown key: never parsed, never rewritten, round-tripped
//! verbatim like any other.

use crate::error::{Error, Result};
use yaml_rust2::{Yaml, YamlLoader};

/// The frontmatter fields B2 projects into the `notes` table. Extraction is
/// best-effort: unparseable frontmatter still round-trips (raw is preserved); the
/// fields just come back empty.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NoteFields {
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
    fm_readable: bool,
}

/// Parse `raw` into a [`ParsedNote`]. Never fails: text with no/!invalid
/// frontmatter is still represented (and still round-trips).
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
    /// The note serialized — byte-identical to what was parsed, plus whatever
    /// splice the caller asked for.
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

    /// Whether the frontmatter block reads as YAML metadata: `true` when there is
    /// no block, an empty one, or one that parses to a key/value mapping; `false`
    /// when the YAML is malformed or isn't a mapping — the raw bytes still
    /// round-trip verbatim, but the projected [`fields`](Self::fields) came back
    /// empty-handed. Surfaced so an adapter can tell the human "B2 can't read this
    /// frontmatter" instead of silently showing blank metadata (GH #79) — warn,
    /// never block: the bytes stay the human's to fix, and nothing B2 does depends
    /// on reading them.
    pub fn frontmatter_readable(&self) -> bool {
        self.fm_readable
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
        // (the same discipline as add_relation/replace_frontmatter; a body could even
        // introduce a frontmatter block if it starts with `---`).
        self.reparse();
    }

    /// Replace the note's **frontmatter YAML** with `new_yaml`, verbatim — the
    /// byte-honest splice behind `Vault::write_frontmatter`, and
    /// [`replace_body`](Self::replace_body)'s frontmatter sibling (GH #79). Only
    /// the bytes between the fences change; the fences and every body byte are
    /// preserved by construction. A note with no frontmatter gains a block; an
    /// empty `new_yaml` removes the block entirely (fences included), exactly as
    /// a hand-edit deleting it would leave the file. The one mechanical touch: a
    /// non-empty `new_yaml` missing its final newline gains one, so the closing
    /// fence always sits on its own line — every other byte is the caller's,
    /// unformatted and unjudged.
    ///
    /// A `new_yaml` containing a top-level `---` line would end the block early
    /// and shift the following bytes into the body; callers that must not touch
    /// the body (the façade op) refuse that input before splicing.
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
        self.reparse();
        Ok(())
    }

    /// Re-derive spans + fields + readability from the mutated `raw` so state
    /// stays exact — every mutator's closing step.
    fn reparse(&mut self) {
        let reparsed = parse(&self.raw);
        self.fm = reparsed.fm;
        self.fields = reparsed.fields;
        self.fm_readable = reparsed.fm_readable;
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

/// Extract the projected fields from a frontmatter region, plus whether the
/// region *read* as YAML metadata (the [`ParsedNote::frontmatter_readable`]
/// flag): `true` for an empty/comments-only block (nothing to read, nothing
/// wrong) or a key/value mapping; `false` when the YAML fails to parse or parses
/// to a non-mapping (a bare scalar, a list) — the fields then come back empty
/// while the raw bytes stay the human's to fix.
fn extract_fields(yaml: &str) -> (NoteFields, bool) {
    let mut f = NoteFields::default();
    let mut readable = false;
    if let Ok(docs) = YamlLoader::load_from_str(yaml) {
        match docs.first() {
            None => readable = true,
            Some(doc) => {
                readable = doc.as_hash().is_some() || matches!(doc, Yaml::Null);
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
    }
    // GH #75's raw `b2id:` line scan lived here: stamping was gated on the id being
    // *definitively* absent, and a parse failure reading as "absent" made ingest
    // stamp a fresh line on every pass — the file grew forever and the note's
    // identity churned. With nothing stamped (GH #170) there is no write to gate, so
    // malformed frontmatter now costs exactly what it should: empty projected fields,
    // raw bytes untouched, and a warning through `frontmatter_readable`.
    (f, readable)
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
        // the projected fields flatten) survives verbatim, and so does the legacy
        // `b2id:` line, which since GH #170 is an unknown key like any other.
        assert_eq!(
            note.frontmatter(),
            Some("b2id: 01ABC\ntitle: Foo\nb2_relations:\n  - references [[x]]\n")
        );
        assert_eq!(note.body(), "body\n");
    }

    /// A vault indexed by an older B2 still has `b2id:` lines in it, and W5 is the
    /// whole migration story: the key is not read, not rewritten, not removed —
    /// which is what makes "delete `.b2/` and reindex" a complete upgrade.
    #[test]
    fn a_legacy_b2id_line_is_an_ordinary_unknown_key() {
        let raw = "---\nb2id: 01JMEM0000000000000000000A\ntype: concept\n---\nThe body.\n";
        let mut note = parse(raw);
        assert_eq!(note.as_str(), raw, "round-trips byte-identically");
        assert_eq!(note.fields().r#type.as_deref(), Some("concept"));
        // A body save splices only the body, so the line survives that too.
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
        // Fences with nothing between them: the block exists (Some) but is empty —
        // distinct from no frontmatter at all (None).
        let note = parse("---\n---\nbody\n");
        assert_eq!(note.frontmatter(), Some(""));
    }

    #[test]
    fn frontmatter_readable_reflects_whether_the_yaml_reads_as_metadata() {
        // No block, an empty block, and a mapping all read fine.
        assert!(parse("just a body\n").frontmatter_readable());
        assert!(parse("---\n---\nbody\n").frontmatter_readable());
        assert!(parse("---\ntitle: X\ntags: [x]\n---\nbody\n").frontmatter_readable());
        // Malformed YAML and a non-mapping block don't — the fields come back
        // empty while the bytes round-trip, so the human should be told.
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
        // The one mechanical touch: without it the closing fence would join the
        // last YAML line and the block would never close.
        let mut note = parse("---\nb2id: 01A\n---\nbody\n");
        note.replace_frontmatter("b2id: 01A\ntitle: X");
        assert_eq!(note.as_str(), "---\nb2id: 01A\ntitle: X\n---\nbody\n");
    }

    #[test]
    fn replace_frontmatter_creates_and_removes_the_block() {
        // No block + non-empty YAML → fences created around it.
        let mut note = parse("only a body\n");
        note.replace_frontmatter("b2id: 01A\n");
        assert_eq!(note.as_str(), "---\nb2id: 01A\n---\nonly a body\n");
        // Non-empty → empty removes the block entirely, fences included.
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
