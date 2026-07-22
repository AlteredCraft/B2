//! Body chunker — the qmd heuristic (index-engine.md §1, issue #19).
//!
//! Splits a note body into **size-targeted, overlapping, Markdown-aware** chunks
//! that each carry a `heading_path` breadcrumb (the H1 › H2 › H3 stack the chunk
//! falls under). The shape, borrowed wholesale from [tobi/qmd](https://github.com/tobi/qmd):
//!
//! - accumulate content toward `cfg.target_tokens` (default 450 — under bge's
//!   512-token truncation, D1);
//! - at the target, scan **backward** over a `cfg.backscan_tokens` window and cut at
//!   the best-scoring structural break (`cfg.weights`: H1=100 … blank=20, list=5),
//!   weighted by a **quadratic distance decay** so the boundary is the cleanest
//!   *near* the target, not an arbitrary slice;
//! - carry `cfg.overlap_frac` of the tail forward, so `char_start..char_end` ranges
//!   **overlap** (they no longer partition the body — D4);
//! - never cut *inside* a fenced code block or a Markdown table — a forced cut with no
//!   clean break in the window is pushed past the block's end so it stays whole (#41),
//!   trading a slightly oversized chunk for a coherent one (`protected_regions`);
//! - track a running heading stack and stamp each chunk's `heading_path` (D3).
//!
//! The core is **model-free** (root `CLAUDE.md`): there is no tokenizer here (it
//! lives in `b2-embed`, behind the seam, and this runs in the model-free projection
//! pass). Chunks are sized by a cheap deterministic proxy — `chars / cfg.chars_per_token`
//! (D2) — and the embedder's own 512-token truncation is the hard backstop, so a
//! proxy under-estimate merely clips the tail of one unusually dense chunk (a table,
//! code) rather than corrupting the index. `chunk_body` is a **pure function** of
//! `(body, ChunkConfig)`: same input ⇒ same chunks ⇒ a reproducible index. Swapping
//! chunkers is a pure re-projection — drop & rebuild — with no schema or invariant
//! change (`token_count`/`heading_path` already exist on the `chunks` table).
//!
//! `char_start..char_end` always addresses the exact body slice that produced `text`
//! (anchoring for explain/highlight), **except** when `cfg.prepend_heading_path` is
//! on: that eval knob (D3, default off) prepends the breadcrumb into the embedded
//! `text`, so `text` then carries a synthetic prefix the range does not cover.

/// The tuning surface for [`chunk_body`] (spec §3, D5). Every lever that shapes a
/// cut lives here; `Default` reproduces the shipped values, so adapters pass
/// `&ChunkConfig::default()` and the Step-3 eval sweeps parameters in one process
/// (a loop over configs) rather than one recompile per cell. Kept a plain params
/// struct — no async/generics/traits — so it stays pure, deterministic, model-free.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkConfig {
    /// Target chunk size in *estimated* tokens (D1). Default 450 — headroom under
    /// bge's 512-token truncation for the D2 proxy's error and any D3 breadcrumb.
    pub target_tokens: usize,
    /// Fraction of a chunk re-shared with the next one (D4). Default 0.15.
    pub overlap_frac: f32,
    /// The D2 model-free token proxy: `tokens ≈ chars / chars_per_token`. Default
    /// 4.0 (English ≈ 4 chars/token; code and tables run denser — a lever, not a law).
    pub chars_per_token: f32,
    /// How far back (in estimated tokens) the boundary search looks from the target.
    /// Default 200.
    pub backscan_tokens: usize,
    /// The Markdown break-point scorer (qmd's H1=100 … list=5).
    pub weights: BreakWeights,
    /// Prepend `heading_path` into the *embedded* `text` (contextual chunk headers,
    /// D3). An eval-gated retrieval knob; **default off**. Storing `heading_path` is
    /// unconditional — this only controls whether it also seeds the vector.
    pub prepend_heading_path: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            target_tokens: 450,
            overlap_frac: 0.15,
            chars_per_token: 4.0,
            backscan_tokens: 200,
            weights: BreakWeights::default(),
            prepend_heading_path: false,
        }
    }
}

/// Markdown break-point weights (qmd's boundary scorer, spec §2). Higher = a cleaner
/// place to end a chunk. `heading[i]` is the weight of an H{i+1}; `word` is the
/// lowest-value fallback that lets a giant single-line paragraph still split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakWeights {
    /// H1..H6 (index 0 = H1). qmd: H1=100, H2=90, then a gentle gradient.
    pub heading: [u32; 6],
    /// A fenced-code delimiter line (```` ``` ```` / `~~~`).
    pub code_fence: u32,
    /// A blank line (a paragraph gap).
    pub blank_line: u32,
    /// A list item (`- `, `* `, `+ `, `1. `).
    pub list_item: u32,
    /// A plain paragraph/text line start.
    pub paragraph: u32,
    /// A word boundary within a line — the finest fallback.
    pub word: u32,
}

impl Default for BreakWeights {
    fn default() -> Self {
        Self {
            heading: [100, 90, 80, 70, 60, 50],
            code_fence: 80,
            blank_line: 20,
            list_item: 5,
            paragraph: 3,
            word: 1,
        }
    }
}

/// One projected chunk of a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub seq: usize,
    pub char_start: usize,
    pub char_end: usize,
    /// The D2 token *estimate* (`chars / chars_per_token`) that sized this chunk —
    /// not an exact token count (it was a whitespace word count under the old splitter).
    pub token_count: usize,
    /// The H1 › H2 › H3 breadcrumb the chunk falls under (`"A > B"`), or `None`
    /// when the chunk starts before any heading.
    pub heading_path: Option<String>,
    pub text: String,
}

/// Chunk `body` into size-targeted, overlapping, Markdown-aware chunks (see the
/// module docs). Returns an empty vec for an empty/all-blank body.
pub fn chunk_body(body: &str, cfg: &ChunkConfig) -> Vec<Chunk> {
    let body_len = body.len();
    if body.trim().is_empty() {
        return Vec::new();
    }

    let (breaks, line_paths) = scan(body, &cfg.weights);

    // Token params → char thresholds once (offsets are bytes; the proxy is uniform,
    // so a token position is just `offset / chars_per_token`).
    let cpt = (cfg.chars_per_token as f64).max(0.1);
    let target_chars = ((cfg.target_tokens as f64) * cpt).round().max(1.0) as usize;
    let backscan_chars = ((cfg.backscan_tokens as f64) * cpt).round() as usize;
    let overlap_chars =
        ((cfg.target_tokens as f64) * (cfg.overlap_frac as f64) * cpt).round() as usize;

    // #41: byte ranges (balanced code fences, GFM tables) a boundary must not cut
    // *inside* — a forced cut with no clean break in the window is exactly where a
    // bisected block would otherwise happen (this dense content is what the D2 proxy
    // mis-sizes, spec §5).
    let regions = protected_regions(body);

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut start = 0usize;

    loop {
        // The remainder fits in one chunk → emit it and stop (never split off a
        // final sliver).
        if body_len - start <= target_chars {
            push_chunk(&mut chunks, body, &line_paths, start, body_len, cfg);
            break;
        }
        let anchor = first_break_at_or_after(&breaks, start + target_chars).unwrap_or(body_len);
        let end = choose_end(
            &breaks,
            start,
            anchor,
            backscan_chars,
            cfg.backscan_tokens,
            cpt,
        );
        // If that cut lands inside a fence/table, push it past the block's end so the
        // whole block stays in one chunk. An oversized chunk (the embedder truncates
        // at 512, D2/§5) is fine; a half-fence / header-less table is not.
        let end = snap_past_region(end, &regions);
        push_chunk(&mut chunks, body, &line_paths, start, end, cfg);
        if end >= body_len {
            break;
        }
        // The next chunk's overlap tail must not start inside a block either — skip
        // such a start to just past the block (no overlap into it, rather than a chunk
        // that opens mid-fence).
        let overlap_start = choose_overlap_start(&breaks, start, end, overlap_chars);
        start = snap_past_region(overlap_start, &regions);
    }

    // Re-number after the fact: an all-whitespace span is skipped by `push_chunk`,
    // so `seq` is assigned here to stay contiguous and gap-free.
    for (i, c) in chunks.iter_mut().enumerate() {
        c.seq = i;
    }
    chunks
}

/// A candidate chunk boundary at a byte `offset`, with the structural `score` of the
/// line/word that starts there (qmd's scorer — higher = a cleaner cut).
struct Break {
    offset: usize,
    score: u32,
}

/// Single pass over the body: the ordered break candidates (line starts scored
/// structurally, plus word starts as the fallback) and, per line start, the heading
/// breadcrumb in effect there. Both vecs come out sorted by offset.
fn scan(body: &str, w: &BreakWeights) -> (Vec<Break>, Vec<(usize, Option<String>)>) {
    let mut breaks = Vec::new();
    let mut line_paths = Vec::new();
    let mut stack: Vec<(u8, String)> = Vec::new();
    let mut in_fence = false;
    let mut offset = 0usize;

    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = content.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");

        // Classify the line for its break score, and detect a heading (never inside
        // a fence — a `# comment` in code must not corrupt the breadcrumb).
        let (score, heading) = if content.trim().is_empty() {
            (w.blank_line, None)
        } else if is_fence {
            (w.code_fence, None)
        } else if in_fence {
            (w.paragraph, None)
        } else if let Some(level) = heading_level(trimmed) {
            (
                w.heading[(level as usize - 1).min(5)],
                Some((level, heading_text(trimmed, level))),
            )
        } else if is_list_item(trimmed) {
            (w.list_item, None)
        } else {
            (w.paragraph, None)
        };

        // Update the stack AFTER classifying, so a chunk starting on `## X` records a
        // path that includes X (the section it opens).
        if let Some((level, text)) = heading {
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            stack.push((level, text));
        }
        if is_fence {
            in_fence = !in_fence;
        }

        breaks.push(Break {
            offset: line_start,
            score,
        });
        line_paths.push((line_start, join_path(&stack)));
        push_word_breaks(&mut breaks, content, line_start, w.word);
    }

    (breaks, line_paths)
}

/// The ATX heading level (1..=6) if `trimmed` is a heading, else `None`. The `#` run
/// must be followed by a space or end-of-line, so `#tag` is not a heading.
fn heading_level(trimmed: &str) -> Option<u8> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) {
        let rest = &trimmed[hashes..];
        if rest.is_empty() || rest.starts_with(' ') {
            return Some(hashes as u8);
        }
    }
    None
}

/// The heading's display text: the `#`s and any ATX closing `#`s stripped, trimmed.
fn heading_text(trimmed: &str, level: u8) -> String {
    trimmed[level as usize..]
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_string()
}

/// Whether `trimmed` opens a Markdown list item (`- `/`* `/`+ ` or `1. `/`1) `).
fn is_list_item(trimmed: &str) -> bool {
    if let Some(rest) = trimmed.strip_prefix(['-', '*', '+']) {
        return rest.starts_with(' ');
    }
    let digits = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        return rest.starts_with(". ") || rest.starts_with(") ");
    }
    false
}

/// Append a low-weight break at each word start within a line (skipping the line
/// start, already a structural break), so a giant single-line paragraph can split.
fn push_word_breaks(breaks: &mut Vec<Break>, content: &str, line_start: usize, word: u32) {
    let mut prev_ws = true;
    for (i, c) in content.char_indices() {
        let ws = c.is_whitespace();
        if prev_ws && !ws && i != 0 {
            breaks.push(Break {
                offset: line_start + i,
                score: word,
            });
        }
        prev_ws = ws;
    }
}

/// The heading breadcrumb (`"A > B"`) for a stack, or `None` when empty.
fn join_path(stack: &[(u8, String)]) -> Option<String> {
    if stack.is_empty() {
        None
    } else {
        Some(
            stack
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(" > "),
        )
    }
}

/// The offset of the first break at or after `off`, or `None` if none exists.
fn first_break_at_or_after(breaks: &[Break], off: usize) -> Option<usize> {
    let idx = breaks.partition_point(|b| b.offset < off);
    breaks.get(idx).map(|b| b.offset)
}

/// Pick the chunk end: the break in `(start, anchor]` within the backscan window
/// maximizing `score · decay²`, where `decay = 1 - dist/backscan` falls off
/// quadratically with token distance from the target. Falls back to `anchor` when
/// the window holds no usable break.
fn choose_end(
    breaks: &[Break],
    start: usize,
    anchor: usize,
    backscan_chars: usize,
    backscan_tokens: usize,
    cpt: f64,
) -> usize {
    let win_start = anchor.saturating_sub(backscan_chars).max(start + 1);
    let lo = breaks.partition_point(|b| b.offset < win_start);
    let mut best: Option<(f64, usize)> = None;
    for b in &breaks[lo..] {
        if b.offset > anchor {
            break;
        }
        if b.offset <= start {
            continue;
        }
        let dist_tokens = (anchor - b.offset) as f64 / cpt;
        let frac = if backscan_tokens == 0 {
            0.0
        } else {
            dist_tokens / backscan_tokens as f64
        };
        let decay = (1.0 - frac).max(0.0);
        let score = b.score as f64 * decay * decay;
        // `>=` so that on a tie the larger offset (nearer the target, less content
        // dropped) wins, since offsets are walked ascending.
        if best.map(|(bs, _)| score >= bs).unwrap_or(true) {
            best = Some((score, b.offset));
        }
    }
    best.map(|(_, o)| o).unwrap_or(anchor)
}

/// The next chunk's start: the break inside `(start, end)` nearest to
/// `end - overlap`, preferring a cleaner (higher-scoring) boundary on a tie. Always
/// `> start` (so the walk makes progress); falls back to `end` (no overlap) when the
/// chunk has no interior break.
fn choose_overlap_start(breaks: &[Break], start: usize, end: usize, overlap_chars: usize) -> usize {
    if overlap_chars == 0 {
        return end;
    }
    let desired = end.saturating_sub(overlap_chars);
    let lo = breaks.partition_point(|b| b.offset <= start);
    let hi = breaks.partition_point(|b| b.offset < end);
    let mut best: Option<((usize, u32), usize)> = None;
    for b in &breaks[lo..hi] {
        // Order by (distance to desired, then higher score).
        let key = (b.offset.abs_diff(desired), u32::MAX - b.score);
        if best.map(|(bk, _)| key < bk).unwrap_or(true) {
            best = Some((key, b.offset));
        }
    }
    best.map(|(_, o)| o).unwrap_or(end)
}

/// If `off` falls **strictly inside** one of the protected `regions` (a balanced
/// fenced code block or a GFM table, from [`protected_regions`]), return that region's
/// end so the boundary clears the block; otherwise return `off` unchanged. A cut
/// exactly at a region edge is already clean and left as-is. `regions` is sorted by
/// start and non-overlapping, so the first region reaching past `off` decides it.
fn snap_past_region(off: usize, regions: &[(usize, usize)]) -> usize {
    for &(rs, re) in regions {
        if rs >= off {
            break;
        }
        if off < re {
            return re;
        }
    }
    off
}

/// Byte ranges `[start, end)` a chunk boundary must not fall **strictly inside**
/// (issue #41): a balanced fenced code block and a GFM table. Cutting at either edge
/// (before the opening fence / table header, or after the closing fence / final row)
/// is clean; cutting in the interior would bisect the block — embedding a code chunk
/// with an unbalanced ```` ``` ```` fence, or orphaning a table's header from its rows.
///
/// Fence tracking mirrors [`scan`]'s (any ```` ``` ````/`~~~` line toggles the state);
/// an **unterminated** fence is left unprotected (treated as prose) rather than
/// swallowing the whole tail into one chunk. A table is a header row directly above a
/// delimiter row (`| --- | :-: |`) plus the contiguous rows beneath it. Regions come
/// out sorted by start and never overlap (table scanning is suppressed inside a fence).
fn protected_regions(body: &str) -> Vec<(usize, usize)> {
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut in_fence = false;
    let mut fence_start = 0usize;
    // Table accumulation across consecutive rows.
    let mut prev_row_start: Option<usize> = None; // last row candidate — a header-in-waiting
    let mut table_start: Option<usize> = None; // set once a delimiter row confirms a table
    let mut table_end = 0usize; // end offset (past the newline) of the table's last row

    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");

        if is_fence {
            // A fence edge ends any table in progress, then toggles the fence.
            if let Some(ts) = table_start.take() {
                regions.push((ts, table_end));
            }
            prev_row_start = None;
            if in_fence {
                regions.push((fence_start, offset));
            } else {
                fence_start = line_start;
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        if is_table_row(trimmed) {
            if table_start.is_some() {
                table_end = offset; // extend the active table by this row
            } else if is_table_delim(trimmed) {
                if let Some(header_start) = prev_row_start {
                    table_start = Some(header_start);
                    table_end = offset;
                }
            }
            if table_start.is_none() {
                prev_row_start = Some(line_start);
            }
        } else {
            if let Some(ts) = table_start.take() {
                regions.push((ts, table_end));
            }
            prev_row_start = None;
        }
    }
    // A table running to EOF (an unterminated fence is intentionally left unprotected).
    if let Some(ts) = table_start.take() {
        regions.push((ts, table_end));
    }

    regions.sort_by_key(|&(s, _)| s);
    regions
}

/// Whether `trimmed` (already left-trimmed) could be a Markdown table row: non-empty
/// and carrying a `|` column separator. Both header and body rows qualify.
fn is_table_row(trimmed: &str) -> bool {
    !trimmed.is_empty() && trimmed.contains('|')
}

/// Whether `trimmed` is a GFM table **delimiter** row — the `| --- | :-: |` line under
/// the header. Requires a `|` (so a setext `---` underline or a `---` rule is *not* one)
/// and at least one `-`, with every character drawn from `-:| ` and nothing else.
fn is_table_delim(trimmed: &str) -> bool {
    trimmed.contains('|')
        && trimmed.contains('-')
        && trimmed.chars().all(|c| matches!(c, '-' | ':' | '|' | ' '))
}

/// Emit `body[start..end]`, trimmed to its non-blank span (so `char_start..char_end`
/// stays exact and the text is clean); skip an all-whitespace span. `seq` is a
/// placeholder — [`chunk_body`] re-numbers at the end.
fn push_chunk(
    chunks: &mut Vec<Chunk>,
    body: &str,
    line_paths: &[(usize, Option<String>)],
    start: usize,
    end: usize,
    cfg: &ChunkConfig,
) {
    let raw = &body[start..end];
    let cs = start + (raw.len() - raw.trim_start().len());
    let ce = cs + body[cs..end].trim_end().len();
    if ce <= cs {
        return;
    }
    let slice = &body[cs..ce];
    let heading_path = heading_path_at(line_paths, cs);
    let token_count =
        ((slice.chars().count() as f64) / (cfg.chars_per_token as f64).max(0.1)).round() as usize;
    let text = match (cfg.prepend_heading_path, &heading_path) {
        (true, Some(hp)) => format!("{hp}\n\n{slice}"),
        _ => slice.to_string(),
    };
    chunks.push(Chunk {
        seq: chunks.len(),
        char_start: cs,
        char_end: ce,
        token_count,
        heading_path,
        text,
    });
}

/// The heading breadcrumb in effect at byte offset `cs` — the path of the last line
/// starting at or before it.
fn heading_path_at(line_paths: &[(usize, Option<String>)], cs: usize) -> Option<String> {
    let idx = line_paths.partition_point(|(ls, _)| *ls <= cs);
    idx.checked_sub(1).and_then(|i| line_paths[i].1.clone())
}
