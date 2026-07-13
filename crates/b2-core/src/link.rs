//! Parse a note body into the links that become edges (planning/data-model.md §2;
//! resource forms: planning/specs/resources-inventory-graph.md §3).
//!
//! Four body constructs, all ordinary Obsidian Markdown:
//!   - a bare `[[path|alias]]` anywhere in prose ⇒ an untyped `references` edge;
//!   - a list item `- <verb> [[path|alias]] — explanation` ⇒ a *typed* edge;
//!   - Markdown's own `[text](path)` / `![alt](path)` (relative vault targets
//!     only — scheme/absolute/fragment-only targets are not vault members and
//!     yield nothing) ⇒ a `references` edge, the `!` marking an **embed** and the
//!     text/alt captured as the edge's **caption** (an image's index text,
//!     slice 3);
//!   - the `![[file.ext|alias]]` embed ⇒ `references` + embed, alias as caption.
//!
//! A typed line *consumes* its wikilink as the edge target, so that link is not
//! also counted as a bare reference. The verb must be lowercase-kebab (the
//! data-model convention), which keeps prose list items like `- See [[x]] …` from
//! being misread as a typed edge of type "see".
//!
//! Hand-rolled (no regex dependency) and deliberately minimal. Known
//! simplifications, to revisit when discovery/queries need them: a typed line
//! yields exactly one edge (extra wikilinks in its trailing text are treated as
//! explanation, not links); links inside code spans/fences are not excluded;
//! only `—`/`:` introduce an explanation; a Markdown link's text stops at the
//! first `]` (no nested brackets) and its target at the first `)` (no titles).

/// A link found in a body, ready to be resolved + projected into `edges`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    /// `references` for a bare link, otherwise the relation verb.
    pub edge_type: String,
    /// The target as written — `[[path|alias]]`'s path part or `[…](path)`'s
    /// parenthesized target (becomes `dst_path_raw`; a `#fragment` suffix is
    /// stripped at *resolution*, never here).
    pub target_path: String,
    /// The `|alias` display text, if present (wikilink forms only).
    pub alias: Option<String>,
    /// Trailing text after `—`/`:` on a typed line.
    pub explanation: Option<String>,
    /// True when this came from a `- <verb> [[..]]` typed line.
    pub typed: bool,
    /// True for an embed form (`![alt](path)` / `![[file]]`) — recorded on the
    /// edge as a display nicety, never a distinct verb (spec §3).
    pub embed: bool,
    /// The authored display text — `![alt](…)`'s alt, `[text](…)`'s text, or a
    /// wikilink's alias. Captured on the edge; it becomes an image's index text
    /// (slice 3).
    pub caption: Option<String>,
}

/// Parse every link in `body`, in document order.
pub fn parse_links(body: &str) -> Vec<ParsedLink> {
    let mut links = Vec::new();
    for line in body.lines() {
        match parse_typed_line(line) {
            Some(link) => links.push(link),
            None => scan_inline_links(line, &mut links),
        }
    }
    links
}

/// Try to read `line` as `- <verb> [[path|alias]] [— explanation]`. Returns
/// `None` for anything that isn't a typed-edge line (callers then scan it for
/// bare links).
fn parse_typed_line(line: &str) -> Option<ParsedLink> {
    let after_marker = strip_list_marker(line.trim_start())?;
    parse_typed_spec(after_marker.trim_start())
}

/// Parse a marker-less typed spec `<verb> [[path|alias]] [— explanation]`. This is
/// the shared core of a body typed-line and a frontmatter `relations:` entry — one
/// syntax, two homes (data-model §2). `None` if it isn't `<verb> <wikilink>`.
pub fn parse_typed_spec(rest: &str) -> Option<ParsedLink> {
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

    let (target_path, alias) = split_target(inner);
    if target_path.is_empty() {
        return None;
    }
    Some(ParsedLink {
        edge_type: verb.to_string(),
        target_path,
        caption: alias.clone(),
        alias,
        explanation: extract_explanation(tail),
        typed: true,
        embed: false,
    })
}

/// Parse one frontmatter `relations:` entry (the string value, no `-` marker): a
/// typed spec `<verb> [[path|alias]] — …`, or a bare `[[path|alias]]` ⇒
/// `references`. `None` if it holds no wikilink. The caller assigns
/// `origin=frontmatter`.
pub fn parse_relation(spec: &str) -> Option<ParsedLink> {
    let spec = spec.trim();
    if let Some(link) = parse_typed_spec(spec) {
        return Some(link);
    }
    // bare wikilink fallback → references
    let mut tmp = Vec::new();
    scan_inline_links(spec, &mut tmp);
    tmp.into_iter().next()
}

/// Strip a single leading list marker (`-`, `*`, `+`) that is followed by
/// whitespace; returns the remainder, or `None` if there is no list marker.
fn strip_list_marker(s: &str) -> Option<&str> {
    let rest = s
        .strip_prefix('-')
        .or_else(|| s.strip_prefix('*'))
        .or_else(|| s.strip_prefix('+'))?;
    if rest.starts_with(char::is_whitespace) {
        Some(rest)
    } else {
        None
    }
}

/// Collect every inline link in `line` as a `references` edge, in written order:
/// `[[path|alias]]` and `![[path|alias]]` (wikilink + embed), and Markdown's own
/// `[text](target)` / `![alt](target)` — the latter only for **vault** targets
/// (a scheme, an absolute path, or a fragment-only target is not a vault member
/// and yields nothing; spec §3).
fn scan_inline_links(line: &str, out: &mut Vec<ParsedLink>) {
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

        // Wikilink (plain or embed): `[[path|alias]]`.
        if let Some(inner_rest) = bracketed.strip_prefix("[[") {
            let Some(close) = inner_rest.find("]]") else {
                i += marker + 1;
                continue;
            };
            let (target_path, alias) = split_target(&inner_rest[..close]);
            if !target_path.is_empty() {
                out.push(ParsedLink {
                    edge_type: "references".to_string(),
                    target_path,
                    caption: alias.clone(),
                    alias,
                    explanation: None,
                    typed: false,
                    embed,
                });
            }
            i += marker + 2 + close + 2;
            continue;
        }

        // Markdown form: `[text](target)`.
        if let Some((text, target, consumed)) = parse_md_link(bracketed) {
            if !is_external_target(&target) {
                let text = text.trim();
                out.push(ParsedLink {
                    edge_type: "references".to_string(),
                    target_path: target,
                    alias: None,
                    caption: (!text.is_empty()).then(|| text.to_string()),
                    explanation: None,
                    typed: false,
                    embed,
                });
            }
            i += marker + consumed;
            continue;
        }

        i += marker + 1;
    }
}

/// Parse a leading `[text](target)`; returns `(text, target, bytes consumed)`.
/// Minimal by design (module doc): text stops at the first `]`, the target at the
/// first `)`, and `](` must be adjacent.
fn parse_md_link(s: &str) -> Option<(String, String, usize)> {
    let inner = s.strip_prefix('[')?;
    let close = inner.find(']')?;
    let text = &inner[..close];
    let target_rest = inner[close + 1..].strip_prefix('(')?;
    let end = target_rest.find(')')?;
    let target = target_rest[..end].trim();
    if target.is_empty() {
        return None;
    }
    // '[' + text + ']' + '(' + target + ')'
    Some((text.to_string(), target.to_string(), close + end + 4))
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
        Some((scheme, _)) => {
            !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Split `path|alias` (alias optional) into trimmed parts.
fn split_target(inner: &str) -> (String, Option<String>) {
    match inner.split_once('|') {
        Some((path, alias)) => (path.trim().to_string(), Some(alias.trim().to_string())),
        None => (inner.trim().to_string(), None),
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

    /// Shorthand: parse one line, return `(type, target, caption, embed, typed)`.
    fn parsed(line: &str) -> Vec<(String, String, Option<String>, bool, bool)> {
        parse_links(line)
            .into_iter()
            .map(|l| (l.edge_type, l.target_path, l.caption, l.embed, l.typed))
            .collect()
    }

    #[test]
    fn markdown_forms_yield_references_with_caption_and_embed() {
        let cases: &[(&str, &[(&str, &str, Option<&str>, bool, bool)])] = &[
            // ![alt](path) — embed with caption
            (
                "See ![a sailboat](img/IMG_2041.jpg) here.",
                &[("references", "img/IMG_2041.jpg", Some("a sailboat"), true, false)],
            ),
            // [text](path) — plain link with caption
            (
                "Read [the paper](papers/attention.pdf).",
                &[("references", "papers/attention.pdf", Some("the paper"), false, false)],
            ),
            // empty alt: still an edge, no caption
            ("![](img/x.png)", &[("references", "img/x.png", None, true, false)]),
            // ![[file.ext]] embed, with and without alias
            ("![[img/photo.png]]", &[("references", "img/photo.png", None, true, false)]),
            (
                "![[img/photo.png|hero shot]]",
                &[("references", "img/photo.png", Some("hero shot"), true, false)],
            ),
            // bare wikilink to a resource: alias doubles as caption
            (
                "[[papers/x.pdf|the paper]]",
                &[("references", "papers/x.pdf", Some("the paper"), false, false)],
            ),
            // a .md markdown link is parsed too — resolution dispatches by extension
            (
                "[background](concepts/memory.md)",
                &[("references", "concepts/memory.md", Some("background"), false, false)],
            ),
            // fragment kept raw (stripped at resolution, not here)
            (
                "[sec](notes/a.md#history)",
                &[("references", "notes/a.md#history", Some("sec"), false, false)],
            ),
            // document order across mixed forms on one line
            (
                "[[a.md]] then ![x](b.png) then [y](c.txt)",
                &[
                    ("references", "a.md", None, false, false),
                    ("references", "b.png", Some("x"), true, false),
                    ("references", "c.txt", Some("y"), false, false),
                ],
            ),
        ];
        for (line, want) in cases {
            let got = parsed(line);
            let want: Vec<_> = want
                .iter()
                .map(|(t, p, c, e, ty)| {
                    (t.to_string(), p.to_string(), c.map(str::to_string), *e, *ty)
                })
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
    fn typed_lines_still_consume_their_wikilink() {
        let links = parse_links("- supports [[papers/x.pdf|the paper]] — key evidence");
        assert_eq!(links.len(), 1);
        let l = &links[0];
        assert!(l.typed);
        assert_eq!(l.edge_type, "supports");
        assert_eq!(l.target_path, "papers/x.pdf");
        assert_eq!(l.caption.as_deref(), Some("the paper"));
        assert_eq!(l.explanation.as_deref(), Some("key evidence"));
        assert!(!l.embed);
    }

    #[test]
    fn malformed_forms_do_not_derail_the_scan() {
        // an unclosed wikilink on one line never hides the next line's links
        assert_eq!(
            parsed("broken [[x\nfine [ok](a.png)"),
            vec![("references".into(), "a.png".into(), Some("ok".into()), false, false)]
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
}
