//! Parse a note body into the links that become edges (planning/data-model.md §2).
//!
//! Two body constructs, both ordinary Obsidian Markdown:
//!   - a bare `[[path|alias]]` anywhere in prose ⇒ an untyped `references` edge;
//!   - a list item `- <verb> [[path|alias]] — explanation` ⇒ a *typed* edge.
//!
//! A typed line *consumes* its wikilink as the edge target, so that link is not
//! also counted as a bare reference. The verb must be lowercase-kebab (the
//! data-model convention), which keeps prose list items like `- See [[x]] …` from
//! being misread as a typed edge of type "see".
//!
//! Hand-rolled (no regex dependency) and deliberately minimal. Known
//! simplifications, to revisit when discovery/queries need them: a typed line
//! yields exactly one edge (extra wikilinks in its trailing text are treated as
//! explanation, not links); wikilinks inside code spans/fences are not excluded;
//! only `—`/`:` introduce an explanation.

/// A link found in a body, ready to be resolved + projected into `edges`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    /// `references` for a bare link, otherwise the relation verb.
    pub edge_type: String,
    /// The path part of `[[path|alias]]`, as written (becomes `dst_path_raw`).
    pub target_path: String,
    /// The `|alias` display text, if present.
    pub alias: Option<String>,
    /// Trailing text after `—`/`:` on a typed line.
    pub explanation: Option<String>,
    /// True when this came from a `- <verb> [[..]]` typed line.
    pub typed: bool,
}

/// Parse every link in `body`, in document order.
pub fn parse_links(body: &str) -> Vec<ParsedLink> {
    let mut links = Vec::new();
    for line in body.lines() {
        match parse_typed_line(line) {
            Some(link) => links.push(link),
            None => scan_bare_links(line, &mut links),
        }
    }
    links
}

/// Try to read `line` as `- <verb> [[path|alias]] [— explanation]`. Returns
/// `None` for anything that isn't a typed-edge line (callers then scan it for
/// bare links).
fn parse_typed_line(line: &str) -> Option<ParsedLink> {
    let after_marker = strip_list_marker(line.trim_start())?;
    let rest = after_marker.trim_start();

    // The verb: a lowercase-kebab token immediately before the wikilink.
    let verb_end = rest.find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    let verb_end = match verb_end {
        Some(0) | None => return None, // no verb (e.g. "- [[..]]") or no following token
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
        alias,
        explanation: extract_explanation(tail),
        typed: true,
    })
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

/// Collect every `[[path|alias]]` in `line` as a `references` edge.
fn scan_bare_links(line: &str, out: &mut Vec<ParsedLink>) {
    let mut rest = line;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let (target_path, alias) = split_target(&after[..close]);
        if !target_path.is_empty() {
            out.push(ParsedLink {
                edge_type: "references".to_string(),
                target_path,
                alias,
                explanation: None,
                typed: false,
            });
        }
        rest = &after[close + 2..];
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
