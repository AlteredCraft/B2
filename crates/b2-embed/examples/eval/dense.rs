//! The dense single-domain fixture (`evals/corpus-dense/`, GH #196/#197 Phase 0b), scored
//! in its own vault and its own row, never averaged in: per-mate ranks, the zero-empty-panes
//! sweep, the fold bench, and the shipped search bar replayed over every note's title.

use crate::common::{cosine_of, pile_stats, term_coverage, title_query, truncate, ScratchVault};
use crate::discovery::AnchorDetail;
use crate::evidence::ServedRow;
use crate::fold::{fold_json, score_fold, FoldBench};
use crate::gate::FLOOR_DENSE_MATE_MRR;
use crate::instrument::timed_embed;
use crate::labels::SimilarSet;
use crate::metrics::{paths_match, rank_str_at, Agg};
use crate::report::{unix_secs, RunId};
use crate::tail::{dense_tail_json, print_dense_tail};
use crate::{K, SIM_K};
use b2_core::embed::Embedder;
use b2_core::vault::Vault;
use b2_embed::{EmbedConfig, LocalEmbedder};
use std::path::Path;

/// Score the dense single-domain fixture (GH #196/#197, Phase 0b) in a throwaway
/// vault of its own: per-mate discovery ranks against `similar-dense.json`, and
/// the empty-pane sweep across **every** note in the fixture — not only the
/// labelled anchors, because the assertion is about the surface ("no pane in a
/// dense vault is dark"), not about the labels.
pub fn score_dense(
    evals_dir: &Path,
    set: &SimilarSet,
) -> Result<DensePass, Box<dyn std::error::Error>> {
    let corpus_dir = evals_dir.join("corpus-dense");
    // A second model load rather than sharing the first vault's: the embedder was
    // moved into that vault, and the fixture's whole point is an isolated run.
    let config = EmbedConfig::load()?;
    let embedder = LocalEmbedder::load(&config)?;
    let model_id = embedder.model_id().to_string();
    let scratch = ScratchVault::copy_flat(&corpus_dir)?;
    let vault = Vault::open_with_embedder(scratch.root(), Box::new(embedder))?;
    vault.project(false)?;
    let (chunks, embed_secs) = timed_embed(&vault)?;

    let mut pass = DensePass {
        notes: 0,
        chunks,
        embed_secs,
        mate: Agg::default(),
        mate_ranks: Vec::new(),
        empty_panes: Vec::new(),
        detail: Vec::new(),
        // The bake-off's absolute bench (GH #200): every note is an anchor here,
        // because the ruling being tested is about the *surface* — a vault where
        // everything relates may never default to "nothing relates" — and the
        // labels cover only a few of these notes.
        fold: score_fold(&vault, set, "dense", true)?,
        // The bar's hardest bench, for the same reason the fold's is: this is the
        // geometry that disqualified the rule that lost (GH #201).
        search: score_dense_search(&vault, &model_id)?,
    };
    // The pane sweep: every note is an anchor, labelled or not.
    for note in vault.list_notes()? {
        pass.notes += 1;
        if vault.similar(&note.path, SIM_K)?.is_empty() {
            pass.empty_panes.push(note.path);
        }
    }
    // Per-mate ranks on the labelled anchors, the orthogonal corpus's metric
    // re-used verbatim (GH #183's non-saturating readout).
    for label in &set.anchors {
        let candidates = vault.similar(&label.anchor, SIM_K)?;
        for expected in &label.expected {
            let rank = candidates
                .iter()
                .position(|c| paths_match(&c.path, expected))
                .map(|p| p + 1);
            pass.mate.add(rank);
            pass.mate_ranks
                .push((label.anchor.clone(), expected.clone(), rank));
        }
        pass.detail.push(AnchorDetail {
            anchor: label.anchor.clone(),
            negative: false,
            candidates: candidates
                .iter()
                .map(|c| {
                    let related = label.expected.iter().any(|e| paths_match(&c.path, e));
                    (c.path.clone(), cosine_of(c.score), related)
                })
                .collect(),
        });
    }
    Ok(pass)
}

/// The dense fixture's reading (see [`score_dense`]).
pub struct DensePass {
    pub notes: usize,
    pub chunks: usize,
    pub embed_secs: f64,
    /// Per-mate ranks at [`SIM_K`] — the fixture's rank metric.
    pub mate: Agg,
    /// (anchor, mate, rank) per labelled mate, for the printed lines and the row.
    pub mate_ranks: Vec<(String, String, Option<usize>)>,
    /// Notes whose discovery pane served nothing — asserted empty (GH #196/#197).
    pub empty_panes: Vec<String>,
    pub detail: Vec<AnchorDetail>,
    /// The fold bake-off on this fixture (GH #200) — swept over **every** note,
    /// which is where the candidates' hardest bench is: a rule whose default
    /// view goes dark on a single-domain vault is disqualified, not re-tuned.
    pub fold: FoldBench,
    /// D2's shipped bar replayed on this fixture (GH #201) — see
    /// [`score_dense_search`].
    pub search: DenseSearch,
}

/// The shipped search evidence bar's reading **on the single-domain fixture** (ADR-0015).
///
/// This exists because the bar's first form died here and nowhere else. A hard `df <= 10%`
/// content ceiling read 0 cut / 0 served on the labelled orthogonal corpus — clean by every
/// number that bench can produce — and then classed `drone` (df 3) and `comb` (df 7) as
/// stopwords in a vault about beekeeping, cutting 3 of 15 answerable queries. The lexical
/// rule's hazard is **topical concentration**, which the orthogonal corpus cannot express, so
/// a run that judges the bar only there judges it on the geometry it survives.
///
/// The reading was first taken once, by hand. Taking it *once* is the thing GH #187 named,
/// so it is re-derived every run, here, beside the fold bench that already sweeps this
/// fixture. **In the exit gate** since GH #202, as a row of its own rather than headroom on
/// the labelled corpus's, because it watches a different *geometry*.
pub struct DenseSearch {
    /// Every note's own title replayed as a query — the **tripwire direction**
    /// (D2: a labelled-relevant query cut is zero with no headroom). Titles need
    /// no labels and so nothing here can be relabelled to clear a reading.
    pub titles: Vec<SearchProbe>,
    /// Nonsense, the defect direction. See [`DENSE_NONSENSE`].
    pub nonsense: Vec<SearchProbe>,
    /// `None` when the active model has no calibrated bar (M2) — the coverage
    /// readings still print, the verdicts do not exist to print.
    pub bar: Option<b2_core::search::EvidenceBar>,
}

/// One query's reading on the dense fixture: the two absolute signals D2 judges,
/// and the engine's own verdict rather than a restatement of it.
pub struct SearchProbe {
    pub query: String,
    /// IDF-weighted term coverage; `None` when no term carries any weight (the
    /// lexical half abstaining, not scoring zero).
    pub coverage: Option<f64>,
    pub best_cos: Option<f64>,
    /// `Vault::search_evidence`'s verdict — what would actually ship. `None`
    /// mirrors [`DenseSearch::bar`].
    pub vouched: Option<bool>,
    /// The served list with its per-hit provenance — the tail bake-off's dense
    /// bench (GH #206). On a title query `keep` is true for every row **by
    /// geometry**, not by label: a single-domain vault's lists are all real
    /// matches, so a tail rule that truncates one is disqualified — the same
    /// absolute GH #200 enforced for discovery, and the reason this fixture
    /// needs no `tail_relevant` labels.
    pub rows: Vec<ServedRow>,
}

/// The negatives replayed on the dense fixture: **nonsense only**. The labelled negatives in
/// `queries.json` are the *orthogonal* corpus's, and process rule 2's token audit is what
/// makes them negatives — an audit that says nothing about a different corpus. Against
/// `corpus-dense` the phrase-shaped ones disqualify themselves on their merits ("why parrots
/// mimic speech" shares `mimic` with `robbing-behavior.md`, which is a thing the rule
/// deliberately serves). Nonsense needs no audit in any vault, which is why it transfers.
pub const DENSE_NONSENSE: [&str; 2] = ["shjfasd", "vrelqip zonktar wembleforth"];

/// Replay the shipped bar over the dense fixture (see [`DenseSearch`]).
///
/// Coverage is read off [`b2_core::vault::QueryTermView::idf`] — the view's own
/// weights, not a second copy of the formula ([`term_coverage`]). The orthogonal
/// corpus's bake-off re-derives its arithmetic deliberately, as a drift check
/// against the engine; one such check is the check, and a second would only be two
/// places to fix.
pub fn score_dense_search(
    vault: &Vault,
    model_id: &str,
) -> Result<DenseSearch, Box<dyn std::error::Error>> {
    let read = |query: &str, keep: bool| -> Result<SearchProbe, Box<dyn std::error::Error>> {
        let view = vault.search_evidence(query, K)?;
        Ok(SearchProbe {
            query: query.to_string(),
            coverage: term_coverage(&view),
            best_cos: view.best_cos,
            vouched: view.vouched,
            rows: view
                .results
                .iter()
                .map(|r| ServedRow {
                    path: r.result.path.clone(),
                    bm25_rank: r.bm25_rank,
                    cos: r.cos,
                    keep,
                })
                .collect(),
        })
    };
    let mut titles = Vec::new();
    for note in vault.list_notes()? {
        // The fixture's notes carry no frontmatter title, so the slug is the
        // query — `drone-comb` → "drone comb", which is the pair of words the
        // retired ceiling called stopwords.
        if let Some(title) = title_query(&note) {
            titles.push(read(&title, true)?);
        }
    }
    Ok(DenseSearch {
        titles,
        nonsense: DENSE_NONSENSE
            .iter()
            .map(|q| read(q, false))
            .collect::<Result<_, _>>()?,
        bar: b2_core::search::EvidenceBar::for_model(model_id),
    })
}

/// Print the dense fixture's search-evidence reading (see [`DenseSearch`]).
pub fn print_dense_search(search: &DenseSearch) {
    println!(
        "  search bar  D2's shipped bar replayed on this geometry (GH #201; GATED since GH #202)"
    );
    // The coverage reading comes FIRST, and above the bar, because it is
    // **model-free**: a fact about this vault's vocabulary, and the lexical
    // half's whole premise. Gating it behind a calibrated bar would print
    // nothing at all on a vault the harness can still say something true about —
    // the defect PR #205's review already fixed in `calibrate.rs` (c03f8cd), met
    // again here (PR #207 review).
    let covs: Vec<f64> = search.titles.iter().filter_map(|p| p.coverage).collect();
    let cov_line = match pile_stats(&covs) {
        Some((min, med, max)) => format!("{min:.2}/{med:.2}/{max:.2}"),
        None => "— (no query carried weight)".to_string(),
    };
    println!("              title-as-query coverage min/med/max {cov_line}");

    // Only the *verdicts* below need a bar, so only they stop here.
    let Some(bar) = search.bar else {
        println!("              no calibrated bar for this model — no verdict is offered (M2)");
        return;
    };
    let cut: Vec<&SearchProbe> = search
        .titles
        .iter()
        .filter(|p| p.vouched == Some(false))
        .collect();
    let served: Vec<&SearchProbe> = search
        .nonsense
        .iter()
        .filter(|p| p.vouched == Some(true))
        .collect();
    println!(
        "              bar under test: coverage ≥ {:.2} or cos ≥ {:.3}",
        bar.min_term_coverage, bar.min_cos,
    );
    println!(
        "              cuts {}/{} title queries   ← the TRIPWIRE direction; the retired df ceiling \
         cut 3 here",
        cut.len(),
        search.titles.len()
    );
    for p in &cut {
        println!(
            "                [CUT] {:<32} cov {:>5}  cos {:>6}",
            truncate(&p.query, 32),
            p.coverage
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string()),
            p.best_cos
                .map(|c| format!("{c:.3}"))
                .unwrap_or_else(|| "—".to_string()),
        );
    }
    println!(
        "              serves {}/{} nonsense queries   ← the reported defect, on this corpus",
        served.len(),
        search.nonsense.len()
    );
}

/// The dense fixture's search reading as JSON (`search_transfer` in the dense
/// row) — every probe, so any bar is re-derivable from a row without re-running
/// the model, the `discovery_fold` convention.
pub fn dense_search_json(search: &DenseSearch) -> serde_json::Value {
    let probes = |pile: &[SearchProbe]| {
        pile.iter()
            .map(|p| {
                serde_json::json!({
                    "query": p.query,
                    "coverage": p.coverage.map(|c| (c * 1e4).round() / 1e4),
                    "best_cos": p.best_cos.map(|c| (c * 1e4).round() / 1e4),
                    "vouched": p.vouched,
                    // NEW subkey (absent before GH #206): the served list's
                    // per-hit provenance, so the tail constraints below are
                    // re-derivable from a row without re-running the model.
                    "rows": p.rows.iter().map(|row| serde_json::json!({
                        "path": row.path,
                        "bm25_rank": row.bm25_rank,
                        "cos": row.cos.map(|c| (c * 1e4).round() / 1e4),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "bar": search.bar.map(|b| serde_json::json!({
            "min_term_coverage": b.min_term_coverage,
            "min_cos": b.min_cos,
        })),
        "titles_cut": search.titles.iter().filter(|p| p.vouched == Some(false)).count(),
        "nonsense_served": search.nonsense.iter().filter(|p| p.vouched == Some(true)).count(),
        "titles": probes(&search.titles),
        "nonsense": probes(&search.nonsense),
        // NEW subkey (absent before GH #206): the per-hit tail families'
        // dense-bench constraints — the absolute this fixture supplies.
        "tail": dense_tail_json(&search.titles),
    })
}

pub fn print_dense_report(dense: &DensePass) {
    println!("\n{}", "=".repeat(78));
    println!(
        "dense fixture — corpus-dense/ ({} notes / {} chunks, single-domain, no loner; GH #196/#197)",
        dense.notes, dense.chunks
    );
    println!(
        "  per-mate   hit@1={:.2}  hit@3={:.2}  MRR@{SIM_K}={:.3}  (n={} mates, GATED at MRR@{SIM_K} ≥ {FLOOR_DENSE_MATE_MRR:.2})",
        dense.mate.hit1(),
        dense.mate.hit3(),
        dense.mate.mrr(),
        dense.mate.n
    );
    for (anchor, mate, rank) in &dense.mate_ranks {
        println!(
            "             {:>5}  {anchor} → {mate}",
            rank_str_at(*rank, SIM_K)
        );
    }
    if dense.empty_panes.is_empty() {
        println!(
            "  panes      {}/{} notes serve candidates — zero empty panes (ASSERTED: a dense vault \
             may never read as \"nothing relates\")",
            dense.notes, dense.notes
        );
    } else {
        println!(
            "  panes      {} of {} notes serve an EMPTY pane: {}",
            dense.empty_panes.len(),
            dense.notes,
            dense.empty_panes.join(", ")
        );
    }
    print_dense_search(&dense.search);
    // Model-geometry reading, not a verdict, so it prints with or without a
    // calibrated bar — the same posture as the coverage line above it.
    print_dense_tail(&dense.search.titles);
}

/// The dense fixture's own JSONL row. Tagged `"corpus": "dense"` — the key that
/// keeps rows from ever averaging across corpora (the orthogonal rows carry
/// `"corpus": "orthogonal"`); a smaller shape than the main row on purpose,
/// since the fixture scores discovery alone.
pub fn dense_row(run: RunId, dense: &DensePass) -> serde_json::Value {
    let RunId { git, model, dim } = run;
    let ts = unix_secs();
    serde_json::json!({
        "ts": ts,
        "git": git,
        "model": model,
        "dim": dim,
        "corpus": "dense",
        "notes": dense.notes,
        "chunks": dense.chunks,
        "embed_secs": dense.embed_secs,
        "similar_per_mate": { "n": dense.mate.n, "hit1": dense.mate.hit1(), "hit3": dense.mate.hit3(), "mrr": dense.mate.mrr() },
        "mates": dense.mate_ranks.iter().map(|(anchor, mate, rank)| serde_json::json!({
            "anchor": anchor, "mate": mate, "rank": rank,
        })).collect::<Vec<_>>(),
        "empty_panes": { "n": dense.notes, "empty": dense.empty_panes.len(), "detail": dense.empty_panes },
        "discovery_fold": fold_json(&dense.fold),
        // NEW key (absent from rows before 2026-08-22): the shipped search bar
        // replayed on this fixture (GH #201). Deliberately NOT the orthogonal
        // row's `search_evidence`: that key holds the *labelled* bake-off, and
        // this is the label-free transfer reading — a different measurement, so
        // it takes a different name. Same convention as every key above: new,
        // never a redefinition, so no reader has to branch on `corpus` to learn
        // which shape it is holding (PR #207 review).
        "search_transfer": dense_search_json(&dense.search),
        "similar_detail": dense.detail.iter().map(|d| serde_json::json!({
            "anchor": d.anchor,
            "negative": d.negative,
            "candidates": d.candidates.iter().map(|(path, cos, related)| serde_json::json!({
                "path": path,
                "cos": (cos * 1e4).round() / 1e4,
                "related": related,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
