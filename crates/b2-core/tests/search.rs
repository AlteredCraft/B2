//! Step 5 — hybrid retrieval (index-engine.md):
//! BM25 ⊕ vector → RRF fusion (k=60), resolved to notes, plus the graph-filtered
//! vector⨝edge join (index-engine.md §3) — the substrate connection discovery
//! runs on.
//!
//! Scope: with the deterministic *fake* embedder, vector ranking is not semantic,
//! so these prove the **plumbing** (fusion math + the join), not model quality —
//! that is the real-embedder eval suite (testability stack, point 5).

mod common;

use b2_core::embed::FakeEmbedder;
use b2_core::id::UlidGen;
use b2_core::ingest::ingest_vault;
use b2_core::search::{self, RRF_K};
use b2_core::{open, search::Hit};
use common::{count, golden_vault_copy, index_conn, ingest_golden, MEMORY_ID, SRS_ID};
use std::fs;

fn note_set(hits: &[Hit]) -> std::collections::BTreeSet<String> {
    hits.iter().map(|h| h.note_b2id.clone()).collect()
}

#[test]
fn rrf_uses_k_60() {
    assert_eq!(RRF_K, 60);
}

/// The candidate depth a search actually reaches is the **composition** of two
/// widenings — the façade's per-view headroom and each signal's `pool_size` — and
/// `vault::{note,chunk}_candidate_pool` are the one place that states it, so a
/// measurement can ask "is this corpus bigger than the pool?" without re-deriving
/// the product (GH #141).
#[test]
fn candidate_pool_states_the_per_signal_depth_a_search_reaches() {
    // A `limit` of 10 (what the eval scores at) reaches 150 candidates per signal
    // for the note view, not 50: it asks retrieval for 3 × 10 hits — dedup headroom
    // — and each signal pulls 5 × that.
    assert_eq!(b2_core::vault::note_candidate_pool(10), 150);
    // The floor still binds at tiny limits — one result still scans 30.
    assert_eq!(b2_core::vault::note_candidate_pool(1), 30);
    // Monotone in `limit`: asking for more never narrows the pool. That is what
    // makes "corpus smaller than the pool ⇒ pool-invariant" safe to conclude from
    // a single K, as the eval's blindness warning does.
    assert!(b2_core::vault::note_candidate_pool(30) > b2_core::vault::note_candidate_pool(10));
    assert!(b2_core::vault::chunk_candidate_pool(30) > b2_core::vault::chunk_candidate_pool(10));
}

/// The GH #142 ruling, pinned: the passage view's headroom exists for a torn read,
/// which is a bounded event, so it is a **constant** — while the note view's exists
/// for dedup, which scales with the ask, so it is a multiple. The two therefore
/// diverge as `limit` grows, and the passage view is always the narrower.
///
/// This is a ranking commitment, not an arithmetic one: `pool_size`'s 5× turns each
/// hit of headroom into five candidates per signal, and RRF over a wider candidate
/// set returns different results (`2/121 > 1/61` at k = 60). Sharing `search`'s 3×
/// here — as GH #140 briefly did — silently widened passage retrieval from 60 to 150
/// candidates, a retrieval-quality change no eval had priced (GH #141, #142).
#[test]
fn the_passage_view_retrieves_a_narrower_pool_than_the_note_view() {
    // The 10-result ask both adapters and the eval use.
    assert_eq!(b2_core::vault::chunk_candidate_pool(10), 60);
    assert_eq!(b2_core::vault::note_candidate_pool(10), 150);

    // Constant vs multiple: the gap widens with the ask, and never closes or flips.
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

/// The blindness #141 names, stated as a property: once a corpus has **no more
/// chunks than the pool**, neither candidate list is truncated, so widening the pool
/// cannot add a candidate and the fused ranking cannot depend on how wide it was — a
/// shallow ask returns a prefix of a deep one, exactly. This is why the 26-chunk eval
/// corpus cannot see a candidate-width change, and why the rank-stability probe
/// (`b2-embed/examples/stability.rs`) runs on a vault big enough for the pool to bind.
///
/// Scoped to *width*: `RRF_K` re-weights the same lists, so it reorders even here.
#[test]
fn a_corpus_no_bigger_than_the_pool_ranks_the_same_at_any_depth() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.reindex().unwrap();

    // The premise is about the *corpus*, not the result count, so it is counted on
    // the chunk rows themselves — 2 results would sit inside any pool regardless.
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

/// `limit` is user input (`b2 search --limit`), and the two widenings compose, so
/// the pool arithmetic must not overflow: before this was saturating, a large
/// `--limit` panicked a debug build outright and would have *wrapped* a release one
/// into a tiny pool — silently wrong results from an absurd-but-harmless ask.
#[test]
fn an_absurd_limit_saturates_the_pool_instead_of_overflowing_it() {
    // Both views saturate: the note view's `× 3` and the passage view's `+ 2` are
    // each one overflow away from wrapping a `usize::MAX` ask into a tiny pool.
    assert_eq!(b2_core::vault::note_candidate_pool(usize::MAX), usize::MAX);
    assert_eq!(b2_core::vault::chunk_candidate_pool(usize::MAX), usize::MAX);

    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.reindex().unwrap();

    // The whole corpus, once, rather than a panic or a truncated-by-wraparound page.
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
    // 20 is rank-1 in BM25 and rank-0 in vector → appearing in both lifts it
    // above 10, which is rank-0 in BM25 but only rank-2 in vector. This is the
    // "hybrid beats either alone" property, at the fusion-math level.
    let bm25 = vec![10, 20, 30];
    let vector = vec![20, 40, 10];
    let fused = search::rrf_fuse(&[bm25, vector], RRF_K);

    assert_eq!(fused[0].0, 20, "doc in both lists wins");
    // every id present, fused score positive and descending
    assert_eq!(fused.len(), 4);
    for w in fused.windows(2) {
        assert!(w[0].1 >= w[1].1, "scores must be descending");
    }
}

#[test]
fn keyword_search_finds_chunks_by_term() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let ids = search::keyword_search(&conn, "forgetting", 10).unwrap();
    assert!(!ids.is_empty());
    // 'forgetting' lives only in spaced-repetition's body.
    let note: String = conn
        .query_row(
            "SELECT note_b2id FROM chunks WHERE id = ?1",
            [ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, SRS_ID);
}

#[test]
fn keyword_search_tolerates_natural_language_punctuation() {
    // Real semantic search invites NL queries: apostrophes, quotes, punctuation are
    // FTS5 *syntax* and would raise a parse error if passed raw (the bug the eval
    // surfaced). They must be sanitized to a safe MATCH, still matching real terms.
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

    // A query with no usable terms is empty, not an error (vector half still runs).
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

    let hits = search::hybrid_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5).unwrap();
    assert!(!hits.is_empty());
    // every hit resolves to a real note, and SRS (the only keyword match) is present
    assert!(hits.iter().all(|h| !h.note_b2id.is_empty()));
    assert!(note_set(&hits).contains(SRS_ID));
}

#[test]
fn graph_filtered_search_restricts_to_reachable_notes() {
    // A 3-note vault: a → b (linked), c disconnected, all share a keyword.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("a.md"),
        "---\nb2id: 01JA0000000000000000000001\ntype: note\ntitle: A\n---\nshared topic alpha. See [[b]].\n",
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        "---\nb2id: 01JB0000000000000000000002\ntype: note\ntitle: B\n---\nshared topic beta.\n",
    )
    .unwrap();
    fs::write(
        vault.join("c.md"),
        "---\nb2id: 01JC0000000000000000000003\ntype: note\ntitle: C\n---\nshared topic gamma.\n",
    )
    .unwrap();

    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();

    // Within 1 hop of A: {A, B}. C is disconnected and must be excluded even
    // though its text matches the query.
    let hits = search::graph_filtered_search(
        &conn,
        &FakeEmbedder::new(64),
        "shared topic",
        "01JA0000000000000000000001",
        1,
        10,
    )
    .unwrap();

    let notes = note_set(&hits);
    assert!(!notes.is_empty());
    assert!(
        !notes.contains("01JC0000000000000000000003"),
        "disconnected note must be filtered out"
    );
    assert!(notes
        .iter()
        .all(|n| n == "01JA0000000000000000000001" || n == "01JB0000000000000000000002"));

    // …and `limit` genuinely truncates that reachable set. This is the complement of
    // tests/vector_pool_scale.rs, which pins the other side — that a limit *above*
    // what is reachable returns everything rather than a silently capped prefix.
    assert!(hits.len() > 1, "the fixture must have room to truncate");
    let capped = search::graph_filtered_search(
        &conn,
        &FakeEmbedder::new(64),
        "shared topic",
        "01JA0000000000000000000001",
        1,
        1,
    )
    .unwrap();
    assert_eq!(capped.len(), 1, "the scan stops at the limit");
}

/// A result's `snippet` must **window around the matched term**, not just show the
/// chunk's head. Under qmd chunking (#19) a chunk is section-sized — far longer than
/// the snippet budget — so a term buried past the head would otherwise never appear
/// in what the human reads, and every hit would look identical.
#[test]
fn a_long_chunks_snippet_windows_around_the_matched_term() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    // ~470 characters of lead-in, then the term — well past the snippet head.
    let lead = "Filler prose that exists only to push the matched term out of the head. ".repeat(7);
    fs::write(
        root.join("long.md"),
        format!(
            "---\nb2id: 01JLONG000000000000000001\ntype: note\n---\n\
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
    // Bounded: the 160-char budget plus at most a leading and trailing ellipsis.
    assert!(hit.snippet.chars().count() <= 162, "{:?}", hit.snippet);

    // A term already inside the head needs no window — the snippet is the head, so
    // it opens with the text itself rather than an ellipsis.
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
    // The sub-note view (`Vault::search_chunks`) the retrieval eval scores passage
    // ranks through (the eval harness, crates/b2-embed/evals/): same retrieval as `search`, no note
    // dedup, each hit resolved to its note path + heading breadcrumb + the chunk's
    // FULL text — containment-scorable, unlike `SearchResult`'s display snippet.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.reindex().unwrap();

    let hits = vault.search_chunks("forgetting curve", 10).unwrap();
    assert!(!hits.is_empty());
    assert!(hits
        .iter()
        .all(|h| !h.path.is_empty() && !h.text.is_empty()));
    // 'forgetting' lives only in spaced-repetition; its hit must carry the full
    // chunk text (the term itself), not a trimmed snippet.
    let srs = hits
        .iter()
        .find(|h| h.b2id == SRS_ID)
        .expect("the one keyword-matching note must surface at chunk level");
    assert!(srs.path.ends_with("spaced-repetition.md"));
    assert!(srs.text.contains("forgetting"));
}

/// GH #137: a ranked chunk that no longer resolves is **skipped over**, not charged
/// against `limit`. The torn read is legitimate, not theoretical — C1 promises
/// readers are never refused while a writer rebuilds (index-engine.md §3), so a
/// `b2 search` racing a `b2 reindex &` can see a chunk id whose row is already gone.
/// Charging it a result slot would silently under-fill the answer.
///
/// The fixture stages exactly that window: an FTS row with no `chunks` row behind
/// it (`chunks_fts` is an external-content table, so the two *can* disagree — which
/// is precisely what a mid-flight `replace_chunks` produces). Its short, term-dense
/// text ranks it first under BM25, and the test asserts that placement rather than
/// assuming it, so a tokenizer or ranking change fails loudly instead of quietly
/// making the case untested.
#[test]
fn a_ranked_chunk_that_no_longer_resolves_costs_no_hit_slot() {
    const DEAD_CHUNK: i64 = 999_999;

    // A purpose-built vault rather than the golden one: this needs several *keyword*
    // matches so a `limit` of 2 has something below it to backfill from, and the
    // BM25-only path is what makes the ranking depend on nothing but the text.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    for (n, id) in [1, 2, 3, 4].iter().zip(['A', 'B', 'C', 'D']) {
        fs::write(
            vault.join(format!("n{n}.md")),
            format!(
                "---\nb2id: 01JDEAD000000000000000000{id}\ntype: note\ntitle: N{n}\n---\n\
                 A note about the capybara, and more capybara prose to rank on.\n"
            ),
        )
        .unwrap();
    }
    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &UlidGen, &FakeEmbedder::new(64)).unwrap();

    let healthy = search::keyword_only_search(&conn, "capybara", 2).unwrap();
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

    // The same live chunks come back, in the same order, still `limit`-many. Their
    // RRF *scores* do shift down a notch — the dead chunk really did occupy rank 0
    // of the BM25 list, which is the whole point — so identity is what's compared.
    let after = search::keyword_only_search(&conn, "capybara", 2).unwrap();
    assert_eq!(
        after.iter().map(|h| h.chunk_id).collect::<Vec<_>>(),
        healthy.iter().map(|h| h.chunk_id).collect::<Vec<_>>(),
        "the dead chunk is stepped over; the live hits below it still fill `limit`"
    );
}

/// The façade's chunk view retrieves a pool wider than `limit` for the same reason
/// (GH #137): it drops a hit whose path/detail lookup misses, and that drop must
/// come out of the headroom rather than out of the caller's result count.
#[test]
fn search_chunks_still_fills_limit_when_a_ranked_chunk_is_dead() {
    const DEAD_CHUNK: i64 = 999_999;

    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.reindex().unwrap();

    let healthy = vault.search_chunks("memory", 2).unwrap();
    assert_eq!(healthy.len(), 2);

    let conn = open(&vault_dir.join(".b2/b2.sqlite")).unwrap();
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

/// `limit == 0` asks for nothing and must get nothing — the loops stop *before*
/// pushing, so a zero budget can never be stepped past into "return everything".
#[test]
fn a_zero_limit_returns_no_hits() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    assert!(search::keyword_only_search(&conn, "memory", 0)
        .unwrap()
        .is_empty());
    assert!(
        search::hybrid_search(&conn, &FakeEmbedder::new(64), "memory", 0)
            .unwrap()
            .is_empty()
    );
    assert!(
        search::graph_filtered_search(&conn, &FakeEmbedder::new(64), "brain", MEMORY_ID, 1, 0)
            .unwrap()
            .is_empty()
    );
}

/// …and it gets there without *doing* anything. The observable proof is the
/// model-mismatch guard: a vault indexed at one dimension and reopened at another
/// fails every real search fast (`Error::ModelMismatch`, so incomparable vectors
/// never rank), because retrieval embeds the query. A zero-limit search returns
/// cleanly instead — it never reached retrieval, which is also what spares the real
/// model a forward pass for an empty answer.
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

    assert!(swapped.search("forgetting", 0).unwrap().is_empty());
    assert!(swapped.search_chunks("forgetting", 0).unwrap().is_empty());
}

#[test]
fn graph_filter_with_zero_hops_is_just_the_anchor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    // 0 hops from memory → only memory's own chunks are eligible.
    let hits =
        search::graph_filtered_search(&conn, &FakeEmbedder::new(64), "brain", MEMORY_ID, 0, 10)
            .unwrap();
    assert!(hits.iter().all(|h| h.note_b2id == MEMORY_ID));
}
