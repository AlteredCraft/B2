//! The dense single-domain fixture (`evals/corpus-dense/`, GH #196/#197 Phase 0b), scored
//! in its own vault and its own row, never averaged in: per-mate ranks, the zero-empty-panes
//! sweep, the fold bench, and the shipped search bar replayed over every note's title.

use crate::common::{cosine_of, pile_stats, term_coverage, title_query, truncate, ScratchVault};
use crate::discovery::AnchorDetail;
use crate::evidence::ServedRow;
use crate::fold::{fold_json, score_fold, FoldBench};
use crate::gate::FLOOR_DENSE_MATE_MRR;
use crate::instrument::{timed_embed, SharedEmbedder};
use crate::labels::SimilarSet;
use crate::metrics::{paths_match, rank_str_at, Agg};
use crate::report::{unix_secs, RunId};
use crate::tail::{dense_tail_json, print_dense_tail};
use crate::{K, SIM_K};
use b2_core::embed::Embedder;
use b2_core::vault::Vault;
use std::path::Path;

/// Score the dense fixture in its own vault: per-mate ranks against `similar-dense.json`,
/// and the empty-pane sweep over every note, since that assertion is about the surface, not
/// the labels.
pub fn score_dense(
    evals_dir: &Path,
    set: &SimilarSet,
    embedder: SharedEmbedder,
) -> Result<DensePass, Box<dyn std::error::Error>> {
    let corpus_dir = evals_dir.join("corpus-dense");
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
        // Every note is an anchor (GH #200): the ruling is about the surface.
        fold: score_fold(&vault, set, "dense", true)?,
        // The geometry that disqualified the losing rule (GH #201).
        search: score_dense_search(&vault, &model_id)?,
    };
    // Every note is an anchor, labelled or not.
    for note in vault.list_notes()? {
        pass.notes += 1;
        if vault.similar(&note.path, SIM_K)?.is_empty() {
            pass.empty_panes.push(note.path);
        }
    }
    // Per-mate ranks on the labelled anchors (GH #183).
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
    /// Per-mate ranks at [`SIM_K`].
    pub mate: Agg,
    /// (anchor, mate, rank) per labelled mate, for the printed lines and the row.
    pub mate_ranks: Vec<(String, String, Option<usize>)>,
    /// Notes whose discovery pane served nothing; asserted empty (GH #196/#197).
    pub empty_panes: Vec<String>,
    pub detail: Vec<AnchorDetail>,
    /// The fold bake-off over every note (GH #200): a rule whose default view goes dark here
    /// is disqualified, not re-tuned.
    pub fold: FoldBench,
    /// D2's shipped bar replayed on this fixture (GH #201).
    pub search: DenseSearch,
}

/// The shipped search evidence bar's reading on the single-domain fixture (ADR-0015). The
/// lexical rule's hazard is topical concentration, which the orthogonal corpus cannot
/// express. Gated since GH #202.
pub struct DenseSearch {
    /// Every note's title replayed as a query: the tripwire direction (D2).
    pub titles: Vec<SearchProbe>,
    /// Nonsense, the defect direction. See [`DENSE_NONSENSE`].
    pub nonsense: Vec<SearchProbe>,
    /// `None` when the model has no calibrated bar (M2); coverage still prints.
    pub bar: Option<b2_core::search::EvidenceBar>,
}

/// One query's reading on the dense fixture: D2's two signals and the engine's verdict.
pub struct SearchProbe {
    pub query: String,
    /// IDF-weighted term coverage; `None` when no term carries weight.
    pub coverage: Option<f64>,
    pub best_cos: Option<f64>,
    /// `Vault::search_evidence`'s verdict. `None` mirrors [`DenseSearch::bar`].
    pub vouched: Option<bool>,
    /// The served list, for the tail bake-off (GH #206). On a title query every row is kept
    /// by geometry, not label, so a tail rule that truncates one is disqualified.
    pub rows: Vec<ServedRow>,
}

/// The negatives replayed on the dense fixture: nonsense only. `queries.json`'s negatives
/// are audited against the orthogonal corpus and don't transfer; nonsense needs no audit.
pub const DENSE_NONSENSE: [&str; 2] = ["shjfasd", "vrelqip zonktar wembleforth"];

/// Replay the shipped bar over the dense fixture, reading coverage off the engine's weights
/// ([`term_coverage`]).
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
        // No frontmatter titles here, so the slug is the query (`drone-comb`).
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
    // Coverage is model-free, so it prints before, and without, a calibrated bar (PR #207).
    let covs: Vec<f64> = search.titles.iter().filter_map(|p| p.coverage).collect();
    let cov_line = match pile_stats(&covs) {
        Some((min, med, max)) => format!("{min:.2}/{med:.2}/{max:.2}"),
        None => "— (no query carried weight)".to_string(),
    };
    println!("              title-as-query coverage min/med/max {cov_line}");

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

/// The dense fixture's search reading as JSON (`search_transfer`), every probe included so
/// any bar is re-derivable without re-running the model.
pub fn dense_search_json(search: &DenseSearch) -> serde_json::Value {
    let probes = |pile: &[SearchProbe]| {
        pile.iter()
            .map(|p| {
                serde_json::json!({
                    "query": p.query,
                    "coverage": p.coverage.map(|c| (c * 1e4).round() / 1e4),
                    "best_cos": p.best_cos.map(|c| (c * 1e4).round() / 1e4),
                    "vouched": p.vouched,
                    // Absent before GH #206.
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
        // Absent before GH #206.
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
    // A geometry reading, not a verdict, so it prints without a calibrated bar too.
    print_dense_tail(&dense.search.titles);
}

/// The dense fixture's own JSONL row, tagged `"corpus": "dense"` so rows never average
/// across corpora.
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
        // Absent before 2026-08-22 (GH #201). Not named `search_evidence`: that key is the
        // labelled bake-off, and a key is never redefined across corpora (PR #207).
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
