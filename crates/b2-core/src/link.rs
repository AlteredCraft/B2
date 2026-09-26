//! Parse a note body into the links that become edges — B2's **one link grammar**.
//!
//! **The body carries no B2 syntax** (ADR-0010): every body construct is ordinary
//! Obsidian Markdown and yields an untyped `references` edge — prose around a link is just
//! prose. Three body constructs: a bare `[[path|alias]]`; Markdown's own `[text](path)` /
//! `![alt](path)` (relative vault targets only, the `!` marking an **embed** and the
//! text/alt captured as the edge's **caption**); and the `![[file.ext|alias]]` embed. A
//! *typed* edge — `<verb> [[path|alias]] — explanation` — exists only as a frontmatter
//! `b2_relations:` entry, parsed by [`parse_relation`] and written by [`render_relation`].
//!
//! Everything that reads link text goes through [`link_spans`]: ingest projects edges from
//! it ([`parse_links`]) and a move rewrites targets in place from it (`mv.rs`), so what a
//! move repairs is exactly what ingest projected. The scan is **per line**: a stray `[[`
//! never pairs with a `]]` on a later line.
//!
//! Hand-rolled and deliberately minimal. Known simplifications, to revisit when queries
//! need them: a typed frontmatter entry yields exactly one edge (extra wikilinks in its
//! trailing text read as explanation); links inside code spans/fences are not excluded;
//! only `—`/`:` introduce an explanation; a Markdown link's text stops at the first `]`
//! and its target at the first `)`.

use std::ops::Range;

/// A link found in a body, ready to be resolved + projected into `edges`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    /// `references` for a bare link, otherwise the relation verb.
    pub edge_type: String,
    /// The target as written — `[[path|alias]]`'s path part or `[…](path)`'s
    /// parenthesized target (becomes `dst_path_raw`; a `#fragment` suffix is
    /// stripped at *resolution*, never here).
    pub target_path: String,
    /// Trailing text after `—`/`:` on a typed frontmatter entry.
    pub explanation: Option<String>,
    /// True for an embed form (`![alt](path)` / `![[file]]`) — recorded on the
    /// edge as a display nicety, never a distinct verb (data-model.md §3).
    pub embed: bool,
    /// True when the target came from a Markdown-form link (`[…](target)`).
    /// Resolution treats these with standard Markdown semantics — note-relative
    /// first, then vault-root — while wikilink targets stay vault-root.
    pub md_form: bool,
    /// The authored display text — `![alt](…)`'s alt, `[text](…)`'s text, or a
    /// wikilink's alias. Stored as the edge's `caption` (data-model.md §3).
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

/// Where one link sits in the scanned text, as byte ranges into it — so the one grammar
/// serves both reading a link ([`parse_links`]) and splicing a new target into it in
/// place (the move's rewrite).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkSpan {
    pub form: LinkForm,
    /// A `!` directly before the opening bracket.
    pub embed: bool,
    /// The target token, trimmed: `concepts/memory` in `[[ concepts/memory | M ]]`,
    /// `img.png` in `![a]( img.png )`. Never empty.
    pub target: Range<usize>,
    /// The display text, trimmed: a wikilink's alias (present — possibly empty — whenever
    /// the link has a `|`), or a Markdown link's text (absent when blank).
    pub caption: Option<Range<usize>>,
}

/// Every vault link in `text`, in document order, scanned line by line. A Markdown-form
/// link at an external target (a scheme, an absolute path, a fragment-only anchor) is
/// not a vault link and yields no span; nor does a wikilink with an empty target.
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

/// Parse every link in `body`, in document order — all untyped `references`
/// edges: the body carries no typed syntax (data-model §2).
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

/// Parse a typed spec `<verb> [[path|alias]] [— explanation]` — the shape of a
/// frontmatter `b2_relations:` entry (data-model §2). `None` if it isn't
/// `<verb> <wikilink>`.
fn parse_typed_spec(rest: &str) -> Option<ParsedLink> {
    // The verb: a lowercase-kebab token immediately before the wikilink.
    let verb_end = rest.find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    let verb_end = match verb_end {
        Some(0) | None => return None, // no verb (e.g. "[[..]]") or no following token
        Some(n) => n,
    };
    let verb = &rest[..verb_end];
    if !verb.starts_with(|c: char| c.is_ascii_lowercase()) {
        return None;
    }

    // The wikilink must follow the verb directly (whitespace allowed).
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

/// Parse one frontmatter `b2_relations:` entry (the string value of one YAML
/// list item): a typed spec `<verb> [[path|alias]] — …`, or a bare
/// `[[path|alias]]` ⇒ `references`. `None` if it holds no wikilink. The caller
/// assigns `origin=frontmatter`. This is the **only** parser that yields a verb
/// or explanation — the body never does (data-model §2).
pub fn parse_relation(spec: &str) -> Option<ParsedLink> {
    let spec = spec.trim();
    if let Some(link) = parse_typed_spec(spec) {
        return Some(link);
    }
    // bare link fallback → references (the entry is one YAML value, scanned whole)
    let mut spans = Vec::new();
    scan_line(spec, 0, &mut spans);
    spans.first().map(|span| reference(spec, span))
}

/// Render a typed frontmatter `b2_relations:` entry — the inverse of [`parse_relation`]
/// for a typed spec: `<verb> [[target_path]]`, then ` — explanation` when one is given
/// (data-model §2). `target_path` is written verbatim as the link text, so the caller
/// chooses the convention (a note's `.md` dropped, as Obsidian writes links). A blank
/// explanation is omitted and a given one trimmed, exactly as parsing reads it back.
pub fn render_relation(verb: &str, target_path: &str, explanation: Option<&str>) -> String {
    match explanation.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => format!("{verb} [[{target_path}]] — {e}"),
        None => format!("{verb} [[{target_path}]]"),
    }
}

/// Collect every link in `line` as a [`LinkSpan`] offset by `base`, in written order:
/// `[[path|alias]]` and `![[path|alias]]` (wikilink + embed), and Markdown's own
/// `[text](target)` / `![alt](target)` — the latter only for **vault** targets
/// (a scheme, an absolute path, or a fragment-only target is not a vault member
/// and yields nothing; data-model.md §10).
fn scan_line(line: &str, base: usize, out: &mut Vec<LinkSpan>) {
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        // An embed marker only counts directly before a bracket.
        let (embed, bracketed) = match rest.strip_prefix('!') {
            Some(r) if r.starts_with('[') => (true, r),
            _ if rest.starts_with('[') => (false, rest),
            _ => {
                // Not a link start — skip one char (multi-byte safe).
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

/// Parse a leading `[text](target)`. Minimal by design (module doc): text stops at the
/// first `]`, the target at the first `)`, and `](` must be adjacent.
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

/// A wikilink's inner text (between `[[` and `]]`, starting at byte `base`) split at the
/// first `|` into the trimmed target and, when a `|` is present, the trimmed alias.
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

/// `r` moved `by` bytes to the right.
fn shift(r: &Range<usize>, by: usize) -> Range<usize> {
    r.start + by..r.end + by
}

/// A Markdown-form target that is **not** a vault member: any scheme
/// (`https://…`, `mailto:…`), an absolute path, or a fragment-only anchor.
/// Wikilink targets never come through here — they are vault paths by
/// construction.
fn is_external_target(target: &str) -> bool {
    if target.starts_with('/') || target.starts_with('#') {
        return true;
    }
    match target.split_once(':') {
        Some((scheme, _)) => !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()),
        None => false,
    }
}

/// Read the explanation after a typed link: trailing text introduced by an
/// em-dash or a colon (data-model §2). Anything else means no explanation.
fn extract_explanation(tail: &str) -> Option<String> {
    let t = tail.trim_start();
    let body = t.strip_prefix('—').or_else(|| t.strip_prefix(':'))?;
    let e = body.trim();
    (!e.is_empty()).then(|| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One expected link, as the case tables spell it: `(type, target, caption, embed)`.
    type Expected<'a> = (&'a str, &'a str, Option<&'a str>, bool);

    /// Shorthand: parse one line, return `(type, target, caption, embed)`.
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
        // The body carries no typed syntax (data-model §2): a list item opening
        // with a verb-looking word is prose, and only its wikilink projects.
        let links = parse_links("- supports [[papers/x.pdf|the paper]] — key evidence");
        assert_eq!(links.len(), 1);
        let l = &links[0];
        assert_eq!(l.edge_type, "references");
        assert_eq!(l.target_path, "papers/x.pdf");
        assert_eq!(l.caption.as_deref(), Some("the paper"));
        assert_eq!(l.explanation, None);
        assert!(!l.embed);
    }

    /// The exact hazard that killed the body typed-line syntax (decision
    /// 2026-07-21): a *lowercase* verb lookalike opening a list item must stay
    /// prose. `- see [[x]]` becoming a typed edge of verb "see" is the failure
    /// mode; only the wikilink may project, always untyped.
    #[test]
    fn lowercase_verb_lookalikes_in_prose_stay_prose() {
        let links = parse_links("- see [[concepts/memory|Human memory]] for the mechanism\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].edge_type, "references");
    }

    /// A prose link and a verb-led list item are the same thing to the parser: two
    /// `references` edges in document order, neither carrying an explanation — no
    /// body shape is ever "special".
    #[test]
    fn body_links_never_gain_a_type_from_surrounding_prose() {
        let body = "Spaced repetition exploits the [[concepts/memory|Human memory]] retrieval curve.\n\n## Relations\n- supports [[concepts/memory|Human memory]] — applies the forgetting curve\n";
        let links = parse_links(body);
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.edge_type == "references"));
        assert!(links.iter().all(|l| l.explanation.is_none()));
    }

    /// A wikilink's caption is its own `|`-part: absent means `None`, never an empty
    /// string.
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

        // A bare entry (no verb) falls back to an untyped reference.
        let bare = parse_relation("[[notes/a|A]]").unwrap();
        assert_eq!(bare.edge_type, "references");
        assert_eq!(bare.target_path, "notes/a");

        // No wikilink at all ⇒ no edge.
        assert!(parse_relation("just some words").is_none());
    }

    /// The two accepted explanation separators and the tolerated verb tail
    /// (relation.rs): `—` is asserted above, `:` here, and a non-core verb is
    /// stored verbatim rather than coerced into the closed core.
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

    /// What `b2 link` writes is what ingest reads back: rendering then parsing a
    /// relation returns the verb, target and explanation it was rendered from — and a
    /// blank explanation renders as none at all.
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
        // same-line recovery: the link is still found (its caption may swallow
        // the stray bracket — the documented first-`]` minimalism)
        let got = parsed("broken [[x then fine [ok](a.png)");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "a.png");
        // reference-style links (no adjacent "](" ) are not vault links
        assert!(parsed("[text][ref]").is_empty());
        // a lone bang is prose
        assert_eq!(parsed("hey! [x](a.png)").len(), 1);
    }

    /// The spans are what a move splices into, so their ranges must name exactly the
    /// trimmed target (and caption) bytes, offset across lines — CRLF endings included.
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

    /// A stray `[[` never pairs with a `]]` on a later line, so the later line's link
    /// is found — by ingest and, through the same spans, by a move's rewrite.
    #[test]
    fn a_stray_open_bracket_never_swallows_the_next_lines_link() {
        let text = "broken [[x\nsee [[old]]\n";
        let spans = link_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].target.clone()], "old");
    }
}
