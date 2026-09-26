//! One-line excerpts of chunk text for display: a search hit's snippet, a similar card's
//! evidence, a citation's excerpt.

/// Longest snippet (in chars) shown for a search hit, so a result stays one line.
const SNIPPET_CHARS: usize = 160;

/// Flatten a chunk's text to a single whitespace-collapsed line.
fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The head of a flattened chunk, bounded to one line.
fn head_snippet(flat: &str) -> String {
    if flat.chars().count() <= SNIPPET_CHARS {
        flat.to_string()
    } else {
        let cut: String = flat.chars().take(SNIPPET_CHARS).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Collapse a chunk's text to a single-line, length-bounded snippet (its head). Used
/// where there is no query to center on (e.g. `similar`'s evidence passage).
pub(crate) fn snippet(text: &str) -> String {
    head_snippet(&flatten(text))
}

/// Like [`snippet`] but windows the excerpt around the first query-term match, so a
/// section-sized chunk still surfaces the matched text instead of only its head.
/// Falls back to the head when no term matches or the match is already in view — a
/// pure vector hit keeps the head.
pub(crate) fn query_snippet(text: &str, query: &str) -> String {
    let flat = flatten(text);
    if flat.chars().count() <= SNIPPET_CHARS {
        return flat;
    }
    let lower = flat.to_lowercase();
    // The same tokenizer the lexical half matched with, so the window lands on a term
    // that actually ranked the hit.
    let match_pos = crate::search::query_terms(query)
        .iter()
        .filter(|t| t.chars().count() >= 2)
        .filter_map(|t| {
            let byte = lower.find(&t.to_lowercase())?;
            Some(lower[..byte].chars().count())
        })
        .min();
    // A little lead-in so the match is not flush against the ellipsis.
    const LEAD: usize = 24;
    let Some(pos) = match_pos.filter(|p| *p > LEAD) else {
        return head_snippet(&flat);
    };
    let chars: Vec<char> = flat.chars().collect();
    // `pos` indexes the lowercased text, whose length can differ from `flat` for
    // exotic Unicode; clamp so the slice below can never go out of range.
    let start = (pos - LEAD).min(chars.len());
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let mut out = String::from("…");
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}
