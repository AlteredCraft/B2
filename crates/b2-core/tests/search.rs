//! Hybrid retrieval (flow ②, index-engine.md §4): BM25 and vector fused by RRF (k=60),
//! resolved to notes. Under the fake embedder these prove plumbing, not model quality.

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::ingest::ingest_vault;
use b2_core::search::{self, RRF_K};
use b2_core::{open, search::Hit};
use common::{
    count, golden_vault_copy, index_conn, ingest_golden, opened_vault, reindexed_vault,
    MEMORY_PATH, SRS_PATH,
};
use std::fs;

fn note_set(hits: &[Hit]) -> std::collections::BTreeSet<String> {
    hits.iter().map(|h| h.note_path.clone()).collect()
}

#[test]
fn rrf_uses_k_60() {
    assert_eq!(RRF_K, 60);
}

/// `vault::{note,chunk}_candidate_pool` state the composed depth a search reaches (view
/// headroom times `pool_size`), so a measurement need not re-derive it (GH #141).
#[test]
fn candidate_pool_states_the_per_signal_depth_a_search_reaches() {
    // 3× dedup headroom, then 5× per signal.
    assert_eq!(b2_core::vault::note_candidate_pool(10), 150);
    // The floor binds at tiny limits.
    assert_eq!(b2_core::vault::note_candidate_pool(1), 30);
    // Monotone in `limit`, which the eval's blindness warning relies on.
    assert!(b2_core::vault::note_candidate_pool(30) > b2_core::vault::note_candidate_pool(10));
    assert!(b2_core::vault::chunk_candidate_pool(30) > b2_core::vault::chunk_candidate_pool(10));
}

/// GH #142: the passage view's headroom covers a torn read, so it is a constant; the note
/// view's covers dedup, so it is a multiple. This is a ranking commitment: a wider pool
/// changes RRF's results.
#[test]
fn the_passage_view_retrieves_a_narrower_pool_than_the_note_view() {
    assert_eq!(b2_core::vault::chunk_candidate_pool(10), 60);
    assert_eq!(b2_core::vault::note_candidate_pool(10), 150);

    // The gap widens with the ask and never flips.
    for limit in [1usize, 2, 5, 10, 50, 500] {
        assert!(
            b2_core::vault::chunk_candidate_pool(limit)
                <= b2_core::vault::note_candidate_pool(limit),
            "the passage view must never out-reach the note view (limit {limit})"
        );
    }
    assert!(
        b2_core::vault::note_candidate_pool(500) - b2_core::vault::chunk_candidate_pool(500)
            > b2_core::vault::note_candidate_pool(10) - b2_core::vault::chunk_candidate_pool(10)
    );
}

/// The blindness GH #141 names: a corpus no bigger than the pool is never truncated, so a
/// shallow ask is a prefix of a deep one. Hence the stability probe needs a bigger vault.
/// Scoped to width: `RRF_K` still reorders here.
#[test]
fn a_corpus_no_bigger_than_the_pool_ranks_the_same_at_any_depth() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, vault_dir) = reindexed_vault(tmp.path());

    // The premise is about the corpus, so count chunk rows, not results.
    let chunks = count(&index_conn(&vault_dir), "chunks");
    assert!(
        chunks <= b2_core::vault::chunk_candidate_pool(2) as i64,
        "the premise: the whole corpus ({chunks} chunks) fits inside even the narrowest pool"
    );

    let shallow = vault.search_chunks("memory", 2).unwrap();
    let deep = vault.search_chunks("memory", 10).unwrap();
    assert_eq!(shallow.len(), 2, "the fixture must have room to truncate");
    assert_eq!(
        shallow.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
        deep.iter()
            .take(shallow.len())
            .map(|h| h.path.clone())
            .collect::<Vec<_>>(),
        "a narrower pool must not reorder what a wider one already saw"
    );
}

/// `limit` is user input, so the pool arithmetic saturates rather than wrapping a large
/// ask into a tiny pool.
#[test]
fn an_absurd_limit_saturates_the_pool_instead_of_overflowing_it() {
    assert_eq!(b2_core::vault::note_candidate_pool(usize::MAX), usize::MAX);
    assert_eq!(b2_core::vault::chunk_candidate_pool(usize::MAX), usize::MAX);

    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, vault_dir) = reindexed_vault(tmp.path());

    let hits = vault.search("memory", usize::MAX).unwrap();
    assert!(!hits.is_empty());
    assert!(!vault
        .search_chunks("memory", usize::MAX)
        .unwrap()
        .is_empty());
    assert!(hits.len() as i64 <= count(&index_conn(&vault_dir), "notes"));
}

#[test]
fn rrf_ranks_a_doc_present_in_both_lists_above_single_list_winners() {
    // 20 is in both lists, so it beats 10 (BM25's top, but vector rank 2).
    let bm25 = vec![10, 20, 30];
    let vector = vec![20, 40, 10];
    let fused = search::rrf_fuse(&[bm25, vector], RRF_K);

    assert_eq!(fused[0].0, 20, "doc in both lists wins");
    assert_eq!(fused.len(), 4);
    for w in fused.windows(2) {
        assert!(w[0].1 >= w[1].1, "scores must be descending");
    }
}

/// RRF over integer ranks makes exact ties structural (GH #156). A tie breaks by rank in
/// the last list given to `rrf_fuse` (the dense list), which named the right answer on
/// the eval's tie; id is only the final determinism key.
#[test]
fn rrf_breaks_symmetric_ties_by_the_dense_lists_rank() {
    // 1 and 2 tie exactly; the dense list prefers 2.
    let bm25 = vec![1, 2];
    let vector = vec![2, 1];
    let fused = search::rrf_fuse(&[bm25, vector], RRF_K);
    assert_eq!(
        fused[0].1, fused[1].1,
        "the fixture must be a genuine exact tie"
    );
    assert_eq!(fused[0].0, 2, "the dense list's preference breaks the tie");
}

#[test]
fn rrf_breaks_cross_signal_ties_toward_the_dense_list() {
    // 7 only in BM25, 9 only in the vector list, both rank 0: an exact tie.
    let fused = search::rrf_fuse(&[vec![7], vec![9]], RRF_K);
    assert_eq!(
        fused[0].1, fused[1].1,
        "the fixture must be a genuine exact tie"
    );
    assert_eq!(fused[0].0, 9, "present-in-dense beats absent-from-dense");
}

#[test]
fn keyword_search_finds_chunks_by_term() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let ids = search::keyword_search(&conn, "forgetting", 10).unwrap();
    assert!(!ids.is_empty());
    let note: String = conn
        .query_row(
            "SELECT note_path FROM chunks WHERE id = ?1",
            [ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, SRS_PATH);
}

#[test]
fn keyword_search_tolerates_natural_language_punctuation() {
    // Punctuation is FTS5 syntax and would raise a parse error if passed raw.
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    for q in [
        "why can't I remember? the \"forgetting\" curve!",
        "forgetting...",
        "-- forgetting --",
    ] {
        let ids = search::keyword_search(&conn, q, 10).unwrap();
        assert!(!ids.is_empty(), "query {q:?} should still find the term");
    }

    // No usable terms is empty, not an error (the vector half still runs).
    assert!(search::keyword_search(&conn, "!!! ??? ...", 10)
        .unwrap()
        .is_empty());
}

#[test]
fn fts5_query_sanitizes_to_ored_literals() {
    assert_eq!(
        search::fts5_query("can't sleep"),
        "\"can\" OR \"t\" OR \"sleep\""
    );
    assert_eq!(search::fts5_query("  !!! "), "");
    assert_eq!(search::fts5_query("forgetting"), "\"forgetting\"");
}

#[test]
fn hybrid_search_combines_signals_and_resolves_to_notes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let hits = search::hybrid_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5)
        .unwrap()
        .hits;
    assert!(!hits.is_empty());
    // SRS is the only keyword match.
    assert!(hits.iter().all(|h| !h.note_path.is_empty()));
    assert!(note_set(&hits).contains(SRS_PATH));
}

/// The dense half alone (GH #158), the eval's ablation beside bm25-only and hybrid.
#[test]
fn vector_only_search_is_the_dense_half_alone() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let hits =
        search::vector_only_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| !h.note_path.is_empty()));
    // Negated-distance scores, best first (discovery's convention).
    for w in hits.windows(2) {
        assert!(w[0].score >= w[1].score, "scores must be descending");
    }
    assert!(hits.iter().all(|h| h.score <= 0.0));

    assert!(
        search::vector_only_search(&conn, &FakeEmbedder::new(64), "forgetting", 0)
            .unwrap()
            .is_empty()
    );
}

/// Dedups like `search`, but an unembedded vault returns nothing: an ablation that fell
/// back to keywords would measure the wrong signal.
#[test]
fn search_vector_only_dedups_and_refuses_to_impersonate_keywords() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = opened_vault(tmp.path());

    vault.project(false).unwrap();
    assert!(!vault.search("memory", 5).unwrap().is_empty());
    assert!(vault.search_vector_only("memory", 5).unwrap().is_empty());

    vault
        .embed(&mut |_| std::ops::ControlFlow::Continue(()))
        .unwrap();
    let hits = vault.search_vector_only("memory", 10).unwrap();
    assert!(!hits.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    for h in &hits {
        assert!(
            seen.insert(h.path.clone()),
            "note {} appeared twice",
            h.path
        );
        assert!(!h.path.is_empty());
    }
}

/// A snippet windows around the matched term: section-sized chunks (GH #19) are far
/// longer than the snippet budget.
#[test]
fn a_long_chunks_snippet_windows_around_the_matched_term() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    // ~470 characters of lead-in, past the snippet head.
    let lead = "Filler prose that exists only to push the matched term out of the head. ".repeat(7);
    fs::write(
        root.join("long.md"),
        format!(
            "---\ntype: note\n---\n\
             {lead}\nThe capybara paragraph is the one the query is looking for.\n"
        ),
    )
    .unwrap();
    let vault = b2_core::Vault::open(&root).unwrap();
    vault.reindex().unwrap();

    let hits = vault.search("capybara", 5).unwrap();
    let hit = hits
        .iter()
        .find(|h| h.path == "long.md")
        .expect("the keyword match must surface");
    assert!(
        hit.snippet.contains("capybara"),
        "the matched term must be inside the window: {:?}",
        hit.snippet
    );
    assert!(
        hit.snippet.starts_with('…'),
        "a windowed snippet opens with an ellipsis: {:?}",
        hit.snippet
    );
    // The 160-char budget plus two ellipses.
    assert!(hit.snippet.chars().count() <= 162, "{:?}", hit.snippet);

    // A term inside the head needs no window.
    let head_hit = vault
        .search("Filler", 5)
        .unwrap()
        .into_iter()
        .find(|h| h.path == "long.md")
        .expect("the head term must surface too");
    assert!(
        head_hit.snippet.starts_with("Filler prose"),
        "a match in the head keeps the head: {:?}",
        head_hit.snippet
    );
    assert!(
        head_hit.snippet.ends_with('…'),
        "…still truncated to budget"
    );
}

#[test]
fn search_chunks_exposes_passage_level_hits() {
    // The passage view the eval scores: no note dedup, and the chunk's full text
    // (containment-scorable, unlike a display snippet).
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = reindexed_vault(tmp.path());

    let hits = vault.search_chunks("forgetting curve", 10).unwrap();
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .all(|h| !h.path.is_empty() && !h.text.is_empty()));
    let srs = hits
        .iter()
        .find(|h| h.path == SRS_PATH)
        .expect("the one keyword-matching note must surface at chunk level");
    assert!(srs.path.ends_with("spaced-repetition.md"));
    assert!(srs.text.contains("forgetting"));
}

/// GH #137: a ranked chunk that no longer resolves (a torn read C1 allows during a
/// reindex) is skipped, not charged against `limit`. The fixture is an FTS row with no
/// `chunks` row, and the test asserts it ranks first rather than assuming so.
#[test]
fn a_ranked_chunk_that_no_longer_resolves_costs_no_hit_slot() {
    const DEAD_CHUNK: i64 = 999_999;

    // Several keyword matches, so a `limit` of 2 has something to backfill from.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    for n in [1, 2, 3, 4] {
        fs::write(
            vault.join(format!("n{n}.md")),
            format!(
                "---\ntype: note\ntitle: N{n}\n---\n\
                 A note about the capybara, and more capybara prose to rank on.\n"
            ),
        )
        .unwrap();
    }
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &FakeEmbedder::new(64)).unwrap();

    let healthy = search::keyword_only_search(&conn, "capybara", 2)
        .unwrap()
        .hits;
    assert_eq!(healthy.len(), 2, "the fixture must have room to under-fill");

    conn.execute(
        "INSERT INTO chunks_fts(rowid, text) VALUES (?1, 'capybara')",
        rusqlite::params![DEAD_CHUNK],
    )
    .unwrap();

    let ranked = search::keyword_search(&conn, "capybara", 10).unwrap();
    assert!(
        ranked.iter().take(2).any(|&id| id == DEAD_CHUNK),
        "the dead chunk must land inside the limit window for this to test anything"
    );

    // Compare identity: the dead chunk's rank shifts the live chunks' RRF scores.
    let after = search::keyword_only_search(&conn, "capybara", 2)
        .unwrap()
        .hits;
    assert_eq!(
        after.iter().map(|h| h.chunk_id).collect::<Vec<_>>(),
        healthy.iter().map(|h| h.chunk_id).collect::<Vec<_>>(),
        "the dead chunk is stepped over; the live hits below it still fill `limit`"
    );
}

/// The façade's chunk view drops a dead hit from its headroom, not from `limit` (GH #137).
#[test]
fn search_chunks_still_fills_limit_when_a_ranked_chunk_is_dead() {
    const DEAD_CHUNK: i64 = 999_999;

    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, vault_dir) = reindexed_vault(tmp.path());

    let healthy = vault.search_chunks("memory", 2).unwrap();
    assert_eq!(healthy.len(), 2);

    let conn = index_conn(&vault_dir);
    conn.execute(
        "INSERT INTO chunks_fts(rowid, text) VALUES (?1, 'memory')",
        rusqlite::params![DEAD_CHUNK],
    )
    .unwrap();

    let after = vault.search_chunks("memory", 2).unwrap();
    assert_eq!(after.len(), 2, "a dead top hit must not cost a result slot");
    assert_eq!(
        after.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
        healthy.iter().map(|h| h.path.clone()).collect::<Vec<_>>(),
    );
}

/// A zero budget must never be stepped past into "return everything".
#[test]
fn a_zero_limit_returns_no_hits() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    assert!(search::keyword_only_search(&conn, "memory", 0)
        .unwrap()
        .hits
        .is_empty());
    assert!(
        search::hybrid_search(&conn, &FakeEmbedder::new(64), "memory", 0)
            .unwrap()
            .hits
            .is_empty()
    );
}

/// A zero-limit search never reaches retrieval. Observed through the model-mismatch
/// guard: every real search on a mismatched vault fails, but a zero-limit one returns.
#[test]
fn a_zero_limit_search_does_no_retrieval_work() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    golden_vault_copy(&root);
    let vault = b2_core::Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(64))).unwrap();
    vault.reindex().unwrap();
    drop(vault);

    let swapped =
        b2_core::Vault::open_with_embedder(&root, Box::new(FakeEmbedder::new(128))).unwrap();
    assert!(
        matches!(
            swapped.search("forgetting", 5).unwrap_err(),
            b2_core::Error::ModelMismatch { .. }
        ),
        "the fixture must be a genuinely mismatched vault"
    );

    assert!(
        matches!(
            swapped.search_vector_only("forgetting", 5).unwrap_err(),
            b2_core::Error::ModelMismatch { .. }
        ),
        "the ablation view shares the model-identity guard"
    );

    assert!(swapped.search("forgetting", 0).unwrap().is_empty());
    assert!(swapped.search_chunks("forgetting", 0).unwrap().is_empty());
    assert!(swapped
        .search_vector_only("forgetting", 0)
        .unwrap()
        .is_empty());
}

// ---------------------------------------------------------------------------
// Query evidence — the absolute signals RRF discards (invariants.md D2, GH #201)
// ---------------------------------------------------------------------------

/// Document frequency over the sanitized `MATCH` terms; an unseen word reads `df == 0`.
#[test]
fn lexical_evidence_reads_document_frequency_per_term() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let ev = search::lexical_evidence(&conn, "memory shjfasd").unwrap();
    assert!(ev.chunk_total > 0, "the golden vault projects chunks");
    let df = |t: &str| ev.terms.iter().find(|e| e.term == t).map(|e| e.df);
    assert!(df("memory").is_some_and(|n| n > 0), "the vault holds it");
    assert_eq!(df("shjfasd"), Some(0), "the vault has never seen it");
}

/// Otherwise a query could raise its coverage by repeating a word.
#[test]
fn lexical_evidence_counts_a_repeated_term_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let ev = search::lexical_evidence(&conn, "memory memory memory").unwrap();
    assert_eq!(ev.terms.len(), 1);
}

/// Absent words weigh most, so pure nonsense reads as coverage zero, not as no reading.
#[test]
fn absent_words_weigh_most_and_drive_coverage_to_zero() {
    let ev = search::LexicalEvidence {
        chunk_total: 100,
        terms: vec![
            search::TermEvidence {
                term: "the".into(),
                df: 95,
            },
            search::TermEvidence {
                term: "parrots".into(),
                df: 0,
            },
            search::TermEvidence {
                term: "mimic".into(),
                df: 0,
            },
        ],
    };
    assert!(
        ev.idf(0) > ev.idf(95),
        "a word the vault lacks outweighs one it repeats"
    );
    // Only "the" is present, and it weighs almost nothing.
    assert!(ev.term_coverage().is_some_and(|c| c < 0.02));
    assert!(!ev.anchored(0.20));
}

/// The eval's off-topic negative ("why parrots mimic speech"): the vault shares only a
/// function word, which carries almost no weight. One of three rare words present does
/// anchor at the shipped bar; ADR-0015's tripwire is cutting a real query.
#[test]
fn a_shared_function_word_is_not_a_lexical_anchor() {
    let ev = search::LexicalEvidence {
        chunk_total: 70,
        terms: vec![
            search::TermEvidence {
                term: "why".into(),
                df: 21,
            },
            search::TermEvidence {
                term: "parrots".into(),
                df: 0,
            },
            search::TermEvidence {
                term: "mimic".into(),
                df: 0,
            },
            search::TermEvidence {
                term: "speech".into(),
                df: 0,
            },
        ],
    };
    assert!(ev.term_coverage().is_some_and(|c| c < 0.15));
    assert!(!ev.anchored(0.20));

    let held = search::LexicalEvidence {
        chunk_total: 70,
        terms: vec![
            search::TermEvidence {
                term: "throat".into(),
                df: 3,
            },
            search::TermEvidence {
                term: "singing".into(),
                df: 6,
            },
        ],
    };
    assert_eq!(held.term_coverage(), Some(1.0));
    assert!(held.anchored(0.20));
}

/// A single-domain vault, where a subject word is in most chunks, still anchors: common
/// is not a stopword (GH #201, GH #196).
#[test]
fn a_saturated_subject_word_still_anchors() {
    let ev = search::LexicalEvidence {
        chunk_total: 15,
        terms: vec![
            search::TermEvidence {
                term: "drone".into(),
                df: 3,
            },
            search::TermEvidence {
                term: "comb".into(),
                df: 7,
            },
        ],
    };
    assert_eq!(ev.term_coverage(), Some(1.0));
    assert!(ev.anchored(0.20));
}

/// An all-stopword query has no reading, not zero coverage: the cosine half decides alone.
#[test]
fn an_all_stopword_query_has_no_coverage_reading() {
    let ev = search::LexicalEvidence {
        chunk_total: 100,
        terms: vec![search::TermEvidence {
            term: "the".into(),
            df: 100,
        }],
    };
    assert_eq!(ev.term_coverage(), None);
    assert!(!ev.anchored(0.0), "abstention is never an anchor");
}

/// D2's verdict is lexical OR semantic: only their joint absence answers "no matches".
#[test]
fn the_verdict_takes_either_signal() {
    let bar = search::EvidenceBar {
        min_term_coverage: 0.20,
        min_cos: 0.55,
    };
    let lexical = |df: usize| search::LexicalEvidence {
        chunk_total: 100,
        terms: vec![search::TermEvidence {
            term: "photosynthesis".into(),
            df,
        }],
    };
    let anchored = search::QueryEvidence {
        lexical: lexical(4),
        best_cos: Some(0.20),
    };
    assert!(anchored.vouched(bar));
    let near = search::QueryEvidence {
        lexical: lexical(0),
        best_cos: Some(0.80),
    };
    assert!(near.vouched(bar));
    let nothing = search::QueryEvidence {
        lexical: lexical(0),
        best_cos: Some(0.44),
    };
    assert!(!nothing.vouched(bar));
    // Unembedded: no dense half to appeal to.
    let unembedded = search::QueryEvidence {
        lexical: lexical(0),
        best_cos: None,
    };
    assert!(!unembedded.vouched(bar));
}

/// The bar is keyed to the model (M2) and absent when uncalibrated. The device suffix is
/// stripped, so a Metal build shares it (GH #40).
#[test]
fn the_bar_is_per_model_and_device_suffixes_share_it() {
    assert!(search::EvidenceBar::for_model("BAAI/bge-base-en-v1.5").is_some());
    assert_eq!(
        search::EvidenceBar::for_model("BAAI/bge-base-en-v1.5@metal"),
        search::EvidenceBar::for_model("BAAI/bge-base-en-v1.5"),
    );
    assert!(search::EvidenceBar::for_model(b2_core::embed::FAKE_MODEL_ID).is_none());
    assert!(search::EvidenceBar::for_model("some/other-model").is_none());
}

/// Provenance names the lists that ranked each hit without changing the fused order.
#[test]
fn fusion_carries_each_hit_s_provenance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let retrieval =
        search::hybrid_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5).unwrap();
    assert!(!retrieval.hits.is_empty());
    for hit in &retrieval.hits {
        assert!(
            hit.provenance.bm25_rank.is_some() || hit.provenance.vector_rank.is_some(),
            "a fused hit came from at least one list"
        );
        // The dense half scans every vector, so distance and dense rank go together.
        assert_eq!(
            hit.provenance.distance.is_some(),
            hit.provenance.vector_rank.is_some()
        );
    }
    assert_eq!(
        retrieval
            .hits
            .iter()
            .map(|h| h.chunk_id)
            .collect::<Vec<_>>(),
        search::hybrid_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5)
            .unwrap()
            .hits
            .iter()
            .map(|h| h.chunk_id)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn the_keyword_only_fallback_reports_no_dense_evidence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let retrieval = search::keyword_only_search(&conn, "memory", 5).unwrap();
    assert!(retrieval.best_cos.is_none());
    assert!(retrieval
        .hits
        .iter()
        .all(|h| h.provenance.vector_rank.is_none()));
}

/// A verdict never reorders or removes a result (D1).
#[test]
fn search_evidence_serves_exactly_what_search_serves() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = reindexed_vault(tmp.path());

    let plain = vault.search("memory", 5).unwrap();
    let view = vault.search_evidence("memory", 5).unwrap();
    assert_eq!(
        view.results
            .iter()
            .map(|r| r.result.path.clone())
            .collect::<Vec<_>>(),
        plain.iter().map(|r| r.path.clone()).collect::<Vec<_>>(),
    );
    // The fake embedder has no calibrated bar.
    assert_eq!(view.vouched, None);
    assert!(view.chunk_total > 0);
    assert!(view.terms.iter().any(|t| t.term == "memory"));
}

/// `limit` caps rows only: the evidence is about the query. (Contrast `search`, which
/// returns before embedding.)
#[test]
fn a_zero_limit_evidence_read_still_reads_the_evidence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = reindexed_vault(tmp.path());

    let view = vault.search_evidence("memory", 0).unwrap();
    assert!(view.results.is_empty(), "a zero limit serves no rows");
    assert!(
        view.best_cos.is_some(),
        "but the dense half still reported — an embedded vault must not read as unembedded"
    );
    assert!(view.terms.iter().any(|t| t.term == "memory"));
}

/// An excluded note's slot backfills from the same ranking, order untouched, so an agent's
/// re-query surfaces the next notes.
#[test]
fn excluding_a_served_note_backfills_from_the_same_ranking() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = reindexed_vault(tmp.path());

    let full = vault.search_evidence("memory", 5).unwrap();
    let served: Vec<String> = full.results.iter().map(|r| r.result.path.clone()).collect();
    assert!(
        served.contains(&MEMORY_PATH.to_string()) && served.contains(&SRS_PATH.to_string()),
        "the fixture must serve both notes for this query: {served:?}"
    );

    let head = served[0].clone();
    let view = vault
        .search_evidence_excluding("memory", 5, std::slice::from_ref(&head))
        .unwrap();
    let remaining: Vec<String> = view.results.iter().map(|r| r.result.path.clone()).collect();
    assert!(
        !remaining.contains(&head),
        "an excluded path is never served"
    );
    assert_eq!(
        remaining,
        served
            .into_iter()
            .filter(|p| *p != head)
            .collect::<Vec<_>>(),
        "the rows that remain are the same ranking minus the exclusion, order untouched"
    );
}

/// `--exclude` subtracts rows, never evidence: the verdict is about the query and the
/// vault (ADR-0015).
#[test]
fn exclusion_subtracts_rows_never_the_evidence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (vault, _) = reindexed_vault(tmp.path());

    let full = vault.search_evidence("memory", 5).unwrap();
    let excluded = vault
        .search_evidence_excluding("memory", 5, &[MEMORY_PATH.to_string()])
        .unwrap();
    assert_eq!(excluded.vouched, full.vouched);
    assert_eq!(excluded.chunk_total, full.chunk_total);
    assert_eq!(excluded.terms, full.terms);
    // Even if the excluded note holds the nearest chunk.
    assert_eq!(excluded.best_cos, full.best_cos);

    let unknown = vault
        .search_evidence_excluding("memory", 5, &["notes/never-was.md".to_string()])
        .unwrap();
    assert_eq!(unknown, full, "an unknown path excludes nothing");
}

/// Terms dedup by FTS5 token, not spelling (PR #205): `Memory`, `memory` and `memories`
/// match the same chunks.
#[test]
fn repeated_terms_are_deduped_by_fts_token_not_spelling() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let ev = search::lexical_evidence(&conn, "Memory memory memories").unwrap();
    assert_eq!(
        ev.terms.len(),
        1,
        "one token, so one term: {:?}",
        ev.terms.iter().map(|t| &t.term).collect::<Vec<_>>()
    );
    // The first spelling survives, not the stem ("memori").
    assert_eq!(ev.terms[0].term, "Memory");

    // Two unseen words share `df == 0` but are not the same evidence.
    let distinct = search::lexical_evidence(&conn, "vrelqip zonktar memory").unwrap();
    assert_eq!(distinct.terms.len(), 3);
}

/// Double-counting a present term flips the verdict at the shipped bar: 0.259 with two
/// copies vs 0.149 with one, across 0.20.
#[test]
fn double_counting_a_present_term_would_cross_the_shipped_bar() {
    let bar = search::EvidenceBar::for_model("BAAI/bge-base-en-v1.5").unwrap();
    let term = |t: &str, df| search::TermEvidence { term: t.into(), df };
    let deduped = search::LexicalEvidence {
        chunk_total: 100,
        terms: vec![term("Memory", 44), term("shjfasd", 0)],
    };
    let doubled = search::LexicalEvidence {
        chunk_total: 100,
        terms: vec![term("Memory", 44), term("memory", 44), term("shjfasd", 0)],
    };
    assert!(!deduped.anchored(bar.min_term_coverage));
    assert!(
        doubled.anchored(bar.min_term_coverage),
        "the bug this dedup exists to prevent"
    );
}
