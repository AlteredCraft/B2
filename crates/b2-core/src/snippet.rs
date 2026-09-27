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

/// A chunk's head as a single-line, length-bounded snippet, where there is no query.
pub(crate) fn snippet(text: &str) -> String {
    head_snippet(&flatten(text))
}

/// Like [`snippet`] but windowed around the first query-term match. Falls back to the
/// head when nothing matches or the match is already in view.
pub(crate) fn query_snippet(text: &str, query: &str) -> String {
    let flat = flatten(text);
    if flat.chars().count() <= SNIPPET_CHARS {
        return flat;
    }
    let lower = flat.to_lowercase();
    // The lexical half's tokenizer, so the window lands on a term that ranked the hit.
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
    // Lowercasing can change length for exotic Unicode; clamp so the slice stays in range.
    let start = (pos - LEAD).min(chars.len());
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let mut out = String::from("…");
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}
