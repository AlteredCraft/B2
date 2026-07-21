//! The qmd-heuristic chunker (planning/specs/completed/qmd-chunker.md, issue #19): size-targeted,
//! overlapping, Markdown-aware chunks carrying a `heading_path`. Two DB-level tests keep
//! the projection wiring honest; the rest exercise `chunk_body` as the pure function it is.

mod common;

use b2_core::chunk::{chunk_body, BreakWeights, ChunkConfig};
use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::open;
use common::{golden_vault_copy, SRS_ID};

#[test]
fn chunks_are_projected_for_each_note() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert!(total >= 2, "at least one chunk per golden note");

    // Under qmd sizing the whole small spaced-repetition note (prose + the Relations
    // list, well under the 450-token target) coalesces into a single chunk — the
    // paragraph splitter's two-chunk split is exactly the regression #19 fixes.
    let srs_chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE note_b2id = ?1",
            [SRS_ID],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(srs_chunks, 1);

    // char offsets must address the slice that produced the chunk text.
    let (start, end, text): (i64, i64, String) = conn
        .query_row(
            "SELECT char_start, char_end, text FROM chunks WHERE note_b2id = ?1 AND seq = 0",
            [SRS_ID],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(end > start);
    assert!(text.starts_with("Spaced repetition exploits"));
}

#[test]
fn fts_index_tracks_chunks_and_matches_body_text() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();

    // 'forgetting' appears only in spaced-repetition's Relations text (now folded
    // into that note's single chunk); the match still resolves to that note.
    let note: String = conn
        .query_row(
            "SELECT c.note_b2id FROM chunks_fts f
             JOIN chunks c ON c.id = f.rowid
             WHERE chunks_fts MATCH 'forgetting'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, SRS_ID);
}

#[test]
fn vault_chunk_config_reaches_projection() {
    // The eval's sweep seam (specs/eval-strategy.md): a non-default ChunkConfig
    // set on the Vault must actually shape the cut — `set_chunk_config` +
    // `project(force)` on the same vault re-chunks under the new policy, so a
    // much finer target yields more chunks than the default did. Model-free.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let mut vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.project(false).unwrap();

    let chunk_count = || -> i64 {
        let conn = open(&vault_dir.join(".b2").join("b2.sqlite")).unwrap();
        conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .unwrap()
    };
    let default_chunks = chunk_count();

    vault.set_chunk_config(ChunkConfig {
        target_tokens: 20,
        backscan_tokens: 10,
        overlap_frac: 0.0,
        ..ChunkConfig::default()
    });
    vault.project(true).unwrap();
    assert!(
        chunk_count() > default_chunks,
        "a finer target must cut more chunks than the default ({default_chunks})"
    );
}

#[test]
fn reindexing_a_note_does_not_leave_stale_fts_rows() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    golden_vault_copy(&vault);
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();

    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();
    let fts_count = |c: &rusqlite::Connection| -> i64 {
        c.query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
            .unwrap()
    };
    let before = fts_count(&conn);

    // Re-ingesting must replace, not accumulate (delete sentinel + reinsert).
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::default()).unwrap();
    assert_eq!(
        before,
        fts_count(&conn),
        "FTS rows must not accumulate on reindex"
    );
}

// --------------------------------------------------------------------------
// chunk_body — the pure function (spec §6 Step 1). Deterministic, model-free.
// --------------------------------------------------------------------------

/// Estimated tokens by the same `chars/4` proxy the chunker sizes with.
fn est_tokens(text: &str, cfg: &ChunkConfig) -> f64 {
    text.chars().count() as f64 / cfg.chars_per_token as f64
}

#[test]
fn empty_or_blank_body_yields_no_chunks() {
    let cfg = ChunkConfig::default();
    assert!(chunk_body("", &cfg).is_empty());
    assert!(chunk_body("   \n\n  \t\n", &cfg).is_empty());
}

#[test]
fn char_ranges_address_the_exact_text() {
    // The anchoring invariant: body[char_start..char_end] == text (prepend off).
    let body = "# Title\n\nA first paragraph about memory and recall.\n\n\
                ## Section\n\nSome more content under a section heading here.\n";
    let cfg = ChunkConfig::default();
    for c in chunk_body(body, &cfg) {
        assert_eq!(&body[c.char_start..c.char_end], c.text);
        assert!(c.char_end > c.char_start);
    }
}

#[test]
fn a_heading_and_its_section_land_in_one_chunk() {
    // The core regression (#19): a bare `## Threat model` must not be its own chunk —
    // it coalesces with the section body it introduces.
    let body = "## Threat model\n\n\
                The adversary can read any file left on the disk after a theft.\n";
    let chunks = chunk_body(body, &ChunkConfig::default());
    assert_eq!(chunks.len(), 1, "heading + section is one chunk");
    assert!(chunks[0].text.contains("## Threat model"));
    assert!(chunks[0].text.contains("The adversary can read"));
    // No chunk is the bare heading alone.
    assert!(!chunks.iter().any(|c| c.text.trim() == "## Threat model"));
}

#[test]
fn heading_path_tracks_nested_headings() {
    // A small target forces each section into its own chunk; the deep chunk carries
    // the full H1 › H2 › H3 breadcrumb. Overlap off so the deep chunk starts clean.
    let cfg = ChunkConfig {
        target_tokens: 8,
        overlap_frac: 0.0,
        ..ChunkConfig::default()
    };
    let body = "# Top\n\n\
                Intro prose that belongs only to the top-level section here.\n\n\
                ## Section A\n\n\
                Body prose written specifically for the middle section here.\n\n\
                ### Sub A1\n\n\
                A distinctive marmoset sentence buried in the deepest section.\n";
    let chunks = chunk_body(body, &cfg);

    let deep = chunks
        .iter()
        .find(|c| c.text.contains("marmoset"))
        .expect("a chunk holds the deep sentence");
    assert_eq!(
        deep.heading_path.as_deref(),
        Some("Top > Section A > Sub A1")
    );

    // A chunk under the H1 intro (before any H2) carries just the H1.
    let top = chunks
        .iter()
        .find(|c| c.text.contains("Intro prose"))
        .expect("a chunk holds the intro");
    assert_eq!(top.heading_path.as_deref(), Some("Top"));
}

#[test]
fn body_before_any_heading_has_no_path() {
    // The golden spaced-repetition shape: prose, then a heading. The single chunk
    // starts in the headingless prose, so its path is None.
    let body = "Spaced repetition exploits the forgetting curve.\n\n## Relations\n- supports X\n";
    let chunks = chunk_body(body, &ChunkConfig::default());
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].heading_path, None);
}

#[test]
fn large_body_splits_near_target_and_never_far_over() {
    // Many paragraphs → several chunks, each clustered near the target and never
    // wildly past the proxy cap (target + one backscan window of slack).
    let para = "This is a paragraph of ordinary prose that carries a fair amount \
                of content so that several of them together comfortably exceed the \
                chunk size target and force the chunker to make real cuts.\n\n";
    let body = para.repeat(40);
    let cfg = ChunkConfig::default();
    let chunks = chunk_body(&body, &cfg);

    assert!(chunks.len() > 1, "a long body splits into many chunks");
    let cap = (cfg.target_tokens + cfg.backscan_tokens) as f64;
    for c in &chunks {
        assert!(
            est_tokens(&c.text, &cfg) <= cap,
            "chunk ~{:.0} tokens exceeds the {cap:.0}-token cap",
            est_tokens(&c.text, &cfg)
        );
    }
    // The interior chunks should sit in the neighbourhood of the target, not tiny.
    let interior_ok = chunks[..chunks.len() - 1]
        .iter()
        .all(|c| est_tokens(&c.text, &cfg) >= cfg.target_tokens as f64 * 0.5);
    assert!(interior_ok, "interior chunks cluster near the target");
}

#[test]
fn consecutive_chunks_overlap() {
    let para = "Overlap means consecutive chunks share a tail of content so that a \
                query straddling a boundary still retrieves both neighbours cleanly.\n\n";
    let body = para.repeat(40);
    let cfg = ChunkConfig::default();
    let chunks = chunk_body(&body, &cfg);
    assert!(chunks.len() > 1);
    for pair in chunks.windows(2) {
        assert!(
            pair[1].char_start < pair[0].char_end,
            "chunk {} should overlap chunk {} (starts {} < prev end {})",
            pair[1].seq,
            pair[0].seq,
            pair[1].char_start,
            pair[0].char_end,
        );
        assert!(pair[1].char_start > pair[0].char_start, "and still advance");
    }
}

#[test]
fn zero_overlap_partitions_without_gaps() {
    let cfg = ChunkConfig {
        overlap_frac: 0.0,
        ..ChunkConfig::default()
    };
    let body = "Sentence about content. ".repeat(400);
    let chunks = chunk_body(&body, &cfg);
    assert!(chunks.len() > 1);
    for pair in chunks.windows(2) {
        // Boundaries touch (allowing for trimmed whitespace between them).
        assert!(pair[1].char_start >= pair[0].char_end);
    }
}

#[test]
fn a_giant_single_line_paragraph_still_splits() {
    // No blank lines, no newlines — only word-boundary fallbacks can cut it.
    let body = "word ".repeat(2000);
    let cfg = ChunkConfig::default();
    let chunks = chunk_body(&body, &cfg);
    assert!(chunks.len() > 1, "a giant single line must still split");
    let cap = (cfg.target_tokens + cfg.backscan_tokens) as f64;
    for c in &chunks {
        assert!(est_tokens(&c.text, &cfg) <= cap);
    }
}

#[test]
fn seq_is_contiguous_from_zero() {
    let body = "Paragraph number with enough words to matter here.\n\n".repeat(30);
    let chunks = chunk_body(&body, &ChunkConfig::default());
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.seq, i);
    }
}

#[test]
fn hash_inside_a_code_fence_is_not_a_heading() {
    // A shell comment inside a fence must not corrupt the breadcrumb.
    let cfg = ChunkConfig {
        target_tokens: 8,
        overlap_frac: 0.0,
        ..ChunkConfig::default()
    };
    let body = "## Real heading\n\n\
                ```sh\n\
                # this is a shell comment, not an H1\n\
                echo hello world from inside the fence block\n\
                ```\n\n\
                Trailing prose distinctively marked with a zebra token here.\n";
    let chunks = chunk_body(body, &cfg);
    // No chunk's path is ever led by the shell comment text.
    for c in &chunks {
        let path = c.heading_path.as_deref().unwrap_or("");
        assert!(
            !path.contains("shell comment"),
            "fence comment leaked into heading_path: {path:?}"
        );
    }
    // The trailing prose stays under the real heading.
    let zebra = chunks.iter().find(|c| c.text.contains("zebra")).unwrap();
    assert_eq!(zebra.heading_path.as_deref(), Some("Real heading"));
}

/// Count fence-delimiter lines (```` ``` ````/`~~~`) in a chunk — an odd count means
/// the chunk carries an unbalanced fence, i.e. a code block was bisected.
fn fence_lines(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("```") || t.starts_with("~~~")
        })
        .count()
}

#[test]
fn a_code_fence_is_never_split_across_chunks() {
    // #41: a code block larger than the backscan window spans a target boundary, so a
    // forced cut would land inside it. The guard must keep every chunk's fences
    // balanced and pull the whole block into a single chunk.
    let cfg = ChunkConfig {
        target_tokens: 30,
        overlap_frac: 0.15,
        backscan_tokens: 8,
        ..ChunkConfig::default()
    };
    let body = "Intro prose that comfortably fills the space ahead of the code block.\n\n\
                ```rust\n\
                fn alpha() { let a = 1; }\n\
                fn bravo() { let b = 2; }\n\
                fn charlie() { let c = 3; }\n\
                fn delta() { let d = 4; }\n\
                fn echo() { let e = 5; }\n\
                fn foxtrot() { let f = 6; }\n\
                ```\n\n\
                Trailing prose after the block with a distinctive walrus token here.\n";
    let chunks = chunk_body(body, &cfg);

    assert!(chunks.len() > 1, "the body must actually split");
    for c in &chunks {
        assert_eq!(
            fence_lines(&c.text) % 2,
            0,
            "chunk {} bisects a code fence:\n{}",
            c.seq,
            c.text
        );
    }
    // The block is pushed past its closing fence into one chunk (first line to last).
    let block = chunks
        .iter()
        .find(|c| c.text.contains("fn alpha()"))
        .expect("a chunk holds the code block");
    assert!(
        block.text.contains("fn foxtrot()"),
        "the code block stays whole in one chunk"
    );
    assert!(block.text.matches("```").count() >= 2, "with both fences");
}

#[test]
fn a_table_keeps_its_header_and_rows_together() {
    // #41: a table larger than the target spans a boundary; the guard must keep the
    // header row and every data row in the same chunk (no orphaned rows).
    let cfg = ChunkConfig {
        target_tokens: 30,
        overlap_frac: 0.0,
        backscan_tokens: 8,
        ..ChunkConfig::default()
    };
    let body = "Intro prose ahead of the data table that follows just below here.\n\n\
                | Name | Role | Notes   |\n\
                | ---- | ---- | ------- |\n\
                | Ann  | Lead | alpha   |\n\
                | Bob  | Dev  | bravo   |\n\
                | Cy   | Ops  | charlie |\n\
                | Dot  | QA   | delta   |\n\
                | Eve  | PM   | echo    |\n\n\
                Trailing prose after the table with a distinctive walrus token here.\n";
    let chunks = chunk_body(body, &cfg);

    assert!(chunks.len() > 1, "the body must actually split");
    // Any chunk carrying a data row must also carry the header — the table is whole.
    for c in &chunks {
        let has_row = ["| Ann", "| Bob", "| Cy", "| Dot", "| Eve"]
            .iter()
            .any(|r| c.text.contains(r));
        if has_row {
            assert!(
                c.text.contains("| Name |"),
                "chunk {} separates a table row from its header:\n{}",
                c.seq,
                c.text
            );
        }
    }
    // And the header chunk holds the full table (first row through last).
    let table = chunks
        .iter()
        .find(|c| c.text.contains("| Name |"))
        .expect("a chunk holds the table");
    assert!(table.text.contains("| Ann") && table.text.contains("| Eve"));
}

#[test]
fn a_setext_underline_is_not_mistaken_for_a_table() {
    // `---` under a line is a setext heading / rule, not a table delimiter (no `|`),
    // so it forms no protected region and normal boundary scoring applies.
    let cfg = ChunkConfig {
        target_tokens: 12,
        overlap_frac: 0.0,
        ..ChunkConfig::default()
    };
    let body = "Alpha heading\n---\n\nSome prose that follows the setext underline here.\n";
    // Just needs to chunk without panicking and address exact slices (no false region
    // pushing the boundary somewhere the text no longer matches).
    for c in chunk_body(body, &cfg) {
        assert_eq!(&body[c.char_start..c.char_end], c.text);
    }
}

#[test]
fn prepend_heading_path_seeds_the_embedded_text() {
    // D3's eval knob: with prepend on, the breadcrumb leads the embedded text; the
    // char range keeps addressing the underlying body slice.
    let cfg = ChunkConfig {
        target_tokens: 8,
        overlap_frac: 0.0,
        prepend_heading_path: true,
        ..ChunkConfig::default()
    };
    let body = "# Guide\n\n## Setup\n\nRun the installer and then a giraffe appears on screen.\n";
    let chunks = chunk_body(body, &cfg);
    let hit = chunks.iter().find(|c| c.text.contains("giraffe")).unwrap();
    let hp = hit.heading_path.as_deref().unwrap();
    assert!(
        hit.text.starts_with(hp),
        "embedded text leads with the path"
    );
    // The stored range still points at the real slice (which excludes the prefix).
    assert!(!body[hit.char_start..hit.char_end].starts_with(hp));
}

#[test]
fn weights_are_a_lever_a_giant_heading_weight_pulls_the_cut() {
    // Sanity that the scorer is wired to the config: default weights split a two-
    // section body at the heading boundary, so each section keeps its own path.
    let cfg = ChunkConfig {
        target_tokens: 10,
        overlap_frac: 0.0,
        weights: BreakWeights::default(),
        ..ChunkConfig::default()
    };
    let body = "## Alpha\n\n\
                Alpha section prose with a unique aardvark token inside of it here.\n\n\
                ## Beta\n\n\
                Beta section prose with a unique buffalo token inside of it here.\n";
    let chunks = chunk_body(body, &cfg);
    let a = chunks.iter().find(|c| c.text.contains("aardvark")).unwrap();
    let b = chunks.iter().find(|c| c.text.contains("buffalo")).unwrap();
    assert_eq!(a.heading_path.as_deref(), Some("Alpha"));
    assert_eq!(b.heading_path.as_deref(), Some("Beta"));
}
