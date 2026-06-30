//! Minimal body chunker (planning/specs/index-engine-build.md step 2).
//!
//! Splits a body into paragraphs — maximal runs of non-blank lines, separated by
//! blank lines — one chunk each. `char_start..char_end` always addresses the
//! exact slice of the body that produced `text`, so chunks stay anchored for
//! explain/highlight.
//!
//! UPGRADE PLAN (step 5, when hybrid-retrieval quality is first measured): replace
//! this with the qmd heuristic borrowed wholesale in the build spec §1.2 —
//! ~900-token chunks, ~15% overlap, Markdown-aware boundary scoring (H1/H2/code
//! fence/…), and a `heading_path` breadcrumb. The `chunks` schema already carries
//! `token_count`/`heading_path` for it; this minimal splitter leaves
//! `heading_path` NULL. Swapping the chunker is a pure re-projection — drop &
//! rebuild — with no schema or invariant change.

/// One projected chunk of a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub seq: usize,
    pub char_start: usize,
    pub char_end: usize,
    pub token_count: usize,
    pub text: String,
}

/// Chunk `body` into paragraphs. Returns an empty vec for an empty/all-blank body.
pub fn chunk_body(body: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut para_start: Option<usize> = None;
    let mut para_end = 0usize;
    let mut offset = 0usize;

    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content = line.trim_end_matches('\n').trim_end_matches('\r');

        if content.trim().is_empty() {
            if let Some(start) = para_start.take() {
                push_chunk(&mut chunks, body, start, para_end);
            }
        } else {
            para_start.get_or_insert(line_start);
            para_end = line_start + content.len();
        }
    }
    if let Some(start) = para_start.take() {
        push_chunk(&mut chunks, body, start, para_end);
    }
    chunks
}

fn push_chunk(chunks: &mut Vec<Chunk>, body: &str, start: usize, end: usize) {
    let text = &body[start..end];
    chunks.push(Chunk {
        seq: chunks.len(),
        char_start: start,
        char_end: end,
        token_count: text.split_whitespace().count(),
        text: text.to_string(),
    });
}
