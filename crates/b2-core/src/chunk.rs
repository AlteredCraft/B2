//! Body chunker, the [tobi/qmd](https://github.com/tobi/qmd) heuristic adapted
//! (index-engine.md §1, GH #19): size-targeted, overlapping, Markdown-aware chunks, each
//! with a `heading_path` breadcrumb. At the target size it scans back for the best
//! structural break under a quadratic distance decay, and never cuts inside a code fence
//! or table (#41).
//!
//! Model-free (ADR-0005): sized by a `chars / chars_per_token` proxy, with the embedder's
//! 512-token truncation as the backstop. A pure function of `(body, ChunkConfig)`.
//! `char_start..char_end` addresses the source slice, except that
//! `cfg.prepend_heading_path` adds text outside it.

/// The tuning surface for [`chunk_body`] (index-engine.md §1). `Default` is the shipped
/// config; `make eval-sweep` varies it in one process.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkConfig {
    /// Target chunk size in estimated tokens. 450 leaves headroom under bge's 512 for the
    /// proxy's error and a prepended breadcrumb.
    pub target_tokens: usize,
    /// Fraction of a chunk re-shared with the next one. Default 0.15.
    pub overlap_frac: f32,
    /// The token proxy: `tokens ≈ chars / chars_per_token`. Default 4.0 (English; code
    /// and tables run denser).
    pub chars_per_token: f32,
    /// How far back (in estimated tokens) the boundary search looks from the target.
    /// Default 200.
    pub backscan_tokens: usize,
    /// The Markdown break-point scorer (qmd's H1=100 … list=5).
    pub weights: BreakWeights,
    /// Prepend `heading_path` into the embedded `text`. An eval knob, measured
    /// rank-neutral; default off. `heading_path` is stored either way.
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

/// Markdown break-point weights (index-engine.md §1). Higher = a cleaner place to cut.
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
    /// A word boundary within a line, so a giant one-line paragraph can still split.
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
    /// The token estimate (`chars / chars_per_token`), not an exact count.
    pub token_count: usize,
    /// The heading breadcrumb (`"A > B"`), or `None` before any heading.
    pub heading_path: Option<String>,
    pub text: String,
}

/// Chunk `body` (see the module docs). Empty for an all-blank body.
pub fn chunk_body(body: &str, cfg: &ChunkConfig) -> Vec<Chunk> {
    let body_len = body.len();
    if body.trim().is_empty() {
        return Vec::new();
    }

    let (breaks, line_paths) = scan(body, &cfg.weights);

    // Token params → byte thresholds once.
    let cpt = (cfg.chars_per_token as f64).max(0.1);
    let target_chars = ((cfg.target_tokens as f64) * cpt).round().max(1.0) as usize;
    let backscan_chars = ((cfg.backscan_tokens as f64) * cpt).round() as usize;
    let overlap_chars =
        ((cfg.target_tokens as f64) * (cfg.overlap_frac as f64) * cpt).round() as usize;

    // #41: fences and tables a boundary must not cut inside.
    let regions = protected_regions(body);

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut start = 0usize;

    loop {
        // The remainder fits: emit it rather than split off a sliver.
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
        // Push a cut inside a fence/table past its end: oversized beats a half-fence.
        let end = snap_past_region(end, &regions);
        push_chunk(&mut chunks, body, &line_paths, start, end, cfg);
        if end >= body_len {
            break;
        }
        // Nor may the next chunk start inside a block.
        let overlap_start = choose_overlap_start(&breaks, start, end, overlap_chars);
        start = snap_past_region(overlap_start, &regions);
    }

    // `push_chunk` skips blank spans, so number contiguously here.
    for (i, c) in chunks.iter_mut().enumerate() {
        c.seq = i;
    }
    chunks
}

/// A candidate chunk boundary at a byte `offset`, with its structural `score`.
struct Break {
    offset: usize,
    score: u32,
}

/// One pass over the body: the break candidates, and the breadcrumb at each line start.
/// Both sorted by offset.
fn scan(body: &str, w: &BreakWeights) -> (Vec<Break>, Vec<(usize, Option<String>)>) {
    let mut breaks = Vec::new();
    let mut line_paths = Vec::new();
    let mut stack: Vec<(u8, String)> = Vec::new();
    let mut in_fence = false;
    let mut offset = 0usize;

    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content = line_content(line);
        let trimmed = content.trim_start();
        let is_fence = is_fence_line(trimmed);

        // Never a heading inside a fence: a `# comment` in code isn't one.
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

        // After classifying, so a chunk starting on `## X` includes X in its path.
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

fn line_content(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

/// Whether a left-trimmed line is a code fence; shared by [`scan`] and [`protected_regions`].
fn is_fence_line(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// The ATX heading level (1..=6), if any. `#tag` is not a heading.
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

fn heading_text(trimmed: &str, level: u8) -> String {
    trimmed[level as usize..]
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_string()
}

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

/// A low-weight break at each word start after the line start.
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

fn first_break_at_or_after(breaks: &[Break], off: usize) -> Option<usize> {
    let idx = breaks.partition_point(|b| b.offset < off);
    breaks.get(idx).map(|b| b.offset)
}

/// The chunk end: the break in the backscan window maximizing `score · decay²`, with
/// `decay = 1 - dist/backscan`. Falls back to `anchor`.
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
        // `>=`: on a tie the later offset, nearer the target, wins.
        if best.map(|(bs, _)| score >= bs).unwrap_or(true) {
            best = Some((score, b.offset));
        }
    }
    best.map(|(_, o)| o).unwrap_or(anchor)
}

/// The next chunk's start: the interior break nearest `end - overlap`, cleaner on a tie.
/// Always `> start`, so the walk progresses; falls back to `end` (no overlap).
fn choose_overlap_start(breaks: &[Break], start: usize, end: usize, overlap_chars: usize) -> usize {
    if overlap_chars == 0 {
        return end;
    }
    let desired = end.saturating_sub(overlap_chars);
    let lo = breaks.partition_point(|b| b.offset <= start);
    let hi = breaks.partition_point(|b| b.offset < end);
    let mut best: Option<((usize, u32), usize)> = None;
    for b in &breaks[lo..hi] {
        let key = (b.offset.abs_diff(desired), u32::MAX - b.score);
        if best.map(|(bk, _)| key < bk).unwrap_or(true) {
            best = Some((key, b.offset));
        }
    }
    best.map(|(_, o)| o).unwrap_or(end)
}

/// `off`, or the end of the protected region it falls strictly inside. `regions` is sorted
/// and non-overlapping.
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

/// Byte ranges `[start, end)` a boundary must not fall strictly inside (#41): balanced code
/// fences and GFM tables. An unterminated fence is left unprotected rather than swallow the
/// tail. Sorted, non-overlapping.
fn protected_regions(body: &str) -> Vec<(usize, usize)> {
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut in_fence = false;
    let mut fence_start = 0usize;
    let mut prev_row_start: Option<usize> = None; // a header-in-waiting
    let mut table_start: Option<usize> = None; // set once a delimiter row confirms a table
    let mut table_end = 0usize;

    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line_content(line).trim_start();
        let is_fence = is_fence_line(trimmed);

        if is_fence {
            // A fence edge ends any table in progress.
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
                table_end = offset;
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
    // A table running to EOF.
    if let Some(ts) = table_start.take() {
        regions.push((ts, table_end));
    }

    regions.sort_by_key(|&(s, _)| s);
    regions
}

/// Whether `trimmed` could be a table row (header or body).
fn is_table_row(trimmed: &str) -> bool {
    !trimmed.is_empty() && trimmed.contains('|')
}

/// Whether `trimmed` is a GFM delimiter row (`| --- | :-: |`). The `|` requirement keeps a
/// `---` rule or setext underline out.
fn is_table_delim(trimmed: &str) -> bool {
    trimmed.contains('|')
        && trimmed.contains('-')
        && trimmed.chars().all(|c| matches!(c, '-' | ':' | '|' | ' '))
}

/// Emit `body[start..end]` trimmed to its non-blank span; skip a blank one. `seq` is set
/// later by [`chunk_body`].
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

/// The breadcrumb in effect at byte offset `cs`.
fn heading_path_at(line_paths: &[(usize, Option<String>)], cs: usize) -> Option<String> {
    let idx = line_paths.partition_point(|(ls, _)| *ls <= cs);
    idx.checked_sub(1).and_then(|i| line_paths[i].1.clone())
}
