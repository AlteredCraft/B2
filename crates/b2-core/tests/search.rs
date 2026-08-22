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
use b2_core::ingest::ingest_vault;
use b2_core::search::{self, RRF_K};
use b2_core::{open, search::Hit};
use common::{count, golden_vault_copy, index_conn, ingest_golden, MEMORY_PATH, SRS_PATH};
use std::fs;

fn note_set(hits: &[Hit]) -> std::collections::BTreeSet<String> {
    hits.iter().map(|h| h.note_path.clone()).collect()
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
/// shallow ask returns a prefix of a deep one, exactly. This is why the 29-chunk eval
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

/// RRF over integer ranks lands on a discrete lattice of reachable sums, so exact
/// score ties are structural, not freak events — the eval corpus produced one on
/// its second run (`photosynthesis.md` vs `houseplant-care.md`, bit-identical f64,
/// GH #156). The old secondary key was ascending chunk id
/// — projection walk order, which is semantically arbitrary. The policy now: a
/// photo finish is broken by the candidate's rank in the **last** list handed to
/// `rrf_fuse` — the dense/vector list in hybrid search — because on the tie the
/// eval decomposed, the semantic signal named the labelled answer and BM25 named
/// the wrong one. Id remains only as the final determinism key.
#[test]
fn rrf_breaks_symmetric_ties_by_the_dense_lists_rank() {
    // ids 1 and 2 tie exactly: ranks (bm25 0, vector 1) vs (bm25 1, vector 0) sum
    // to the same score. The dense list prefers 2, so 2 must win — under the old
    // id tie-break, 1 won by being the smaller id.
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
    // 7 appears only in BM25 (rank 0), 9 only in the vector list (rank 0): equal
    // single-term sums. The candidate the dense signal saw at all outranks the one
    // it never surfaced.
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
    // 'forgetting' lives only in spaced-repetition's body.
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

    let hits = search::hybrid_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5)
        .unwrap()
        .hits;
    assert!(!hits.is_empty());
    // every hit resolves to a real note, and SRS (the only keyword match) is present
    assert!(hits.iter().all(|h| !h.note_path.is_empty()));
    assert!(note_set(&hits).contains(SRS_PATH));
}

/// The dense half alone (GH #158): the ablation instrument the eval scores beside
/// bm25-only and hybrid. Plumbing only here (fake vectors are not semantic): it
/// ranks by vector distance, resolves to notes, and honors `limit` and zero-limit.
#[test]
fn vector_only_search_is_the_dense_half_alone() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let hits =
        search::vector_only_search(&conn, &FakeEmbedder::new(64), "forgetting curve", 5).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| !h.note_path.is_empty()));
    // Negated-distance scores, best first — the graph_filtered_search convention.
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

/// The façade view dedups to notes exactly as `search` does, and a
/// projected-but-unembedded vault returns nothing — where `search` honestly falls
/// back to keywords, an ablation that quietly did the same would measure the
/// wrong signal.
#[test]
fn search_vector_only_dedups_and_refuses_to_impersonate_keywords() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();

    // Projection only: no embedding space yet. The hybrid view falls back to
    // BM25; the ablation view must return nothing rather than do the same.
    vault.project(false).unwrap();
    assert!(!vault.search("memory", 5).unwrap().is_empty());
    assert!(vault.search_vector_only("memory", 5).unwrap().is_empty());

    // Embedded: hits flow, one per note.
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

#[test]
fn graph_filtered_search_restricts_to_reachable_notes() {
    // A 3-note vault: a → b (linked), c disconnected, all share a keyword.
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(
        vault.join("a.md"),
        "---\ntype: note\ntitle: A\n---\nshared topic alpha. See [[b]].\n",
    )
    .unwrap();
    fs::write(
        vault.join("b.md"),
        "---\ntype: note\ntitle: B\n---\nshared topic beta.\n",
    )
    .unwrap();
    fs::write(
        vault.join("c.md"),
        "---\ntype: note\ntitle: C\n---\nshared topic gamma.\n",
    )
    .unwrap();

    let conn = open(&tmp.path().join("b2.sqlite")).unwrap();
    ingest_vault(&conn, &vault, &FakeEmbedder::new(64)).unwrap();

    // Within 1 hop of A: {A, B}. C is disconnected and must be excluded even
    // though its text matches the query.
    let hits =
        search::graph_filtered_search(&conn, &FakeEmbedder::new(64), "shared topic", "a.md", 1, 10)
            .unwrap();

    let notes = note_set(&hits);
    assert!(!notes.is_empty());
    assert!(
        !notes.contains("c.md"),
        "disconnected note must be filtered out"
    );
    assert!(notes.iter().all(|n| n == "a.md" || n == "b.md"));

    // …and `limit` genuinely truncates that reachable set. This is the complement of
    // tests/vector_pool_scale.rs, which pins the other side — that a limit *above*
    // what is reachable returns everything rather than a silently capped prefix.
    assert!(hits.len() > 1, "the fixture must have room to truncate");
    let capped =
        search::graph_filtered_search(&conn, &FakeEmbedder::new(64), "shared topic", "a.md", 1, 1)
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
        .find(|h| h.path == SRS_PATH)
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

    // The same live chunks come back, in the same order, still `limit`-many. Their
    // RRF *scores* do shift down a notch — the dead chunk really did occupy rank 0
    // of the BM25 list, which is the whole point — so identity is what's compared.
    let after = search::keyword_only_search(&conn, "capybara", 2)
        .unwrap()
        .hits;
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
        .hits
        .is_empty());
    assert!(
        search::hybrid_search(&conn, &FakeEmbedder::new(64), "memory", 0)
            .unwrap()
            .hits
            .is_empty()
    );
    assert!(search::graph_filtered_search(
        &conn,
        &FakeEmbedder::new(64),
        "brain",
        MEMORY_PATH,
        1,
        0
    )
    .unwrap()
    .is_empty());
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

#[test]
fn graph_filter_with_zero_hops_is_just_the_anchor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    // 0 hops from memory → only memory's own chunks are eligible.
    let hits =
        search::graph_filtered_search(&conn, &FakeEmbedder::new(64), "brain", MEMORY_PATH, 0, 10)
            .unwrap();
    assert!(hits.iter().all(|h| h.note_path == MEMORY_PATH));
}

// ---------------------------------------------------------------------------
// Query evidence — the absolute signals RRF discards (invariants.md D2, GH #201)
// ---------------------------------------------------------------------------

/// Document frequency is read over the same sanitized terms the `MATCH`
/// expression searches for, and a word the vault has never seen reads `df == 0`
/// — the honest zero the fused surface could not say.
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

/// A repeated term is one piece of evidence, not two — otherwise a query could
/// talk its own coverage up by saying the same word twice.
#[test]
fn lexical_evidence_counts_a_repeated_term_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let conn = ingest_golden(tmp.path(), &FakeEmbedder::new(64));

    let ev = search::lexical_evidence(&conn, "memory memory memory").unwrap();
    assert_eq!(ev.terms.len(), 1);
}

/// Absent words carry the **most** weight, which is what makes a query of pure
/// nonsense read as coverage zero rather than as having no content at all.
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
    // Only "the" is present, and it weighs almost nothing beside the two it is
    // measured against.
    assert!(ev.term_coverage().is_some_and(|c| c < 0.02));
    assert!(!ev.anchored(0.20));
}

/// The off-topic query's shape, and why weighting is what catches it: the vault
/// shares only a **function word** with it, and a function word carries almost
/// none of the query's weight. This is the labelled negative "why parrots mimic
/// speech" on the eval corpus, where it reads about 0.08.
///
/// Note what the rule does *not* claim: three equally rare words, one of them
/// present, is about a third of the query's weight and **is** an anchor at the
/// shipped bar. That is deliberate — a vault that holds one of a query's rare
/// words has something to say about it, and D2's tripwire is cutting a real
/// query, not serving a thin one.
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

    // The same vault, asked about what it actually holds: every term present.
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

/// The rule survives a **single-domain** vault, where a subject word is in most
/// chunks — the geometry that broke the hard-ceiling rule this one replaced (GH
/// #201's transfer check; GH #196's finding on the lexical axis). A word in 7 of
/// 15 chunks is common, but it is not a stopword, and weighting is what tells
/// the difference.
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

/// An all-stopword query has **no reading**, not a coverage of zero: nothing
/// content-bearing was found or missed, so the lexical half abstains and the
/// cosine half decides alone.
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

/// D2's verdict is lexical **OR** semantic: either signal alone vouches, and
/// only their joint absence answers "no matches".
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
    // Lexical alone, with the dense half far away.
    let anchored = search::QueryEvidence {
        lexical: lexical(4),
        best_cos: Some(0.20),
    };
    assert!(anchored.vouched(bar));
    // Semantic alone, with nothing lexical to stand on.
    let near = search::QueryEvidence {
        lexical: lexical(0),
        best_cos: Some(0.80),
    };
    assert!(near.vouched(bar));
    // Neither — the reported defect, answered.
    let nothing = search::QueryEvidence {
        lexical: lexical(0),
        best_cos: Some(0.44),
    };
    assert!(!nothing.vouched(bar));
    // A projected-but-unembedded vault has no dense half to appeal to.
    let unembedded = search::QueryEvidence {
        lexical: lexical(0),
        best_cos: None,
    };
    assert!(!unembedded.vouched(bar));
}

/// The bar is keyed to the model (M2) and **absent** for anything uncalibrated —
/// no verdict rather than a borrowed one. The device suffix is stripped, so a
/// Metal build answers to the same reading (GH #40).
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

/// Provenance rides beside the fused order without touching it: the same hits in
/// the same order, each now naming the lists that ranked it. A hit BM25 never saw
/// carries `bm25_rank: None` — the per-hit shape of D2's defect.
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
        // The dense half scans every stored vector, so every chunk has a distance
        // exactly when it has a dense rank.
        assert_eq!(
            hit.provenance.distance.is_some(),
            hit.provenance.vector_rank.is_some()
        );
    }
    // The order is the fused order, unchanged by carrying the provenance.
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

/// The keyword-only fallback was already honest about zero, and says so: no dense
/// half means no `best_cos` to overrule the lexical half's silence.
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

/// The façade's evidence read serves the same rows `search` does, in the same
/// order — a verdict is something a surface acts on, never something that
/// quietly reorders or removes a result (D1: reachability is untouchable).
#[test]
fn search_evidence_serves_exactly_what_search_serves() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.reindex().unwrap();

    let plain = vault.search("memory", 5).unwrap();
    let view = vault.search_evidence("memory", 5).unwrap();
    assert_eq!(
        view.results
            .iter()
            .map(|r| r.result.path.clone())
            .collect::<Vec<_>>(),
        plain.iter().map(|r| r.path.clone()).collect::<Vec<_>>(),
    );
    // A fake-embedded space has no calibrated bar, so no verdict is offered.
    assert_eq!(view.vouched, None);
    assert!(view.chunk_total > 0);
    assert!(view.terms.iter().any(|t| t.term == "memory"));
}

/// `limit` caps the rows and nothing else: a zero-limit evidence read still gets
/// the vault's full reading, because the question is about the query rather than
/// about how many results were wanted. (Contrast `search`, which returns before
/// embedding at all — there, nothing is being asked.)
#[test]
fn a_zero_limit_evidence_read_still_reads_the_evidence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_dir = tmp.path().join("vault");
    golden_vault_copy(&vault_dir);
    let vault = b2_core::Vault::open(&vault_dir).unwrap();
    vault.reindex().unwrap();

    let view = vault.search_evidence("memory", 0).unwrap();
    assert!(view.results.is_empty(), "a zero limit serves no rows");
    assert!(
        view.best_cos.is_some(),
        "but the dense half still reported — an embedded vault must not read as unembedded"
    );
    assert!(view.terms.iter().any(|t| t.term == "memory"));
}

/// Terms are deduped by the identity **FTS5** uses, not by spelling (PR #205
/// review). `chunks_fts` is tokenized, so `Memory` / `memory` / `memories` are
/// one token and match the same chunks; counting them separately would let a
/// query weigh one piece of evidence three times.
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
    // The *first spelling* survives, not the index-internal stem ("memori"),
    // which no reader typed.
    assert_eq!(ev.terms[0].term, "Memory");

    // Words the tokenizer keeps apart stay apart — including two it has never
    // seen, which share a `df` of 0 but are not the same evidence.
    let distinct = search::lexical_evidence(&conn, "vrelqip zonktar memory").unwrap();
    assert_eq!(distinct.terms.len(), 3);
}

/// The dedup is not cosmetic: spelling-based dedup double-counts a present term
/// against an absent one, and at the shipped bar that flips the verdict.
///
/// The arithmetic, at `chunk_total = 100` with a term in 44 chunks: two copies
/// of it against one absent word read `2·0.808 / (2·0.808 + 4.615) = 0.259`,
/// one copy reads `0.808 / (0.808 + 4.615) = 0.149` — across the shipped 0.20
/// coverage bar. Asserted on the arithmetic rather than a fixture so the case
/// stays legible when the corpus moves.
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
