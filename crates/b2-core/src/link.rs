//! B2's one link grammar: parse a note body into the links that become edges.
//!
//! The body carries no B2 syntax (ADR-0010): `[[path|alias]]`, `![[file|alias]]`,
//! `[text](path)` and `![alt](path)` all yield untyped `references` edges. A typed edge
//! (`<verb> [[path]] — explanation`) exists only in frontmatter `b2_relations:`
//! ([`parse_relation`], [`render_relation`]).
//!
//! Ingest and `mv.rs` both read links via [`link_spans`], so a move repairs exactly what
//! ingest projected. The scan is per line. Known simplifications: links in code are not
//! excluded; only `—`/`:` introduce an explanation; a Markdown link's text stops at the
//! first `]` and its target at the first `)`.

use std::ops::Range;

/// A link found in a body, ready to be resolved + projected into `edges`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    /// `references` for a bare link, otherwise the relation verb.
    pub edge_type: String,
    /// The target as written (`dst_path_raw`); a `#fragment` is stripped at resolution.
    pub target_path: String,
    /// Trailing text after `—`/`:` on a typed frontmatter entry.
    pub explanation: Option<String>,
    /// An embed form (`![…]`): a display nicety, never a distinct verb (data-model.md §3).
    pub embed: bool,
    /// A Markdown-form link, resolved note-relative first, then vault-root.
    pub md_form: bool,
    /// The alt, link text or alias; the edge's `caption` (data-model.md §3).
    pub caption: Option<String>,
}

/// Which syntax wrote a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkForm {
    /// `[[path|alias]]` / `![[path|alias]]` — a vault-root target.
    Wiki,
    /// `[text](target)` / `![alt](target)` — note-relative first, then vault-root.
    Markdown,
}

/// Where one link sits in the text, as byte ranges, so a move can splice in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkSpan {
    pub form: LinkForm,
    /// A `!` directly before the opening bracket.
    pub embed: bool,
    /// The target token, trimmed. Never empty.
    pub target: Range<usize>,
    /// Trimmed: a wikilink's alias (present, maybe empty, when there is a `|`), or a
    /// Markdown link's text (absent when blank).
    pub caption: Option<Range<usize>>,
}

/// Every vault link in `text`, in document order, scanned line by line.
pub(crate) fn link_spans(text: &str) -> Vec<LinkSpan> {
    let mut spans = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let content = match line.strip_suffix('\n') {
            Some(l) => l.strip_suffix('\r').unwrap_or(l),
            None => line,
        };
        scan_line(content, line_start, &mut spans);
        line_start += line.len();
    }
    spans
}

/// Every link in `body` as an untyped `references` edge (data-model §2).
pub fn parse_links(body: &str) -> Vec<ParsedLink> {
    link_spans(body)
        .iter()
        .map(|span| reference(body, span))
        .collect()
}

/// The untyped `references` edge a span authors.
fn reference(text: &str, span: &LinkSpan) -> ParsedLink {
    ParsedLink {
        edge_type: "references".to_string(),
        target_path: text[span.target.clone()].to_string(),
        explanation: None,
        embed: span.embed,
        md_form: span.form == LinkForm::Markdown,
        caption: span.caption.clone().map(|c| text[c].to_string()),
    }
}

/// Parse a typed spec `<verb> [[path|alias]] [— explanation]` (data-model §2).
fn parse_typed_spec(rest: &str) -> Option<ParsedLink> {
    // A lowercase-kebab verb.
    let verb_end = rest.find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    let verb_end = match verb_end {
        Some(0) | None => return None, // no verb (e.g. "[[..]]") or no following token
        Some(n) => n,
    };
    let verb = &rest[..verb_end];
    if !verb.starts_with(|c: char| c.is_ascii_lowercase()) {
        return None;
    }

    let after_verb = rest[verb_end..].trim_start();
    let inner_and_rest = after_verb.strip_prefix("[[")?;
    let close = inner_and_rest.find("]]")?;
    let inner = &inner_and_rest[..close];
    let tail = &inner_and_rest[close + 2..];

    let (target, alias) = split_wiki_inner(inner, 0);
    if target.is_empty() {
        return None;
    }
    Some(ParsedLink {
        edge_type: verb.to_string(),
        target_path: inner[target].to_string(),
        caption: alias.map(|a| inner[a].to_string()),
        explanation: extract_explanation(tail),
        embed: false,
        md_form: false,
    })
}

/// Parse one `b2_relations:` entry: a typed spec, or a bare `[[…]]` ⇒ `references`. The
/// only parser that yields a verb or explanation (data-model §2).
pub fn parse_relation(spec: &str) -> Option<ParsedLink> {
    let spec = spec.trim();
    if let Some(link) = parse_typed_spec(spec) {
        return Some(link);
    }
    // Bare link → references.
    let mut spans = Vec::new();
    scan_line(spec, 0, &mut spans);
    spans.first().map(|span| reference(spec, span))
}

/// Render a typed `b2_relations:` entry, the inverse of [`parse_relation`] (data-model
/// §2). `target_path` is written verbatim; a blank explanation is omitted.
pub fn render_relation(verb: &str, target_path: &str, explanation: Option<&str>) -> String {
    match explanation.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => format!("{verb} [[{target_path}]] — {e}"),
        None => format!("{verb} [[{target_path}]]"),
    }
}

/// Collect every link in `line` as a [`LinkSpan`] offset by `base`. Markdown-form links
/// count only for vault targets (data-model.md §10).
fn scan_line(line: &str, base: usize, out: &mut Vec<LinkSpan>) {
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        // An embed marker only counts directly before a bracket.
        let (embed, bracketed) = match rest.strip_prefix('!') {
            Some(r) if r.starts_with('[') => (true, r),
            _ if rest.starts_with('[') => (false, rest),
            _ => {
                // Skip one char, multi-byte safe.
                i += rest.chars().next().map_or(1, char::len_utf8);
                continue;
            }
        };
        let marker = usize::from(embed); // the '!' byte, when present
        let open = base + i + marker; // the first '[' byte

        // Wikilink (plain or embed): `[[path|alias]]`.
        if let Some(inner_rest) = bracketed.strip_prefix("[[") {
            let Some(close) = inner_rest.find("]]") else {
                i += marker + 1;
                continue;
            };
            let (target, alias) = split_wiki_inner(&inner_rest[..close], open + 2);
            if !target.is_empty() {
                out.push(LinkSpan {
                    form: LinkForm::Wiki,
                    embed,
                    target,
                    caption: alias,
                });
            }
            i += marker + 2 + close + 2;
            continue;
        }

        // Markdown form: `[text](target)`.
        if let Some(md) = parse_md_link(bracketed) {
            let target = shift(&md.target, open);
            if !is_external_target(&line[target.start - base..target.end - base]) {
                out.push(LinkSpan {
                    form: LinkForm::Markdown,
                    embed,
                    target,
                    caption: (!md.text.is_empty()).then(|| shift(&md.text, open)),
                });
            }
            i += marker + md.consumed;
            continue;
        }

        i += marker + 1;
    }
}

/// A leading Markdown link's parts, as ranges into the string it was parsed from.
struct MdLink {
    /// The link text, trimmed (possibly empty).
    text: Range<usize>,
    /// The target, trimmed (never empty).
    target: Range<usize>,
    /// Bytes from the `[` through the closing `)`.
    consumed: usize,
}

/// Parse a leading `[text](target)`. Minimal by design (module doc); `](` must be adjacent.
fn parse_md_link(s: &str) -> Option<MdLink> {
    let inner = s.strip_prefix('[')?;
    let close = inner.find(']')?;
    let target_rest = inner[close + 1..].strip_prefix('(')?;
    let end = target_rest.find(')')?;
    let target_start = close + 3; // '[' + text + ']' + '('
    let target = trimmed(&target_rest[..end], target_start);
    if target.is_empty() {
        return None;
    }
    Some(MdLink {
        text: trimmed(&inner[..close], 1),
        target,
        // '[' + text + ']' + '(' + target + ')'
        consumed: close + end + 4,
    })
}

/// Split a wikilink's inner text (at byte `base`) at the first `|` into target and alias.
fn split_wiki_inner(inner: &str, base: usize) -> (Range<usize>, Option<Range<usize>>) {
    match inner.split_once('|') {
        Some((path, alias)) => (
            trimmed(path, base),
            Some(trimmed(alias, base + path.len() + 1)),
        ),
        None => (trimmed(inner, base), None),
    }
}

/// The range of `s.trim()` within the text `s` starts at byte `base` of.
fn trimmed(s: &str, base: usize) -> Range<usize> {
    let start = base + (s.len() - s.trim_start().len());
    start..start + s.trim().len()
}

fn shift(r: &Range<usize>, by: usize) -> Range<usize> {
    r.start + by..r.end + by
}

/// A Markdown-form target outside the vault: a scheme, an absolute path, or `#anchor`.
fn is_external_target(target: &str) -> bool {
    if target.starts_with('/') || target.starts_with('#') {
        return true;
    }
    match target.split_once(':') {
        Some((scheme, _)) => !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()),
        None => false,
    }
}

/// The explanation after a typed link, introduced by `—` or `:` (data-model §2).
fn extract_explanation(tail: &str) -> Option<String> {
    let t = tail.trim_start();
    let body = t.strip_prefix('—').or_else(|| t.strip_prefix(':'))?;
    let e = body.trim();
    (!e.is_empty()).then(|| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(type, target, caption, embed)`.
    type Expected<'a> = (&'a str, &'a str, Option<&'a str>, bool);

    fn parsed(line: &str) -> Vec<(String, String, Option<String>, bool)> {
        parse_links(line)
            .into_iter()
            .map(|l| (l.edge_type, l.target_path, l.caption, l.embed))
            .collect()
    }

    #[test]
    fn markdown_forms_yield_references_with_caption_and_embed() {
        let cases: &[(&str, &[Expected])] = &[
            // ![alt](path) — embed with caption
            (
                "See ![a sailboat](img/IMG_2041.jpg) here.",
                &[("references", "img/IMG_2041.jpg", Some("a sailboat"), true)],
            ),
            // [text](path) — plain link with caption
            (
                "Read [the paper](papers/attention.pdf).",
                &[(
                    "references",
                    "papers/attention.pdf",
                    Some("the paper"),
                    false,
                )],
            ),
            // empty alt: still an edge, no caption
            ("![](img/x.png)", &[("references", "img/x.png", None, true)]),
            // ![[file.ext]] embed, with and without alias
            (
                "![[img/photo.png]]",
                &[("references", "img/photo.png", None, true)],
            ),
            (
                "![[img/photo.png|hero shot]]",
                &[("references", "img/photo.png", Some("hero shot"), true)],
            ),
            // bare wikilink to a resource: alias doubles as caption
            (
                "[[papers/x.pdf|the paper]]",
                &[("references", "papers/x.pdf", Some("the paper"), false)],
            ),
            // a .md markdown link is parsed too — resolution dispatches by extension
            (
                "[background](concepts/memory.md)",
                &[(
                    "references",
                    "concepts/memory.md",
                    Some("background"),
                    false,
                )],
            ),
            // fragment kept raw (stripped at resolution, not here)
            (
                "[sec](notes/a.md#history)",
                &[("references", "notes/a.md#history", Some("sec"), false)],
            ),
            // document order across mixed forms on one line
            (
                "[[a.md]] then ![x](b.png) then [y](c.txt)",
                &[
                    ("references", "a.md", None, false),
                    ("references", "b.png", Some("x"), true),
                    ("references", "c.txt", Some("y"), false),
                ],
            ),
        ];
        for (line, want) in cases {
            let got = parsed(line);
            let want: Vec<_> = want
                .iter()
                .map(|(t, p, c, e)| (t.to_string(), p.to_string(), c.map(str::to_string), *e))
                .collect();
            assert_eq!(got, want, "line: {line}");
        }
    }

    #[test]
    fn external_targets_yield_nothing() {
        let cases = [
            "[site](https://example.com/a.png)",
            "[mail](mailto:a@b.c)",
            "[abs](/etc/passwd)",
            "[frag](#heading-only)",
            "![remote](http://x.y/img.png)",
        ];
        for line in cases {
            assert!(parsed(line).is_empty(), "line must yield nothing: {line}");
        }
    }

    #[test]
    fn a_verb_shaped_body_list_item_is_just_a_reference() {
        // The body carries no typed syntax (data-model §2).
        let links = parse_links("- supports [[papers/x.pdf|the paper]] — key evidence");
        assert_eq!(links.len(), 1);
        let l = &links[0];
        assert_eq!(l.edge_type, "references");
        assert_eq!(l.target_path, "papers/x.pdf");
        assert_eq!(l.caption.as_deref(), Some("the paper"));
        assert_eq!(l.explanation, None);
        assert!(!l.embed);
    }

    /// The hazard that retired body typed-line syntax (decision 2026-07-21).
    #[test]
    fn lowercase_verb_lookalikes_in_prose_stay_prose() {
        let links = parse_links("- see [[concepts/memory|Human memory]] for the mechanism\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].edge_type, "references");
    }

    #[test]
    fn body_links_never_gain_a_type_from_surrounding_prose() {
        let body = "Spaced repetition exploits the [[concepts/memory|Human memory]] retrieval curve.\n\n## Relations\n- supports [[concepts/memory|Human memory]] — applies the forgetting curve\n";
        let links = parse_links(body);
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.edge_type == "references"));
        assert!(links.iter().all(|l| l.explanation.is_none()));
    }

    #[test]
    fn a_wikilink_without_an_alias_has_no_caption() {
        let links = parse_links("Refer to [[concepts/memory]].\n");
        assert_eq!(links[0].target_path, "concepts/memory");
        assert_eq!(links[0].caption, None);
    }

    #[test]
    fn parse_relation_reads_verb_link_and_explanation() {
        let l = parse_relation("supports [[papers/x.pdf|the paper]] — key evidence").unwrap();
        assert_eq!(l.edge_type, "supports");
        assert_eq!(l.target_path, "papers/x.pdf");
        assert_eq!(l.caption.as_deref(), Some("the paper"));
        assert_eq!(l.explanation.as_deref(), Some("key evidence"));

        let bare = parse_relation("[[notes/a|A]]").unwrap();
        assert_eq!(bare.edge_type, "references");
        assert_eq!(bare.target_path, "notes/a");

        assert!(parse_relation("just some words").is_none());
    }

    #[test]
    fn relation_accepts_a_colon_separator_and_keeps_a_tail_verb_verbatim() {
        let colon =
            parse_relation("supersedes [[notes/old-plan|Old plan]] : replaced after Q2").unwrap();
        assert_eq!(colon.edge_type, "supersedes");
        assert_eq!(colon.explanation.as_deref(), Some("replaced after Q2"));

        let tail = parse_relation("inspired-by [[notes/x|X]]").unwrap();
        assert_eq!(tail.edge_type, "inspired-by");
        assert_eq!(tail.explanation, None);
    }

    #[test]
    fn a_rendered_relation_parses_back_to_what_it_was_rendered_from() {
        let cases: &[(&str, &str, Option<&str>)] = &[
            (
                "supports",
                "concepts/memory",
                Some("applies the forgetting curve"),
            ),
            ("contradicts", "notes/a.md", None),
            ("references", "papers/x.pdf", Some("the key table")),
            ("inspired-by", "deep/nested/note", Some("a: colon inside")),
        ];
        for &(verb, target, explanation) in cases {
            let spec = render_relation(verb, target, explanation);
            let got = parse_relation(&spec).unwrap();
            assert_eq!(
                (got.edge_type.as_str(), got.target_path.as_str()),
                (verb, target),
                "{spec}"
            );
            assert_eq!(got.explanation.as_deref(), explanation, "{spec}");
        }
        assert_eq!(
            render_relation("supports", "a", Some("  ")),
            "supports [[a]]"
        );
        assert_eq!(
            render_relation("supports", "a", Some(" why ")),
            "supports [[a]] — why"
        );
    }

    #[test]
    fn malformed_forms_do_not_derail_the_scan() {
        // an unclosed wikilink on one line never hides the next line's links
        assert_eq!(
            parsed("broken [[x\nfine [ok](a.png)"),
            vec![(
                "references".into(),
                "a.png".into(),
                Some("ok".into()),
                false
            )]
        );
        // same-line recovery (its caption may swallow the stray bracket)
        let got = parsed("broken [[x then fine [ok](a.png)");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "a.png");
        // reference-style links (no adjacent "](" ) are not vault links
        assert!(parsed("[text][ref]").is_empty());
        // a lone bang is prose
        assert_eq!(parsed("hey! [x](a.png)").len(), 1);
    }

    #[test]
    fn spans_address_the_trimmed_target_bytes_across_lines() {
        let text = "a [[ x/y | Why ]] b\r\n![alt]( img.png ) and [t](https://e.x)\n[[z]]";
        let spans = link_spans(text);
        let slices: Vec<(LinkForm, bool, &str, Option<&str>)> = spans
            .iter()
            .map(|s| {
                (
                    s.form,
                    s.embed,
                    &text[s.target.clone()],
                    s.caption.clone().map(|c| &text[c]),
                )
            })
            .collect();
        assert_eq!(
            slices,
            vec![
                (LinkForm::Wiki, false, "x/y", Some("Why")),
                (LinkForm::Markdown, true, "img.png", Some("alt")),
                (LinkForm::Wiki, false, "z", None),
            ]
        );
    }

    #[test]
    fn a_stray_open_bracket_never_swallows_the_next_lines_link() {
        let text = "broken [[x\nsee [[old]]\n";
        let spans = link_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].target.clone()], "old");
    }
}
